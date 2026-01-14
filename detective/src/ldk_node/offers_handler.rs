use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::{anyhow, ensure, Result};
use bitcoin::secp256k1;
use lightning::blinded_path::message::OffersContext;
use lightning::ln::channelmanager::PaymentId;
use lightning::ln::inbound_payment::ExpandedKey;
use lightning::offers::invoice::Bolt12Invoice;
use lightning::offers::invoice_error::InvoiceError;
use lightning::offers::static_invoice::StaticInvoice;
use lightning::onion_message::messenger::{Responder, ResponseInstruction};
use lightning::onion_message::offers::{OffersMessage, OffersMessageHandler};
use tokio::sync::oneshot;

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone)]
pub enum Bolt12InvoiceResponse {
    Invoice(Bolt12Invoice),
    StaticInvoice(StaticInvoice),
    InvoiceError(InvoiceError),
}

pub struct OffersHandler {
    inbound_payment_key: ExpandedKey,
    secp_ctx: secp256k1::Secp256k1<secp256k1::All>,
    pending: Mutex<HashMap<PaymentId, oneshot::Sender<Result<Bolt12InvoiceResponse>>>>,
}

impl OffersHandler {
    pub fn new(inbound_payment_key: ExpandedKey) -> Self {
        Self {
            inbound_payment_key,
            secp_ctx: secp256k1::Secp256k1::new(),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn register(
        &self,
        payment_id: PaymentId,
    ) -> Result<oneshot::Receiver<Result<Bolt12InvoiceResponse>>> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().unwrap();
        ensure!(!pending.contains_key(&payment_id), "Duplicated payment id");
        pending.insert(payment_id, tx);
        Ok(rx)
    }

    fn resolve(&self, payment_id: PaymentId, res: Result<Bolt12InvoiceResponse>) {
        let tx = self.pending.lock().unwrap().remove(&payment_id);
        if let Some(tx) = tx {
            let _ = tx.send(res);
        }
    }
}

impl OffersMessageHandler for OffersHandler {
    fn handle_message(
        &self,
        message: OffersMessage,
        context: Option<OffersContext>,
        _responder: Option<Responder>,
    ) -> Option<(OffersMessage, ResponseInstruction)> {
        let (payment_id, nonce) = match context {
            Some(OffersContext::OutboundPayment { payment_id, nonce }) => (payment_id, nonce),
            _ => return None,
        };
        let response = match message {
            OffersMessage::Invoice(invoice) => {
                match invoice.verify_using_payer_data(
                    payment_id,
                    nonce,
                    &self.inbound_payment_key,
                    &self.secp_ctx,
                ) {
                    Ok(_) => Ok(Bolt12InvoiceResponse::Invoice(invoice)),
                    Err(()) => Err(anyhow!("Verification failed")),
                }
            }
            OffersMessage::StaticInvoice(static_invoice) => {
                // Static invoices do not provide proof-of-payment, but should still be correlated
                // to a pending outbound request via the reply path context.
                Ok(Bolt12InvoiceResponse::StaticInvoice(static_invoice))
            }
            OffersMessage::InvoiceError(error) => Ok(Bolt12InvoiceResponse::InvoiceError(error)),
            OffersMessage::InvoiceRequest(_) => return None,
        };

        self.resolve(payment_id, response);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightning::offers::nonce::Nonce;

    #[tokio::test]
    async fn delivers_invoice_error_to_registered_payment_id() {
        let entropy = lightning::sign::KeysManager::new(&[1u8; 32], 1, 1, true);
        let handler = OffersHandler::new(ExpandedKey::new([2u8; 32]));

        let payment_id = PaymentId([9u8; 32]);
        let nonce = Nonce::from_entropy_source(&entropy);
        let rx = handler.register(payment_id).unwrap();

        let error = InvoiceError::from_string("nope".to_string());
        handler.handle_message(
            OffersMessage::InvoiceError(error.clone()),
            Some(OffersContext::OutboundPayment { payment_id, nonce }),
            None,
        );

        let res = rx.await.unwrap().unwrap();
        match res {
            Bolt12InvoiceResponse::InvoiceError(e) => assert_eq!(e.to_string(), "nope"),
            other => panic!("unexpected response: {other:?}"),
        }
    }
}
