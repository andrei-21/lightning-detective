use anyhow::{anyhow, bail, ensure, Context, Error, Result};
use bitcoin::hex::FromHex;
pub use bitcoin_payment_instructions::hrn_resolution::HumanReadableName;
use lightning::offers::invoice::Bolt12Invoice;
use lightning::offers::offer::Offer;
use lightning::offers::refund::Refund;
use lightning::offers::static_invoice::StaticInvoice;
use lightning_invoice::Bolt11Invoice;
use reqwest::Url;
use silentpayments::SilentPaymentAddress;
use std::str::FromStr;

use crate::liquid_address::{parse_liquid_uri, LiquidAddress, LiquidUri};
use crate::lnurl::LightningAddress;
use crate::types::Sat;
use crate::{InvestigateValue, InvestigateValueKind, OnchainAddress};

const BITCOIN_PREFIX: &str = "bitcoin:";
const LIQUID_PREFIX: &str = "liquidnetwork:";
const LIQUID_TESTNET_PREFIX: &str = "liquidtestnet:";

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub enum DecodedData {
    OnchainAddress(OnchainAddress),
    LiquidAddress(LiquidAddress),
    LiquidUri(LiquidUri),
    Invoice(Bolt11Invoice),
    Offer(Offer),
    Refund(Refund),
    LightningAddress(LightningAddress),
    LnUrl(LnUrl),
    Bip21(Bip21),
    Bip353(HumanReadableName),
    Bip353OrLightningAddress(HumanReadableName, LightningAddress),
    SilentPaymentAddress(SilentPaymentAddress),
    Bolt12Invoice(Bolt12Invoice),
    Bolt12StaticInvoice(StaticInvoice),
}

pub fn decode(input: &str) -> Result<DecodedData> {
    let input = input.trim();
    let lowercased = input.to_lowercase();

    // TODO: Decode BIP 72.
    // TODO: Decode xpub, xpriv.

    let decoded_data = if let Some(value) = InvestigateValue::parse(input) {
        let payload = Vec::<u8>::from_hex(&value.payload).map_err(Error::msg)?;
        match value.kind {
            InvestigateValueKind::Bolt12Invoice => {
                let invoice = Bolt12Invoice::try_from(payload).map_err(|e| anyhow!("{e:?}"))?;
                DecodedData::Bolt12Invoice(invoice)
            }
            InvestigateValueKind::Bolt12StaticInvoice => {
                let invoice = StaticInvoice::try_from(payload).map_err(|e| anyhow!("{e:?}"))?;
                DecodedData::Bolt12StaticInvoice(invoice)
            }
        }
    } else if let Some(lowercased) = lowercased.strip_prefix("lightning:") {
        decode_lightning(lowercased)?
    } else if lowercased.starts_with(LIQUID_PREFIX) || lowercased.starts_with(LIQUID_TESTNET_PREFIX)
    {
        let liquid_uri = parse_liquid_uri(input).context("Failed to parse Liquid URI")?;
        DecodedData::LiquidUri(liquid_uri)
    } else if let Ok(address) = LiquidAddress::from_str(input) {
        DecodedData::LiquidAddress(address)
    } else if let Ok(address) = OnchainAddress::from_str(input) {
        DecodedData::OnchainAddress(address)
    } else if let Ok(address) = SilentPaymentAddress::try_from(input) {
        DecodedData::SilentPaymentAddress(address)
    } else if lowercased.starts_with(BITCOIN_PREFIX) {
        let bip21 = parse_bip21(input).context("Failed to parse BIP-21 URI")?;
        DecodedData::Bip21(bip21)
    } else if input.starts_with('₿') {
        let hrn =
            HumanReadableName::from_encoded(input).map_err(|()| anyhow!("Invalid BIP-353 name"))?;
        DecodedData::Bip353(hrn)
    } else if let Ok(hrn) = HumanReadableName::from_encoded(input) {
        if let Ok(lightning_address) = LightningAddress::from_str(input) {
            DecodedData::Bip353OrLightningAddress(hrn, lightning_address)
        } else {
            DecodedData::Bip353(hrn)
        }
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
        let lightning_address = LightningAddress::from_str(input)?;
        DecodedData::LightningAddress(lightning_address)
    } else if input.starts_with("lno") {
        let offer = Offer::from_str(&filtered_input)
            .map_err(|e| anyhow!("Failed to parse BOLT-12 offer: {e:?}"))?;
        DecodedData::Offer(offer)
    } else if input.starts_with("lnr") {
        let refund = Refund::from_str(&filtered_input)
            .map_err(|e| anyhow!("Failed to parse BOLT-12 refund: {e:?}"))?;
        DecodedData::Refund(refund)
    } else if input.starts_with("lnurlc")
        || input.starts_with("lnurlp")
        || input.starts_with("lnurlw")
        || input.starts_with("keyauth")
    {
        let lnurl = LnUrl::from_str(input)?;
        DecodedData::LnUrl(lnurl)
    } else if input.starts_with("lnurl") {
        let lnurl = lnurl::lnurl::LnUrl::from_str(input).context("Failed to parse LNURL")?;
        let kind = LnUrlKind::Unknown;
        DecodedData::LnUrl(LnUrl {
            kind,
            url: lnurl.url,
        })
    } else if input.starts_with("ln") {
        let invoice = Bolt11Invoice::from_str(input)
            .map_err(|e| anyhow!("Failed to parse BOLT-11 invoice: {e:?}"))?;
        DecodedData::Invoice(invoice)
    } else {
        bail!("Input is not recognized");
    };
    Ok(decoded_data)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LnUrlKind {
    Channel,
    Pay,
    Withdraw,
    Login,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct LnUrl {
    pub kind: LnUrlKind,
    pub url: String,
}

impl FromStr for LnUrl {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let url = Url::parse(s)?;
        let kind = match url.scheme() {
            "lnurlc" => LnUrlKind::Channel,
            "lnurlp" => LnUrlKind::Pay,
            "lnurlw" => LnUrlKind::Withdraw,
            "keyauth" => LnUrlKind::Login,
            scheme => bail!("Invalid scheme `{scheme}` for LNURL LUD-17"),
        };
        let scheme = scheme_for(&url);
        let (_scheme, tail) = s
            .split_once(':')
            .ok_or(anyhow!("Valid URL must have `:`"))?;
        let url = format!("{scheme}:{tail}");
        Ok(LnUrl { kind, url })
    }
}

fn scheme_for(url: &Url) -> &'static str {
    match url.domain() {
        Some(domain) if domain.ends_with(".onion") => "http",
        _ => "https",
    }
}

#[derive(Debug)]
pub struct Bip21 {
    pub address: Option<OnchainAddress>,
    pub params: Vec<Bip21Param>,
}

#[derive(Debug, PartialEq, PartialOrd, Eq, Ord)]
pub enum Bip21Param {
    Amount(Sat),
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

pub fn parse_bip21(uri: &str) -> Result<Bip21> {
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
        Some(OnchainAddress::from_str(address)?)
    };

    Ok(Bip21 { address, params })
}

impl TryFrom<(&str, &str)> for Bip21Param {
    type Error = anyhow::Error;

    fn try_from((key, value): (&str, &str)) -> Result<Self, Self::Error> {
        Ok(match key {
            "amount" => {
                let amount = bitcoin::Amount::from_str_in(value, bitcoin::Denomination::Bitcoin)
                    .map_err(|e| anyhow!("Failed to amount decode param: {e}"))?;
                Self::Amount(amount.into())
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
    use crate::liquid_address::LiquidNetwork;
    use crate::types::Sat;
    use bitcoin::{AddressType, Network};

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
            DecodedData::LightningAddress(LightningAddress {
                username,
                domain,
                lnurl,
            }) => {
                assert_eq!(username, "example");
                assert_eq!(domain, "getalby.com");
                assert_eq!(lnurl.kind, LnUrlKind::Pay);
            }
            other => panic!("expected lightning address, got {other:?}"),
        }
    }

    #[test]
    fn decodes_lnurl() {
        let lnurl = "lnurl1dp68gurn8ghj7mrww4exctnxd9shg6npvchxxmmd9akxuatjdskhqcte8aek2umnd9hku0fj89jxxct989jrgve3xvmk2erzxpjx2decxp3kxv33xqckve3c8qmxxd3cvvuxxepnv3nrwe3hxvukzwp3xsex2v3cxejxgcnrxgukguq0868";

        match decode(lnurl).unwrap() {
            DecodedData::LnUrl(lnurl) => assert_eq!(lnurl.url, "https://lnurl.fiatjaf.com/lnurl-pay?session=29dcae9d43137edb0de780cc2101ff886c68c8cd3df7f739a8142e286ddbc29d"),
            other => panic!("expected LNURL, got {other:?}"),
        }
    }

    #[test]
    fn decodes_onchain_address_without_scheme() {
        let input = "bc1qztwy6xen3zdtt7z0vrgapmjtfz8acjkfp5fp7l";

        match decode(input).unwrap() {
            DecodedData::OnchainAddress(address) => {
                assert_eq!(
                    address.address,
                    "bc1qztwy6xen3zdtt7z0vrgapmjtfz8acjkfp5fp7l"
                );
                assert_eq!(address.address_type, Some(AddressType::P2wpkh));
                assert_eq!(address.valid_networks, vec![Network::Bitcoin]);
            }
            other => panic!("expected on-chain address, got {other:?}"),
        };
    }

    #[test]
    fn decodes_onchain_address_with_whitespace() {
        let input = " \t1BoatSLRHtKNngkdXEeobR76b53LETtpyT \n";

        match decode(input).unwrap() {
            DecodedData::OnchainAddress(address) => {
                assert_eq!(address.address, "1BoatSLRHtKNngkdXEeobR76b53LETtpyT");
                assert_eq!(address.address_type, Some(AddressType::P2pkh));
                assert_eq!(address.valid_networks, vec![Network::Bitcoin]);
            }
            other => panic!("expected on-chain address, got {other:?}"),
        };
    }

    #[test]
    fn decodes_bip21_uri_with_params() {
        let uri = "bitcoin:bc1qztwy6xen3zdtt7z0vrgapmjtfz8acjkfp5fp7l?amount=0.001&label=Donation&message=Thanks%20for%20your%20support";
        match decode(uri).unwrap() {
            DecodedData::Bip21(Bip21 {
                address: Some(address),
                params,
            }) => {
                assert_eq!(
                    address.address,
                    "bc1qztwy6xen3zdtt7z0vrgapmjtfz8acjkfp5fp7l"
                );
                assert_eq!(
                    params,
                    vec![
                        Bip21Param::Amount(Sat(100_000)),
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
            DecodedData::Bip353OrLightningAddress(hrn, lightning_address) => {
                assert_eq!(hrn.user(), "lnurl");
                assert_eq!(hrn.domain(), "bitcoin.org");
                assert_eq!(lightning_address.username, "lnurl");
                assert_eq!(lightning_address.domain, "bitcoin.org");
            }
            other => panic!("expected BIP-353 name or lightning address, got {other:?}"),
        }
    }

    #[test]
    fn decodes_liquid_address() {
        let input = "ex1q7gkeyjut0mrxc3j0kjlt7rmcnvsh0gt45d3fud";
        match decode(input).unwrap() {
            DecodedData::LiquidAddress(address) => {
                assert_eq!(address.address, input);
                assert_eq!(address.address_type, Some("p2wpkh".to_string()));
                assert_eq!(address.valid_networks, vec![LiquidNetwork::Liquid]);
                assert!(!address.is_confidential);
            }
            other => panic!("expected Liquid address, got {other:?}"),
        }
    }

    #[test]
    fn decodes_liquid_uri_with_params() {
        let uri = "liquidnetwork:ex1q7gkeyjut0mrxc3j0kjlt7rmcnvsh0gt45d3fud?amount=0.001&assetid=6f0279e9ed52f4f7b18016d875f794f6f4f08484f6a5f6f5f1f4f4f4f4f4f4f4&label=Donation&message=Thanks%20Liquid";
        match decode(uri).unwrap() {
            DecodedData::LiquidUri(liquid_uri) => {
                assert_eq!(liquid_uri.scheme, LiquidNetwork::Liquid);
                assert_eq!(liquid_uri.amount, Some("100,000 sats".to_string()));
                assert_eq!(
                    liquid_uri.asset_id,
                    Some(
                        "6f0279e9ed52f4f7b18016d875f794f6f4f08484f6a5f6f5f1f4f4f4f4f4f4f4"
                            .to_string()
                    )
                );
                assert_eq!(liquid_uri.label, Some("Donation".to_string()));
                assert_eq!(liquid_uri.message, Some("Thanks Liquid".to_string()));
                assert!(liquid_uri.unknown_params.is_empty());
                assert!(liquid_uri.address.is_some());
            }
            other => panic!("expected Liquid URI, got {other:?}"),
        }
    }
}
