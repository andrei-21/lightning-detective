use anyhow::{ensure, Error, Result};
use lnurl::{decode_ln_url_response_from_json, LnUrlResponse};
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::time::Duration;
use thousands::Separable;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

#[derive(Debug)]
pub enum LnUrlResponseDetails {
    Pay(LnUrlPayDetails),
}

#[derive(Debug)]
pub struct LnUrlPayDetails {
    pub callback: String,
    pub sendable_amount: String,
    pub metadata: String,
    pub comment_allowed: Option<String>,
    pub allows_nostr: Option<bool>,
    pub nostr_pubkey: Option<String>,
}

impl From<&LnUrlResponse> for LnUrlResponseDetails {
    fn from(response: &LnUrlResponse) -> Self {
        if let LnUrlResponse::LnUrlPayResponse(response) = response {
            let sendable_amount = if response.min_sendable == response.max_sendable {
                format_msat(response.min_sendable)
            } else {
                let min = format_msat_0(response.min_sendable);
                let max = format_msat_0(response.max_sendable);
                format!("{min}–{max} sats")
            };
            Self::Pay(LnUrlPayDetails {
                callback: response.callback.clone(),
                sendable_amount,
                metadata: response.metadata.clone(),
                comment_allowed: response.comment_allowed.map(up_to),
                allows_nostr: response.allows_nostr,
                nostr_pubkey: response.nostr_pubkey.map(|key| key.to_string()),
            })
        } else {
            panic!()
        }
    }
}

fn up_to(num: u32) -> String {
    match num {
        0 => "no".to_string(),
        1 => "Up to one character".to_string(),
        n => format!("Up to {n} characters"),
    }
}

fn format_msat_0(msat: u64) -> String {
    match msat {
        1000 => "1".to_string(),
        msat if msat % 1000 == 0 => (msat / 1000).separate_with_commas().to_string(),
        msat => {
            let sat = msat / 1000;
            let sat = sat.separate_with_commas();
            let msat = msat % 1000;
            format!("{sat}.{msat:03}")
        }
    }
}

fn format_msat(msat: u64) -> String {
    match msat {
        1000 => "1 sat".to_string(),
        msat if msat % 1000 == 0 => format!("{} sats", (msat / 1000).separate_with_commas()),
        msat => {
            let sat = msat / 1000;
            let sat = sat.separate_with_commas();
            let msat = msat % 1000;
            format!("{sat}.{msat:03} sats")
        }
    }
}

#[derive(Debug)]
pub enum Event {
    Requesting(Method, String),
    ResponseReceived(StatusCode),
    ResponseBodyReceived(String),
    JsonParsed(Value),
    Result(Result<LnUrlResponse>),
}

pub fn resolve_lnurl(url: String) -> impl Stream<Item = Event> {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(async move {
        let result = resolve_lnurl_impl(url, tx.clone()).await;
        let _ = tx.send(Event::Result(result)).await;
    });
    ReceiverStream::new(rx)
}

pub async fn resolve_lnurl_impl(url: String, events: mpsc::Sender<Event>) -> Result<LnUrlResponse> {
    let method = Method::GET;
    events
        .send(Event::Requesting(method.clone(), url.clone()))
        .await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;

    let response = client.request(method, url).send().await?;
    let status = response.status();
    events.send(Event::ResponseReceived(status)).await?;

    let body = response.text().await?;
    events
        .send(Event::ResponseBodyReceived(body.clone()))
        .await?;

    ensure!(status.is_success(), "Status is not success");
    let json: Value = serde_json::from_str(&body)?;
    events.send(Event::JsonParsed(json.clone())).await?;

    decode_ln_url_response_from_json(json).map_err(Error::from)

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
