#![warn(unused_crate_dependencies)]

use anyhow::{anyhow, bail, Context, Error, Result};
use askama::filters::Safe;
use askama::Template;
use axum::body::Body;
use axum::extract::{Form, Query, State};
use axum::http::{header, Response};
use axum::response::Html;
use axum::routing::{get, post};
use axum::Router;
use bitcoin::constants::ChainHash;
use bytes::Bytes;
use detective::decoder::{parse_bip21, DecodedData, HumanReadableName};
use detective::offer_details::OfferDetails;
use detective::types::Msat;
use detective::{
    resolve_bip353, resolve_lnurl, Bolt12InvoiceDetails, Bolt12StaticInvoiceDetails,
    InvoiceDetails, JsonRpcEvent, LnUrlResponse, PayOfferParams,
};
use futures_util::StreamExt;
use rand::distributions::Alphanumeric;
use rand::Rng;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex, OnceLock};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod templates;

use crate::templates::{
    ArkAddressTemplate, Bip21Template, Bip353OrLightningAddressTemplate, Bip353Template,
    Bolt12InvoiceTemplate, Bolt12StaticInvoiceTemplate, CashuPaymentRequestTemplate, DocTemplate,
    ErrorTemplate, IndexTemplate, InvoiceTemplate, LightningAddressTemplate, LiquidAddressTemplate,
    LiquidUriTemplate, LnurlRequestInvoiceEventTemplate, LnurlTemplate,
    OfferRequestInvoiceEventTemplate, OfferTemplate, OnchainAddressTemplate,
    RequestInvoiceStreamTemplate, SilentPaymentAddressTemplate,
};

const STYLESHEET: &str = include_str!("../static/styles.css");
const APP_SCRIPT: &str = include_str!("../static/app.js");
const PICO_CSS: &str = include_str!("../static/vendor/pico.min.css");
const HTMX_SCRIPT: &str = include_str!("../static/vendor/htmx.min.js");
const HTMX_SSE_SCRIPT: &str = include_str!("../static/vendor/htmx-sse.js");
const QRCODE_SCRIPT: &str = include_str!("../static/vendor/qrcode.min.js");
const PAYMENT_INSTRUCTIONS: &[u8] = include_bytes!("../static/payment-instructions.png");
static OFFER_REQUESTS: OnceLock<Mutex<HashMap<String, OfferRequestInvoiceInput>>> = OnceLock::new();
static LNURL_REQUESTS: OnceLock<Mutex<HashMap<String, LnurlRequestInvoiceInput>>> = OnceLock::new();

#[derive(Clone)]
struct AppState {
    detective: Arc<detective::InvoiceDetective>,
}

fn offer_requests() -> &'static Mutex<HashMap<String, OfferRequestInvoiceInput>> {
    OFFER_REQUESTS.get_or_init(Default::default)
}

fn lnurl_requests() -> &'static Mutex<HashMap<String, LnurlRequestInvoiceInput>> {
    LNURL_REQUESTS.get_or_init(Default::default)
}

const DEFAULT_SERVER_ADDRESS: &str = "0.0.0.0:3000";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or("info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();
    tracing::info!("Starting...");

    let app = app(build_state().await?);

    let server_address =
        std::env::var("SERVER_ADDRESS").unwrap_or(DEFAULT_SERVER_ADDRESS.to_string());
    let addr: SocketAddr = server_address.parse().context("Invalid bind address")?;
    tracing::info!("Listening on http://{addr}");
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("Failed to bind TCP listener")?;
    axum::serve(listener, app).await.context("Server error")?;
    Ok(())
}

async fn build_state() -> Result<AppState> {
    Ok(AppState {
        detective: Arc::new(
            detective::InvoiceDetective::new()
                .await
                .context("Failed to construct InvoiceDetective")?,
        ),
    })
}

fn app(state: AppState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/parse", post(parse))
        .route("/api/lnurl-request-invoice", post(request_invoice))
        .route(
            "/api/lnurl-request-invoice/stream",
            get(lnurl_request_invoice_stream),
        )
        .route("/api/offer-request-invoice", post(request_offer_invoice))
        .route(
            "/api/offer-request-invoice/stream",
            get(offer_request_invoice_stream),
        )
        .route("/doc", get(doc))
        .route("/static/styles.css", get(stylesheet))
        .route("/static/app.js", get(app_script))
        .route("/static/vendor/pico.min.css", get(pico_css))
        .route("/static/vendor/htmx.min.js", get(htmx_script))
        .route("/static/vendor/htmx-sse.js", get(htmx_sse_script))
        .route("/static/vendor/qrcode.min.js", get(qrcode_script))
        .route(
            "/static/payment-instructions.png",
            get(payment_instructions_png),
        )
        .with_state(state)
}

#[derive(Deserialize, Default)]
struct IndexQuery {
    r: Option<String>,
}

async fn index(State(state): State<AppState>, Query(params): Query<IndexQuery>) -> Html<String> {
    let request = params.r.unwrap_or_default();
    let result = if request.is_empty() {
        Safe(String::new())
    } else {
        Safe(match parse_impl(&request, &state.detective).await {
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

async fn parse(State(state): State<AppState>, Form(input): Form<Input>) -> Html<String> {
    parse_impl(&input.text, &state.detective)
        .await
        .unwrap_or_else(render_error)
        .into()
}

async fn parse_impl(input: &str, detective: &detective::InvoiceDetective) -> Result<String> {
    let input = input.trim();

    let decoded = detective::decoder::decode(input)?;

    match decoded {
        DecodedData::OnchainAddress(address) => OnchainAddressTemplate { address }.render(),
        DecodedData::LiquidAddress(address) => LiquidAddressTemplate { address }.render(),
        DecodedData::LiquidUri(uri) => LiquidUriTemplate { uri }.render(),
        DecodedData::SilentPaymentAddress(address) => {
            SilentPaymentAddressTemplate { address }.render()
        }
        DecodedData::Offer(offer) => {
            let offer = OfferDetails::from(offer);
            OfferTemplate { offer }.render()
        }
        DecodedData::Invoice(invoice) => {
            let findings = detective
                .investigate_bolt11(&invoice)
                .await
                .context("Failed to investigate invoice")?;
            let invoice = InvoiceDetails::from(&invoice);
            InvoiceTemplate { invoice, findings }.render()
        }
        DecodedData::Bolt12Invoice(invoice) => {
            let findings = detective
                .investigate_bolt12_invoice(&invoice)
                .await
                .context("Failed to investigate invoice")?;
            let details = Bolt12InvoiceDetails::from(&invoice);
            Bolt12InvoiceTemplate { details, findings }.render()
        }
        DecodedData::Bolt12StaticInvoice(invoice) => {
            let findings = detective
                .investigate_bolt12_static_invoice(&invoice)
                .await
                .context("Failed to investigate invoice")?;
            let details = Bolt12StaticInvoiceDetails::from(&invoice);
            Bolt12StaticInvoiceTemplate { details, findings }.render()
        }
        DecodedData::CashuPaymentRequest(request) => {
            CashuPaymentRequestTemplate { request }.render()
        }
        DecodedData::ArkAddress(address) => ArkAddressTemplate { address }.render(),
        DecodedData::Bip21(bip21) => Bip21Template { bip21 }.render(),
        DecodedData::Bip353(hrn) => resolve_and_build_bip353(&hrn).await?.render(),
        DecodedData::LnUrl(lnurl) => {
            let stream = resolve_lnurl(lnurl.clone());
            let events: Vec<JsonRpcEvent<LnUrlResponse>> = stream.collect().await;
            LnurlTemplate { events }.render()
        }
        DecodedData::LightningAddress(lightning_address) => {
            let stream = resolve_lnurl(lightning_address.lnurl.clone());
            let events: Vec<JsonRpcEvent<LnUrlResponse>> = stream.collect().await;
            LightningAddressTemplate {
                lightning_address,
                events,
            }
            .render()
        }
        DecodedData::Bip353OrLightningAddress(hrn, lightning_address) => {
            let bip353 = resolve_and_build_bip353(&hrn).await;

            let stream = resolve_lnurl(lightning_address.lnurl.clone());
            let events: Vec<JsonRpcEvent<LnUrlResponse>> = stream.collect().await;

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

#[derive(Deserialize, Debug)]
struct LnurlRequestInvoiceInput {
    callback: String,
    amount: u64,
    comment: Option<String>,
}

async fn request_invoice(Form(input): Form<LnurlRequestInvoiceInput>) -> Html<String> {
    let id = generate_id();
    let mut store = lnurl_requests().lock().unwrap();
    let stream_url = format!("/api/lnurl-request-invoice/stream?id={id}");
    store.insert(id, input);
    Html(render_template(&RequestInvoiceStreamTemplate {
        stream_url,
    }))
}

#[derive(Deserialize)]
struct RequestInvoiceStreamQuery {
    id: String,
}

async fn lnurl_request_invoice_stream(
    Query(params): Query<RequestInvoiceStreamQuery>,
) -> Response<Body> {
    let input = {
        let mut store = lnurl_requests().lock().unwrap();
        store.remove(&params.id)
    };
    let Some(input) = input else {
        return Response::builder()
            .status(404)
            .body(Body::from("LNURL request not found"))
            .unwrap();
    };
    build_lnurl_invoice_sse_stream(input)
        .await
        .unwrap_or_else(sse_error_response)
}

async fn build_lnurl_invoice_sse_stream(input: LnurlRequestInvoiceInput) -> Result<Response<Body>> {
    let stream = detective::request_invoice(input.callback, Msat(input.amount), input.comment);
    let event_stream = stream.enumerate().map(|(index, event)| {
        let html = LnurlRequestInvoiceEventTemplate {
            index: index + 1,
            event,
        }
        .render()
        .unwrap_or_else(|err| render_error(Error::new(err)));
        Ok::<Bytes, Infallible>(Bytes::from(format_sse("message", &html)))
    });

    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(event_stream))
        .map_err(Error::new)
}

#[derive(Deserialize, Debug)]
struct OfferRequestInvoiceInput {
    offer: String,
    amount_msats: Option<u64>,
    quantity: Option<u64>,
    payer_note: Option<String>,
}

async fn request_offer_invoice(Form(input): Form<OfferRequestInvoiceInput>) -> Html<String> {
    let id = generate_id();
    let mut store = offer_requests().lock().unwrap();
    let stream_url = format!("/api/offer-request-invoice/stream?id={id}");
    store.insert(id, input);
    Html(render_template(&RequestInvoiceStreamTemplate {
        stream_url,
    }))
}

async fn offer_request_invoice_stream(
    Query(params): Query<RequestInvoiceStreamQuery>,
) -> Response<Body> {
    let input = {
        let mut store = offer_requests().lock().unwrap();
        store.remove(&params.id)
    };
    let Some(input) = input else {
        return Response::builder()
            .status(404)
            .body(Body::from("Offer request not found"))
            .unwrap();
    };

    build_offer_invoice_sse_stream(input)
        .await
        .unwrap_or_else(sse_error_response)
}

async fn build_offer_invoice_sse_stream(input: OfferRequestInvoiceInput) -> Result<Response<Body>> {
    let decoded = detective::decoder::decode(&input.offer)?;
    let offer = match decoded {
        DecodedData::Offer(offer) => offer,
        _ => bail!("Input is not a BOLT-12 offer"),
    };

    let chain = offer
        .chains()
        .first()
        .copied()
        .unwrap_or(ChainHash::BITCOIN);
    let params = PayOfferParams {
        chain,
        blinded_path_index: 0,
        amount_msats: input.amount_msats,
        quantity: input.quantity,
        payer_note: input.payer_note,
    };

    let stream = detective::request_bolt12_invoice(offer, params).await;
    let event_stream = stream.enumerate().map(|(index, event)| {
        let html = OfferRequestInvoiceEventTemplate {
            index: index + 1,
            event,
        }
        .render()
        .unwrap_or_else(|err| render_error(Error::new(err)));
        Ok::<Bytes, Infallible>(Bytes::from(format_sse("message", &html)))
    });

    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from_stream(event_stream))
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

fn format_sse(event: &str, data: &str) -> String {
    let mut payload = String::new();
    payload.push_str("event: ");
    payload.push_str(event);
    payload.push('\n');
    for line in data.lines() {
        payload.push_str("data: ");
        payload.push_str(line);
        payload.push('\n');
    }
    payload.push('\n');
    payload
}

fn sse_error_response(err: Error) -> Response<Body> {
    let html = render_error(err);
    let payload = format_sse("message", &html);
    Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(Body::from(payload))
        .unwrap()
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

async fn htmx_sse_script() -> Response<Body> {
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )
        .body(Body::from(HTMX_SSE_SCRIPT))
        .expect("Failed to render htmx SSE script")
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

async fn qrcode_script() -> Response<Body> {
    Response::builder()
        .header(
            header::CONTENT_TYPE,
            "application/javascript; charset=utf-8",
        )
        .body(Body::from(QRCODE_SCRIPT))
        .expect("Failed to render QR code script")
}

async fn payment_instructions_png() -> Response<Body> {
    Response::builder()
        .header(header::CONTENT_TYPE, "image/png")
        .body(Body::from(PAYMENT_INSTRUCTIONS))
        .expect("Failed to render payment instructions image")
}

fn generate_id() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(24)
        .map(char::from)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::to_bytes;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    const ONCHAIN_ADDRESS: &str = "bc1p5d7rjq7g6rdk2yhzks9smlaqtedr4dekq08ge8ztwac72sfr9rusxg3297";
    const LIQUID_URI: &str = "liquidnetwork:ex1q7gkeyjut0mrxc3j0kjlt7rmcnvsh0gt45d3fud\
        ?amount=0.001\
        &assetid=6f0279e9ed041c3d710a9f57d0c02928416460c4b722ae3457a11eec381c526d\
        &label=Donation\
        &message=Thanks%20Liquid";
    const SILENT_PAYMENT_ADDRESS: &str =
        "sp1qqweplq6ylpfrzuq6hfznzmv28djsraupudz0s0dclyt8erh70pgwxqkz2ydatksrdzf770\
        umsntsmcjp4kcz7jqu03jeszh0gdmpjzmrf5u4zh0c";
    const ARK_ADDRESS: &str =
        "ark1pwh9vsmezqqpharv69q4z8m6x364d5m5prnmcalcalq9pdmzw0y7mpveck4pcfhezqyp\
        czkrrj3lkx5ue4qrf4jc7ztpt9htdttmh2judhqnu7aue8p0y9mqkr4cf5";
    const BOLT11_INVOICE: &str =
        "lnbc1pvjluezsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygspp5\
        qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqdpl2pkx2ctnv5sxxmm\
        wwd5kgetjypeh2ursdae8g6twvus8g6rfwvs8qun0dfjkxaq9qrsgq357wnc5r2ueh7ck\
        6q93dj32dlqnls087fxdwk8qakdyafkq3yap9us6v52vjjsrvywa6rt52cm9r9zqt8r2\
        t7mlcwspyetp5h2tztugp9lfyql";

    async fn parse_html(input: &str) -> String {
        let app = app(build_state().await.unwrap());
        let body = format!("text={}", urlencoding::encode(input));
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/parse")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn renders_onchain_address_html() {
        let html = parse_html(ONCHAIN_ADDRESS).await;

        assert!(html.contains("On-Chain Address"));
        assert!(html.contains("p2tr"));
        assert!(html.contains("Bitcoin"));
        assert!(html.contains(ONCHAIN_ADDRESS));
        assert!(html.contains("Show&nbsp;QR") || html.contains("Show QR"));
    }

    #[tokio::test]
    async fn renders_liquid_uri_html() {
        let html = parse_html(LIQUID_URI).await;

        assert!(html.contains("Liquid URI"));
        assert!(html.contains("100,000 sats"));
        assert!(html.contains("Donation"));
        assert!(html.contains("Thanks Liquid"));
        assert!(html.contains("6f0279e9ed041c3d710a9f57d0c02928416460c4b722ae3457a11eec381c526d"));
    }

    #[tokio::test]
    async fn renders_silent_payment_address_html() {
        let html = parse_html(SILENT_PAYMENT_ADDRESS).await;

        assert!(html.contains("Silent Payment Address"));
        assert!(html.contains("Mainnet"));
        assert!(html.contains("Scan public key"));
        assert!(html.contains("Spend public key"));
    }

    #[tokio::test]
    async fn renders_ark_address_html() {
        let html = parse_html(ARK_ADDRESS).await;

        assert!(html.contains("Ark Address"));
        assert!(html.contains("mainnet"));
        assert!(html.contains("Ark ID"));
        assert!(html.contains("Policy"));
        assert!(html.contains("Delivery"));
    }

    #[tokio::test]
    async fn renders_bolt11_invoice_html() {
        let html = parse_html(BOLT11_INVOICE).await;

        assert!(html.contains("BOLT-11 Invoice"));
        assert!(html.contains("Investigative Findings"));
        assert!(html.contains("Details"));
        assert!(html.contains("Bitcoin"));
        assert!(html.contains("Please consider supporting this project"));
        assert!(html.contains("0001020304050607080900010203040506070809000102030405060708090102"));
        assert!(html.contains("03e7156ae33b0a208d0744199163177e909e80176e55d97a2f221ede0f934dd9ad"));
        assert!(html.contains("Payment secret"));
        assert!(html.contains("Minimum final CLTV expiry"));
        assert!(html.contains("Features"));
    }

    #[test]
    fn renders_lnurl_zap_support_html() {
        let html = render_lnurl_pay_with_zap(detective::ZapSupport {
            allowed: true,
            nostr_pubkey: Some(
                "9630f464cca6a5147aa8a35f0bcdd3ce485324e732fd39e09233b1d848238f31".to_string(),
            ),
            nostr_pubkey_valid: true,
        });

        assert!(html.contains("NIP-57 Zap Support"));
        assert!(html.contains("supported"));
        assert!(html.contains("Zap receipt signer"));
        assert!(!html.contains("Request NIP-57 Zap Invoice"));
        assert!(!html.contains("/api/lnurl-request-zap-invoice"));
    }

    #[test]
    fn renders_invalid_lnurl_zap_config_without_form() {
        let html = render_lnurl_pay_with_zap(detective::ZapSupport {
            allowed: true,
            nostr_pubkey: Some("not-a-pubkey".to_string()),
            nostr_pubkey_valid: false,
        });

        assert!(html.contains("invalid configuration"));
        assert!(!html.contains("Request NIP-57 Zap Invoice"));
    }

    #[tokio::test]
    async fn renders_error_html_for_invalid_input() {
        let html = parse_html("not a payment instruction").await;

        assert!(html.contains("class=\"error\""));
        assert!(html.contains("Error"));
        assert!(html.contains("file an issue on GitHub"));
    }

    fn render_lnurl_pay_with_zap(zap: detective::ZapSupport) -> String {
        let pay = detective::PayResponse {
            sendable_amount: detective::types::MsatRange::Between(Msat(1_000), Msat(1_000_000)),
            description: "Zap demo".to_string(),
            long_description: None,
            image: None,
            comment_allowed: Some(100),
            callback: "https://example.com/callback".to_string(),
            zap,
            metadata: Vec::new(),
        };
        LnurlTemplate {
            events: vec![JsonRpcEvent::Result(Ok(LnUrlResponse::Pay(pay)))],
        }
        .render()
        .unwrap()
    }
}
