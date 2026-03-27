mod node;
mod offers_handler;

use crate::{InvestigateValue, InvestigateValueKind};
use anyhow::{anyhow, Result};
use bitcoin::hex::DisplayHex;
use bitcoin::Network;
use lightning::offers::offer::Offer;
use lightning::util::ser::Writeable;
pub use node::{LdkNode, LdkNodeConfig, OnionEvent, PayOfferParams};
pub use offers_handler::Bolt12InvoiceResponse;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;

pub async fn request_bolt12_invoice(
    offer: Offer,
    params: PayOfferParams,
) -> impl Stream<Item = OnionEvent> {
    let (tx, rx) = mpsc::channel(100);
    tokio::spawn(async move {
        let result = request_bolt12_invoice_impl(&offer, params, tx.clone()).await;
        let result = match result {
            Ok(Bolt12InvoiceResponse::Invoice(invoice)) => Ok(InvestigateValue::new(
                InvestigateValueKind::Bolt12Invoice,
                invoice.encode().as_hex().to_string(),
            )
            .as_encoded()),
            Ok(Bolt12InvoiceResponse::StaticInvoice(invoice)) => Ok(InvestigateValue::new(
                InvestigateValueKind::Bolt12StaticInvoice,
                invoice.encode().as_hex().to_string(),
            )
            .as_encoded()),
            Ok(Bolt12InvoiceResponse::InvoiceError(e)) => Err(anyhow!("{e:?}")),
            Err(e) => Err(e),
        };
        let _ = tx.send(OnionEvent::Result(result)).await;
    });
    ReceiverStream::new(rx)
}

async fn request_bolt12_invoice_impl(
    offer: &Offer,
    params: PayOfferParams,
    events: mpsc::Sender<OnionEvent>,
) -> Result<Bolt12InvoiceResponse> {
    let seed = [42u8; 32];
    let config = LdkNodeConfig {
        network: Network::Bitcoin,
        seed,
        inbound_payment_key: seed,
        peer_manager_ephemeral_random_data: seed,
    };
    let node = LdkNode::new(config, events);
    node.start().await?;
    node.request_invoice(offer, params).await
}
