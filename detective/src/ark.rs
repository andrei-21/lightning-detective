use std::fmt;
use std::str::FromStr;

use bark::ark::address::{Address as BarkAddress, VtxoDelivery as BarkVtxoDelivery};
use bark::ark::VtxoPolicy as BarkVtxoPolicy;
use bitcoin::hex::DisplayHex;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArkAddress {
    pub address: String,
    pub network: ArkNetwork,
    pub ark_id: ArkId,
    pub policy: VtxoPolicy,
    pub delivery: Vec<VtxoDelivery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArkNetwork {
    Mainnet,
    Testnet,
}

impl fmt::Display for ArkNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mainnet => f.write_str("mainnet"),
            Self::Testnet => f.write_str("testnet"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArkId(pub String);

impl fmt::Display for ArkId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VtxoPolicy {
    Pubkey {
        user_pubkey: String,
    },
    ServerHtlcSend {
        user_pubkey: String,
        payment_hash: String,
        htlc_expiry: u32,
    },
    ServerHtlcRecv {
        user_pubkey: String,
        payment_hash: String,
        htlc_expiry: u32,
        htlc_expiry_delta: u16,
    },
}

impl VtxoPolicy {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Pubkey { .. } => "pubkey",
            Self::ServerHtlcSend { .. } => "server HTLC send",
            Self::ServerHtlcRecv { .. } => "server HTLC receive",
        }
    }
}

impl fmt::Display for VtxoPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Pubkey { user_pubkey } => {
                write!(f, "{} policy for user pubkey {user_pubkey}", self.kind())
            }
            Self::ServerHtlcSend {
                user_pubkey,
                payment_hash,
                htlc_expiry,
            } => write!(
                f,
                "{} policy for user pubkey {user_pubkey}, payment hash {payment_hash}, HTLC expiry {htlc_expiry}",
                self.kind()
            ),
            Self::ServerHtlcRecv {
                user_pubkey,
                payment_hash,
                htlc_expiry,
                htlc_expiry_delta,
            } => write!(
                f,
                "{} policy for user pubkey {user_pubkey}, payment hash {payment_hash}, HTLC expiry {htlc_expiry}, HTLC expiry delta {htlc_expiry_delta}",
                self.kind()
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VtxoDelivery {
    ServerMailbox { blinded_id: String },
    Unknown { delivery_type: u8, data: String },
    Unsupported { description: String },
}

impl VtxoDelivery {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::ServerMailbox { .. } => "server mailbox",
            Self::Unknown { .. } => "unknown",
            Self::Unsupported { .. } => "unsupported",
        }
    }
}

impl fmt::Display for VtxoDelivery {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServerMailbox { blinded_id } => {
                write!(
                    f,
                    "{} delivery to blinded mailbox {blinded_id}",
                    self.kind()
                )
            }
            Self::Unknown {
                delivery_type,
                data,
            } => write!(
                f,
                "{} delivery type {delivery_type} with payload {data}",
                self.kind()
            ),
            Self::Unsupported { description } => {
                write!(f, "{} delivery: {description}", self.kind())
            }
        }
    }
}

impl FromStr for ArkAddress {
    type Err = bark::ark::address::ParseAddressError;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let address = BarkAddress::from_str(input)?;
        Ok(Self::from(address))
    }
}

impl fmt::Display for ArkAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.address)
    }
}

impl From<BarkAddress> for ArkAddress {
    fn from(address: BarkAddress) -> Self {
        let network = match address.is_testnet() {
            true => ArkNetwork::Testnet,
            false => ArkNetwork::Mainnet,
        };
        let ark_id = ArkId(address.ark_id().to_string());
        let policy = VtxoPolicy::from(address.policy());
        let delivery = address.delivery().iter().map(VtxoDelivery::from).collect();

        Self {
            address: address.to_string(),
            network,
            ark_id,
            policy,
            delivery,
        }
    }
}

impl From<&BarkVtxoPolicy> for VtxoPolicy {
    fn from(policy: &BarkVtxoPolicy) -> Self {
        match policy {
            BarkVtxoPolicy::Pubkey(policy) => Self::Pubkey {
                user_pubkey: policy.user_pubkey.to_string(),
            },
            BarkVtxoPolicy::ServerHtlcSend(policy) => Self::ServerHtlcSend {
                user_pubkey: policy.user_pubkey.to_string(),
                payment_hash: policy.payment_hash.to_string(),
                htlc_expiry: policy.htlc_expiry,
            },
            BarkVtxoPolicy::ServerHtlcRecv(policy) => Self::ServerHtlcRecv {
                user_pubkey: policy.user_pubkey.to_string(),
                payment_hash: policy.payment_hash.to_string(),
                htlc_expiry: policy.htlc_expiry,
                htlc_expiry_delta: policy.htlc_expiry_delta,
            },
        }
    }
}

impl From<&BarkVtxoDelivery> for VtxoDelivery {
    fn from(delivery: &BarkVtxoDelivery) -> Self {
        match delivery {
            BarkVtxoDelivery::ServerMailbox { blinded_id } => Self::ServerMailbox {
                blinded_id: blinded_id.to_string(),
            },
            BarkVtxoDelivery::Unknown {
                delivery_type,
                data,
            } => Self::Unknown {
                delivery_type: *delivery_type,
                data: data.as_hex().to_string(),
            },
            other => Self::Unsupported {
                description: format!("{other:?}"),
            },
        }
    }
}

pub fn is_ark_address(input: &str) -> bool {
    let lowercased = input.to_ascii_lowercase();
    (lowercased.starts_with("ark1p") || lowercased.starts_with("tark1p"))
        && input.chars().all(|c| c.is_ascii_alphanumeric())
}
