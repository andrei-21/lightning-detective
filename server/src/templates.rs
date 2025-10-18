use anyhow::Error;
use askama::filters::Safe;
use askama::Template;
use detective::offer_details::{IntroductionNode, OfferDetails};
use detective::{
    Description, FeatureFlag, InvestigativeFindings, InvoiceDetails, Node, RecipientNode,
    RouteHintDetails,
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
#[template(path = "features.html")]
pub struct FeaturesTemplate<'a> {
    pub features: &'a Vec<(String, FeatureFlag)>,
}

#[derive(Template)]
#[template(path = "routing_hints.html")]
pub struct RouteHintsTemplate<'a> {
    pub route: &'a RouteHintDetails,
}

pub fn format_feature_flag(flag: &FeatureFlag) -> Safe<String> {
    let result = match flag {
        FeatureFlag::Required => "<mark class=\"badge-required\">required</mark>".to_string(),
        FeatureFlag::Supported => "<mark class=\"badge-supported\">supported</mark>".to_string(),
        FeatureFlag::NotSupported => {
            "<mark class=\"badge-not-supported\">not supported</mark>".to_string()
        }
    };
    Safe(result)
}

pub fn format_features(features: &Option<Vec<(String, FeatureFlag)>>) -> Safe<String> {
    let features = match features {
        Some(features) => features,
        None => return mute("empty"),
    };
    Safe(FeaturesTemplate { features }.render().unwrap())
}

pub fn format_routing_hints(route: &RouteHintDetails) -> Safe<String> {
    Safe(RouteHintsTemplate { route }.render().unwrap())
}

pub fn mute(message: &str) -> Safe<String> {
    Safe(format!("<span class=\"muted\">{message}</span>"))
}

pub fn external_link(href: &str, title: &str) -> Safe<String> {
    Safe(format!(
        "<a href=\"{href}\" target=\"_blank\" rel=\"noreferrer\">{title}</a>"
    ))
}

pub fn explorer_link(node: &Node) -> Safe<String> {
    external_link(&node.pubkey, node.alias.as_ref().unwrap_or(&node.pubkey))
}

pub mod filters {
    use super::mute;
    use askama::filters::{MaybeSafe, Safe};
    use askama::{Result, Values};

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

    pub fn hex<T: std::fmt::Display>(s: &T, _: &dyn Values) -> Result<Safe<String>> {
        Ok(Safe(format!("<code class=\"code-value\">{s}</code>")))
    }
}
