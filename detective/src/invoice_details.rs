use crate::{FeatureFlag, InvoiceDetails};
use chrono::{DateTime, Utc};
use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescriptionRef, RouteHint, RouteHintHop};
use std::convert::TryInto;
use std::fmt::Write;
use std::time::{Duration, SystemTime};
use thousands::Separable;

#[derive(Debug, Clone)]
pub struct RouteHintHopDetails {
    pub src_node_id: String,
    pub short_channel_id: u64,
    pub fees_base_msat: u32,
    pub fees_proportional_millionths: u32,
    pub cltv_expiry_delta: u16,
    pub htlc_minimum_msat: Option<u64>,
    pub htlc_maximum_msat: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct RouteHintDetails {
    pub hops: Vec<RouteHintHopDetails>,
}

impl From<RouteHint> for RouteHintDetails {
    fn from(route: RouteHint) -> Self {
        Self {
            hops: route.0.into_iter().map(RouteHintHopDetails::from).collect(),
        }
    }
}

impl From<RouteHintHop> for RouteHintHopDetails {
    fn from(hop: RouteHintHop) -> Self {
        Self {
            src_node_id: hop.src_node_id.to_string(),
            short_channel_id: hop.short_channel_id,
            fees_base_msat: hop.fees.base_msat,
            fees_proportional_millionths: hop.fees.proportional_millionths,
            cltv_expiry_delta: hop.cltv_expiry_delta,
            htlc_minimum_msat: hop.htlc_minimum_msat,
            htlc_maximum_msat: hop.htlc_maximum_msat,
        }
    }
}

fn duration_to_datetime(duration: Duration) -> Option<DateTime<Utc>> {
    let seconds: i64 = duration.as_secs().try_into().ok()?;
    DateTime::from_timestamp(seconds, duration.subsec_nanos())
}

fn to_lower_hex(data: impl AsRef<[u8]>) -> String {
    let bytes = data.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{:02x}", byte);
    }
    output
}

fn format_msat(msat: u64) -> String {
    match msat {
        1000 => "1 sat".to_string(),
        msat if msat % 1000 == 0 => format!("{} sats", (msat / 1000).separate_with_commas()),
        msat => {
            let sat = msat / 1000;
            let sat = sat.separate_with_commas();
            let msat = msat % 1000;
            format!("{sat}.{msat:03} sats")
        }
    }
}

fn to_features(features: String) -> Vec<(String, FeatureFlag)> {
    let mut result = Vec::new();
    for feature in features.split(", ") {
        let (feature, flag) = feature.split_once(": ").unwrap();
        if feature == "unknown flags" {
            // TODO: Handle.
            continue;
        }
        let flag = match flag {
            "required" => FeatureFlag::Required,
            "supported" => FeatureFlag::Supported,
            "not supported" => FeatureFlag::NotSupported,
            _ => panic!(),
        };
        result.push((feature.to_string(), flag));
    }
    result
}

#[derive(Debug)]
pub enum Description {
    Direct(String),
    Hash(String),
}

impl Default for Description {
    fn default() -> Self {
        Description::Direct(String::new())
    }
}

impl From<&Bolt11Invoice> for InvoiceDetails {
    fn from(invoice: &Bolt11Invoice) -> Self {
        let network = invoice.network().to_string();
        let description = match invoice.description() {
            Bolt11InvoiceDescriptionRef::Direct(description) => {
                Description::Direct(description.to_string())
            }
            Bolt11InvoiceDescriptionRef::Hash(hash) => Description::Hash(to_lower_hex(hash.0)),
        };
        let amount = invoice.amount_milli_satoshis().map(format_msat);
        let payment_hash = invoice.payment_hash().to_string();
        let payment_secret = invoice.payment_secret().to_string();
        let payment_metadata = invoice.payment_metadata().map(to_lower_hex);
        let features = invoice.features().map(|f| to_features(f.to_string()));
        let created_at = invoice.timestamp().into();
        let expires_at = invoice.expires_at().and_then(duration_to_datetime);
        let now_duration = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_else(|_| Duration::from_secs(0));
        let has_expired = invoice.would_expire(now_duration);
        let expiry = invoice.expiry_time();
        let min_final_cltv_expiry_delta = invoice.min_final_cltv_expiry_delta();
        let fallback_addresses = invoice
            .fallback_addresses()
            .into_iter()
            .map(|address| address.to_string())
            .collect();
        let route_hints = invoice
            .route_hints()
            .into_iter()
            .map(RouteHintDetails::from)
            .collect();
        let payee_pub_key = invoice.get_payee_pub_key().to_string();
        let payee_pub_key_recovered = invoice.payee_pub_key().is_none();
        let signable_hash = to_lower_hex(invoice.signable_hash());

        Self {
            network,
            description,
            amount,
            payment_hash,
            payment_secret,
            payment_metadata,
            features,
            created_at,
            expires_at,
            has_expired,
            expiry,
            min_final_cltv_expiry_delta,
            fallback_addresses,
            route_hints,
            payee_pub_key,
            payee_pub_key_recovered,
            signable_hash,
        }
    }
}
