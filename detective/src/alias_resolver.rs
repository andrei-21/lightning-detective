use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use log::debug;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MEMPOOL_NODE_URL: &str = "https://mempool.space/api/v1/lightning/nodes";
const ALIAS_CACHE_PATH: &str = "/tmp/lndetective-node-aliases.json";
const ALIAS_CACHE_TTL: Duration = Duration::from_secs(60 * 60 * 24 * 7);

pub(crate) struct AliasResolver {
    client: reqwest::Client,
    cache_path: PathBuf,
    cache: Mutex<HashMap<String, AliasCacheEntry>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AliasCacheEntry {
    alias: Option<String>,
    cached_at: u64,
}

impl AliasResolver {
    pub fn new() -> Self {
        let cache_path = PathBuf::from(ALIAS_CACHE_PATH);
        let cache = read_alias_cache(&cache_path).unwrap_or_default();
        let client = match reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
        {
            Ok(client) => client,
            Err(_) => reqwest::Client::new(),
        };
        Self {
            client,
            cache_path,
            cache: Mutex::new(cache),
        }
    }

    pub async fn resolve(&self, pubkey: &str) -> Option<String> {
        if let Some(alias) = self.read_cached_alias(pubkey) {
            return alias;
        }

        match self.fetch_alias(pubkey).await {
            Ok(alias) => {
                self.write_cached_alias(pubkey, alias.clone());
                alias
            }
            Err(e) => {
                debug!("Failed to resolve node alias for {pubkey}: {e:#}");
                None
            }
        }
    }

    fn read_cached_alias(&self, pubkey: &str) -> Option<Option<String>> {
        let cache = self.cache.lock().unwrap();
        let entry = cache.get(pubkey)?;
        let now = unix_timestamp();
        if now.saturating_sub(entry.cached_at) > ALIAS_CACHE_TTL.as_secs() {
            return None;
        }
        Some(entry.alias.clone())
    }

    fn write_cached_alias(&self, pubkey: &str, alias: Option<String>) {
        let cache_snapshot = {
            let mut cache = self.cache.lock().unwrap();
            cache.insert(
                pubkey.to_string(),
                AliasCacheEntry {
                    alias,
                    cached_at: unix_timestamp(),
                },
            );
            cache.clone()
        };

        if let Err(e) = write_alias_cache(&self.cache_path, &cache_snapshot) {
            debug!(
                "Failed to write alias cache at {}: {e:#}",
                self.cache_path.display()
            );
        }
    }

    async fn fetch_alias(&self, pubkey: &str) -> Result<Option<String>> {
        let url = format!("{MEMPOOL_NODE_URL}/{pubkey}");
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| anyhow!("failed to fetch node details: {e}"))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        let response = response
            .error_for_status()
            .map_err(|e| anyhow!("failed to fetch node details: {e}"))?;
        let body = response
            .text()
            .await
            .map_err(|e| anyhow!("failed to read node details body: {e}"))?;
        parse_alias(&body)
    }
}

fn parse_alias(body: &str) -> Result<Option<String>> {
    let value: Value =
        serde_json::from_str(body).map_err(|e| anyhow!("failed to parse node details: {e}"))?;
    Ok(find_alias(&value).filter(|alias| !alias.is_empty()))
}

fn find_alias(value: &Value) -> Option<String> {
    value
        .get("alias")
        .and_then(Value::as_str)
        .or_else(|| {
            value
                .get("node")
                .and_then(|node| node.get("alias"))
                .and_then(Value::as_str)
        })
        .map(str::trim)
        .map(str::to_string)
}

fn read_alias_cache(path: &Path) -> Result<HashMap<String, AliasCacheEntry>> {
    let bytes = std::fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_alias_cache(path: &Path, cache: &HashMap<String, AliasCacheEntry>) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(cache)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

fn unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_top_level_alias() {
        let alias = parse_alias(r#"{"public_key":"abc","alias":"ACME"}"#).unwrap();

        assert_eq!(alias, Some("ACME".to_string()));
    }

    #[test]
    fn parses_nested_node_alias() {
        let alias = parse_alias(r#"{"node":{"alias":"ACME"}}"#).unwrap();

        assert_eq!(alias, Some("ACME".to_string()));
    }

    #[test]
    fn ignores_empty_alias() {
        let alias = parse_alias(r#"{"alias":"   "}"#).unwrap();

        assert_eq!(alias, None);
    }
}
