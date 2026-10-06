use bridge_core::{BlockHeight, BridgeError, TxId};
use bridge_proto::compact_tx_streamer_client::CompactTxStreamerClient;
use bridge_proto::{
    BlockId, BlockRange, CompactBlock, GetSubtreeRootsArg, RawTransaction, SubtreeRoot, TreeState,
    TxFilter,
};
use std::time::Duration;
use tonic::transport::Channel;

#[derive(Clone)]
pub struct UpstreamClient {
    endpoint: String,
    timeout: Duration,
}

impl UpstreamClient {
    pub fn new(endpoint: impl Into<String>, timeout_sec: u64) -> Self {
        Self { endpoint: endpoint.into(), timeout: Duration::from_secs(timeout_sec) }
    }

    async fn connect(&self) -> Result<CompactTxStreamerClient<Channel>, BridgeError> {
        let endpoint = Channel::from_shared(self.endpoint.clone())
            .map_err(|e| BridgeError::Upstream(format!("Invalid upstream URL: {e}")))?
            .timeout(self.timeout)
            .connect_timeout(Duration::from_secs(10));

        let channel = endpoint.connect().await.map_err(|e| {
            BridgeError::Upstream(format!("Failed to connect to upstream {}: {e}", self.endpoint))
        })?;

        Ok(CompactTxStreamerClient::new(channel))
    }

    pub async fn get_latest_block(&self) -> Result<BlockId, BridgeError> {
        let mut client = self.connect().await?;
        let res = client
            .get_latest_block(bridge_proto::ChainSpec {})
            .await
            .map_err(|e| BridgeError::Upstream(format!("GetLatestBlock RPC failed: {e}")))?;

        Ok(res.into_inner())
    }

    pub async fn get_block(&self, height: BlockHeight) -> Result<CompactBlock, BridgeError> {
        let mut client = self.connect().await?;
        let req = BlockId { height: height.0 as u64, hash: vec![] };
        let res = client.get_block(req).await.map_err(|e| {
            BridgeError::Upstream(format!("GetBlock RPC failed for height {}: {e}", height.0))
        })?;

        Ok(res.into_inner())
    }

    pub async fn get_block_range(
        &self,
        start: BlockHeight,
        end: BlockHeight,
    ) -> Result<Vec<CompactBlock>, BridgeError> {
        let mut client = self.connect().await?;
        let req = BlockRange {
            start: Some(BlockId { height: start.0 as u64, hash: vec![] }),
            end: Some(BlockId { height: end.0 as u64, hash: vec![] }),
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

    pub async fn get_transaction(&self, txid: &TxId) -> Result<RawTransaction, BridgeError> {
        let mut client = self.connect().await?;
        let req = TxFilter { block: None, index: 0, hash: txid.0.to_vec() };

        let res = client.get_transaction(req).await.map_err(|e| {
            BridgeError::Upstream(format!("GetTransaction RPC failed for {txid}: {e}"))
        })?;

        Ok(res.into_inner())
    }

    pub async fn get_tree_state(&self, height: BlockHeight) -> Result<TreeState, BridgeError> {
        let mut client = self.connect().await?;
        let req = BlockId { height: height.0 as u64, hash: vec![] };

        let res = client.get_tree_state(req).await.map_err(|e| {
            BridgeError::Upstream(format!("GetTreeState RPC failed for height {}: {e}", height.0))
        })?;

        Ok(res.into_inner())
    }

    pub async fn get_subtree_roots(
        &self,
        pool: i32,
        start_index: u32,
        max_entries: u32,
    ) -> Result<Vec<SubtreeRoot>, BridgeError> {
        let mut client = self.connect().await?;
        let req = GetSubtreeRootsArg { shielded_protocol: pool, start_index, max_entries };

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
}
