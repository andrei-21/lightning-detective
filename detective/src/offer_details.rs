pub use crate::bolt12::{Amount, BlindedPath, IntroductionNode};
use crate::features::Features;
use crate::{bolt12::format_supported_quantity, chain_hash::ChainHash};
use bitcoin::hex::DisplayHex;
use chrono::{DateTime, Utc};
use lightning::offers::offer::Offer;

#[derive(Debug)]
pub struct OfferDetails {
    pub id: String,
    pub raw_offer: String,
    pub chains: Vec<String>,
    pub amount: Option<Amount>,
    pub supported_quantity: String,
    pub description: Option<String>,
    pub issuer: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub has_expired: bool,
    pub metadata: Option<String>,
    pub features: Features,
    pub signing_pubkey: Option<String>,
    pub paths: Vec<BlindedPath>,
}

impl From<Offer> for OfferDetails {
    fn from(offer: Offer) -> Self {
        let raw_offer = offer.to_string();
        let mut chains = offer
            .chains()
            .into_iter()
            .map(ChainHash::from)
            .collect::<Vec<_>>();
        chains.sort();
        let chains = chains.into_iter().map(|c| c.to_string()).collect();

        let amount = offer.amount().map(Amount::from);
        let supported_quantity = format_supported_quantity(offer.supported_quantity());
        let description = offer.description().map(|s| s.to_string());
        let issuer = offer.issuer().map(|s| s.to_string());
        let expires_at = offer
            .absolute_expiry()
            .map(|d| DateTime::from_timestamp(d.as_secs() as i64, 0).unwrap());
        let has_expired = offer.is_expired();
        let metadata = offer.metadata().map(|s| s.as_hex().to_string());
        let features = offer.offer_features().into();

        let signing_pubkey = offer.issuer_signing_pubkey().map(|k| k.to_string());
        let paths = offer.paths().iter().map(BlindedPath::from).collect();

        Self {
            id: offer.id().0.as_hex().to_string(),
            raw_offer,
            chains,
            amount,
            supported_quantity,
            description,
            issuer,
            expires_at,
            has_expired,
            metadata,
            features,
            signing_pubkey,
            paths,
        }
    }
}
