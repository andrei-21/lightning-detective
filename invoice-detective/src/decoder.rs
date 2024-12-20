use anyhow::{anyhow, bail, Result};
use lightning::offers::offer::Offer;
use lightning::offers::refund::Refund;
use lightning_invoice::Bolt11Invoice;
use lnurl::lightning_address::LightningAddress;
use lnurl::lnurl::LnUrl;
use lnurl::pay::LnURLPayInvoice;
use lnurl::{decode_ln_url_response, LnUrlResponse};
use std::io;
use std::io::Write;
use std::str::FromStr;
use std::time::Duration;

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum DecodedData {
    Invoice(Bolt11Invoice),
    Offer(Offer),
    Refund(Refund),
    LightningAddress(LightningAddress),
    LnUrl(LnUrl),
    Bip21(String, Vec<Bip21Param>),
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
        let invoice = input.parse::<Bolt11Invoice>()?;
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

fn parse_bip21(uri: &str) -> Result<(String, Vec<Bip21Param>)> {
    // TODO: Strip prefix ignore case.
    let uri = uri
        .strip_prefix("bitcoin:")
        .ok_or(anyhow!("Missing bitcoin prefix"))?;

    let (address, params) = match uri.split_once('?') {
        Some(pair) => pair,
        None => (uri, ""),
    };

    let params = try_collect(
        params
            .split('&')
            .map(|p| p.split_once('=').ok_or(anyhow!("Invalid param")))
            .map(|p| p.and_then(Bip21Param::try_from)),
    )?;

    Ok((address.to_string(), params))
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
            "pjos" => bail!("Unexpected value for pjso param"),
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

pub async fn resolve_lnurl(lnurl: LnUrl) -> Result<String> {
    println!("Quering {}", lnurl.url);
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let response = client.get(&lnurl.url).send().await?;
    let text = response.error_for_status()?.text().await?;
    println!("Response: {text}");
    print!("Decoding as JSON: ");
    let _ = io::stdout().flush();
    let response = decode_ln_url_response(&text)?;
    println!("OK");

    let pay = match response {
        LnUrlResponse::LnUrlPayResponse(pay_response) => pay_response,
        LnUrlResponse::LnUrlWithdrawResponse(_) => bail!("LNURL Withdraw"),
        LnUrlResponse::LnUrlChannelResponse(_) => bail!("LNURL channel request"),
    };

    let symbol = if pay.callback.contains('?') { '&' } else { '?' };
    let url = format!("{}{symbol}amount={}", pay.callback, pay.min_sendable);
    println!("Quering {url}");
    let response = client.get(&url).send().await?;
    let text = response.error_for_status()?.text().await?;
    println!("Response: {text}");
    print!("Decoding as JSON: ");
    let _ = io::stdout().flush();
    let json: serde_json::Value = serde_json::from_str(&text)?;
    println!("OK");
    print!("Decoding as LNURL pay invoice response: ");
    let _ = io::stdout().flush();
    let reponse: LnURLPayInvoice = serde_json::from_value(json)?;
    println!("OK");
    Ok(reponse.pr)
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
