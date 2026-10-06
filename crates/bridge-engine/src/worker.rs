use crate::client::UpstreamClient;
use crate::scheduler::next_interval;
use bridge_core::{BlockHash, BlockHeight, BridgeConfig, BridgeError, TxId};
use bridge_proto::{RawTransaction, TreeState};
use bridge_storage::{StorageBackend, VerifiedIntervalBatch};
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

        // Ensure coverage is initialized in storage
        if let Ok(meta_opt) = self.storage.get_coverage_metadata().await {
            if meta_opt.is_none() {
                let _ = self
                    .storage
                    .init_coverage(self.config.network, BlockHeight(self.config.coverage_start_height))
                    .await;
            }
        }

        loop {
            // Check for shutdown signal
            if *self.shutdown_rx.borrow() {
                info!("Acquisition worker received shutdown signal, exiting loop.");
                break;
            }

            match self.sync_step().await {
                Ok(has_more) => {
                    if !has_more {
                        // At chain tip; wait for next block
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
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                        _ = self.shutdown_rx.changed() => {},
                    }
                }
            }
        }
    }

    async fn sync_step(&self) -> Result<bool, BridgeError> {
        // 1. Get current committed height
        let meta = self
            .storage
            .get_coverage_metadata()
            .await?
            .ok_or_else(|| BridgeError::Storage("Missing coverage metadata".to_string()))?;

        // 2. Fetch upstream latest block tip
        let tip_info = self.upstream.get_latest_block().await?;
        let tip_height = BlockHeight(tip_info.height as u32);

        // 3. Determine next interval
        let interval = match next_interval(meta.committed_height, tip_height, self.config.interval_size) {
            Some(inv) => inv,
            None => return Ok(false), // Fully synced
        };

        info!(
            "Acquiring verified interval [{}..={}] (tip: {})",
            interval.start, interval.end, tip_height
        );

        // 4. Discover complete block set using individual blocks to guarantee no missing txs
        let mut blocks = Vec::with_capacity(interval.len() as usize);
        for h in interval.start.0..=interval.end.0 {
            let block = self.upstream.get_block(BlockHeight(h)).await?;
            blocks.push(block);
        }

        // 5. Verify block adjacency and sequence continuity
        let prev_hash_ref = if meta.committed_height.0 >= self.config.coverage_start_height {
            Some(&meta.latest_block_hash)
        } else {
            None
        };
        validate_block_sequence(&blocks, interval.start, prev_hash_ref)?;

        // 6. Discover every referenced transaction ID across all blocks in this interval
        let mut txids_to_fetch = Vec::new();
        for b in &blocks {
            for vtx in &b.vtx {
                if vtx.txid.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&vtx.txid);
                    txids_to_fetch.push((TxId(arr), b.height as u32));
                }
            }
        }

        // 7. Concurrently download all full transactions
        let semaphore = Arc::new(Semaphore::new(self.config.acquisition_concurrency));
        let mut join_handles = Vec::new();

        for (txid, height) in txids_to_fetch {
            let sem = semaphore.clone();
            let upstream = self.upstream.clone();

            join_handles.push(tokio::spawn(async move {
                let _permit = sem.acquire().await.map_err(|e| BridgeError::Upstream(e.to_string()))?;
                let mut raw_tx = upstream.get_transaction(&txid).await?;
                raw_tx.height = height as u64;

                // Cryptographically verify TxID matches raw transaction bytes
                verify_transaction(&raw_tx.data, &txid)?;
                Ok::<RawTransaction, BridgeError>(raw_tx)
            }));
        }

        let mut full_transactions = Vec::new();
        for h in join_handles {
            let raw_tx = h.await.map_err(|e| BridgeError::Upstream(format!("Task join failed: {e}")))??;
            full_transactions.push(raw_tx);
        }

        // 8. Fetch tree state for interval end
        let end_tree_state = self.upstream.get_tree_state(interval.end).await.unwrap_or(TreeState {
            network: self.config.network.to_string(),
            height: interval.end.0 as u64,
            hash: hex::encode(&blocks.last().unwrap().hash),
            time: blocks.last().unwrap().time,
            sapling_tree: String::new(),
            orchard_tree: String::new(),
            ironwood_tree: String::new(),
        });

        // 9. Prepare and commit verified batch atomically
        let end_block = blocks.last().unwrap();
        let mut end_hash_arr = [0u8; 32];
        if end_block.hash.len() == 32 {
            end_hash_arr.copy_from_slice(&end_block.hash);
        }

        let batch = VerifiedIntervalBatch {
            blocks,
            transactions: full_transactions,
            tree_states: vec![end_tree_state],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: interval.end,
            end_block_hash: BlockHash(end_hash_arr),
        };

        self.storage.commit_verified_interval(batch).await?;
        info!("Committed verified interval [{}..={}] to disk", interval.start, interval.end);

        Ok(interval.end < tip_height)
    }
}
