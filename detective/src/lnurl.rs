use anyhow::{anyhow, bail, ensure, Context, Error, Result};
use base64::{engine::general_purpose, Engine as _};
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
    pub metadata: Vec<(String, String)>,
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
        let parsed: Value =
            serde_json::from_str(&pay.metadata).context("LNURL metadata is not valid JSON")?;
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
            sendable_amount: MsatRange::Between(Msat(pay.min_sendable), Msat(pay.max_sendable)),
            image,
            comment_allowed: pay.comment_allowed,
            callback: pay.callback,
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
        let result = request_invoice_impl(url, tx.clone()).await;
        let _ = tx.send(JsonRpcEvent::Result(result)).await;
    });
    ReceiverStream::new(rx)
}

async fn request_invoice_impl(
    url: String,
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
    Ok(invoice_response.pr)
}

async fn resolve_lnurl_impl(
    lnurl: LnUrl,
    events: mpsc::Sender<JsonRpcEvent<LnUrlResponse>>,
) -> Result<LnUrlResponse> {
    let method = Method::GET;
    events
        .send(JsonRpcEvent::Requesting(method.clone(), lnurl.url.clone()))
        .await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let response = client.request(method, lnurl.url).send().await?;
    let status = response.status();
    events.send(JsonRpcEvent::ResponseReceived(status)).await?;

    let body = response.text().await?;
    events
        .send(JsonRpcEvent::ResponseBodyReceived(body.clone()))
        .await?;

    ensure!(status.is_success(), "HTTP status is not success");
    let json: Value = serde_json::from_str(&body)?;
    events.send(JsonRpcEvent::JsonParsed(json.clone())).await?;

    let response = decode_ln_url_response_from_json(json).map_err(Error::from)?;

    if let Some(expected) = expected_response_kind(&lnurl.kind) {
        let actual = response_kind(&response);
        ensure!(
            actual == expected,
            "LNURL kind mismatch: expected {expected:?}, got {actual:?}"
        );
    }
    response.try_into()
}

#[derive(Deserialize)]
struct RequestInvoiceResponse {
    pr: String,
}

fn response_kind(response: &lnurl::LnUrlResponse) -> LnUrlKind {
    match response {
        lnurl::LnUrlResponse::LnUrlPayResponse(_) => LnUrlKind::Pay,
        lnurl::LnUrlResponse::LnUrlWithdrawResponse(_) => LnUrlKind::Withdraw,
        lnurl::LnUrlResponse::LnUrlChannelResponse(_) => LnUrlKind::Channel,
    }
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
