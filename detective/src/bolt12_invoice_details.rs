use crate::bolt12::{format_supported_quantity, Amount, BlindedPath};
use crate::duration::format_duration;
use crate::features::Features;
use crate::types::Msat;
use bitcoin::hex::DisplayHex;
use chrono::{DateTime, Utc};
use lightning::offers::invoice::Bolt12Invoice;
use lightning::offers::static_invoice::StaticInvoice;

#[derive(Debug)]
pub struct Bolt12InvoiceDetails {
    pub chain: String,
    pub offer_chains: Vec<String>,
    pub amount: Option<Amount>,
    pub amount_msats: Option<Msat>,
    pub quantity: Option<u64>,
    pub supported_quantity: Option<String>,
    pub description: Option<String>,
    pub issuer: Option<String>,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
    pub absolute_expiry: Option<DateTime<Utc>>,
    pub relative_expiry: String,
    pub has_expired: bool,
    pub signing_pubkey: String,
    pub issuer_signing_pubkey: Option<String>,
    pub payer_signing_pubkey: Option<String>,
    pub payer_note: Option<String>,
    pub payment_hash: Option<String>,
    pub invoice_features: Features,
    pub offer_features: Features,
    pub signature: String,
    pub signable_hash: Option<String>,
    pub message_paths: Vec<BlindedPath>,
}

impl From<&Bolt12Invoice> for Bolt12InvoiceDetails {
    fn from(invoice: &Bolt12Invoice) -> Self {
        let chain = invoice.chain().to_string();
        let offer_chains = invoice
            .offer_chains()
            .unwrap_or_default()
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>();

        let amount = invoice.amount().map(Amount::from);
        let amount_msats = Some(Msat(invoice.amount_msats()));
        let quantity = invoice.quantity();
        let supported_quantity = invoice.supported_quantity().map(format_supported_quantity);
        let description = invoice.description().map(|v| v.to_string());
        let issuer = invoice.issuer().map(|v| v.to_string());
        let metadata = invoice.metadata().map(|v| v.as_hex().to_string());
        let created_at =
            DateTime::from_timestamp(invoice.created_at().as_secs() as i64, 0).unwrap();
        let absolute_expiry = invoice
            .absolute_expiry()
            .map(|d| DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap());
        let relative_expiry = format_duration(&invoice.relative_expiry());
        let has_expired = invoice.is_expired();
        let signing_pubkey = invoice.signing_pubkey().to_string();
        let issuer_signing_pubkey = invoice.issuer_signing_pubkey().map(|v| v.to_string());
        let payer_signing_pubkey = Some(invoice.payer_signing_pubkey().to_string());
        let payer_note = invoice.payer_note().map(|v| v.to_string());
        let payment_hash = Some(invoice.payment_hash().to_string());
        let invoice_features = invoice.invoice_features().into();
        let offer_features = invoice
            .offer_features()
            .map(Features::from)
            .unwrap_or_default();
        let signature = invoice.signature().to_string();
        let signable_hash = Some(invoice.signable_hash().as_hex().to_string());
        let message_paths = invoice
            .message_paths()
            .iter()
            .map(BlindedPath::from)
            .collect();

        Self {
            chain,
            offer_chains,
            amount,
            amount_msats,
            quantity,
            supported_quantity,
            description,
            issuer,
            metadata,
            created_at,
            absolute_expiry,
            relative_expiry,
            has_expired,
            signing_pubkey,
            issuer_signing_pubkey,
            payer_signing_pubkey,
            payer_note,
            payment_hash,
            invoice_features,
            offer_features,
            signature,
            signable_hash,
            message_paths,
        }
    }
}

#[derive(Debug)]
pub struct Bolt12StaticInvoiceDetails {
    pub chain: String,
    pub amount: Option<Amount>,
    pub supported_quantity: String,
    pub description: Option<String>,
    pub issuer: Option<String>,
    pub metadata: Option<String>,
    pub created_at: DateTime<Utc>,
    pub absolute_expiry: Option<DateTime<Utc>>,
    pub relative_expiry: String,
    pub has_expired: bool,
    pub signing_pubkey: String,
    pub issuer_signing_pubkey: Option<String>,
    pub invoice_features: Features,
    pub offer_features: Features,
    pub signature: String,
    pub message_paths: Vec<BlindedPath>,
    pub offer_message_paths: Vec<BlindedPath>,
}

impl From<&StaticInvoice> for Bolt12StaticInvoiceDetails {
    fn from(invoice: &StaticInvoice) -> Self {
        let chain = invoice.chain().to_string();
        let amount = invoice.amount().map(Amount::from);
        let supported_quantity = format_supported_quantity(invoice.supported_quantity());
        let description = invoice.description().map(|v| v.to_string());
        let issuer = invoice.issuer().map(|v| v.to_string());
        let metadata = invoice.metadata().map(|v| v.as_hex().to_string());
        let created_at =
            DateTime::from_timestamp(invoice.created_at().as_secs() as i64, 0).unwrap();
        let absolute_expiry = invoice
            .absolute_expiry()
            .map(|d| DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap());
        let relative_expiry = format_duration(&invoice.relative_expiry());
        let has_expired = invoice.is_expired();
        let signing_pubkey = invoice.signing_pubkey().to_string();
        let issuer_signing_pubkey = invoice.issuer_signing_pubkey().map(|v| v.to_string());
        let invoice_features = invoice.invoice_features().into();
        let offer_features = invoice.offer_features().into();
        let signature = invoice.signature().to_string();
        let message_paths = invoice
            .message_paths()
            .iter()
            .map(BlindedPath::from)
            .collect();
        let offer_message_paths = invoice
            .offer_message_paths()
            .iter()
            .map(BlindedPath::from)
            .collect();

        Self {
            chain,
            amount,
            supported_quantity,
            description,
            issuer,
            metadata,
            created_at,
            absolute_expiry,
            relative_expiry,
            has_expired,
            signing_pubkey,
            issuer_signing_pubkey,
            invoice_features,
            offer_features,
            signature,
            message_paths,
            offer_message_paths,
        }
    }
}
