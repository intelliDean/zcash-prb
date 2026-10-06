use crate::client::UpstreamClient;
use crate::scheduler::next_interval;
use bridge_core::{BlockHash, BlockHeight, BridgeConfig, BridgeError, CoverageMetadata, IntervalRange, TxId};
use bridge_proto::{CompactBlock, RawTransaction, TreeState};
use bridge_storage::{
    StorageBackend, TransparentOutputRecord, TransparentSpendRecord, VerifiedIntervalBatch,
};
use bridge_verifier::{validate_block_sequence, verify_transaction};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{watch, Semaphore};
use tracing::{error, info, warn};

pub struct AcquisitionWorker {
    config: BridgeConfig,
    storage: Arc<dyn StorageBackend>,
    upstream: UpstreamClient,
    shutdown_rx: watch::Receiver<bool>,
}

impl AcquisitionWorker {
    pub fn new(
        config: BridgeConfig,
        storage: Arc<dyn StorageBackend>,
        shutdown_rx: watch::Receiver<bool>,
    ) -> Self {
        let upstream = UpstreamClient::new(
            config.upstream_provider.clone(),
            config.request_timeout_sec,
        );
        Self {
            config,
            storage,
            upstream,
            shutdown_rx,
        }
    }

    pub async fn run(mut self) {
        info!(
            "Starting acquisition worker. Coverage start height: {}",
            self.config.coverage_start_height
        );

        self.ensure_coverage_initialized().await;

        loop {
            if *self.shutdown_rx.borrow() {
                info!("Acquisition worker received shutdown signal, exiting loop.");
                break;
            }

            match self.sync_step().await {
                Ok(has_more) => {
                    if !has_more {
                        tokio::select! {
                            _ = tokio::time::sleep(Duration::from_secs(10)) => {},
                            _ = self.shutdown_rx.changed() => {},
                        }
                    }
                }
                Err(BridgeError::ReorgDetected { height, expected, actual }) => {
                    warn!(
                        "Chain reorganization detected at height {}: expected prev_hash {}, got {}. Rolling back...",
                        height, expected, actual
                    );
                    if let Err(e) = self.storage.handle_reorg(BlockHeight(height.saturating_sub(1))).await {
                        error!("Failed to handle chain reorganization: {e}");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
                Err(e) => {
                    error!("Error during sync step: {e}. Retrying in 5s...");
                    let _ = self.storage.record_acquisition_failure(&e.to_string()).await;
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                        _ = self.shutdown_rx.changed() => {},
                    }
                }
            }
        }
    }

    async fn ensure_coverage_initialized(&self) {
        if let Ok(meta_opt) = self.storage.get_coverage_metadata().await {
            if meta_opt.is_none() {
                let _ = self
                    .storage
                    .init_coverage(self.config.network, BlockHeight(self.config.coverage_start_height))
                    .await;
            }
        }
    }

    async fn sync_step(&self) -> Result<bool, BridgeError> {
        let meta = self
            .storage
            .get_coverage_metadata()
            .await?
            .ok_or_else(|| BridgeError::Storage("Missing coverage metadata".to_string()))?;

        let (interval, tip_height) = match self.determine_target_interval(&meta).await? {
            Some(res) => res,
            None => return Ok(false),
        };

        info!(
            "Acquiring verified interval [{}..={}] (tip: {})",
            interval.start, interval.end, tip_height
        );

        let prev_hash_ref = if meta.committed_height.0 >= self.config.coverage_start_height {
            Some(&meta.latest_block_hash)
        } else {
            None
        };

        // 1. Fetch and validate block sequence
        let blocks = self.fetch_and_validate_blocks(interval, prev_hash_ref).await?;

        // 2. Fetch and cryptographically verify all transactions
        let full_transactions = self.fetch_and_verify_transactions(&blocks).await?;

        // 3. Extract transparent outpoints and spends for local indexing
        let (transparent_outputs, transparent_spends) =
            self.extract_transparent_records(&full_transactions);

        // 4. Fetch tree state for interval end
        let end_block = blocks.last().expect("Interval must contain at least one block");
        let end_tree_state = self.fetch_end_tree_state(interval.end, end_block).await;

        // 5. Commit verified batch atomically
        let mut end_hash_arr = [0u8; 32];
        if end_block.hash.len() == 32 {
            end_hash_arr.copy_from_slice(&end_block.hash);
        }

        let batch = VerifiedIntervalBatch {
            blocks,
            transactions: full_transactions,
            tree_states: vec![end_tree_state],
            subtree_roots: vec![],
            transparent_outputs,
            transparent_spends,
            end_height: interval.end,
            end_block_hash: BlockHash(end_hash_arr),
        };

        self.storage.commit_verified_interval(batch).await?;
        info!("Committed verified interval [{}..={}] to disk", interval.start, interval.end);

        Ok(interval.end < tip_height)
    }

    async fn determine_target_interval(
        &self,
        meta: &CoverageMetadata,
    ) -> Result<Option<(IntervalRange, BlockHeight)>, BridgeError> {
        let tip_info = self.upstream.get_latest_block().await?;
        let tip_height = BlockHeight(tip_info.height as u32);

        match next_interval(meta.committed_height, tip_height, self.config.interval_size) {
            Some(inv) => Ok(Some((inv, tip_height))),
            None => Ok(None),
        }
    }

    async fn fetch_and_validate_blocks(
        &self,
        interval: IntervalRange,
        prev_hash: Option<&BlockHash>,
    ) -> Result<Vec<CompactBlock>, BridgeError> {
        let mut blocks = Vec::with_capacity(interval.len() as usize);
        for h in interval.start.0..=interval.end.0 {
            let block = self.upstream.get_block(BlockHeight(h)).await?;
            blocks.push(block);
        }

        validate_block_sequence(&blocks, interval.start, prev_hash)?;
        Ok(blocks)
    }

    async fn fetch_and_verify_transactions(
        &self,
        blocks: &[CompactBlock],
    ) -> Result<Vec<RawTransaction>, BridgeError> {
        let mut txids_to_fetch = Vec::new();
        for b in blocks {
            for vtx in &b.vtx {
                if vtx.txid.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&vtx.txid);
                    txids_to_fetch.push((TxId(arr), b.height as u32));
                }
            }
        }

        let semaphore = Arc::new(Semaphore::new(self.config.acquisition_concurrency));
        let mut join_handles = Vec::new();

        for (txid, height) in txids_to_fetch {
            let sem = semaphore.clone();
            let upstream = self.upstream.clone();

            join_handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.map_err(|e| BridgeError::Upstream(e.to_string()))?;
                let mut raw_tx = upstream.get_transaction(&txid).await?;
                raw_tx.height = height as u64;

                verify_transaction(&raw_tx.data, &txid)?;
                Ok::<RawTransaction, BridgeError>(raw_tx)
            }));
        }

        let mut full_transactions = Vec::new();
        for h in join_handles {
            let raw_tx = h.await.map_err(|e| BridgeError::Upstream(format!("Task join failed: {e}")))??;
            full_transactions.push(raw_tx);
        }

        Ok(full_transactions)
    }

    fn extract_transparent_records(
        &self,
        full_transactions: &[RawTransaction],
    ) -> (Vec<TransparentOutputRecord>, Vec<TransparentSpendRecord>) {
        let mut transparent_outputs = Vec::new();
        let mut transparent_spends = Vec::new();

        for raw_tx in full_transactions {
            let txid = match bridge_verifier::compute_raw_txid(&raw_tx.data) {
                Ok(id) => id,
                Err(_) => continue,
            };
            let height = BlockHeight(raw_tx.height as u32);

            if let Ok((inputs, outputs)) =
                bridge_core::parse_transparent_transaction(&raw_tx.data, self.config.network)
            {
                for out in outputs {
                    transparent_outputs.push(TransparentOutputRecord {
                        txid,
                        vout: out.vout,
                        address: out.address,
                        value_zat: out.value_zat,
                        script_pubkey: out.script_pubkey,
                        height,
                    });
                }
                for inp in inputs {
                    transparent_spends.push(TransparentSpendRecord {
                        prev_txid: inp.prev_txid,
                        prev_vout: inp.prev_vout,
                        spending_txid: txid,
                        spending_height: height,
                    });
                }
            }
        }

        (transparent_outputs, transparent_spends)
    }

    async fn fetch_end_tree_state(
        &self,
        interval_end: BlockHeight,
        last_block: &CompactBlock,
    ) -> TreeState {
        self.upstream.get_tree_state(interval_end).await.unwrap_or(TreeState {
            network: self.config.network.to_string(),
            height: interval_end.0 as u64,
            hash: hex::encode(&last_block.hash),
            time: last_block.time,
            sapling_tree: String::new(),
            orchard_tree: String::new(),
            ironwood_tree: String::new(),
        })
    }
}
