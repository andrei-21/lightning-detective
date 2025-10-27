use anyhow::{bail, Error, Result};
use bitcoin::hex::DisplayHex;
use bitcoin_payment_instructions::dns_resolver::DNSHrnResolver;
use bitcoin_payment_instructions::hrn_resolution::{HrnResolution, HrnResolver, HumanReadableName};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

pub struct Bip353Result {
    pub bip21: String,
    pub proof: String,
}

pub async fn resolve_bip353(name: &HumanReadableName) -> Result<Bip353Result> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)), 53);
    let resolver = DNSHrnResolver(addr);
    let result = resolver.resolve_hrn(name).await.map_err(Error::msg)?;
    match result {
        HrnResolution::DNSSEC {
            proof: Some(proof),
            result,
        } => Ok(Bip353Result {
            bip21: result,
            proof: proof.to_lower_hex_string(),
        }),
        HrnResolution::DNSSEC { proof: None, .. } => bail!("DNS resolution result misses proof"),
        HrnResolution::LNURLPay { .. } => bail!("Unexpected LNURLPay result on DNS resolution"),
    }
}
