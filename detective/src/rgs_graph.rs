use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{anyhow, Result};
use bitcoin::network::Network;
use bitcoin::secp256k1::PublicKey;
use lightning::routing::gossip::{NetworkGraph, NodeId};
use lightning::util::logger::{Level, Logger, Record};
use lightning_rapid_gossip_sync::RapidGossipSync;
use log::{debug, warn, RecordBuilder};

use crate::node::Node;

const RGS_URL: &str = "https://rapidsync.lightningdevkit.org/v2";
pub(crate) const RGS_CACHE_PATH: &str = "/tmp/ldk-rgs-0.bin";

pub(crate) type LdkNetworkGraph = NetworkGraph<Arc<LdkLogger>>;
pub(crate) type LdkRapidGossipSync = RapidGossipSync<Arc<LdkNetworkGraph>, Arc<LdkLogger>>;

pub(crate) struct LdkLogger;

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

pub(crate) struct RgsGraph {
    pub network_graph: Arc<LdkNetworkGraph>,
    pub rapid_gossip_sync: Arc<LdkRapidGossipSync>,
}

impl RgsGraph {
    pub fn new(network: Network, logger: Arc<LdkLogger>) -> Self {
        let network_graph = Arc::new(NetworkGraph::new(network, Arc::clone(&logger)));
        let rapid_gossip_sync = Arc::new(RapidGossipSync::new(Arc::clone(&network_graph), logger));
        Self {
            network_graph,
            rapid_gossip_sync,
        }
    }

    pub async fn load_or_empty(network: Network) -> Self {
        let graph = Self::new(network, Arc::new(LdkLogger));
        if let Err(e) = graph.update_from_default_source().await {
            warn!("Failed to load RGS graph, continuing with an empty graph: {e:#}");
        }
        graph
    }

    pub async fn update_from_default_source(&self) -> Result<u32> {
        update_network_graph(&self.rapid_gossip_sync, Path::new(RGS_CACHE_PATH)).await
    }

    pub fn query(&self, pubkey: impl AsRef<str>) -> Node {
        query_network_graph(&self.network_graph, pubkey.as_ref())
    }
}

pub(crate) async fn update_network_graph(
    rapid_gossip_sync: &LdkRapidGossipSync,
    cache_path: &Path,
) -> Result<u32> {
    let url = format!("{RGS_URL}/0.bin");
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
    rapid_gossip_sync
        .update_network_graph(snapshot_bytes.as_ref())
        .map_err(|e| anyhow!("Failed to apply RGS snapshot: {e:?}"))
}

fn query_network_graph(network_graph: &LdkNetworkGraph, pubkey: &str) -> Node {
    let unknown = || Node {
        pubkey: pubkey.to_string(),
        alias: None,
        is_announced: false,
    };

    let Ok(pubkey) = pubkey.parse::<PublicKey>() else {
        return unknown();
    };
    let node_id = NodeId::from_pubkey(&pubkey);
    let network_graph = network_graph.read_only();
    let Some(node_info) = network_graph.node(&node_id) else {
        return unknown();
    };
    let Some(announcement_info) = node_info.announcement_info.as_ref() else {
        return Node {
            pubkey: pubkey.to_string(),
            alias: None,
            is_announced: true,
        };
    };
    let alias = announcement_info.alias().to_string();
    let alias = (!alias.is_empty()).then_some(alias);
    Node {
        pubkey: pubkey.to_string(),
        alias,
        is_announced: true,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_empty_graph_returns_unannounced_node() {
        let graph = RgsGraph::new(Network::Bitcoin, Arc::new(LdkLogger));
        let pubkey = "020000000000000000000000000000000000000000000000000000000000000001";

        let node = graph.query(pubkey);

        assert_eq!(node.pubkey, pubkey);
        assert_eq!(node.alias, None);
        assert!(!node.is_announced);
    }

    #[test]
    fn query_invalid_pubkey_returns_unannounced_node() {
        let graph = RgsGraph::new(Network::Bitcoin, Arc::new(LdkLogger));

        let node = graph.query("not-a-pubkey");

        assert_eq!(node.pubkey, "not-a-pubkey");
        assert_eq!(node.alias, None);
        assert!(!node.is_announced);
    }
}
