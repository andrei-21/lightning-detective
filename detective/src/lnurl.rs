use anyhow::{bail, Result};
use lnurl::lnurl::LnUrl;
use lnurl::pay::LnURLPayInvoice;
use lnurl::{decode_ln_url_response, LnUrlResponse};
use std::io;
use std::io::Write;
use std::time::Duration;

pub async fn resolve_lnurl(lnurl: LnUrl) -> Result<String> {
    println!("Querying {}", lnurl.url);
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
    println!("Querying {url}");
    let response = client.get(&url).send().await?;
    let text = response.error_for_status()?.text().await?;
    println!("Response: {text}");
    print!("Decoding as JSON: ");
    let _ = io::stdout().flush();
    let json: serde_json::Value = serde_json::from_str(&text)?;
    println!("OK");
    print!("Decoding as LNURL pay invoice response: ");
    let _ = io::stdout().flush();
    let invoice_response: LnURLPayInvoice = serde_json::from_value(json)?;
    println!("OK");
    Ok(invoice_response.pr)
}
