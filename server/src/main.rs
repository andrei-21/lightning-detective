use askama::filters::Safe;
use askama::Template;
use axum::extract::{Form, Query};
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use detective::decoder::DecodedData;
use detective::offer_details::OfferDetails;
use detective::InvoiceDetails;
use serde::Deserialize;
use std::net::SocketAddr;
use templates::InvoiceTemplate;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod templates;

use crate::templates::{ErrorTemplate, IndexTemplate, OfferTemplate};

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

#[derive(Deserialize, Default)]
struct IndexQuery {
    r: Option<String>,
}

async fn index(Query(params): Query<IndexQuery>) -> Html<String> {
    let template = match params.r {
        Some(request) => {
            let result = Safe(parse0(&request));
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
    Html(parse0(&input.text))
}

fn parse0(input: &str) -> String {
    let result = match detective::decoder::decode(input) {
        Ok(result) => result,
        Err(err) => return ErrorTemplate { err }.render().unwrap(),
    };
    let detective = detective::InvoiceDetective::new().unwrap();
    match result {
        DecodedData::Offer(offer) => {
            let offer = OfferDetails::from(offer);
            let offer_template = OfferTemplate { offer };
            offer_template.render().unwrap()
        }
        DecodedData::Invoice(invoice) => {
            let findings = detective.investigate_bolt11(&invoice).unwrap();
            let invoice = InvoiceDetails::from(&invoice);
            let invoice_template = InvoiceTemplate { invoice, findings };
            invoice_template.render().unwrap()
        }
        _ => panic!(),
    }
}
