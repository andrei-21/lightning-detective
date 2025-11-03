use bech32::{self, Bech32m, Hrp};
use bitcoin::secp256k1::PublicKey;
use lightning_invoice::Bolt11Invoice;
use std::convert::TryFrom;

const RECEIVER_IDENTITY_PUBLIC_KEY_SHORT_CHANNEL_ID: u64 = 17592187092992000001;

pub(crate) fn detect_spark_address(invoice: &Bolt11Invoice) -> Option<String> {
    let network = SparkNetwork::try_from(invoice.network()).ok()?;
    for route_hint in invoice.route_hints() {
        for hop in route_hint.0 {
            if hop.short_channel_id == RECEIVER_IDENTITY_PUBLIC_KEY_SHORT_CHANNEL_ID {
                return spark_address_string(&hop.src_node_id, network);
            }
        }
    }
    None
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum SparkNetwork {
    Mainnet,
    Regtest,
    Signet,
    Testnet,
}

impl SparkNetwork {
    fn hrp(self) -> Hrp {
        match self {
            SparkNetwork::Mainnet => Hrp::parse_unchecked("spark"),
            SparkNetwork::Regtest => Hrp::parse_unchecked("sparkrt"),
            SparkNetwork::Signet => Hrp::parse_unchecked("sparks"),
            SparkNetwork::Testnet => Hrp::parse_unchecked("sparkt"),
        }
    }
}

impl TryFrom<bitcoin::Network> for SparkNetwork {
    type Error = ();

    fn try_from(value: bitcoin::Network) -> Result<Self, Self::Error> {
        Ok(match value {
            bitcoin::Network::Bitcoin => Self::Mainnet,
            bitcoin::Network::Regtest => Self::Regtest,
            bitcoin::Network::Signet => Self::Signet,
            bitcoin::Network::Testnet | bitcoin::Network::Testnet4 => Self::Testnet,
        })
    }
}

fn spark_address_string(identity_public_key: &PublicKey, network: SparkNetwork) -> Option<String> {
    let compressed = identity_public_key.serialize();
    let mut payload = Vec::with_capacity(1 + 1 + compressed.len());
    encode_bytes_field(1, &compressed, &mut payload);
    bech32::encode::<Bech32m>(network.hrp(), &payload).ok()
}

fn encode_bytes_field(field_number: u32, data: &[u8], buffer: &mut Vec<u8>) {
    const WIRE_TYPE_LENGTH_DELIMITED: u64 = 2;
    let key = ((field_number as u64) << 3) | WIRE_TYPE_LENGTH_DELIMITED;
    encode_varint(key, buffer);
    encode_varint(data.len() as u64, buffer);
    buffer.extend_from_slice(data);
}

fn encode_varint(mut value: u64, buffer: &mut Vec<u8>) {
    while value >= 0x80 {
        buffer.push(((value as u8) & 0x7f) | 0x80);
        value >>= 7;
    }
    buffer.push(value as u8);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_spark_address_in_invoice() {
        let invoice = "lnbc1u1p5wdp0xpp5s3khzljgdv6t4jngpsm35hccssewwsusklrhxxsu28ngjw6xg77qsp5u3aqrswqlfxemzgkvqv9js2lqmyanmcvn95fcr6vpazsraf78zcqxq9z0rgqnp4qvyndeaqzman7h898jxm98dzkm0mlrsx36s93smrur7h0azyyuxc5rzjq25carzepgd4vqsyn44jrk85ezrpju92xyrk9apw4cdjh6yrwt5jgqqqqrt49lmtcqqqqqqqqqqq86qq9qrzjqwghf7zxvfkxq5a6sr65g0gdkv768p83mhsnt0msszapamzx2qvuxqqqqrt49lmtcqqqqqqqqqqq86qq9qrzjqdzuk95ac59waxpymqfqynxcm6darlnz0lvutxkkl530z8l6wp9f0apyqr6zgqqqq8hxk2qqae4jsqyugqcqzpgdqq9qyyssqywn6dknvak25pa7hrmryrz2lxdv6t4fsc9zt3mrceu8vtxfv68xnu0ykd3qy20c96885ga8ca0ahmzqwq5plgeyf4hv0rcnrf45w4dspd8zz2k";
        let invoice = invoice.parse::<Bolt11Invoice>().expect("valid invoice");
        let detected = detect_spark_address(&invoice);
        assert_eq!(
            detected.as_deref(),
            Some("spark1pgssx3wtz6wu2zhwnqjdsyszfnvdax73le38lkw9ntt06gh3rla8qj5hh06qsh")
        );
    }
}
