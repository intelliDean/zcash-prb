use async_trait::async_trait;
use bridge_core::{BlockHash, BlockHeight, BridgeError, CoverageMetadata, IntervalRange, Network, TransparentAddress, TxId};
use bridge_proto::{CompactBlock, GetAddressUtxosReply, RawTransaction, SubtreeRoot, TreeState};

#[derive(Debug, Clone)]
pub struct TransparentOutputRecord {
    pub txid: TxId,
    pub vout: u32,
    pub address: TransparentAddress,
    pub value_zat: u64,
    pub script_pubkey: Vec<u8>,
    pub height: BlockHeight,
}

#[derive(Debug, Clone)]
pub struct TransparentSpendRecord {
    pub prev_txid: TxId,
    pub prev_vout: u32,
    pub spending_txid: TxId,
    pub spending_height: BlockHeight,
}

#[derive(Debug, Clone)]
pub struct VerifiedIntervalBatch {
    pub blocks: Vec<CompactBlock>,
    pub transactions: Vec<RawTransaction>,
    pub tree_states: Vec<TreeState>,
    pub subtree_roots: Vec<SubtreeRoot>,
    pub transparent_outputs: Vec<TransparentOutputRecord>,
    pub transparent_spends: Vec<TransparentSpendRecord>,
    pub end_height: BlockHeight,
    pub end_block_hash: BlockHash,
}

#[async_trait]
pub trait StorageBackend: Send + Sync + 'static {
    async fn get_coverage_metadata(&self) -> Result<Option<CoverageMetadata>, BridgeError>;
    async fn init_coverage(&self, network: Network, start_height: BlockHeight) -> Result<(), BridgeError>;
    async fn get_latest_block(&self) -> Result<Option<(BlockHeight, BlockHash)>, BridgeError>;
    async fn get_compact_block(&self, height: BlockHeight) -> Result<Option<CompactBlock>, BridgeError>;
    async fn get_compact_block_range(&self, start: BlockHeight, end: BlockHeight) -> Result<Vec<CompactBlock>, BridgeError>;
    async fn get_full_transaction(&self, txid: &TxId) -> Result<Option<RawTransaction>, BridgeError>;
    async fn get_tree_state(&self, height: BlockHeight) -> Result<Option<TreeState>, BridgeError>;
    async fn get_subtree_roots(&self, pool: i32, start_index: u32, max_entries: u32) -> Result<Vec<SubtreeRoot>, BridgeError>;
    async fn get_address_utxos(&self, address: &TransparentAddress) -> Result<Vec<GetAddressUtxosReply>, BridgeError>;
    async fn get_taddress_transactions(
        &self,
        address: &TransparentAddress,
        range: Option<IntervalRange>,
    ) -> Result<Vec<RawTransaction>, BridgeError>;
    async fn commit_verified_interval(&self, batch: VerifiedIntervalBatch) -> Result<(), BridgeError>;
    async fn handle_reorg(&self, fork_height: BlockHeight) -> Result<(), BridgeError>;
    async fn record_acquisition_failure(&self, error: &str) -> Result<(), BridgeError>;
}
