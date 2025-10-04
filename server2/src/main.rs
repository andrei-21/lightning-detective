use askama::filters::Safe;
use askama::Template;
use axum::extract::Form;
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use detective::decoder::DecodedData;
use detective::offer_details::{IntroductionNode, OfferDetails};
use serde::Deserialize;
use std::net::SocketAddr;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or("info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let app = Router::new()
        .route("/", get(index))
        .route("/api/parse", post(parse));

    let addr: SocketAddr = "0.0.0.0:3000".parse().unwrap();
    tracing::info!("listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn index() -> Html<&'static str> {
    Html(include_str!("index.html"))
}

#[derive(Deserialize)]
struct Input {
    text: String,
}

async fn parse(Form(input): Form<Input>) -> Html<String> {
    let result = match detective::decoder::decode(&input.text) {
        Ok(result) => result,
        Err(err) => return Html(ErrorTemplate { err }.render().unwrap()),
    };
    let offer = match result {
        DecodedData::Offer(offer) => offer,
        _ => panic!(),
    };
    let offer = detective::offer_details::OfferDetails::from(offer);

    let offer_template = OfferTemplate { offer };
    Html(offer_template.render().unwrap())
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate {
    err: anyhow::Error,
}

#[derive(Template)]
#[template(path = "offer.html")]
struct OfferTemplate {
    offer: OfferDetails,
}

fn mute(message: &str) -> Safe<String> {
    Safe(format!("<span class=\"muted\">{}</span>", message))
}

mod filters {
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
