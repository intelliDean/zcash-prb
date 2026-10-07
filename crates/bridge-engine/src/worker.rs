use crate::client::UpstreamClient;
use crate::scheduler::next_interval;
use bridge_core::{
    BlockHash, BlockHeight, BridgeConfig, BridgeError, CoverageMetadata, IntervalRange, TxId,
};
use bridge_proto::{CompactBlock, RawTransaction, SubtreeRoot, TreeState};
use bridge_storage::{
    StorageBackend, TransparentOutputRecord, TransparentSpendRecord, VerifiedIntervalBatch,
};
use bridge_verifier::{validate_block_sequence, verify_transaction};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{Semaphore, watch};
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
        let upstream =
            UpstreamClient::new(config.upstream_provider.clone(), config.request_timeout_sec);
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
                Err(BridgeError::ReorgDetected {
                    height,
                    expected,
                    actual,
                }) => {
                    warn!(
                        "Chain reorganization detected at height {}: expected prev_hash {}, got {}. Walking back to common ancestor...",
                        height, expected, actual
                    );
                    let tip_h = self
                        .upstream
                        .get_latest_block()
                        .await
                        .map(|t| BlockHeight(t.height as u32))
                        .unwrap_or(BlockHeight(height));
                    if let Err(e) = self
                        .find_common_ancestor_and_rollback(
                            BlockHeight(height.saturating_sub(1)),
                            tip_h,
                        )
                        .await
                    {
                        error!("Failed to handle chain reorganization: {e}");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
                Err(e) => {
                    error!("Error during sync step: {e}. Retrying in 5s...");
                    let _ = self
                        .storage
                        .record_acquisition_failure(&e.to_string())
                        .await;
                    tokio::select! {
                        _ = tokio::time::sleep(Duration::from_secs(5)) => {},
                        _ = self.shutdown_rx.changed() => {},
                    }
                }
            }
        }
    }

    async fn ensure_coverage_initialized(&self) {
        if let Ok(meta_opt) = self.storage.get_coverage_metadata().await
            && meta_opt.is_none()
        {
            let _ = self
                .storage
                .init_coverage(
                    self.config.network,
                    BlockHeight(self.config.coverage_start_height),
                )
                .await;
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
        let blocks = self
            .fetch_and_validate_blocks(interval, prev_hash_ref)
            .await?;

        // 2. Fetch and cryptographically verify all transactions
        let full_transactions = self.fetch_and_verify_transactions(&blocks).await?;

        // 3. Extract transparent outpoints and spends for local indexing
        let (transparent_outputs, transparent_spends) =
            self.extract_transparent_records(&full_transactions);

        // 4. Fetch tree state for interval end
        let end_tree_state = self.fetch_end_tree_state(interval.end).await?;

        // 5. Fetch subtree roots completing within or up to this interval
        let subtree_roots = self.fetch_subtree_roots(interval.end).await?;

        // 6. Commit verified batch atomically
        let end_block = blocks
            .last()
            .expect("Interval must contain at least one block");
        let mut end_hash_arr = [0u8; 32];
        if end_block.hash.len() == 32 {
            end_hash_arr.copy_from_slice(&end_block.hash);
        }

        let batch = VerifiedIntervalBatch {
            blocks,
            transactions: full_transactions,
            tree_states: vec![end_tree_state],
            subtree_roots,
            transparent_outputs,
            transparent_spends,
            end_height: interval.end,
            end_block_hash: BlockHash(end_hash_arr),
        };

        self.storage.commit_verified_interval(batch).await?;
        info!(
            "Committed verified interval [{}..={}] to disk",
            interval.start, interval.end
        );

        Ok(interval.end < tip_height)
    }

    async fn determine_target_interval(
        &self,
        meta: &CoverageMetadata,
    ) -> Result<Option<(IntervalRange, BlockHeight)>, BridgeError> {
        let tip_info = self.upstream.get_latest_block().await?;
        let tip_height = BlockHeight(tip_info.height as u32);

        let matches_chain = self
            .check_chain_identity_at_tip(meta, tip_height, &tip_info.hash)
            .await?;

        if !matches_chain {
            self.find_common_ancestor_and_rollback(meta.committed_height, tip_height)
                .await?;
            let updated_meta = self
                .storage
                .get_coverage_metadata()
                .await?
                .ok_or_else(|| BridgeError::Storage("Missing coverage metadata".to_string()))?;
            return match next_interval(
                updated_meta.committed_height,
                tip_height,
                self.config.interval_size,
            ) {
                Some(inv) => Ok(Some((inv, tip_height))),
                None => Ok(None),
            };
        }

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
                if vtx.txid.len() != 32 {
                    return Err(BridgeError::Verification(format!(
                        "Compact block at height {} declared malformed txid of length {} (expected 32)",
                        b.height,
                        vtx.txid.len()
                    )));
                }
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&vtx.txid);
                txids_to_fetch.push((TxId(arr), b.height as u32));
            }
        }

        let semaphore = Arc::new(Semaphore::new(self.config.acquisition_concurrency));
        let mut join_handles = Vec::new();

        for (txid, height) in txids_to_fetch {
            let sem = semaphore.clone();
            let upstream = self.upstream.clone();

            join_handles.push(tokio::spawn(async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|e| BridgeError::Upstream(e.to_string()))?;
                let mut raw_tx = upstream.get_transaction(&txid).await?;
                raw_tx.height = height as u64;

                verify_transaction(&raw_tx.data, &txid)?;
                Ok::<RawTransaction, BridgeError>(raw_tx)
            }));
        }

        let mut full_transactions = Vec::new();
        for h in join_handles {
            let raw_tx = h
                .await
                .map_err(|e| BridgeError::Upstream(format!("Task join failed: {e}")))??;
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
    ) -> Result<TreeState, BridgeError> {
        self.upstream.get_tree_state(interval_end).await
    }

    async fn fetch_subtree_roots(
        &self,
        interval_end: BlockHeight,
    ) -> Result<Vec<SubtreeRoot>, BridgeError> {
        let mut roots = Vec::new();
        for pool in [0, 1] {
            let existing_roots = self.storage.get_subtree_roots(pool, 0, u32::MAX).await?;
            let start_index = existing_roots.len() as u32;

            let fetched = match self.upstream.get_subtree_roots(pool, start_index, 0).await {
                Ok(r) => r,
                Err(BridgeError::Upstream(_)) => vec![],
                Err(e) => return Err(e),
            };

            for r in fetched {
                if r.completing_block_height <= interval_end.0 as u64 {
                    roots.push(r);
                }
            }
        }
        Ok(roots)
    }

    async fn check_chain_identity_at_tip(
        &self,
        meta: &CoverageMetadata,
        tip_height: BlockHeight,
        tip_hash: &[u8],
    ) -> Result<bool, BridgeError> {
        if meta.committed_height.0 < self.config.coverage_start_height {
            return Ok(true);
        }

        if tip_height < meta.committed_height {
            warn!(
                "Tip regression detected: upstream tip {} is below committed height {}",
                tip_height, meta.committed_height
            );
            return Ok(false);
        }

        if tip_height == meta.committed_height {
            if tip_hash.len() == 32 {
                let matches = tip_hash == meta.latest_block_hash.as_bytes();
                if !matches {
                    warn!(
                        "Unchanged-height replacement detected at height {}: upstream hash {} differs from committed {}",
                        tip_height,
                        hex::encode(tip_hash),
                        meta.latest_block_hash.to_hex()
                    );
                }
                return Ok(matches);
            }
            let upstream_block = self.upstream.get_block(meta.committed_height).await?;
            let matches = upstream_block.hash == meta.latest_block_hash.as_bytes();
            if !matches {
                warn!(
                    "Unchanged-height replacement detected at height {}: upstream block hash differs from committed {}",
                    tip_height,
                    meta.latest_block_hash.to_hex()
                );
            }
            return Ok(matches);
        }

        Ok(true)
    }

    async fn find_common_ancestor_and_rollback(
        &self,
        from_height: BlockHeight,
        tip_height: BlockHeight,
    ) -> Result<(), BridgeError> {
        let check_limit = from_height.min(tip_height);
        let start_h = self.config.coverage_start_height;
        let mut common_ancestor = None;

        for h in (start_h..=check_limit.0).rev() {
            let upstream_block = match self.upstream.get_block(BlockHeight(h)).await {
                Ok(b) => b,
                Err(e) => {
                    warn!(
                        "Failed to fetch upstream block at height {h} during reorg walkback: {e}"
                    );
                    continue;
                }
            };
            let local_block = match self.storage.get_compact_block(BlockHeight(h)).await? {
                Some(b) => b,
                None => continue,
            };

            if upstream_block.hash == local_block.hash {
                common_ancestor = Some(BlockHeight(h));
                break;
            }
        }

        let rollback_height = match common_ancestor {
            Some(ancestor) => {
                info!(
                    "Found common ancestor at height {ancestor}; rolling back storage from {}",
                    ancestor.0 + 1
                );
                BlockHeight(ancestor.0 + 1)
            }
            None => {
                warn!(
                    "No common ancestor found in coverage; rolling back to coverage start {start_h}"
                );
                BlockHeight(start_h)
            }
        };

        self.storage.handle_reorg(rollback_height).await?;
        Ok(())
    }
}
