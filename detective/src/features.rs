use bitcoin::hex::DisplayHex;

#[derive(Debug)]
pub enum FeatureFlag {
    Required,
    Supported,
    NotSupported,
}

pub type Feature = (String, FeatureFlag);

#[derive(Debug, Default)]
pub struct Features {
    pub hex: Option<String>,
    pub features: Vec<Feature>,
}

macro_rules! impl_from_ldk_features {
    ($($ldk_type:ty),+ $(,)?) => {
        $(
            impl From<&$ldk_type> for Features {
                fn from(features: &$ldk_type) -> Self {
					match features.le_flags() {
						[] => Default::default(),
						bytes => {
							let hex = Some(bytes.to_lower_hex_string());
							let features = parse_features(&features.to_string());
							Self { hex, features }
						}
					}
                }
            }
        )+
    };
}

impl_from_ldk_features!(
    lightning::types::features::InitFeatures,
    lightning::types::features::NodeFeatures,
    lightning::types::features::ChannelFeatures,
    lightning::types::features::Bolt11InvoiceFeatures,
    lightning::types::features::OfferFeatures,
    lightning::types::features::InvoiceRequestFeatures,
    lightning::types::features::Bolt12InvoiceFeatures,
    lightning::types::features::BlindedHopFeatures,
    lightning::types::features::ChannelTypeFeatures,
);

pub fn parse_features(features: &str) -> Vec<Feature> {
    features
        .split(", ")
        .filter_map(|feature| match feature.split_once(": ") {
            Some((name, "required")) => Some((name.to_string(), FeatureFlag::Required)),
            Some((name, "supported")) => Some((name.to_string(), FeatureFlag::Supported)),
            Some((name, "not supported")) => Some((name.to_string(), FeatureFlag::NotSupported)),
            _ => None,
        })
        .collect()
}
