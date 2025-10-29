use anyhow::{anyhow, bail, ensure, Context, Result};
use bitcoin_payment_instructions::hrn_resolution::HumanReadableName;
use lightning::offers::offer::Offer;
use lightning::offers::refund::Refund;
use lightning_invoice::Bolt11Invoice;
use lnurl::lightning_address::LightningAddress;
use lnurl::lnurl::LnUrl;
use std::str::FromStr;

const BITCOIN_PREFIX: &str = "bitcoin:";

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
    let input = input.trim();
    let lowercased = input.to_lowercase();

    // TODO: Decode on-chain addresses.
    // TODO: Decode BIP 72.
    // TODO: Decode xpub, xpriv.
    // TODO: Support LUD-17: Protocol schemes and raw (non bech32-encoded) URLs.

    let decoded_data = if let Some(lowercased) = lowercased.strip_prefix("lightning:") {
        decode_lightning(lowercased)?
    } else if lowercased.starts_with(BITCOIN_PREFIX) {
        let (address, params) = parse_bip21(input).context("Failed to parse BIP-21 URI")?;
        DecodedData::Bip21(address, params)
    } else if input.starts_with('₿') {
        let hrn =
            HumanReadableName::from_encoded(input).map_err(|()| anyhow!("Invalid BIP-353 name"))?;
        DecodedData::Bip353(hrn)
    } else if let Ok(hrn) = HumanReadableName::from_encoded(input) {
        // TODO: Can be a lightning address also.
        DecodedData::Bip353(hrn)
    } else {
        decode_lightning(&lowercased)?
    };

    Ok(decoded_data)
}

fn decode_lightning(input: &str) -> Result<DecodedData> {
    let filtered_input: String = input
        .chars()
        .filter(|c| *c != '+' && !c.is_whitespace())
        .collect();
    let decoded_data = if input.contains('@') {
        let address =
            LightningAddress::from_str(input).context("Failed to parse lightning address")?;
        DecodedData::LightningAddress(address)
    } else if input.starts_with("lno") {
        let offer = Offer::from_str(&filtered_input)
            .map_err(|e| anyhow!("Failed to parse BOLT-12 offer: {e:?}"))?;
        DecodedData::Offer(offer)
    } else if input.starts_with("lnr") {
        let refund = Refund::from_str(&filtered_input)
            .map_err(|e| anyhow!("Failed to parse BOLT-12 refund: {e:?}"))?;
        DecodedData::Refund(refund)
    } else if input.starts_with("lnurl") {
        let lnurl = LnUrl::from_str(input).context("Failed to parse LNURL")?;
        DecodedData::LnUrl(lnurl)
    } else if input.starts_with("ln") {
        let invoice = Bolt11Invoice::from_str(input)
            .map_err(|e| anyhow!("Failed to parse BOLT-11 invoice: {e:?}"))?;
        DecodedData::Invoice(invoice)
    } else {
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
    let (prefix, uri) = uri
        .split_at_checked(BITCOIN_PREFIX.len())
        .ok_or(anyhow!("Invalid prefix"))?;
    ensure!(
        prefix.eq_ignore_ascii_case(BITCOIN_PREFIX),
        "Invalid prefix"
    );

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
    fn decodes_bolt11_invoice_without_scheme() {
        let invoice = "LNTB10U1PJKVQ6MPP5ZSZJFREHD5Y8SQ4W47JEGJY5XGLW3SMCFELFKQUD56VTQ9C48KMSDQQCQZZSXQYZ5VQSP5KGJY259SN4T24ER4HAWCSR9ZL9U7VRKDK7A9KCS9FFURY0KF50CQ9QYYSSQEPT74LW02KKNG3CPZQHYRWT542CT6DTFCZ7MTESFGGT57R5J7DJYZ7Z5DE4CYAUPEHHWYV7QL6YATQE3E4HVNP2LVPVDWXSTPY2RNWQQ89P90D";
        let input = format!("  {invoice}\n");

        match decode(&input).unwrap() {
            DecodedData::Invoice(parsed) => assert_eq!(parsed.to_string(), invoice.to_lowercase()),
            other => panic!("expected invoice, got {other:?}"),
        }
    }

    #[test]
    fn decodes_bolt11_invoice_with_scheme() {
        let invoice = "lntb10u1pjkvq6mpp5zszjfrehd5y8sq4w47jegjy5xglw3smcfelfkqud56vtq9c48kmsdqqcqzzsxqyz5vqsp5kgjy259sn4t24er4hawcsr9zl9u7vrkdk7a9kcs9ffury0kf50cq9qyyssqept74lw02kkng3cpzqhyrwt542ct6dtfcz7mtesfggt57r5j7djyz7z5de4cyaupehhwyv7ql6yatqe3e4hvnp2lvpvdwxstpy2rnwqq89p90d";
        let input = format!("  lightning:{invoice}\n");

        match decode(&input).unwrap() {
            DecodedData::Invoice(parsed) => assert_eq!(parsed.to_string(), invoice),
            other => panic!("expected invoice, got {other:?}"),
        }
    }

    #[test]
    fn decodes_lightning_address_with_scheme() {
        match decode("lightning:example@getalby.com").unwrap() {
            DecodedData::LightningAddress(address) => {
                assert_eq!(address.to_string(), "example@getalby.com");
            }
            other => panic!("expected lightning address, got {other:?}"),
        }
    }

    #[test]
    fn decodes_lnurl() {
        let lnurl = "lnurl1dp68gurn8ghj7mrww4exctnxd9shg6npvchxxmmd9akxuatjdskhqcte8aek2umnd9hku0fj89jxxct989jrgve3xvmk2erzxpjx2decxp3kxv33xqckve3c8qmxxd3cvvuxxepnv3nrwe3hxvukzwp3xsex2v3cxejxgcnrxgukguq0868";
        let expected = LnUrl::from_str(lnurl).expect("expected valid LNURL");

        match decode(lnurl).unwrap() {
            DecodedData::LnUrl(actual) => assert_eq!(actual, expected),
            other => panic!("expected LNURL, got {other:?}"),
        }
    }

    #[test]
    fn decodes_bip21_uri_with_params() {
        let uri = "bitcoin:bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080?amount=0.001&label=Donation&message=Thanks%20for%20your%20support";
        match decode(uri).unwrap() {
            DecodedData::Bip21(address, params) => {
                assert_eq!(
                    address.as_deref(),
                    Some("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kygt080")
                );
                assert_eq!(
                    params,
                    vec![
                        Bip21Param::Amount(100_000),
                        Bip21Param::Label("Donation".to_string()),
                        Bip21Param::Message("Thanks for your support".to_string())
                    ]
                );
            }
            other => panic!("expected BIP-21 data, got {other:?}"),
        }
    }

    #[test]
    fn decodes_bip353_name() {
        let input = "₿satoshi@bitcoin.org";
        match decode(input).unwrap() {
            DecodedData::Bip353(hrn) => {
                assert_eq!(hrn.user(), "satoshi");
                assert_eq!(hrn.domain(), "bitcoin.org");
            }
            other => panic!("expected BIP-353 name, got {other:?}"),
        }

        let input = "lnurl@bitcoin.org";
        match decode(input).unwrap() {
            DecodedData::Bip353(hrn) => {
                assert_eq!(hrn.user(), "lnurl");
                assert_eq!(hrn.domain(), "bitcoin.org");
            }
            other => panic!("expected BIP-353 name, got {other:?}"),
        }
    }
}
