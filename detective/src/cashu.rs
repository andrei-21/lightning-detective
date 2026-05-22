use anyhow::{anyhow, bail, ensure, Context, Result};
use base64::prelude::{Engine as _, BASE64_URL_SAFE, BASE64_URL_SAFE_NO_PAD};
use bech32::primitives::decode::CheckedHrpstring;
use bech32::{Bech32m, Hrp};
use bitcoin::hex::DisplayHex;
use serde::Deserialize;
use std::io::Cursor;

const NUT18_PREFIX_LEN: usize = 5;
const NUT26_HRP: &str = "creqb";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaymentRequestEncoding {
    Nut18,
    Nut26,
}

impl std::fmt::Display for PaymentRequestEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Nut18 => write!(f, "NUT-18 CBOR/base64"),
            Self::Nut26 => write!(f, "NUT-26 Bech32m/TLV"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaymentRequest {
    pub encoding: PaymentRequestEncoding,
    pub id: Option<String>,
    pub amount: Option<u64>,
    pub unit: Option<String>,
    pub single_use: Option<bool>,
    pub mints: Vec<String>,
    pub description: Option<String>,
    pub transports: Vec<Transport>,
    pub nut10: Option<Nut10Option>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transport {
    pub kind: String,
    pub target: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Nut10Option {
    pub kind: String,
    pub data: String,
    pub tags: Vec<Tag>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    pub key: String,
    pub values: Vec<String>,
}

pub fn is_payment_request(input: &str) -> bool {
    let lowercased = input.to_ascii_lowercase();
    (lowercased.starts_with("creqa") || lowercased.starts_with("creqb1"))
        && input.chars().all(is_payment_request_char)
}

fn is_payment_request_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '=')
}

pub fn parse_payment_request(input: &str) -> Result<PaymentRequest> {
    let lowercased = input.to_ascii_lowercase();
    if lowercased.starts_with("creqa") {
        parse_nut18(input)
    } else if lowercased.starts_with("creqb1") {
        parse_nut26(input)
    } else {
        bail!("input is not a Cashu payment request");
    }
}

fn parse_nut18(input: &str) -> Result<PaymentRequest> {
    let payload = input
        .get(NUT18_PREFIX_LEN..)
        .ok_or_else(|| anyhow!("missing NUT-18 payment request payload"))?;
    let cbor = BASE64_URL_SAFE
        .decode(payload)
        .or_else(|_| BASE64_URL_SAFE_NO_PAD.decode(payload))
        .context("failed to decode NUT-18 payment request base64")?;
    let request: Nut18PaymentRequest = ciborium::from_reader(Cursor::new(cbor))
        .context("failed to decode NUT-18 payment request CBOR")?;
    Ok(request.into_payment_request(PaymentRequestEncoding::Nut18))
}

fn parse_nut26(input: &str) -> Result<PaymentRequest> {
    let checked = CheckedHrpstring::new::<Bech32m>(input)
        .context("failed to decode NUT-26 payment request bech32m")?;
    ensure!(
        checked.hrp() == Hrp::parse_unchecked(NUT26_HRP),
        "invalid NUT-26 payment request HRP"
    );
    let payload = checked.byte_iter().collect::<Vec<_>>();
    let mut request = PaymentRequest {
        encoding: PaymentRequestEncoding::Nut26,
        id: None,
        amount: None,
        unit: None,
        single_use: None,
        mints: Vec::new(),
        description: None,
        transports: Vec::new(),
        nut10: None,
    };

    for field in parse_tlv_entries(&payload)? {
        match field.kind {
            0x01 => request.id = Some(utf8(field.value, "id")?),
            0x02 => request.amount = Some(parse_u64(field.value, "amount")?),
            0x03 => request.unit = Some(parse_unit(field.value)?),
            0x04 => request.single_use = Some(parse_bool(field.value, "single_use")?),
            0x05 => request.mints.push(utf8(field.value, "mint")?),
            0x06 => request.description = Some(utf8(field.value, "description")?),
            0x07 => request.transports.push(parse_transport_tlv(field.value)?),
            0x08 => request.nut10 = Some(parse_nut10_tlv(field.value)?),
            _ => {}
        }
    }

    Ok(request)
}

#[derive(Deserialize)]
struct Nut18PaymentRequest {
    #[serde(rename = "i")]
    id: Option<String>,
    #[serde(rename = "a")]
    amount: Option<u64>,
    #[serde(rename = "u")]
    unit: Option<String>,
    #[serde(rename = "s")]
    single_use: Option<bool>,
    #[serde(rename = "m", default)]
    mints: Vec<String>,
    #[serde(rename = "d")]
    description: Option<String>,
    #[serde(rename = "t", default)]
    transports: Vec<Nut18Transport>,
    #[serde(rename = "nut10")]
    nut10: Option<Nut18Nut10Option>,
}

impl Nut18PaymentRequest {
    fn into_payment_request(self, encoding: PaymentRequestEncoding) -> PaymentRequest {
        PaymentRequest {
            encoding,
            id: self.id,
            amount: self.amount,
            unit: self.unit,
            single_use: self.single_use,
            mints: self.mints,
            description: self.description,
            transports: self
                .transports
                .into_iter()
                .map(Nut18Transport::into_transport)
                .collect(),
            nut10: self.nut10.map(Nut18Nut10Option::into_nut10),
        }
    }
}

#[derive(Deserialize)]
struct Nut18Transport {
    #[serde(rename = "t")]
    kind: String,
    #[serde(rename = "a")]
    target: String,
    #[serde(rename = "g", default)]
    tags: Vec<Vec<String>>,
}

impl Nut18Transport {
    fn into_transport(self) -> Transport {
        Transport {
            kind: self.kind,
            target: self.target,
            tags: tags_from_nested_strings(self.tags),
        }
    }
}

#[derive(Deserialize)]
struct Nut18Nut10Option {
    #[serde(rename = "k")]
    kind: String,
    #[serde(rename = "d")]
    data: String,
    #[serde(rename = "t", default)]
    tags: Vec<Vec<String>>,
}

impl Nut18Nut10Option {
    fn into_nut10(self) -> Nut10Option {
        Nut10Option {
            kind: self.kind,
            data: self.data,
            tags: tags_from_nested_strings(self.tags),
        }
    }
}

fn tags_from_nested_strings(tags: Vec<Vec<String>>) -> Vec<Tag> {
    tags.into_iter()
        .filter_map(|mut values| {
            if values.is_empty() {
                return None;
            }
            let key = values.remove(0);
            Some(Tag { key, values })
        })
        .collect()
}

struct TlvEntry<'a> {
    kind: u8,
    value: &'a [u8],
}

fn parse_tlv_entries(mut input: &[u8]) -> Result<Vec<TlvEntry<'_>>> {
    let mut fields = Vec::new();
    while !input.is_empty() {
        ensure!(input.len() >= 3, "truncated TLV entry");
        let kind = input[0];
        let len = u16::from_be_bytes([input[1], input[2]]) as usize;
        input = &input[3..];
        ensure!(input.len() >= len, "truncated TLV value");
        let (value, rest) = input.split_at(len);
        fields.push(TlvEntry { kind, value });
        input = rest;
    }
    Ok(fields)
}

fn parse_transport_tlv(input: &[u8]) -> Result<Transport> {
    let mut kind = None;
    let mut target = None;
    let mut tags = Vec::new();

    for field in parse_tlv_entries(input)? {
        match field.kind {
            0x01 => kind = Some(parse_u8(field.value, "transport kind")?),
            0x02 => target = Some(field.value.to_vec()),
            0x03 => tags.push(parse_tag_tuple(field.value)?),
            _ => {}
        }
    }

    let kind = kind.ok_or_else(|| anyhow!("transport missing kind"))?;
    let target = target.ok_or_else(|| anyhow!("transport missing target"))?;
    let (kind, target) = match kind {
        0x00 => ("nostr".to_string(), target.as_hex().to_string()),
        0x01 => ("post".to_string(), utf8(target, "transport target")?),
        other => (format!("unknown({other})"), target.as_hex().to_string()),
    };

    Ok(Transport { kind, target, tags })
}

fn parse_nut10_tlv(input: &[u8]) -> Result<Nut10Option> {
    let mut kind = None;
    let mut data = None;
    let mut tags = Vec::new();

    for field in parse_tlv_entries(input)? {
        match field.kind {
            0x01 => kind = Some(parse_u8(field.value, "NUT-10 kind")?),
            0x02 => data = Some(field.value.to_vec()),
            0x03 => tags.push(parse_tag_tuple(field.value)?),
            _ => {}
        }
    }

    let kind = match kind.ok_or_else(|| anyhow!("NUT-10 option missing kind"))? {
        0x00 => "P2PK".to_string(),
        0x01 => "HTLC".to_string(),
        other => format!("unknown({other})"),
    };
    let data = bytes_to_display_string(
        &data.ok_or_else(|| anyhow!("NUT-10 option missing data"))?,
        "NUT-10 data",
    )?;

    Ok(Nut10Option { kind, data, tags })
}

fn parse_tag_tuple(mut input: &[u8]) -> Result<Tag> {
    ensure!(!input.is_empty(), "empty tag tuple");
    let key_len = input[0] as usize;
    input = &input[1..];
    ensure!(input.len() >= key_len, "truncated tag tuple key");
    let (key, rest) = input.split_at(key_len);
    input = rest;

    let mut values = Vec::new();
    while !input.is_empty() {
        let value_len = input[0] as usize;
        input = &input[1..];
        ensure!(input.len() >= value_len, "truncated tag tuple value");
        let (value, rest) = input.split_at(value_len);
        values.push(utf8(value, "tag tuple value")?);
        input = rest;
    }

    Ok(Tag {
        key: utf8(key, "tag tuple key")?,
        values,
    })
}

fn parse_u8(input: &[u8], name: &str) -> Result<u8> {
    ensure!(input.len() == 1, "{name} must be one byte");
    Ok(input[0])
}

fn parse_u64(input: &[u8], name: &str) -> Result<u64> {
    ensure!(!input.is_empty(), "{name} must not be empty");
    ensure!(input.len() <= 8, "{name} must fit in u64");
    let mut bytes = [0_u8; 8];
    bytes[8 - input.len()..].copy_from_slice(input);
    Ok(u64::from_be_bytes(bytes))
}

fn parse_bool(input: &[u8], name: &str) -> Result<bool> {
    match parse_u8(input, name)? {
        0 => Ok(false),
        1 => Ok(true),
        other => bail!("{name} must be 0 or 1, got {other}"),
    }
}

fn parse_unit(input: &[u8]) -> Result<String> {
    match input {
        [0x00] => Ok("sat".to_string()),
        bytes => bytes_to_display_string(bytes, "unit"),
    }
}

fn utf8(input: impl AsRef<[u8]>, name: &str) -> Result<String> {
    String::from_utf8(input.as_ref().to_vec()).with_context(|| format!("{name} is not valid UTF-8"))
}

fn bytes_to_display_string(input: &[u8], name: &str) -> Result<String> {
    std::str::from_utf8(input)
        .map(str::to_string)
        .or_else(|_| Ok::<_, anyhow::Error>(input.as_hex().to_string()))
        .with_context(|| format!("failed to decode {name}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bech32::Hrp;

    const NUT18_SPEC_EXAMPLE: &str = "creqApWF0gaNhdGVub3N0cmFheKlucHJvZmlsZTFxeTI4d3VtbjhnaGo3dW45ZDNzaGp0bnl2OWtoMnVld2Q5aHN6OW1od2RlbjV0ZTB3ZmprY2N0ZTljdXJ4dmVuOWVlaHFjdHJ2NWhzenJ0aHdkZW41dGUwZGVoaHh0bnZkYWtxcWd5ZGFxeTdjdXJrNDM5eWtwdGt5c3Y3dWRoZGh1NjhzdWNtMjk1YWtxZWZkZWhrZjBkNDk1Y3d1bmw1YWeBgmFuYjE3YWloYjdhOTAxNzZhYQphdWNzYXRhbYF4Imh0dHBzOi8vbm9mZWVzLnRlc3RudXQuY2FzaHUuc3BhY2U=";

    #[test]
    fn detects_only_payment_requests_with_allowed_characters() {
        assert!(is_payment_request(NUT18_SPEC_EXAMPLE));
        assert!(is_payment_request("CREQB1QYQQWER9D4HNZV3NQGQQSQQQQQQQQQQRAQPSQQGQQSQQZQG9QQVXSAR5WPEN5TE0D45KUAPWV4UXZMTSD3JJUCM0D5RQQRJRDANXVET9YPCXZ7TDV4H8GXHR3TQ"));
        assert!(!is_payment_request("creqa@getalby.com"));
        assert!(!is_payment_request("creqb1@getalby.com"));
    }

    #[test]
    fn decodes_nut18_spec_example() {
        let request = parse_payment_request(NUT18_SPEC_EXAMPLE).unwrap();

        assert_eq!(request.encoding, PaymentRequestEncoding::Nut18);
        assert_eq!(request.id.as_deref(), Some("b7a90176"));
        assert_eq!(request.amount, Some(10));
        assert_eq!(request.unit.as_deref(), Some("sat"));
        assert_eq!(
            request.mints,
            vec!["https://nofees.testnut.cashu.space".to_string()]
        );
        assert_eq!(request.transports[0].kind, "nostr");
        assert_eq!(request.transports[0].tags[0].key, "n");
        assert_eq!(request.transports[0].tags[0].values, vec!["17"]);
    }

    #[test]
    fn decodes_unpadded_nut18() {
        let request = parse_payment_request(NUT18_SPEC_EXAMPLE.trim_end_matches('=')).unwrap();

        assert_eq!(request.id.as_deref(), Some("b7a90176"));
    }

    #[test]
    fn rejects_malformed_nut18() {
        assert!(parse_payment_request("creqAnot-valid-cbor").is_err());
    }

    #[test]
    fn decodes_nut26_spec_example() {
        let request = parse_payment_request("CREQB1QYQQWER9D4HNZV3NQGQQSQQQQQQQQQQRAQPSQQGQQSQQZQG9QQVXSAR5WPEN5TE0D45KUAPWV4UXZMTSD3JJUCM0D5RQQRJRDANXVET9YPCXZ7TDV4H8GXHR3TQ").unwrap();

        assert_eq!(request.encoding, PaymentRequestEncoding::Nut26);
        assert_eq!(request.id.as_deref(), Some("demo123"));
        assert_eq!(request.amount, Some(1000));
        assert_eq!(request.unit.as_deref(), Some("sat"));
        assert_eq!(request.single_use, Some(true));
        assert_eq!(request.mints, vec!["https://mint.example.com"]);
        assert_eq!(request.description.as_deref(), Some("Coffee payment"));
    }

    #[test]
    fn decodes_lowercase_nut26() {
        let input = "CREQB1QYQQWER9D4HNZV3NQGQQSQQQQQQQQQQRAQPSQQGQQSQQZQG9QQVXSAR5WPEN5TE0D45KUAPWV4UXZMTSD3JJUCM0D5RQQRJRDANXVET9YPCXZ7TDV4H8GXHR3TQ";
        assert!(parse_payment_request(&input.to_ascii_lowercase()).is_ok());
    }

    #[test]
    fn ignores_unknown_nut26_tlv_tags() {
        let mut payload = Vec::new();
        encode_tlv(0x99, b"ignored", &mut payload);
        encode_tlv(0x01, b"id", &mut payload);
        let encoded = bech32::encode::<Bech32m>(Hrp::parse_unchecked(NUT26_HRP), &payload).unwrap();

        let request = parse_payment_request(&encoded).unwrap();

        assert_eq!(request.id.as_deref(), Some("id"));
    }

    #[test]
    fn rejects_truncated_nut26_tlv() {
        let payload = [0x01, 0x00, 0x02, b'i'];
        let encoded = bech32::encode::<Bech32m>(Hrp::parse_unchecked(NUT26_HRP), &payload).unwrap();

        assert!(parse_payment_request(&encoded).is_err());
    }

    #[test]
    fn rejects_empty_nut26_amount() {
        let payload = [0x02, 0x00, 0x00];
        let encoded = bech32::encode::<Bech32m>(Hrp::parse_unchecked(NUT26_HRP), &payload).unwrap();

        assert!(parse_payment_request(&encoded).is_err());
    }

    #[test]
    fn rejects_wrong_nut26_hrp() {
        let encoded = bech32::encode::<Bech32m>(Hrp::parse_unchecked("wrong"), &[]).unwrap();

        assert!(parse_payment_request(&encoded).is_err());
    }

    fn encode_tlv(kind: u8, value: &[u8], payload: &mut Vec<u8>) {
        payload.push(kind);
        payload.extend_from_slice(&(value.len() as u16).to_be_bytes());
        payload.extend_from_slice(value);
    }
}
