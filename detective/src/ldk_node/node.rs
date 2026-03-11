use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, bail, ensure, Error, Result};
use bitcoin::constants::ChainHash;
use bitcoin::key::Secp256k1;
use bitcoin::network::Network;
use bitcoin::secp256k1::PublicKey;
use lightning::blinded_path::message::{MessageContext, OffersContext};
use lightning::blinded_path::{Direction, EmptyNodeIdLookUp, IntroductionNode};
use lightning::ln::channelmanager::PaymentId;
use lightning::ln::inbound_payment::ExpandedKey;
use lightning::ln::msgs::SocketAddress;
use lightning::ln::peer_handler::{
    ErroringMessageHandler, IgnoringMessageHandler, MessageHandler, PeerManager,
};
use lightning::offers::nonce::Nonce;
use lightning::offers::offer::Offer;
use lightning::onion_message::messenger::{
    DefaultMessageRouter, Destination, MessageSendInstructions, OnionMessenger, SendSuccess,
};
use lightning::onion_message::offers::OffersMessage;
use lightning::routing::gossip::NetworkGraph;
use lightning::sign::EntropySource;
use lightning::sign::KeysManager;
use lightning::util::logger::{Level, Logger, Record};
use lightning_net_tokio::setup_outbound;
use lightning_rapid_gossip_sync::RapidGossipSync;
use log::{debug, RecordBuilder};
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::time::{sleep, Instant};

use crate::offer_details;

use super::offers_handler::{Bolt12InvoiceResponse, OffersHandler};

type LdkMessageRouter =
    DefaultMessageRouter<Arc<NetworkGraph<Arc<LdkLogger>>>, Arc<LdkLogger>, Arc<KeysManager>>;

type LdkRapidGossipSync = RapidGossipSync<Arc<NetworkGraph<Arc<LdkLogger>>>, Arc<LdkLogger>>;

type LdkOnionMessenger = OnionMessenger<
    Arc<KeysManager>,
    Arc<KeysManager>,
    Arc<LdkLogger>,
    Arc<EmptyNodeIdLookUp>,
    Arc<LdkMessageRouter>,
    Arc<OffersHandler>,
    Arc<IgnoringMessageHandler>,
    Arc<IgnoringMessageHandler>,
    Arc<IgnoringMessageHandler>,
>;

type LdkPeerManager = PeerManager<
    lightning_net_tokio::SocketDescriptor,
    Arc<ErroringMessageHandler>,
    Arc<IgnoringMessageHandler>,
    Arc<LdkOnionMessenger>,
    Arc<LdkLogger>,
    Arc<IgnoringMessageHandler>,
    Arc<KeysManager>,
    Arc<IgnoringMessageHandler>,
>;

pub struct LdkLogger;

impl Logger for LdkLogger {
    fn log(&self, record: Record) {
        let level = match record.level {
            Level::Gossip | Level::Trace => log::Level::Trace,
            Level::Debug => log::Level::Debug,
            Level::Info => log::Level::Info,
            Level::Warn => log::Level::Warn,
            Level::Error => log::Level::Error,
        };
        let args = format_args!("{}", record.args);
        let record = RecordBuilder::new()
            .level(level)
            .target(record.module_path)
            .module_path_static(Some(record.module_path))
            .line(Some(record.line))
            .args(args)
            .build();
        log::logger().log(&record);
    }
}

#[derive(Debug, Clone)]
pub struct PayOfferParams {
    pub chain: ChainHash,
    pub blinded_path_index: usize,
    pub amount_msats: Option<u64>,
    pub quantity: Option<u64>,
    pub payer_note: Option<String>,
}

impl Default for PayOfferParams {
    fn default() -> Self {
        Self {
            chain: ChainHash::BITCOIN,
            blinded_path_index: 0,
            amount_msats: None,
            quantity: None,
            payer_note: None,
        }
    }
}

#[derive(Clone)]
pub struct LdkNodeConfig {
    pub network: Network,
    pub seed: [u8; 32],
    pub inbound_payment_key: [u8; 32],
    pub peer_manager_ephemeral_random_data: [u8; 32],
}

#[derive(Debug)]
pub enum OnionEvent {
    Resolving(offer_details::IntroductionNode),
    Resolved(Vec<String>),
    Connecting(String),
    ConnectionError(Error),
    Connected,
    WaitingForHandshake,
    Handshaked,
    SendingOnion,
    OnionSent,
    ConnectionNeeded(String),
    Result(Result<String>),
    //    Result(Box<Result<Bolt12InvoiceResponse>>),
}

pub struct LdkNode {
    events: mpsc::Sender<OnionEvent>,
    pub network: Network,
    pub keys_manager: Arc<KeysManager>,
    pub inbound_payment_key: ExpandedKey,
    pub offers_handler: Arc<OffersHandler>,
    pub network_graph: Arc<NetworkGraph<Arc<LdkLogger>>>,
    pub rapid_gossip_sync: Arc<LdkRapidGossipSync>,
    pub onion_messenger: Arc<LdkOnionMessenger>,
    pub peer_manager: Arc<LdkPeerManager>,
}

impl LdkNode {
    pub fn new(config: LdkNodeConfig, events: mpsc::Sender<OnionEvent>) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let logger = Arc::new(LdkLogger);

        let keys_manager = Arc::new(KeysManager::new(
            &config.seed,
            now.as_secs(),
            now.subsec_nanos(),
            true,
        ));

        let inbound_payment_key = ExpandedKey::new(config.inbound_payment_key);
        let offers_handler = Arc::new(OffersHandler::new(inbound_payment_key));

        let network_graph = Arc::new(NetworkGraph::new(config.network, Arc::clone(&logger)));

        let rapid_gossip_sync = Arc::new(RapidGossipSync::new(
            Arc::clone(&network_graph),
            Arc::clone(&logger),
        ));

        let message_router = Arc::new(LdkMessageRouter::new(
            Arc::clone(&network_graph),
            Arc::clone(&keys_manager),
        ));

        let node_id_lookup = Arc::new(EmptyNodeIdLookUp {});
        let async_payments_handler = Arc::new(IgnoringMessageHandler {});
        let dns_resolution_handler = Arc::new(IgnoringMessageHandler {});
        let custom_onion_handler = Arc::new(IgnoringMessageHandler {});

        let onion_messenger = Arc::new(LdkOnionMessenger::new(
            Arc::clone(&keys_manager),
            Arc::clone(&keys_manager),
            Arc::clone(&logger),
            Arc::clone(&node_id_lookup),
            Arc::clone(&message_router),
            Arc::clone(&offers_handler),
            Arc::clone(&async_payments_handler),
            Arc::clone(&dns_resolution_handler),
            Arc::clone(&custom_onion_handler),
        ));

        let peer_manager = Arc::new(LdkPeerManager::new(
            MessageHandler {
                chan_handler: Arc::new(ErroringMessageHandler::new()),
                route_handler: Arc::new(IgnoringMessageHandler {}),
                onion_message_handler: Arc::clone(&onion_messenger),
                custom_message_handler: Arc::new(IgnoringMessageHandler {}),
                send_only_message_handler: Arc::new(IgnoringMessageHandler {}),
            },
            now.as_secs() as u32,
            &config.peer_manager_ephemeral_random_data,
            Arc::clone(&logger),
            Arc::clone(&keys_manager),
        ));

        Self {
            events,
            network: config.network,
            keys_manager,
            inbound_payment_key,
            offers_handler,
            network_graph,
            rapid_gossip_sync,
            onion_messenger,
            peer_manager,
        }
    }

    pub async fn start(&self) -> Result<()> {
        self.update_network_graph().await.map(drop)
    }

    pub async fn request_invoice(
        &self,
        offer: &Offer,
        params: PayOfferParams,
    ) -> Result<Bolt12InvoiceResponse> {
        ensure!(
            ChainHash::using_genesis_block(self.network) == params.chain,
            "LDK network does not match selected offer chain"
        );

        let payment_id = PaymentId(self.keys_manager.get_secure_random_bytes());
        let nonce = Nonce::from_entropy_source(&*self.keys_manager);
        let secp_ctx = Secp256k1::new();

        let builder = offer
            .request_invoice(&self.inbound_payment_key, nonce, &secp_ctx, payment_id)
            .map_err(|e| anyhow!("Failed to create invoice request builder: {e:?}"))?;

        let builder = builder
            .chain(self.network)
            .map_err(|e| anyhow!("Failed to set invoice request chain: {e:?}"))?;

        let builder = match params.amount_msats {
            Some(amount_msats) => builder
                .amount_msats(amount_msats)
                .map_err(|e| anyhow!("Failed to set invoice request amount: {e:?}"))?,
            None => builder,
        };

        let builder = match params.quantity {
            Some(quantity) => builder
                .quantity(quantity)
                .map_err(|e| anyhow!("Failed to set invoice request quantity: {e:?}"))?,
            None => builder,
        };

        let builder = match params.payer_note {
            Some(note) => builder.payer_note(note),
            None => builder,
        };

        let invoice_request = builder
            .build_and_sign()
            .map_err(|e| anyhow!("Failed to build and sign invoice request: {e:?}"))?;

        let context = MessageContext::Offers(OffersContext::OutboundPayment { payment_id, nonce });
        let destination = match offer.paths().get(params.blinded_path_index) {
            Some(path) => Destination::BlindedPath(path.clone()),
            None => Destination::Node(offer.issuer_signing_pubkey().unwrap()),
        };

        let introduction_node = match &destination {
            Destination::BlindedPath(path) => path.introduction_node().clone(),
            Destination::Node(pubkey) => IntroductionNode::NodeId(*pubkey),
        };
        self.resolve_and_connect(&introduction_node).await?;

        let instructions = MessageSendInstructions::WithReplyPath {
            destination,
            context,
        };
        let message = OffersMessage::InvoiceRequest(invoice_request);

        let response_rx = self.offers_handler.register(payment_id)?;

        self.events.send(OnionEvent::SendingOnion).await?;

        let result = self
            .onion_messenger
            .send_onion_message(message, instructions);
        match result {
            Ok(SendSuccess::Buffered) => (),
            Ok(SendSuccess::BufferedAwaitingConnection(pubkey)) => {
                bail!("Failed to send onion message: needs connection to {pubkey}")
            }
            Err(e) => bail!("Failed to send onion message: {e:?}"),
        };
        self.events.send(OnionEvent::OnionSent).await?;

        let response = response_rx
            .await
            .map_err(|e| anyhow!("Failed to wait for response: {e:?}"))?;
        debug!("Response received");
        response
    }

    async fn resolve_and_connect(&self, introduction_node: &IntroductionNode) -> Result<()> {
        self.events
            .send(OnionEvent::Resolving(introduction_node.into()))
            .await?;

        let pubkey = match introduction_node {
            IntroductionNode::NodeId(pubkey) => *pubkey,
            IntroductionNode::DirectedShortChannelId(direction, scid) => {
                let network_graph = self.network_graph.read_only();
                let channel = network_graph
                    .channel(*scid)
                    .ok_or_else(|| anyhow!("Introduction node channel {scid} not found"))?;
                let node_id = match direction {
                    Direction::NodeOne => channel.node_one,
                    Direction::NodeTwo => channel.node_two,
                };
                node_id.as_pubkey().map_err(|e| {
                    anyhow!("Failed to parse introduction node pubkey from channel {scid}: {e}")
                })?
            }
        };

        let addresses: Vec<_> = self
            .network_graph
            .read_only()
            .get_addresses(&pubkey)
            .ok_or(anyhow!("Introduction node public key not found"))?
            .into_iter()
            .filter_map(to_socket_addr)
            .collect();
        let addresses_strings = addresses.iter().map(SocketAddr::to_string).collect();
        self.events
            .send(OnionEvent::Resolved(addresses_strings))
            .await?;

        for address in addresses {
            self.events
                .send(OnionEvent::Connecting(address.to_string()))
                .await?;
            match self.connect_peer(pubkey, &address).await {
                Ok(()) => {
                    self.events.send(OnionEvent::Connected).await?;
                    self.events.send(OnionEvent::WaitingForHandshake).await?;
                    self.wait_for_handshake(pubkey).await?;
                    self.events.send(OnionEvent::Handshaked).await?;
                    return Ok(());
                }
                Err(e) => {
                    self.events.send(OnionEvent::ConnectionError(e)).await?;
                }
            }
        }
        bail!("All connection attempts failed");
    }

    async fn wait_for_handshake(&self, pubkey: PublicKey) -> Result<()> {
        const TIMEOUT: Duration = Duration::from_mins(1);

        let now = Instant::now();
        loop {
            if self.peer_manager.peer_by_node_id(&pubkey).is_some() {
                return Ok(());
            }
            ensure!(
                now.elapsed() < TIMEOUT,
                "Failed to init connection after {TIMEOUT:?}"
            );
            sleep(Duration::from_secs(1)).await;
        }
    }

    async fn connect_peer(&self, pubkey: PublicKey, addr: &SocketAddr) -> Result<()> {
        let connect_fut = async {
            TcpStream::connect(addr)
                .await
                .map(|s| s.into_std().unwrap())
        };
        match tokio::time::timeout(Duration::from_secs(10), connect_fut).await {
            Ok(stream) => {
                let stream = stream?;
                let _disconnect_future =
                    setup_outbound(Arc::clone(&self.peer_manager), pubkey, stream);
                Ok(())
            }
            Err(_elapsed) => bail!("Failed to connect: timeout"),
        }
    }

    async fn update_network_graph(&self) -> Result<u32> {
        const RGS_URL: &str = "https://rapidsync.lightningdevkit.org/v2";
        const RGS_CACHE_PATH: &str = "/tmp/ldk-rgs-0.bin";
        let url = format!("{RGS_URL}/0.bin");
        let cache_path = Path::new(RGS_CACHE_PATH);

        let snapshot_bytes = if let Some(bytes) = read_cached_rgs_snapshot(cache_path) {
            debug!("Using cached RGS snapshot at {}", cache_path.display());
            bytes
        } else {
            debug!("Fetching RGS snapshot {url}");
            let response = reqwest::get(url)
                .await
                .map_err(|e| anyhow!("Failed to fetch RGS snapshot: {e}"))?
                .error_for_status()
                .map_err(|e| anyhow!("Failed to fetch RGS snapshot: {e}"))?;

            let snapshot_bytes = response
                .bytes()
                .await
                .map_err(|e| anyhow!("Failed to read RGS snapshot body: {e}"))?
                .to_vec();
            if let Err(e) = std::fs::write(cache_path, &snapshot_bytes) {
                debug!(
                    "Failed to write RGS snapshot cache at {}: {e}",
                    cache_path.display()
                );
            }
            snapshot_bytes
        };

        debug!("Applying RGS snapshot");
        self.rapid_gossip_sync
            .update_network_graph(snapshot_bytes.as_ref())
            .map_err(|e| anyhow!("Failed to apply RGS snapshot: {e:?}"))
    }
}

fn to_socket_addr(addr: SocketAddress) -> Option<SocketAddr> {
    match addr {
        SocketAddress::TcpIpV4 { addr, port } => {
            Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(addr)), port))
        }
        SocketAddress::TcpIpV6 { addr, port } => {
            Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::from(addr)), port))
        }
        _ => None,
    }
}

fn read_cached_rgs_snapshot(cache_path: &Path) -> Option<Vec<u8>> {
    let metadata = std::fs::metadata(cache_path).ok()?;
    let modified = metadata.modified().ok()?;
    let age = SystemTime::now().duration_since(modified).ok()?;
    if age > Duration::from_secs(60 * 60 * 24) {
        return None;
    }
    match std::fs::read(cache_path) {
        Ok(bytes) => Some(bytes),
        Err(e) => {
            debug!(
                "Failed to read RGS snapshot cache at {}: {e}",
                cache_path.display()
            );
            None
        }
    }
}
