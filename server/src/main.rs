#![warn(unused_crate_dependencies)]

use anyhow::{bail, Context, Error, Result};
use askama::filters::Safe;
use askama::Template;
use axum::extract::{Form, Query};
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use detective::decoder::{parse_bip21, DecodedData};
use detective::offer_details::OfferDetails;
use detective::{resolve_bip353, resolve_lnurl, Event, InvoiceDetails};
use serde::Deserialize;
use std::net::SocketAddr;
use templates::LnurlTemplate;
use tokio_stream::StreamExt;
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

    let addr: SocketAddr = "0.0.0.0:3000".parse().context("Invalid bind address")?;
    tracing::info!("Listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind TCP listener")?;
    axum::serve(listener, app).await.context("Server error")?;
    Ok(())
}

#[derive(Deserialize, Default)]
struct IndexQuery {
    r: Option<String>,
}

async fn index(Query(params): Query<IndexQuery>) -> Html<String> {
    let request = params.r.unwrap_or_default();
    let result = if request.is_empty() {
        Safe(String::new())
    } else {
        Safe(match parse_impl(&request).await {
            Ok(html) => html,
            Err(err) => render_error(err),
        })
    };

    let template = IndexTemplate { request, result };
    Html(render_template(&template))
}

#[derive(Deserialize)]
struct Input {
    text: String,
}

async fn parse(Form(input): Form<Input>) -> Html<String> {
    let html = match parse_impl(&input.text).await {
        Ok(content) => content,
        Err(err) => render_error(err),
    };
    Html(html)
}

async fn parse_impl(input: &str) -> Result<String> {
    let decoded = detective::decoder::decode(input)?;

    match decoded {
        DecodedData::Offer(offer) => {
            let offer = OfferDetails::from(offer);
            OfferTemplate { offer }.render()
        }
        DecodedData::Invoice(invoice) => {
            let detective = detective::InvoiceDetective::new()
                .context("Failed to construct InvoiceDetective")?;
            let findings = detective
                .investigate_bolt11(&invoice)
                .context("Failed to investigate invoice")?;
            let invoice = InvoiceDetails::from(&invoice);
            InvoiceTemplate { invoice, findings }.render()
        }
        DecodedData::Bip21(address, params) => Bip21Template { address, params }.render(),
        DecodedData::Bip353(hrn) => {
            let result = resolve_bip353(&hrn)
                .await
                .context("Failed to resolve BIP-353 address")?;
            let (address, params) =
                parse_bip21(&result.bip21).context("Failed to parse resolved BIP-21 URI")?;
            Bip353Template {
                hrn: (hrn.user().to_string(), hrn.domain().to_string()),
                result,
                address,
                params,
            }
            .render()
        }
        DecodedData::LnUrl(lnurl) => {
            let stream = resolve_lnurl(lnurl.url.clone());
            let events: Vec<Event> = stream.collect().await;
            LnurlTemplate { events }.render()
        }
        other => bail!("Unsupported decoded data: {other:?}"),
    }
    .map_err(Error::new)
}

fn render_template<T: Template>(template: &T) -> String {
    template
        .render()
        .unwrap_or_else(|err| render_error(Error::new(err)))
}

fn render_error(err: Error) -> String {
    tracing::error!(error = %err, "Request handling error");
    ErrorTemplate { err }
        .render()
        .unwrap_or_else(|render_err| format!("Failed to render error page: {render_err}"))
}
