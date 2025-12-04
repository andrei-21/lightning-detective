#![warn(unused_crate_dependencies)]

use anyhow::{anyhow, Context, Error, Result};
use askama::filters::Safe;
use askama::Template;
use axum::body::Body;
use axum::extract::{Form, Query};
use axum::http::{header, Response};
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use detective::decoder::{parse_bip21, DecodedData, HumanReadableName};
use detective::offer_details::OfferDetails;
use detective::{resolve_bip353, resolve_lnurl, Event, InvoiceDetails};
use serde::Deserialize;
use std::net::SocketAddr;
use tokio_stream::StreamExt;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod templates;

use crate::templates::{
    Bip21Template, Bip353OrLightningAddressTemplate, Bip353Template, DocTemplate, ErrorTemplate,
    IndexTemplate, InvoiceTemplate, LightningAddressTemplate, LnurlTemplate, OfferTemplate,
    OnchainAddressTemplate, SilentPaymentAddressTemplate,
};

static STYLESHEET: &str = include_str!("../static/styles.css");
static APP_SCRIPT: &str = include_str!("../static/app.js");
static PICO_CSS: &str = include_str!("../static/vendor/pico.min.css");
static HTMX_SCRIPT: &str = include_str!("../static/vendor/htmx.min.js");

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
        .route("/api/parse", post(parse))
        .route("/doc", get(doc))
        .route("/static/styles.css", get(stylesheet))
        .route("/static/app.js", get(app_script))
        .route("/static/vendor/pico.min.css", get(pico_css))
        .route("/static/vendor/htmx.min.js", get(htmx_script));

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

async fn doc() -> Html<String> {
    Html(DocTemplate.render().unwrap())
}

#[derive(Deserialize)]
struct Input {
    text: String,
}

async fn parse(Form(input): Form<Input>) -> Html<String> {
    parse_impl(&input.text)
        .await
        .unwrap_or_else(render_error)
        .into()
}

async fn parse_impl(input: &str) -> Result<String> {
    let decoded = detective::decoder::decode(input)?;

    match decoded {
        DecodedData::OnchainAddress(address) => OnchainAddressTemplate { address }.render(),
        DecodedData::SilentPaymentAddress(address) => {
            SilentPaymentAddressTemplate { address }.render()
        }
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
        DecodedData::Bip21(bip21) => Bip21Template { bip21 }.render(),
        DecodedData::Bip353(hrn) => resolve_and_build_bip353(&hrn).await?.render(),
        DecodedData::LnUrl(lnurl) => {
            let stream = resolve_lnurl(lnurl.clone());
            let events: Vec<Event> = stream.collect().await;
            LnurlTemplate { events }.render()
        }
        DecodedData::LightningAddress(lightning_address) => {
            let stream = resolve_lnurl(lightning_address.lnurl.clone());
            let events: Vec<Event> = stream.collect().await;
            LightningAddressTemplate {
                lightning_address,
                events,
            }
            .render()
        }
        DecodedData::Bip353OrLightningAddress(hrn, lightning_address) => {
            let bip353 = resolve_and_build_bip353(&hrn).await;

            let stream = resolve_lnurl(lightning_address.lnurl.clone());
            let events: Vec<Event> = stream.collect().await;

            Bip353OrLightningAddressTemplate {
                bip353,
                events,
                lightning_address,
            }
            .render()
        }
        DecodedData::Refund(_refund) => {
            return Ok(render_error(anyhow!(
                "BOLT-12 Refund is not yet implemented"
            )));
        }
    }
    .map_err(Error::new)
}

async fn resolve_and_build_bip353(hrn: &HumanReadableName) -> Result<Bip353Template> {
    let result = resolve_bip353(hrn)
        .await
        .context("Failed to resolve BIP-353 address")?;
    let bip21 = parse_bip21(&result.bip21).context("Failed to parse resolved BIP-21 URI")?;
    Ok(Bip353Template {
        hrn: (hrn.user().to_string(), hrn.domain().to_string()),
        result,
        bip21,
    })
}

fn render_template<T: Template>(template: &T) -> String {
    template
        .render()
        .unwrap_or_else(|err| render_error(Error::new(err)))
}

fn render_error(err: Error) -> String {
    tracing::error!(error = format!("{err:#}"), "Request handling error");
    ErrorTemplate { err }
        .render()
        .unwrap_or_else(|render_err| format!("Failed to render error page: {render_err}"))
}

async fn stylesheet() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(Body::from(STYLESHEET))
        .expect("Failed to render stylesheet")
}

async fn app_script() -> Response<Body> {
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )
        .body(Body::from(APP_SCRIPT))
        .expect("Failed to render app script")
}

async fn pico_css() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "text/css; charset=utf-8")
        .body(Body::from(PICO_CSS))
        .expect("Failed to render Pico CSS")
}

async fn htmx_script() -> Response<Body> {
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )
        .body(Body::from(HTMX_SCRIPT))
        .expect("Failed to render htmx script")
}
