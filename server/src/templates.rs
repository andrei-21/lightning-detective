use anyhow::Error;
use askama::filters::Safe;
use askama::Template;
use build_html::{Html, HtmlContainer, HtmlElement, HtmlTag};
use detective::decoder::Bip21Param;
use detective::offer_details::{IntroductionNode, OfferDetails};
use detective::{
    Bip353Result, Description, Event, FeatureFlag, InvestigativeFindings, InvoiceDetails,
    LnUrlResponseDetails, Node, RecipientNode,
};

#[derive(Template)]
#[template(path = "index.html")]
pub struct IndexTemplate {
    pub request: String,
    pub result: Safe<String>,
}

#[derive(Template)]
#[template(path = "error.html")]
pub struct ErrorTemplate {
    pub err: Error,
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
    pub address: Option<String>,
    pub params: Vec<Bip21Param>,
}

#[derive(Template)]
#[template(path = "bip353.html")]
pub struct Bip353Template {
    pub hrn: (String, String),
    pub result: Bip353Result,
    pub address: Option<String>,
    pub params: Vec<Bip21Param>,
}

#[derive(Template)]
#[template(path = "lnurl.html")]
pub struct LnurlTemplate {
    pub events: Vec<Event>,
}

pub fn format_sat(sat: &u64) -> String {
    match sat {
        1 => "1 sat".to_string(),
        sat => format!("{} sats", sat),
    }
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
