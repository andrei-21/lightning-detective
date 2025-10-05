use anyhow::Error;
use askama::filters::Safe;
use askama::Template;
use detective::offer_details::{IntroductionNode, OfferDetails};

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

pub fn mute(message: &str) -> Safe<String> {
    Safe(format!("<span class=\"muted\">{}</span>", message))
}

pub mod filters {
    use super::mute;
    use askama::filters::MaybeSafe;
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
}
