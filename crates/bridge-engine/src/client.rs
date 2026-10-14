use bridge_core::{BlockHeight, BridgeError, TxId};
use bridge_proto::compact_tx_streamer_client::CompactTxStreamerClient;
use bridge_proto::{
    BlockId, BlockRange, CompactBlock, GetSubtreeRootsArg, RawTransaction, SubtreeRoot, TreeState,
    TxFilter,
};
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tonic::transport::Channel;

/// Environment variable naming a file to which every upstream RPC is appended
/// as one JSON object per line. Unset = tracing disabled.
pub const UPSTREAM_TRACE_ENV: &str = "PRB_UPSTREAM_TRACE";

fn trace_sink() -> Option<&'static Mutex<File>> {
    static SINK: OnceLock<Option<Mutex<File>>> = OnceLock::new();
    SINK.get_or_init(|| {
        let path = std::env::var(UPSTREAM_TRACE_ENV).ok()?;
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()
            .map(Mutex::new)
    })
    .as_ref()
}

/// Append one upstream RPC record. Failures to write the trace are ignored so
/// tracing can never change acquisition behaviour.
fn record_upstream_call(
    endpoint: &str,
    method: &str,
    arg: &str,
    started: Instant,
    result: Result<usize, &BridgeError>,
) {
    let Some(sink) = trace_sink() else { return };
    let ts_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let elapsed_ms = started.elapsed().as_millis();
    let (ok, bytes, err) = match result {
        Ok(b) => (true, b, String::new()),
        Err(e) => (
            false,
            0,
            e.to_string().replace('\\', "\\\\").replace('"', "\\\""),
        ),
    };
    let line = format!(
        "{{\"ts_ms\":{ts_ms},\"endpoint\":\"{endpoint}\",\"method\":\"{method}\",\"arg\":\"{arg}\",\"ok\":{ok},\"response_bytes\":{bytes},\"elapsed_ms\":{elapsed_ms},\"error\":\"{err}\"}}\n"
    );
    if let Ok(mut f) = sink.lock() {
        let _ = f.write_all(line.as_bytes());
    }
}

#[derive(Clone)]
pub struct UpstreamClient {
    endpoint: String,
    timeout: Duration,
    cached_channel: Arc<RwLock<Option<Channel>>>,
}

impl UpstreamClient {
    pub fn new(endpoint: impl Into<String>, timeout_sec: u64) -> Self {
        Self {
            endpoint: endpoint.into(),
            timeout: Duration::from_secs(timeout_sec),
            cached_channel: Arc::new(RwLock::new(None)),
        }
    }

    fn trace<T>(
        &self,
        method: &str,
        arg: &str,
        started: Instant,
        result: &Result<T, BridgeError>,
        size: impl Fn(&T) -> usize,
    ) {
        record_upstream_call(
            &self.endpoint,
            method,
            arg,
            started,
            result.as_ref().map(size),
        );
    }

    async fn channel(&self) -> Result<Channel, BridgeError> {
        {
            let r = self.cached_channel.read().await;
            if let Some(ch) = r.as_ref() {
                return Ok(ch.clone());
            }
        }
        let mut w = self.cached_channel.write().await;
        if let Some(ch) = w.as_ref() {
            return Ok(ch.clone());
        }

        let mut endpoint = Channel::from_shared(self.endpoint.clone())
            .map_err(|e| BridgeError::Upstream(format!("Invalid upstream URL: {e}")))?
            .timeout(self.timeout)
            .connect_timeout(Duration::from_secs(10));

        if self.endpoint.starts_with("https://") {
            let tls_config = tonic::transport::ClientTlsConfig::new().with_webpki_roots();
            endpoint = endpoint
                .tls_config(tls_config)
                .map_err(|e| BridgeError::Upstream(format!("Failed to configure TLS: {e}")))?;
        }

        let channel = endpoint.connect().await.map_err(|e| {
            BridgeError::Upstream(format!(
                "Failed to connect to upstream {}: {e}",
                self.endpoint
            ))
        })?;

        *w = Some(channel.clone());
        Ok(channel)
    }

    async fn connect(&self) -> Result<CompactTxStreamerClient<Channel>, BridgeError> {
        let channel = self.channel().await?;
        Ok(CompactTxStreamerClient::new(channel))
    }

    pub async fn get_latest_block(&self) -> Result<BlockId, BridgeError> {
        let started = Instant::now();
        let result = async {
            let mut client = self.connect().await?;
            let res = client
                .get_latest_block(bridge_proto::ChainSpec {})
                .await
                .map_err(|e| BridgeError::Upstream(format!("GetLatestBlock RPC failed: {e}")))?;
            Ok(res.into_inner())
        }
        .await;
        self.trace(
            "GetLatestBlock",
            "",
            started,
            &result,
            prost::Message::encoded_len,
        );
        result
    }

    pub async fn get_block(&self, height: BlockHeight) -> Result<CompactBlock, BridgeError> {
        let started = Instant::now();
        let result = async {
            let mut client = self.connect().await?;
            let req = BlockId {
                height: height.0 as u64,
                hash: vec![],
            };
            let res = client.get_block(req).await.map_err(|e| {
                BridgeError::Upstream(format!("GetBlock RPC failed for height {}: {e}", height.0))
            })?;
            Ok(res.into_inner())
        }
        .await;
        self.trace(
            "GetBlock",
            &format!("height={}", height.0),
            started,
            &result,
            prost::Message::encoded_len,
        );
        result
    }

    pub async fn get_block_range(
        &self,
        start: BlockHeight,
        end: BlockHeight,
    ) -> Result<Vec<CompactBlock>, BridgeError> {
        let started = Instant::now();
        let result = async {
            let mut client = self.connect().await?;
            let req = BlockRange {
                start: Some(BlockId {
                    height: start.0 as u64,
                    hash: vec![],
                }),
                end: Some(BlockId {
                    height: end.0 as u64,
                    hash: vec![],
                }),
                pool_types: vec![],
            };

            let mut stream = client
                .get_block_range(req)
                .await
                .map_err(|e| BridgeError::Upstream(format!("GetBlockRange RPC failed: {e}")))?
                .into_inner();

            let mut blocks = Vec::new();
            while let Some(block) = stream
                .message()
                .await
                .map_err(|e| BridgeError::Upstream(format!("Streaming block failed: {e}")))?
            {
                blocks.push(block);
            }
            Ok(blocks)
        }
        .await;
        self.trace(
            "GetBlockRange",
            &format!("start={},end={}", start.0, end.0),
            started,
            &result,
            |v: &Vec<CompactBlock>| v.iter().map(prost::Message::encoded_len).sum(),
        );
        result
    }

    pub async fn get_transaction(&self, txid: &TxId) -> Result<RawTransaction, BridgeError> {
        let started = Instant::now();
        let result = async {
            let mut client = self.connect().await?;
            let req = TxFilter {
                block: None,
                index: 0,
                hash: txid.0.to_vec(),
            };
            let res = client.get_transaction(req).await.map_err(|e| {
                BridgeError::Upstream(format!("GetTransaction RPC failed for {txid}: {e}"))
            })?;
            Ok(res.into_inner())
        }
        .await;
        self.trace(
            "GetTransaction",
            &format!("txid={txid}"),
            started,
            &result,
            prost::Message::encoded_len,
        );
        result
    }

    pub async fn get_tree_state(&self, height: BlockHeight) -> Result<TreeState, BridgeError> {
        let started = Instant::now();
        let result = async {
            let mut client = self.connect().await?;
            let req = BlockId {
                height: height.0 as u64,
                hash: vec![],
            };
            let res = client.get_tree_state(req).await.map_err(|e| {
                BridgeError::Upstream(format!(
                    "GetTreeState RPC failed for height {}: {e}",
                    height.0
                ))
            })?;
            Ok(res.into_inner())
        }
        .await;
        self.trace(
            "GetTreeState",
            &format!("height={}", height.0),
            started,
            &result,
            prost::Message::encoded_len,
        );
        result
    }

    pub async fn get_subtree_roots(
        &self,
        pool: i32,
        start_index: u32,
        max_entries: u32,
    ) -> Result<Vec<SubtreeRoot>, BridgeError> {
        let started = Instant::now();
        let result = async {
            let mut client = self.connect().await?;
            let req = GetSubtreeRootsArg {
                shielded_protocol: pool,
                start_index,
                max_entries,
            };

            let mut stream = client
                .get_subtree_roots(req)
                .await
                .map_err(|e| BridgeError::Upstream(format!("GetSubtreeRoots RPC failed: {e}")))?
                .into_inner();

            let mut roots = Vec::new();
            while let Some(root) = stream
                .message()
                .await
                .map_err(|e| BridgeError::Upstream(format!("Subtree root stream error: {e}")))?
            {
                roots.push(root);
            }
            Ok(roots)
        }
        .await;
        self.trace(
            "GetSubtreeRoots",
            &format!("pool={pool},start_index={start_index},max_entries={max_entries}"),
            started,
            &result,
            |v: &Vec<SubtreeRoot>| v.iter().map(prost::Message::encoded_len).sum(),
        );
        result
    }
}
