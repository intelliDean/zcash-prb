use crate::types::Network;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Configuration for Zcash Private Receive Bridge daemon.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeConfig {
    /// Target network (mainnet, testnet, regtest)
    #[serde(default = "default_network")]
    pub network: Network,

    /// Upstream lightwalletd or Zaino gRPC endpoint (e.g. "https://mainnet.lightwalletd.com:9067")
    pub upstream_provider: String,

    /// Local gRPC bind address (must be localhost only)
    #[serde(default = "default_bind_address")]
    pub bind_address: String,

    /// Block height from which coverage begins
    pub coverage_start_height: u32,

    /// Path to durable SQLite database file
    #[serde(default = "default_storage_path")]
    pub storage_path: PathBuf,

    /// Optional disk storage limit in Gigabytes
    pub storage_limit_gb: Option<u64>,

    /// Maximum concurrent full transaction fetches from upstream
    #[serde(default = "default_acquisition_concurrency")]
    pub acquisition_concurrency: usize,

    /// Batch size for public block intervals
    #[serde(default = "default_interval_size")]
    pub interval_size: u32,

    /// Request timeout in seconds for upstream gRPC calls
    #[serde(default = "default_request_timeout_sec")]
    pub request_timeout_sec: u64,
}

fn default_network() -> Network {
    Network::Mainnet
}

fn default_bind_address() -> String {
    "127.0.0.1:9067".to_string()
}

fn default_storage_path() -> PathBuf {
    PathBuf::from("data/bridge.db")
}

fn default_acquisition_concurrency() -> usize {
    8
}

fn default_interval_size() -> u32 {
    50
}

fn default_request_timeout_sec() -> u64 {
    15
}

impl Default for BridgeConfig {
    fn default() -> Self {
        Self {
            network: default_network(),
            upstream_provider: "https://mainnet.lightwalletd.com:9067".to_string(),
            bind_address: default_bind_address(),
            coverage_start_height: 3_000_000,
            storage_path: default_storage_path(),
            storage_limit_gb: Some(10),
            acquisition_concurrency: default_acquisition_concurrency(),
            interval_size: default_interval_size(),
            request_timeout_sec: default_request_timeout_sec(),
        }
    }
}

impl BridgeConfig {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Self, crate::error::BridgeError> {
        let content = std::fs::read_to_string(path.as_ref())
            .map_err(|e| crate::error::BridgeError::Config(format!("Failed to read config file: {e}")))?;
        toml::from_str(&content)
            .map_err(|e| crate::error::BridgeError::Config(format!("Failed to parse config file: {e}")))
    }

    /// Enforces security invariant that the daemon binds to localhost only.
    pub fn validate(&self) -> Result<(), crate::error::BridgeError> {
        let host = self.bind_address.split(':').next().unwrap_or("");
        if host != "127.0.0.1" && host != "localhost" && host != "::1" {
            return Err(crate::error::BridgeError::Config(format!(
                "Security violation: bind_address '{}' must be localhost only (127.0.0.1 or ::1)",
                self.bind_address
            )));
        }
        if self.acquisition_concurrency == 0 {
            return Err(crate::error::BridgeError::Config(
                "acquisition_concurrency must be greater than 0".to_string(),
            ));
        }
        if self.interval_size == 0 {
            return Err(crate::error::BridgeError::Config(
                "interval_size must be greater than 0".to_string(),
            ));
        }
        Ok(())
    }
}
