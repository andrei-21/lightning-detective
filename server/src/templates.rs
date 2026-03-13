use anyhow::{Error, Result};
use askama::filters::Safe;
use askama::Template;
use build_html::{Html, HtmlContainer, HtmlElement, HtmlTag};
use detective::decoder::{Bip21, Bip21Param};
use detective::offer_details::{IntroductionNode, OfferDetails};
use detective::{
    Bip353Result, Description, FeatureFlag, InvestigativeFindings, InvoiceDetails, JsonRpcEvent,
    LightningAddress, LiquidAddress, LiquidUri, LnUrlResponse, Node, OnchainAddress, OnionEvent,
    RecipientNode, ServiceKind, SilentPaymentAddress,
};

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub request: String,
    pub result: Safe<String>,
}

#[derive(Template)]
#[template(path = "doc.html")]
pub struct DocTemplate;

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub err: Error,
}

#[derive(Template)]
#[template(path = "onchain_address.html")]
pub struct OnchainAddressTemplate {
    pub address: OnchainAddress,
}

#[derive(Template)]
#[template(path = "silent_payment_address.html")]
pub struct SilentPaymentAddressTemplate {
    pub address: SilentPaymentAddress,
}

#[derive(Template)]
#[template(path = "liquid_address.html")]
pub struct LiquidAddressTemplate {
    pub address: LiquidAddress,
}

#[derive(Template)]
#[template(path = "liquid_uri.html")]
pub struct LiquidUriTemplate {
    pub uri: LiquidUri,
}

#[derive(Template)]
#[template(path = "offer.html")]
pub struct OfferTemplate {
    pub offer: OfferDetails,
}

#[derive(Template)]
#[template(path = "invoice.html")]
pub struct InvoiceTemplate {
    pub invoice: InvoiceDetails,
    pub findings: InvestigativeFindings,
}

#[derive(Template)]
#[template(path = "bip21.html")]
pub struct Bip21Template {
    pub bip21: Bip21,
}

#[derive(Template)]
#[template(path = "bip353.html")]
pub struct Bip353Template {
    pub hrn: (String, String),
    pub result: Bip353Result,
    pub bip21: Bip21,
}

#[derive(Template)]
#[template(path = "lnurl.html")]
pub struct LnurlTemplate {
    pub events: Vec<JsonRpcEvent<LnUrlResponse>>,
}

#[derive(Template)]
#[template(path = "request-invoice-stream.html")]
pub struct RequestInvoiceStreamTemplate {
    pub stream_url: String,
}

#[derive(Template)]
#[template(path = "lnurl-request-invoice-event.html")]
pub struct LnurlRequestInvoiceEventTemplate {
    pub index: usize,
    pub event: JsonRpcEvent<String>,
}

#[derive(Template)]
#[template(path = "offer-request-invoice-event.html")]
pub struct OfferRequestInvoiceEventTemplate {
    pub index: usize,
    pub event: OnionEvent,
}

#[derive(Template)]
#[template(path = "lightning-address.html")]
pub struct LightningAddressTemplate {
    pub lightning_address: LightningAddress,
    pub events: Vec<JsonRpcEvent<LnUrlResponse>>,
}

#[derive(Template)]
#[template(path = "bip353-or-lightning-address.html")]
pub struct Bip353OrLightningAddressTemplate {
    pub bip353: Result<Bip353Template>,
    pub lightning_address: LightningAddress,
    pub events: Vec<JsonRpcEvent<LnUrlResponse>>,
}

pub fn format_feature_flag(flag: &FeatureFlag) -> Safe<String> {
    let result = match flag {
        FeatureFlag::Required => HtmlElement::new(HtmlTag::Mark)
            .with_attribute("class", "badge-required")
            .with_child("required".into()),
        FeatureFlag::Supported => HtmlElement::new(HtmlTag::Mark)
            .with_attribute("class", "badge-supported")
            .with_child("supported".into()),
        FeatureFlag::NotSupported => HtmlElement::new(HtmlTag::Mark)
            .with_attribute("class", "badge-not-supported")
            .with_child("not supported".into()),
    };
    Safe(result.to_html_string())
}

pub fn mute(message: &str) -> Safe<String> {
    Safe(
        HtmlElement::new(HtmlTag::Span)
            .with_attribute("class", "muted")
            .with_child(message.into())
            .to_html_string(),
    )
}

pub fn investigate_link(payload: &String) -> Safe<String> {
    Safe(
        HtmlElement::new(HtmlTag::Bold)
            .with_link(format!("/?r={payload}#result"), "Investigate →")
            .to_html_string(),
    )
}

pub fn external_link(link: &str, title: &str) -> Safe<String> {
    let title = format!("{title}<svg width=\"13.5\" height=\"13.5\" aria-hidden=\"true\" viewBox=\"0 0 24 24\"><path fill=\"currentColor\" d=\"M21 13v10h-21v-19h12v2h-10v15h17v-8h2zm3-12h-10.988l4.035 4-6.977 7.07 2.828 2.828 6.977-7.07 4.125 4.172v-11z\"></path></svg>");
    Safe(
        HtmlElement::new(HtmlTag::Link)
            .with_attribute("href", link)
            .with_attribute("target", "_blank")
            .with_attribute("rel", "noreferrer")
            .with_child(title.into())
            .to_html_string(),
    )
}

pub fn explorer_link(node: &Node) -> Safe<String> {
    const EXPLORER_URL: &str = "https://mempool.space/lightning/node";
    let link = format!("{EXPLORER_URL}/{}", node.pubkey);
    external_link(&link, node.alias.as_ref().unwrap_or(&node.pubkey))
}

pub fn sparkscan_link(spark_address: &String) -> Safe<String> {
    const EXPLORER_URL: &str = "https://www.sparkscan.io/address";
    // TODO: Mind network.
    let link = format!("{EXPLORER_URL}/{spark_address}");
    external_link(&link, "View on Sparkscan")
}

pub fn doc(term: &str) -> Safe<String> {
    Safe(format!("<sup><a href=\"/doc#{term}\">[doc]</a></sup>"))
}

pub fn bip(id: u32) -> Safe<String> {
    let link = format!("https://bips.dev/{id}");
    let title = format!("BIP-{id} specification");
    external_link(&link, &title)
}

pub fn lud(id: u32) -> Safe<String> {
    let link = format!("https://github.com/lnurl/luds/blob/luds/{id:02}.md");
    let title = format!("LUD-{id:02} specification");
    external_link(&link, &title)
}

pub mod filters {
    use super::mute;
    use askama::filters::{MaybeSafe, Safe};
    use askama::{Result, Values};
    use build_html::{Html, HtmlElement, HtmlTag};
    use serde_json::Value;

    pub fn or_empty<T: std::fmt::Display>(
        s: &Option<T>,
        _: &dyn Values,
    ) -> Result<MaybeSafe<String>> {
        let s = match s {
            Some(s) => MaybeSafe::NeedsEscaping(s.to_string()),
            None => MaybeSafe::Safe(mute("empty").0),
        };
        Ok(s)
    }

    pub fn or_empty_hex<T: std::fmt::Display>(
        s: &Option<T>,
        v: &dyn Values,
    ) -> Result<Safe<String>> {
        match s {
            Some(s) => hex(s, v),
            None => Ok(mute("empty")),
        }
    }

    pub fn hex<T: std::fmt::Display>(s: &T, _: &dyn Values) -> Result<Safe<String>> {
        Ok(Safe(
            HtmlElement::new(HtmlTag::CodeText)
                .with_attribute("class", "code-value")
                .with_child(s.to_string().into())
                .to_html_string(),
        ))
    }

    pub fn json_pretty(value: &Value, _: &dyn Values) -> Result<String> {
        Ok(serde_json::to_string_pretty(value).expect("serializing Value to JSON never fails"))
    }
}
