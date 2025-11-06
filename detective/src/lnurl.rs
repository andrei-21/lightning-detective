use anyhow::{anyhow, bail, ensure, Context, Error, Result};
use base64::{engine::general_purpose, Engine as _};
use lnurl::decode_ln_url_response_from_json;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

use crate::decoder::{LnUrl, LnUrlKind};
use crate::types::{Msat, MsatRange};

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
    pub default_description: String,
    pub callback: String,
    pub k1: String,
    pub max_withdrawable: u64,
    pub min_withdrawable: Option<u64>,
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

    fn try_from(_withdraw: lnurl::withdraw::WithdrawalResponse) -> Result<Self> {
        todo!()
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
pub enum Event {
    Requesting(Method, String),
    ResponseReceived(StatusCode),
    ResponseBodyReceived(String),
    JsonParsed(Value),
    Result(Result<LnUrlResponse>),
}

pub fn resolve_lnurl(lnurl: LnUrl) -> impl Stream<Item = Event> {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(async move {
        let result = resolve_lnurl_impl(lnurl, tx.clone()).await;
        let _ = tx.send(Event::Result(result)).await;
    });
    ReceiverStream::new(rx)
}

pub async fn resolve_lnurl_impl(
    lnurl: LnUrl,
    events: mpsc::Sender<Event>,
) -> Result<LnUrlResponse> {
    let method = Method::GET;
    events
        .send(Event::Requesting(method.clone(), lnurl.url.clone()))
        .await?;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let response = client.request(method, lnurl.url).send().await?;
    let status = response.status();
    events.send(Event::ResponseReceived(status)).await?;

    let body = response.text().await?;
    events
        .send(Event::ResponseBodyReceived(body.clone()))
        .await?;

    ensure!(status.is_success(), "HTTP status is not success");
    let json: Value = serde_json::from_str(&body)?;
    events.send(Event::JsonParsed(json.clone())).await?;

    let response = decode_ln_url_response_from_json(json).map_err(Error::from)?;

    if let Some(expected) = expected_response_kind(&lnurl.kind) {
        let actual = response_kind(&response);
        ensure!(
            actual == expected,
            "LNURL kind mismatch: expected {expected:?}, got {actual:?}"
        );
    }
    response.try_into()

    // let symbol = if pay.callback.contains('?') { '&' } else { '?' };
    // let url = format!("{}{symbol}amount={}", pay.callback, pay.min_sendable);
    // println!("Querying {url}");
    // let response = client.get(&url).send().await?;
    // let text = response.error_for_status()?.text().await?;
    // println!("Response: {text}");
    // print!("Decoding as JSON: ");
    // let _ = io::stdout().flush();
    // let json: serde_json::Value = serde_json::from_str(&text)?;
    // println!("OK");
    // print!("Decoding as LNURL pay invoice response: ");
    // let _ = io::stdout().flush();
    // let invoice_response: LnURLPayInvoice = serde_json::from_value(json)?;
    // println!("OK");
    // Ok(invoice_response.pr)
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
