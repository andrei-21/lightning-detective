use anyhow::{anyhow, bail, Error, Result};
use bitcoin_payment_instructions::hrn_resolution::HumanReadableName;
use lightning::offers::offer::Offer;
use lightning::offers::refund::Refund;
use lightning_invoice::Bolt11Invoice;
use lnurl::lightning_address::LightningAddress;
use lnurl::lnurl::LnUrl;
use std::str::FromStr;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum DecodedData {
    Invoice(Bolt11Invoice),
    Offer(Offer),
    Refund(Refund),
    LightningAddress(LightningAddress),
    LnUrl(LnUrl),
    Bip21(Option<String>, Vec<Bip21Param>),
    Bip353(HumanReadableName),
}

pub fn decode(input: &str) -> Result<DecodedData> {
    println!("    Input: {input}");
    // TODO: Do not lowercase for BIP 21.
    let input = input.trim().to_lowercase();
    let input = input.strip_prefix("lightning:").unwrap_or(&input);
    println!("Sanitized: {input}");
    let filtered_input: String = input
        .chars()
        .filter(|c| *c != '+' && !c.is_whitespace())
        .collect();

    let decoded_data = if input.contains('@') {
        match HumanReadableName::from_encoded(input) {
            Ok(name) => return Ok(DecodedData::Bip353(name)),
            Err(()) => println!("Not a BIP-353 name"),
        };

        println!("Decoding as a lightning address");
        let address = LightningAddress::from_str(input)?;
        DecodedData::LightningAddress(address)
    } else if input.starts_with("lnurl") {
        // TODO: Support LUD-17: Protocol schemes and raw (non bech32-encoded) URLs.
        println!("Decoding as LNURL");
        let lnurl = LnUrl::from_str(input)?;
        DecodedData::LnUrl(lnurl)
    } else if filtered_input.starts_with("lno") {
        println!("Decoding as BOLT12 offer");
        let offer = Offer::from_str(input).map_err(|e| anyhow!("{e:?}"))?;
        DecodedData::Offer(offer)
    } else if filtered_input.starts_with("lnr") {
        println!("Decoding as BOLT12 refund (naked invoice request)");
        let refund = Refund::from_str(input).map_err(|e| anyhow!("{e:?}"))?;
        DecodedData::Refund(refund)
    } else if input.starts_with("ln") {
        println!("Decoding as BOLT11 invoice");
        let invoice = input.parse::<Bolt11Invoice>().map_err(Error::msg)?;
        DecodedData::Invoice(invoice)
    } else if input.starts_with("bitcoin:") {
        println!("Decoding as BIP21 URI");
        let (address, params) = parse_bip21(input)?;
        DecodedData::Bip21(address, params)
    } else {
        // TODO: Decode on-chain addresses.
        // TODO: Decode BIP 72.
        // TODO: Decode xpub, xpriv.
        bail!("Input is not recognized");
    };
    Ok(decoded_data)
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum Bip21Param {
    Amount(u64),
    Label(String),
    Message(String),
    Lightning(String),
    Offer(String),
    SilentPayment(String),
    // BIP 78: A Simple Payjoin Proposal.
    PayjoinEndpoint(String),
    PayjoinDisallowOutputSubstitution,
    Unknown(String, String),
}

pub fn parse_bip21(uri: &str) -> Result<(Option<String>, Vec<Bip21Param>)> {
    // TODO: Strip prefix ignore case.
    let uri = uri
        .strip_prefix("bitcoin:")
        .ok_or(anyhow!("Missing bitcoin prefix"))?;

    let (address, params) = uri.split_once('?').unwrap_or((uri, ""));

    let params = match params.is_empty() {
        true => Vec::new(),
        false => try_collect(
            params
                .split('&')
                .map(|p| p.split_once('=').ok_or(anyhow!("Invalid param")))
                .map(|p| p.and_then(Bip21Param::try_from)),
        )?,
    };

    let address = if address.is_empty() {
        None
    } else {
        Some(address.to_string())
    };

    Ok((address, params))
}

impl TryFrom<(&str, &str)> for Bip21Param {
    type Error = anyhow::Error;

    fn try_from((key, value): (&str, &str)) -> Result<Self, Self::Error> {
        Ok(match key {
            "amount" => {
                let amount = bitcoin::Amount::from_str_in(value, bitcoin::Denomination::Bitcoin)
                    .map_err(|e| anyhow!("Failed to amount decode param: {e}"))?;
                Self::Amount(amount.to_sat())
            }
            "label" => Self::Label(decode_percent(value)?),
            "message" => Self::Message(decode_percent(value)?),
            "lightning" => Self::Lightning(decode_percent(value)?),
            "lno" => Self::Offer(decode_percent(value)?),
            "sp" => Self::SilentPayment(decode_percent(value)?),
            "pj" => Self::PayjoinEndpoint(decode_percent(value)?),
            "pjos" if value == "0" => Self::PayjoinDisallowOutputSubstitution,
            "pjos" => bail!("Unexpected value for pjos param"),
            _ => Self::Unknown(decode_percent(key)?, decode_percent(value)?),
        })
    }
}

fn decode_percent(input: &str) -> Result<String> {
    Ok(percent_encoding_rfc3986::percent_decode_str(input)
        .map_err(|e| anyhow!("Failed to decode param: {e}"))?
        .decode_utf8()
        .map_err(|e| anyhow!("Failed to decode param: {e}"))?
        .to_string())
}

fn try_collect<I, T, E>(iter: I) -> std::result::Result<Vec<T>, E>
where
    I: IntoIterator<Item = std::result::Result<T, E>>,
{
    let mut result = Vec::new();
    for item in iter {
        result.push(item?);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode() {
        let d = decode("satoshi@bitcoin.org").unwrap();
        println!("{d:?}");

        let d = decode("LNURL1DP68GURN8GHJ7MRWW4EXCTNXD9SHG6NPVCHXXMMD9AKXUATJDSKHQCTE8AEK2UMND9HKU0FJ89JXXCT989JRGVE3XVMK2ERZXPJX2DECXP3KXV33XQCKVE3C8QMXXD3CVVUXXEPNV3NRWE3HXVUKZWP3XSEX2V3CXEJXGCNRXGUKGUQ0868").unwrap();
        println!("{d:?}");

        let d = decode("lntb10u1pjkvq6mpp5zszjfrehd5y8sq4w47jegjy5xglw3smcfelfkqud56vtq9c48kmsdqqcqzzsxqyz5vqsp5kgjy259sn4t24er4hawcsr9zl9u7vrkdk7a9kcs9ffury0kf50cq9qyyssqept74lw02kkng3cpzqhyrwt542ct6dtfcz7mtesfggt57r5j7djyz7z5de4cyaupehhwyv7ql6yatqe3e4hvnp2lvpvdwxstpy2rnwqq89p90d").unwrap();
        println!("{d:?}");
    }
}
