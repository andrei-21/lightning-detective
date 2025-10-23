use anyhow::Result;
use lnurl::lnurl::LnUrl;
use lnurl::{decode_ln_url_response, LnUrlResponse};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum Event {
    Querying(String),
    ResponseReceived(String),
}

pub async fn resolve_lnurl(lnurl: LnUrl, events: mpsc::Sender<Event>) -> Result<LnUrlResponse> {
    events.send(Event::Querying(lnurl.url.clone())).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()?;

    let response = client.get(&lnurl.url).send().await?;
    let text = response.error_for_status()?.text().await?;
    events.send(Event::ResponseReceived(text.clone())).await?;

    decode_ln_url_response(&text).map_err(anyhow::Error::from)

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
