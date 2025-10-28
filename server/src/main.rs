#![warn(unused_crate_dependencies)]

use anyhow::{Error, Result};
use askama::filters::Safe;
use askama::Template;
use axum::extract::{Form, Query};
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use detective::decoder::{parse_bip21, DecodedData};
use detective::offer_details::OfferDetails;
use detective::{resolve_bip353, InvoiceDetails};
use serde::Deserialize;
use std::net::SocketAddr;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod templates;

use crate::templates::{
    Bip21Template, Bip353Template, ErrorTemplate, IndexTemplate, InvoiceTemplate, OfferTemplate,
};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or("info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
    tracing::info!("Starting...");

    let app = Router::new()
        .route("/", get(index))
        .route("/api/parse", post(parse));

    let addr: SocketAddr = "0.0.0.0:3000".parse().map_err(Error::msg)?;
    tracing::info!("Listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await?;
    Ok(())
}

#[derive(Deserialize, Default)]
struct IndexQuery {
    r: Option<String>,
}

async fn index(Query(params): Query<IndexQuery>) -> Html<String> {
    let template = match params.r {
        Some(request) => {
            let result = Safe(parse0(&request).await);
            IndexTemplate { request, result }
        }
        None => IndexTemplate {
            request: String::new(),
            result: Safe(String::new()),
        },
    };
    Html(template.render().unwrap())
}

#[derive(Deserialize)]
struct Input {
    text: String,
}

async fn parse(Form(input): Form<Input>) -> Html<String> {
    Html(parse0(&input.text).await)
}

async fn parse0(input: &str) -> String {
    let result = match detective::decoder::decode(input) {
        Ok(result) => result,
        Err(err) => return ErrorTemplate { err }.render().unwrap(),
    };
    let detective = detective::InvoiceDetective::new().unwrap();
    let result = match result {
        DecodedData::Offer(offer) => {
            let offer = OfferDetails::from(offer);
            OfferTemplate { offer }.render()
        }
        DecodedData::Invoice(invoice) => {
            let findings = detective.investigate_bolt11(&invoice).unwrap();
            let invoice = InvoiceDetails::from(&invoice);
            InvoiceTemplate { invoice, findings }.render()
        }
        DecodedData::Bip21(address, params) => Bip21Template { address, params }.render(),
        DecodedData::Bip353(hrn) => {
            let result = resolve_bip353(&hrn).await.unwrap();
            let (address, params) = parse_bip21(&result.bip21).unwrap();
            Bip353Template {
                hrn: (hrn.user().to_string(), hrn.domain().to_string()),
                result,
                address,
                params,
            }
            .render()
        }
        _ => panic!(),
    };
    result.unwrap()
}
