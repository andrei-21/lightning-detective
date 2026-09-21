use anyhow::{anyhow, bail, ensure, Context, Error, Result};
use base64::{engine::general_purpose, Engine as _};
use bitcoin::secp256k1::XOnlyPublicKey;
use lightning_invoice::Bolt11Invoice;
use lnurl::decode_ln_url_response_from_json;
use reqwest::{Method, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use std::str::FromStr;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

use crate::decoder::{LnUrl, LnUrlKind};
use crate::types::{Msat, MsatRange};

#[derive(Debug, Clone)]
pub struct LightningAddress {
    pub username: String,
    pub domain: String,
    pub lnurl: LnUrl,
}

impl FromStr for LightningAddress {
    type Err = Error;
    fn from_str(input: &str) -> Result<Self> {
        let (username, domain) = input
            .split_once('@')
            .ok_or(anyhow!("Lightning address must have `@`"))?;
        ensure!(
            is_valid_lightning_address_username(username),
            "Invalid Lightning address username"
        );
        ensure!(is_domain(domain), "Invalid Lightning address domain");
        let lnurl = LnUrl::from_str(&format!("lnurlp://{domain}/.well-known/lnurlp/{username}"))?;
        Ok(Self {
            username: username.to_string(),
            domain: domain.to_string(),
            lnurl,
        })
    }
}

#[derive(Debug, Clone)]
pub enum LnUrlResponse {
    Pay(PayResponse),
    Withdraw(WithdrawalResponse),
    Channel(ChannelResponse),
}

impl TryFrom<lnurl::LnUrlResponse> for LnUrlResponse {
    type Error = Error;

    fn try_from(response: lnurl::LnUrlResponse) -> Result<Self> {
        Ok(match response {
            lnurl::LnUrlResponse::LnUrlPayResponse(response) => {
                LnUrlResponse::Pay(response.try_into()?)
            }
            lnurl::LnUrlResponse::LnUrlWithdrawResponse(response) => {
                LnUrlResponse::Withdraw(response.try_into()?)
            }
            lnurl::LnUrlResponse::LnUrlChannelResponse(response) => {
                LnUrlResponse::Channel(response.try_into()?)
            }
        })
    }
}

#[derive(Debug, Clone)]
pub enum Image {
    Png(Vec<u8>),
    Jpeg(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct PayResponse {
    pub sendable_amount: MsatRange,
    pub description: String,
    pub long_description: Option<String>,
    pub image: Option<Image>,
    pub comment_allowed: Option<u32>,
    pub callback: String,
    pub zap: ZapSupport,
    pub metadata: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct ZapSupport {
    pub allowed: bool,
    pub nostr_pubkey: Option<String>,
    pub nostr_pubkey_valid: bool,
}

impl ZapSupport {
    fn unavailable() -> Self {
        Self {
            allowed: false,
            nostr_pubkey: None,
            nostr_pubkey_valid: false,
        }
    }

    pub fn is_usable(&self) -> bool {
        self.allowed && self.nostr_pubkey_valid
    }

    fn from_raw(allows_nostr: Option<bool>, nostr_pubkey: Option<String>) -> Self {
        let allowed = allows_nostr.unwrap_or(false);
        let nostr_pubkey_valid = nostr_pubkey
            .as_deref()
            .map(is_valid_nostr_pubkey)
            .unwrap_or(false);

        if !allowed {
            return Self::unavailable();
        }

        Self {
            allowed,
            nostr_pubkey,
            nostr_pubkey_valid,
        }
    }

    fn from_typed(allows_nostr: Option<bool>, nostr_pubkey: Option<XOnlyPublicKey>) -> Self {
        let allowed = allows_nostr.unwrap_or(false);
        if !allowed {
            return Self::unavailable();
        }
        let nostr_pubkey_valid = nostr_pubkey.is_some();

        Self {
            allowed,
            nostr_pubkey: nostr_pubkey.map(|pubkey| pubkey.to_string()),
            nostr_pubkey_valid,
        }
    }
}

#[derive(Debug, Clone)]
pub struct WithdrawalResponse {
    pub amount: MsatRange,
    pub default_description: String,
    pub callback: String,
    pub k1: String,
}

#[derive(Debug, Clone)]
pub struct ChannelResponse {
    pub uri: String,
    pub callback: String,
    pub k1: String,
}

impl TryFrom<lnurl::pay::PayResponse> for PayResponse {
    type Error = Error;

    fn try_from(pay: lnurl::pay::PayResponse) -> Result<Self> {
        Self::try_from_parts(
            pay.min_sendable,
            pay.max_sendable,
            pay.metadata,
            pay.comment_allowed,
            pay.callback,
            ZapSupport::from_typed(pay.allows_nostr, pay.nostr_pubkey),
        )
    }
}

impl PayResponse {
    fn try_from_parts(
        min_sendable: u64,
        max_sendable: u64,
        metadata_raw: String,
        comment_allowed: Option<u32>,
        callback: String,
        zap: ZapSupport,
    ) -> Result<Self> {
        let parsed: Value =
            serde_json::from_str(&metadata_raw).context("LNURL metadata is not valid JSON")?;
        let entries = parsed
            .as_array()
            .ok_or(anyhow!("LNURL metadata is not a JSON array"))?;

        let mut description: Option<String> = None;
        let mut long_description: Option<String> = None;
        let mut image = None;
        let mut metadata = Vec::new();

        for (index, entry) in entries.iter().enumerate() {
            let array = entry
                .as_array()
                .ok_or(anyhow!("LNURL metadata entry #{index} is not an array"))?;
            let (key, value) = match &array[..] {
                [key, value] => (key, value),
                _ => bail!("LNURL metadata entry #{index} must have exactly two elements"),
            };
            let key = key.as_str().ok_or(anyhow!(
                "LNURL metadata entry #{index} type is not a string"
            ))?;
            let value = value.as_str().ok_or(anyhow!(
                "LNURL metadata entry #{index} value is not a string"
            ))?;

            match key {
                "text/plain" => {
                    ensure!(
                        !value.is_empty(),
                        "LNURL metadata text/plain value must not be empty"
                    );
                    ensure!(
                        description.is_none(),
                        "LNURL metadata must have no more than one text/plain value"
                    );
                    description = Some(value.to_string());
                }
                "text/long-desc" | "text/longdesc" => {
                    ensure!(
                        !value.is_empty(),
                        "LNURL metadata text/long-desc value must not be empty"
                    );
                    long_description = Some(value.to_string());
                }
                "image/png;base64" => {
                    let bytes = decode_base64(value, "image/png")?;
                    ensure!(
                        image.is_none(),
                        "LNURL metadata must have no more than one image/png;base64 or image/jpeg;base64 value"
                    );
                    image = Some(Image::Png(bytes));
                }
                "image/jpeg;base64" => {
                    let bytes = decode_base64(value, "image/jpeg")?;
                    ensure!(
                        image.is_none(),
                        "LNURL metadata must have no more than one image/png;base64 or image/jpeg;base64 value"
                    );
                    image = Some(Image::Jpeg(bytes));
                }
                key => metadata.push((key.to_string(), value.to_string())),
            }
        }

        let description = description.ok_or(anyhow!(
            "LNURL metadata is missing required text/plain entry"
        ))?;

        // TODO: Validate amounts.
        Ok(Self {
            description,
            long_description,
            sendable_amount: MsatRange::Between(Msat(min_sendable), Msat(max_sendable)),
            image,
            comment_allowed,
            callback,
            zap,
            metadata,
        })
    }
}

impl TryFrom<lnurl::withdraw::WithdrawalResponse> for WithdrawalResponse {
    type Error = Error;

    fn try_from(withdraw: lnurl::withdraw::WithdrawalResponse) -> Result<Self> {
        ensure!(
            matches!(withdraw.tag, lnurl::Tag::WithdrawRequest),
            "LNURL withdraw tag must be withdrawRequest"
        );
        ensure!(
            !withdraw.default_description.is_empty(),
            "LNURL withdraw defaultDescription must not be empty"
        );
        ensure!(
            !withdraw.callback.is_empty(),
            "LNURL withdraw callback must not be empty"
        );
        ensure!(
            !withdraw.k1.is_empty(),
            "LNURL withdraw k1 must not be empty"
        );

        let min_withdrawable = withdraw.min_withdrawable.unwrap_or(0);
        ensure!(
            min_withdrawable <= withdraw.max_withdrawable,
            "LNURL withdraw maxWithdrawable must be greater than or equal to minWithdrawable"
        );

        Ok(Self {
            amount: MsatRange::Between(Msat(min_withdrawable), Msat(withdraw.max_withdrawable)),
            default_description: withdraw.default_description,
            callback: withdraw.callback,
            k1: withdraw.k1,
        })
    }
}

impl TryFrom<lnurl::channel::ChannelResponse> for ChannelResponse {
    type Error = Error;

    fn try_from(_channel: lnurl::channel::ChannelResponse) -> Result<Self> {
        todo!()
    }
}

fn decode_base64(value: &str, label: &str) -> Result<Vec<u8>> {
    let bytes = general_purpose::STANDARD
        .decode(value.as_bytes())
        .context(format!("LNURL metadata {label} value is not valid base64"))?;
    Ok(bytes)
}

#[derive(Debug)]
pub enum JsonRpcEvent<R> {
    Requesting(Method, String),
    ResponseReceived(StatusCode),
    ResponseBodyReceived(String),
    JsonParsed(Value),
    Result(Result<R>),
}

pub fn resolve_lnurl(lnurl: LnUrl) -> impl Stream<Item = JsonRpcEvent<LnUrlResponse>> {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(async move {
        let result = resolve_lnurl_impl(lnurl, tx.clone()).await;
        let _ = tx.send(JsonRpcEvent::Result(result)).await;
    });
    ReceiverStream::new(rx)
}

pub fn request_invoice(
    callback: String,
    amount: Msat,
    comment: Option<String>,
) -> impl Stream<Item = JsonRpcEvent<String>> {
    let delimiter = if callback.contains('?') { '&' } else { '?' };
    let comment = match comment {
        Some(comment) => format!("&comment={}", urlencoding::encode(&comment)),
        None => String::new(),
    };
    let url = format!("{callback}{delimiter}amount={}{comment}", amount.0);
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(async move {
        let result = request_invoice_impl(url, amount, tx.clone()).await;
        let _ = tx.send(JsonRpcEvent::Result(result)).await;
    });
    ReceiverStream::new(rx)
}

async fn request_invoice_impl(
    url: String,
    requested_amount: Msat,
    events: mpsc::Sender<JsonRpcEvent<String>>,
) -> Result<String> {
    let method = Method::GET;
    events
        .send(JsonRpcEvent::Requesting(method.clone(), url.clone()))
        .await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let response = client.request(method, url).send().await?;
    let status = response.status();
    events.send(JsonRpcEvent::ResponseReceived(status)).await?;

    let body = response.text().await?;
    events
        .send(JsonRpcEvent::ResponseBodyReceived(body.clone()))
        .await?;

    ensure!(status.is_success(), "HTTP status is not success");
    let json: Value = serde_json::from_str(&body)?;
    events.send(JsonRpcEvent::JsonParsed(json.clone())).await?;
    let invoice_response: RequestInvoiceResponse =
        serde_json::from_value(json).context("LNURL invoice response is malformed")?;
    verify_invoice_amount(&invoice_response.pr, requested_amount)?;
    Ok(invoice_response.pr)
}

fn verify_invoice_amount(invoice: &str, requested_amount: Msat) -> Result<()> {
    let invoice = invoice
        .parse::<Bolt11Invoice>()
        .context("LNURL callback returned an invalid BOLT11 invoice")?;

    match invoice.amount_milli_satoshis() {
        Some(invoice_amount) if invoice_amount == requested_amount.0 => Ok(()),
        Some(invoice_amount) => bail!(
            "LNURL invoice amount mismatch: requested {} msat, invoice contains {invoice_amount} msat",
            requested_amount.0
        ),
        None => bail!("LNURL callback returned an amountless BOLT11 invoice"),
    }
}

async fn resolve_lnurl_impl(
    lnurl: LnUrl,
    events: mpsc::Sender<JsonRpcEvent<LnUrlResponse>>,
) -> Result<LnUrlResponse> {
    let method = Method::GET;
    let request_url = lnurl.url.clone();
    events
        .send(JsonRpcEvent::Requesting(
            method.clone(),
            request_url.clone(),
        ))
        .await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let response = client.request(method, request_url.clone()).send().await?;
    let status = response.status();
    events.send(JsonRpcEvent::ResponseReceived(status)).await?;

    let body = response.text().await?;
    events
        .send(JsonRpcEvent::ResponseBodyReceived(body.clone()))
        .await?;

    ensure!(status.is_success(), "HTTP status is not success");
    let json: Value = serde_json::from_str(&body)?;
    events.send(JsonRpcEvent::JsonParsed(json.clone())).await?;

    let actual = response_kind_from_json(&json)?;

    if let Some(expected) = expected_response_kind(&lnurl.kind) {
        ensure!(
            actual == expected,
            "LNURL kind mismatch: expected {expected:?}, got {actual:?}"
        );
    }

    if actual == LnUrlKind::Pay {
        return Ok(LnUrlResponse::Pay(
            LenientPayResponse::try_from(json)?.try_into()?,
        ));
    }

    match decode_ln_url_response_from_json(json).map_err(Error::from)? {
        lnurl::LnUrlResponse::LnUrlPayResponse(_) => {
            unreachable!("pay responses are handled above")
        }
        lnurl::LnUrlResponse::LnUrlWithdrawResponse(response) => {
            Ok(LnUrlResponse::Withdraw(response.try_into()?))
        }
        lnurl::LnUrlResponse::LnUrlChannelResponse(response) => {
            Ok(LnUrlResponse::Channel(response.try_into()?))
        }
    }
}

#[derive(Deserialize)]
struct RequestInvoiceResponse {
    pr: String,
}

fn response_kind_from_json(json: &Value) -> Result<LnUrlKind> {
    let tag = json
        .get("tag")
        .and_then(Value::as_str)
        .ok_or(anyhow!("LNURL response is missing tag"))?;
    Ok(match tag {
        "payRequest" => LnUrlKind::Pay,
        "withdrawRequest" => LnUrlKind::Withdraw,
        "channelRequest" => LnUrlKind::Channel,
        tag => bail!("Unknown LNURL response tag `{tag}`"),
    })
}

fn expected_response_kind(kind: &LnUrlKind) -> Option<LnUrlKind> {
    match kind {
        LnUrlKind::Pay => Some(LnUrlKind::Pay),
        LnUrlKind::Withdraw => Some(LnUrlKind::Withdraw),
        LnUrlKind::Channel => Some(LnUrlKind::Channel),
        LnUrlKind::Login => Some(LnUrlKind::Login),
        LnUrlKind::Unknown => None,
    }
}

fn is_valid_lightning_address_username(username: &str) -> bool {
    // TODO: Support + in lightning addresses.
    username
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_numeric() || ['-', '_', '.'].contains(&c))
}

fn is_domain(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 253
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
        && !s.starts_with('-')
        && !s.ends_with('-')
        && !s.starts_with('.')
        && !s.ends_with('.')
        && s.split('.').all(|l| !l.is_empty() && l.len() <= 63)
}

fn is_valid_nostr_pubkey(value: &str) -> bool {
    value.parse::<XOnlyPublicKey>().is_ok()
}

#[derive(Deserialize)]
struct LenientPayResponse {
    callback: String,
    #[serde(rename = "maxSendable")]
    max_sendable: u64,
    #[serde(rename = "minSendable")]
    min_sendable: u64,
    metadata: String,
    #[serde(rename = "commentAllowed")]
    comment_allowed: Option<u32>,
    #[serde(rename = "allowsNostr")]
    allows_nostr: Option<bool>,
    #[serde(rename = "nostrPubkey")]
    nostr_pubkey: Option<String>,
}

impl TryFrom<Value> for LenientPayResponse {
    type Error = Error;

    fn try_from(value: Value) -> Result<Self> {
        serde_json::from_value(value).context("LNURL pay response is malformed")
    }
}

impl TryFrom<LenientPayResponse> for PayResponse {
    type Error = Error;

    fn try_from(pay: LenientPayResponse) -> Result<Self> {
        Self::try_from_parts(
            pay.min_sendable,
            pay.max_sendable,
            pay.metadata,
            pay.comment_allowed,
            pay.callback,
            ZapSupport::from_raw(pay.allows_nostr, pay.nostr_pubkey),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_NOSTR_PUBKEY: &str =
        "9630f464cca6a5147aa8a35f0bcdd3ce485324e732fd39e09233b1d848238f31";
    const INVOICE_250_000_000_MSAT: &str =
        "lnbc2500u1pvjluezsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygspp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdpquwpc4curk03c9wlrswe78q4eyqc7d8d0xqzpu9qrsgqhtjpauu9ur7fw2thcl4y9vfvh4m9wlfyz2gem29g5ghe2aak2pm3ps8fdhtceqsaagty2vph7utlgj48u0ged6a337aewvraedendscp573dxr";
    const AMOUNTLESS_INVOICE: &str =
        "lnbc1pvjluezsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygspp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdpl2pkx2ctnv5sxxmmwwd5kgetjypeh2ursdae8g6twvus8g6rfwvs8qun0dfjkxaq9qrsgq357wnc5r2ueh7ck6q93dj32dlqnls087fxdwk8qakdyafkq3yap9us6v52vjjsrvywa6rt52cm9r9zqt8r2t7mlcwspyetp5h2tztugp9lfyql";
    const INVOICE_967_878_534_MSAT: &str =
        "lnbc9678785340p1pwmna7lpp5gc3xfm08u9qy06djf8dfflhugl6p7lgza6dsjxq454gxhj9t7a0sd8dgfkx7cmtwd68yetpd5s9xar0wfjn5gpc8qhrsdfq24f5ggrxdaezqsnvda3kkum5wfjkzmfqf3jkgem9wgsyuctwdus9xgrcyqcjcgpzgfskx6eqf9hzqnteypzxz7fzypfhg6trddjhygrcyqezcgpzfysywmm5ypxxjemgw3hxjmn8yptk7untd9hxwg3q2d6xjcmtv4ezq7pqxgsxzmnyyqcjqmt0wfjjq6t5v4khxsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygsxqyjw5qcqp2rzjq0gxwkzc8w6323m55m4jyxcjwmy7stt9hwkwe2qxmy8zpsgg7jcuwz87fcqqeuqqqyqqqqlgqqqqn3qq9q9qrsgqrvgkpnmps664wgkp43l22qsgdw4ve24aca4nymnxddlnp8vh9v2sdxlu5ywdxefsfvm0fq3sesf08uf6q9a2ke0hc9j6z6wlxg5z5kqpu2v9wz";

    #[test]
    fn accepts_invoice_with_requested_amount() {
        verify_invoice_amount(INVOICE_250_000_000_MSAT, Msat(250_000_000)).unwrap();
    }

    #[test]
    fn rejects_invoice_below_requested_amount() {
        let error =
            verify_invoice_amount(INVOICE_250_000_000_MSAT, Msat(2_000_000_000)).unwrap_err();

        assert!(error.to_string().contains("amount mismatch"));
    }

    #[test]
    fn rejects_invoice_above_requested_amount() {
        let error = verify_invoice_amount(INVOICE_250_000_000_MSAT, Msat(1_000)).unwrap_err();

        assert!(error.to_string().contains("amount mismatch"));
    }

    #[test]
    fn rejects_amountless_invoice() {
        let error = verify_invoice_amount(AMOUNTLESS_INVOICE, Msat(1_000)).unwrap_err();

        assert!(error.to_string().contains("amountless"));
    }

    #[test]
    fn rejects_malformed_invoice() {
        let error = verify_invoice_amount("not-an-invoice", Msat(1_000)).unwrap_err();

        assert!(error.to_string().contains("invalid BOLT11 invoice"));
    }

    #[test]
    fn accepts_invoice_with_sub_satoshi_amount() {
        verify_invoice_amount(INVOICE_967_878_534_MSAT, Msat(967_878_534)).unwrap();
    }

    #[test]
    fn parses_missing_zap_support_as_unavailable() {
        let zap = ZapSupport::from_raw(None, None);

        assert!(!zap.allowed);
        assert!(!zap.is_usable());
        assert!(zap.nostr_pubkey.is_none());
    }

    #[test]
    fn parses_valid_zap_support() {
        let zap = ZapSupport::from_raw(Some(true), Some(VALID_NOSTR_PUBKEY.to_string()));

        assert!(zap.allowed);
        assert!(zap.is_usable());
        assert_eq!(zap.nostr_pubkey.as_deref(), Some(VALID_NOSTR_PUBKEY));
    }

    #[test]
    fn marks_invalid_zap_pubkey_as_not_usable() {
        let zap = ZapSupport::from_raw(Some(true), Some("not-a-pubkey".to_string()));

        assert!(zap.allowed);
        assert!(!zap.is_usable());
        assert_eq!(zap.nostr_pubkey.as_deref(), Some("not-a-pubkey"));
    }

    #[test]
    fn preserves_typed_zap_support() {
        let pubkey = VALID_NOSTR_PUBKEY.parse::<XOnlyPublicKey>().unwrap();

        let zap = ZapSupport::from_typed(Some(true), Some(pubkey));

        assert!(zap.allowed);
        assert!(zap.is_usable());
        assert_eq!(zap.nostr_pubkey.as_deref(), Some(VALID_NOSTR_PUBKEY));
    }

    #[test]
    fn try_from_typed_pay_response_preserves_zap_support() {
        let pubkey = VALID_NOSTR_PUBKEY.parse::<XOnlyPublicKey>().unwrap();
        let pay = lnurl::pay::PayResponse {
            callback: "https://example.com/callback".to_string(),
            max_sendable: 1_000_000,
            min_sendable: 1_000,
            tag: lnurl::Tag::PayRequest,
            metadata: "[[\"text/plain\",\"Zap demo\"]]".to_string(),
            comment_allowed: None,
            allows_nostr: Some(true),
            nostr_pubkey: Some(pubkey),
        };

        let pay = PayResponse::try_from(pay).unwrap();

        assert!(pay.zap.allowed);
        assert!(pay.zap.is_usable());
        assert_eq!(pay.zap.nostr_pubkey.as_deref(), Some(VALID_NOSTR_PUBKEY));
    }

    #[test]
    fn converts_lenient_pay_response_with_invalid_zap_pubkey() {
        let json = serde_json::json!({
            "tag": "payRequest",
            "callback": "https://example.com/callback",
            "minSendable": 1000,
            "maxSendable": 1000000,
            "metadata": "[[\"text/plain\",\"Zap demo\"]]",
            "allowsNostr": true,
            "nostrPubkey": "not-a-pubkey"
        });

        let pay = PayResponse::try_from(LenientPayResponse::try_from(json).unwrap()).unwrap();

        assert_eq!(pay.description, "Zap demo");
        assert!(pay.zap.allowed);
        assert!(!pay.zap.is_usable());
        assert_eq!(pay.zap.nostr_pubkey.as_deref(), Some("not-a-pubkey"));
    }
}
