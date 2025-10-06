use anyhow::Error;
use askama::filters::Safe;
use askama::Template;
use detective::offer_details::{IntroductionNode, OfferDetails};
use detective::{Description, FeatureFlag, InvoiceDetails};
use std::time::Duration;

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
}

#[derive(Template)]
#[template(path = "features.html")]
pub struct FeaturesTemplate<'a> {
    pub features: &'a Vec<(String, FeatureFlag)>,
}

pub fn format_duration(duration: &Duration) -> Safe<String> {
    let secs = duration.as_secs();
    let (days, hrs, mins, secs) = (
        secs / 86400,
        (secs % 86400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days} day{}", if days == 1 { "" } else { "s" }));
    }
    if hrs > 0 {
        parts.push(format!("{hrs} hour{}", if hrs == 1 { "" } else { "s" }));
    }
    if mins > 0 {
        parts.push(format!("{mins} min{}", if mins == 1 { "" } else { "s" }));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{secs} second{}", if secs == 1 { "" } else { "s" }));
    }
    Safe(parts.join(", "))
}

pub fn format_number_of_blocks(number: &u64) -> Safe<String> {
    format_duration(&Duration::from_secs(60 * 10 * number))
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

pub fn mute(message: &str) -> Safe<String> {
    Safe(format!("<span class=\"muted\">{}</span>", message))
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
