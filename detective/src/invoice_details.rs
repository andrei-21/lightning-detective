use chrono::{DateTime, Utc};
use lightning_invoice::{Bolt11Invoice, Bolt11InvoiceDescriptionRef, RouteHint, RouteHintHop};
use std::fmt::Write;
use std::time::Duration;
use thousands::Separable;

#[derive(Debug)]
pub enum FeatureFlag {
    Required,
    Supported,
    NotSupported,
}

#[derive(Debug, Default)]
pub struct InvoiceDetails {
    pub network: String,
    pub description: Description,
    pub amount: Option<String>,
    pub payment_hash: String,
    pub payment_secret: String,
    pub payment_metadata: Option<String>,
    pub features: Option<Vec<(String, FeatureFlag)>>,
    pub created_at: DateTime<Utc>,
    pub expiry: String,
    pub has_expired: bool,
    pub min_final_cltv_expiry_delta: String,
    pub fallback_addresses: Vec<String>,
    pub route_hints: Vec<RouteHintDetails>,
    pub payee_pub_key: String,
    pub payee_pub_key_recovered: bool,
    pub signable_hash: String,
}

#[derive(Debug, Clone)]
pub struct RouteHintHopDetails {
    pub src_node_id: String,
    pub short_channel_id: u64,
    pub base_fee: String,
    pub proportional_fee: String,
    pub cltv_expiry_delta: String,
    pub htlc_limits: String,
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

fn format_limits(min_msat: Option<u64>, max_msat: Option<u64>) -> String {
    match (min_msat, max_msat) {
        (None, None) => "any".to_string(),
        (Some(min_msat), None) => format!("≥ {}", format_msat(min_msat)),
        (None, Some(max_msat)) => format!("≤ {}", format_msat(max_msat)),
        (Some(min_msat), Some(max_msat)) => {
            format!("{}–{}", format_msat_0(min_msat), format_msat_0(max_msat))
        }
    }
}

impl From<RouteHintHop> for RouteHintHopDetails {
    fn from(hop: RouteHintHop) -> Self {
        Self {
            src_node_id: hop.src_node_id.to_string(),
            short_channel_id: hop.short_channel_id,
            base_fee: format_msat(hop.fees.base_msat as u64),
            proportional_fee: format_proportional(hop.fees.proportional_millionths),
            cltv_expiry_delta: format_number_of_blocks(hop.cltv_expiry_delta as u64),
            htlc_limits: format_limits(hop.htlc_minimum_msat, hop.htlc_maximum_msat),
        }
    }
}

fn to_lower_hex(data: impl AsRef<[u8]>) -> String {
    let bytes = data.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(&mut output, "{:02x}", byte);
    }
    output
}

fn format_msat_0(msat: u64) -> String {
    match msat {
        1000 => "1".to_string(),
        msat if msat % 1000 == 0 => (msat / 1000).separate_with_commas().to_string(),
        msat => {
            let sat = msat / 1000;
            let sat = sat.separate_with_commas();
            let msat = msat % 1000;
            format!("{sat}.{msat:03}")
        }
    }
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

fn format_proportional(ppm: u32) -> String {
    let percents = ppm / 10_000;
    let fraction = ppm % 10_000;
    if fraction == 0 {
        return format!("{percents}%");
    }
    let fraction = format!("{fraction:04}");
    let fraction = fraction.trim_end_matches('0');
    format!("{percents}.{fraction}%")
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
        let expiry = format_duration(&invoice.expiry_time());
        let has_expired = invoice.is_expired();
        let min_final_cltv_expiry_delta =
            format_number_of_blocks(invoice.min_final_cltv_expiry_delta());
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
            expiry,
            has_expired,
            min_final_cltv_expiry_delta,
            fallback_addresses,
            route_hints,
            payee_pub_key,
            payee_pub_key_recovered,
            signable_hash,
        }
    }
}

fn format_duration(duration: &Duration) -> String {
    let secs = duration.as_secs();
    let (days, hrs, mins, secs) = (
        secs / 86400,
        (secs % 86400) / 3600,
        (secs % 3600) / 60,
        secs % 60,
    );

    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{days} day{}", plural(days)));
    }
    if hrs > 0 {
        parts.push(format!("{hrs} hour{}", plural(hrs)));
    }
    if mins > 0 {
        parts.push(format!("{mins} min{}", plural(mins)));
    }
    if secs > 0 || parts.is_empty() {
        parts.push(format!("{secs} second{}", plural(secs)));
    }
    parts.join(", ")
}

fn plural(number: u64) -> &'static str {
    if number == 1 {
        ""
    } else {
        "s"
    }
}

fn format_number_of_blocks(number: u64) -> String {
    let s = if number == 1 { "" } else { "s" };
    let duration = Duration::from_secs(60 * 10 * number);
    let duration = format_duration(&duration);
    format!("{number} block{s} (≈ {duration})")
}

#[cfg(test)]
mod tests {
    use super::format_proportional;

    #[test]
    fn test_format_proportional() {
        assert_eq!(format_proportional(120_000), "12%");
        assert_eq!(format_proportional(120_100), "12.01%");
        assert_eq!(format_proportional(120_010), "12.001%");
        assert_eq!(format_proportional(120_001), "12.0001%");
        assert_eq!(format_proportional(120_110), "12.011%");
    }
}
