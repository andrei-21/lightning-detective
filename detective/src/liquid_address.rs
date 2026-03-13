use anyhow::{anyhow, bail, Error, Result};
use bitcoin::Denomination;
use elements::address::Payload;
use elements::{Address, AddressParams};
use std::fmt;
use std::str::FromStr;

use crate::types::Sat;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidNetwork {
    Liquid,
    LiquidTestnet,
    ElementsRegtest,
}

impl LiquidNetwork {
    pub fn from_uri_scheme(scheme: &str) -> Option<Self> {
        if scheme.eq_ignore_ascii_case("liquidnetwork") {
            Some(Self::Liquid)
        } else if scheme.eq_ignore_ascii_case("liquidtestnet") {
            Some(Self::LiquidTestnet)
        } else {
            None
        }
    }
}

impl fmt::Display for LiquidNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Liquid => write!(f, "Liquid"),
            Self::LiquidTestnet => write!(f, "Liquid testnet"),
            Self::ElementsRegtest => write!(f, "Elements regtest"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct LiquidAddress {
    pub address: String,
    pub address_type: Option<String>,
    pub valid_networks: Vec<LiquidNetwork>,
    pub is_confidential: bool,
}

impl FromStr for LiquidAddress {
    type Err = Error;

    fn from_str(address: &str) -> Result<Self, Self::Err> {
        let address = Address::from_str(address)?;
        let network = network_from_params(address.params)
            .ok_or(anyhow!("Failed to detect Liquid network from address"))?;

        Ok(Self {
            address: address.to_string(),
            address_type: Some(payload_type(&address.payload)),
            valid_networks: vec![network],
            is_confidential: address.is_blinded(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct LiquidUri {
    pub scheme: LiquidNetwork,
    pub address: Option<LiquidAddress>,
    pub amount: Option<String>,
    pub asset_id: Option<String>,
    pub label: Option<String>,
    pub message: Option<String>,
    pub unknown_params: Vec<(String, String)>,
}

pub fn parse_liquid_uri(uri: &str) -> Result<LiquidUri> {
    let (scheme, tail) = uri
        .split_once(':')
        .ok_or(anyhow!("Liquid URI must include ':' after the scheme"))?;

    let scheme = LiquidNetwork::from_uri_scheme(scheme).ok_or(anyhow!(
        "Unsupported Liquid URI scheme, expected `liquidnetwork:` or `liquidtestnet:`"
    ))?;

    let (address, params_raw) = tail.split_once('?').unwrap_or((tail, ""));
    let address = if address.is_empty() {
        None
    } else {
        let address = LiquidAddress::from_str(address)?;
        if !address.valid_networks.contains(&scheme) {
            bail!("Address network does not match URI scheme");
        }
        Some(address)
    };

    let mut amount = None;
    let mut asset_id = None;
    let mut label = None;
    let mut message = None;
    let mut unknown_params = Vec::new();

    if !params_raw.is_empty() {
        for param in params_raw.split('&') {
            if param.is_empty() {
                continue;
            }
            let (key, value) = param
                .split_once('=')
                .ok_or(anyhow!("Invalid URI param `{param}`"))?;
            let key = decode_percent(key)?;
            let value = decode_percent(value)?;

            match key.as_str() {
                "amount" => {
                    let parsed = bitcoin::Amount::from_str_in(&value, Denomination::Bitcoin)
                        .map_err(|e| anyhow!("Failed to decode amount param: {e}"))?;
                    amount = Some(Sat::from(parsed).to_string());
                }
                "assetid" => asset_id = Some(value),
                "label" => label = Some(value),
                "message" => message = Some(value),
                _ => unknown_params.push((key, value)),
            }
        }
    }

    Ok(LiquidUri {
        scheme,
        address,
        amount,
        asset_id,
        label,
        message,
        unknown_params,
    })
}

fn network_from_params(params: &'static AddressParams) -> Option<LiquidNetwork> {
    if params == &AddressParams::LIQUID {
        Some(LiquidNetwork::Liquid)
    } else if params == &AddressParams::LIQUID_TESTNET {
        Some(LiquidNetwork::LiquidTestnet)
    } else if params == &AddressParams::ELEMENTS {
        Some(LiquidNetwork::ElementsRegtest)
    } else {
        None
    }
}

fn payload_type(payload: &Payload) -> String {
    match payload {
        Payload::PubkeyHash(_) => "p2pkh".to_string(),
        Payload::ScriptHash(_) => "p2sh".to_string(),
        Payload::WitnessProgram { version, program } => match (version.to_u8(), program.len()) {
            (0, 20) => "p2wpkh".to_string(),
            (0, 32) => "p2wsh".to_string(),
            (1, 32) => "p2tr".to_string(),
            (v, _) => format!("witness_v{v}"),
        },
    }
}

fn decode_percent(input: &str) -> Result<String> {
    Ok(percent_encoding_rfc3986::percent_decode_str(input)
        .map_err(|e| anyhow!("Failed to decode URI param: {e}"))?
        .decode_utf8()
        .map_err(|e| anyhow!("Failed to decode URI param: {e}"))?
        .to_string())
}
