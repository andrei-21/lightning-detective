use anyhow::Error;
use bitcoin::{Address, AddressType, Network};
use std::str::FromStr;

#[derive(Debug)]
pub struct OnchainAddress {
    pub address: String,
    pub address_type: Option<AddressType>,
    pub valid_networks: Vec<Network>,
}

impl FromStr for OnchainAddress {
    type Err = Error;
    fn from_str(address: &str) -> Result<Self, Error> {
        let address = Address::from_str(address)?;
        let valid_networks = [
            Network::Bitcoin,
            Network::Testnet,
            Network::Testnet4,
            Network::Signet,
            Network::Regtest,
        ]
        .into_iter()
        .filter(|n| address.is_valid_for_network(*n))
        .collect();
        let address = address.assume_checked();

        Ok(Self {
            address: address.to_string(),
            address_type: address.address_type(),
            valid_networks,
        })
    }
}
