#![warn(unused_crate_dependencies)]

mod bip353;
mod chain_hash;
pub mod decoder;
mod features;
mod graph_database;
mod invoice_details;
mod ldk_node;
mod liquid_address;
mod lnurl;
mod node;
pub mod offer_details;
mod onchain_address;
mod recipient;
mod spark;
pub mod types;

pub use crate::bip353::{resolve_bip353, Bip353Result};
pub use crate::features::{Feature, FeatureFlag, Features};
use crate::graph_database::GraphDatabase;
pub use crate::invoice_details::{
    Description, InvoiceDetails, RouteHintDetails, RouteHintHopDetails,
};
pub use crate::ldk_node::{request_bolt12_invoice, OnionEvent, PayOfferParams};
pub use crate::liquid_address::{parse_liquid_uri, LiquidAddress, LiquidNetwork, LiquidUri};
pub use crate::lnurl::{
    request_invoice, resolve_lnurl, Image, JsonRpcEvent, LightningAddress, LnUrlResponse,
    PayResponse,
};
pub use crate::node::Node;
use crate::recipient::RecipientDecoder;
pub use crate::recipient::{RecipientNode, ServiceKind};
use crate::spark::detect_spark_address;
use anyhow::{anyhow, Error, Result};
use bitcoin::secp256k1::PublicKey;
use lightning::blinded_path::message::BlindedMessagePath;
use lightning::blinded_path::IntroductionNode;
use lightning::offers::offer::Offer;
use lightning_invoice::{Bolt11Invoice, RouteHint};
pub use onchain_address::OnchainAddress;
pub use silentpayments::SilentPaymentAddress;

const BOLTZ_MAGIC_ROUTING_HINT_CONSTANT: u64 = 596385002596073472;

#[derive(Debug)]
pub struct InvestigativeFindings {
    pub recipient: RecipientNode,
    pub payee: Node,
    pub route_hints: Vec<Vec<Node>>,
    pub botlz_mrh_pubkey: Option<String>,
}

pub struct InvoiceDetective {
    graph_database: GraphDatabase,
    recipient_decoder: RecipientDecoder,
}

impl InvoiceDetective {
    pub fn new() -> Result<Self> {
        const DATABASE_PATH: &str = "./graph.db3";
        let graph_database = GraphDatabase::open(DATABASE_PATH)?;
        let recipient_decoder = RecipientDecoder::new();
        Ok(Self {
            graph_database,
            recipient_decoder,
        })
    }

    pub fn investigate(&self, invoice: &str) -> Result<InvestigativeFindings> {
        let invoice = invoice
            .trim()
            .parse::<Bolt11Invoice>()
            .map_err(Error::msg)?;
        self.investigate_bolt11(&invoice)
    }

    pub fn investigate_bolt11(&self, invoice: &Bolt11Invoice) -> Result<InvestigativeFindings> {
        let pubkey = invoice
            .payee_pub_key()
            .copied()
            .unwrap_or_else(|| invoice.recover_payee_pub_key())
            .to_string();
        let payee = self.graph_database.query(pubkey.clone())?;
        let route_hints = self.process_route_hints(&invoice.route_hints())?;
        let recipient = self.recipient_decoder.decode(&pubkey, &route_hints);
        let recipient = match recipient {
            RecipientNode::NonCustodial { lsp, .. } if lsp.service == ServiceKind::Spark => {
                // TODO: Handle None value better.
                let id = detect_spark_address(invoice).unwrap_or_default();
                RecipientNode::NonCustodial { id, lsp }
            }
            recipient => recipient,
        };

        let botlz_mrh_pubkey = invoice
            .private_routes()
            .iter()
            .flat_map(|route| &route.0)
            .find(|hint| hint.short_channel_id == BOLTZ_MAGIC_ROUTING_HINT_CONSTANT)
            .map(|h| h.src_node_id.to_string());
        if let Some(ref botlz_mrh_pubkey) = botlz_mrh_pubkey {
            println!("Invoice has magic routing hint: {botlz_mrh_pubkey:?}");
        }

        Ok(InvestigativeFindings {
            recipient,
            payee,
            route_hints,
            botlz_mrh_pubkey,
        })
    }

    pub fn investigate_bolt12(&self, offer: Offer) -> Result<InvestigativeFindings> {
        if offer.paths().is_empty() {
            let pubkey = offer
                .issuer_signing_pubkey()
                .ok_or(anyhow!("Blinded path and signing key are empty"))?
                .to_string();
            let payee = self.graph_database.query(pubkey.clone())?;
            let recipient = self.recipient_decoder.decode(&pubkey, &Vec::new());

            return Ok(InvestigativeFindings {
                recipient,
                payee,
                route_hints: Vec::new(),
                botlz_mrh_pubkey: None,
            });
        }

        let introduction_node = offer
            .paths()
            .first()
            .map(BlindedMessagePath::introduction_node);
        let destination = match introduction_node {
            Some(IntroductionNode::NodeId(introduction_node_id)) => Destination::Blinded {
                introduction_node_id: *introduction_node_id,
            },
            Some(IntroductionNode::DirectedShortChannelId(_direction, _channel_id)) => {
                unimplemented!();
            }
            None => Destination::Node(
                offer
                    .issuer_signing_pubkey()
                    .ok_or(anyhow!("Blinded path and signing key are empty"))?,
            ),
        };
        let pubkey = destination.pubkey().to_string();
        let payee = self.graph_database.query(pubkey.clone())?;
        let recipient = self.recipient_decoder.decode(&pubkey, &Vec::new());

        Ok(InvestigativeFindings {
            recipient,
            payee,
            route_hints: Vec::new(),
            botlz_mrh_pubkey: None,
        })
    }

    fn process_route_hints(&self, route_hints: &Vec<RouteHint>) -> Result<Vec<Vec<Node>>> {
        let mut result = Vec::new();
        for hint in route_hints {
            let mut x = Vec::new();
            for hop in &hint.0 {
                let node = self.graph_database.query(hop.src_node_id.to_string())?;
                x.push(node);
            }
            result.push(x);
        }
        Ok(result)
    }
}

#[derive(Debug)]
enum Destination {
    Node(PublicKey),
    Blinded { introduction_node_id: PublicKey },
}

impl Destination {
    fn pubkey(&self) -> &PublicKey {
        match self {
            Destination::Node(key) => key,
            Destination::Blinded {
                introduction_node_id,
            } => introduction_node_id,
        }
    }
}
