pub mod batch;
pub mod coverage;
pub mod queries;
pub mod reorg;
pub mod transparent;

use crate::migrations::apply_migrations;
use crate::traits::{StorageBackend, VerifiedIntervalBatch};
use async_trait::async_trait;
use bridge_core::{
    BlockHash, BlockHeight, BridgeError, CoverageMetadata, IntervalRange, Network,
    TransparentAddress, TxId,
};
use bridge_proto::{CompactBlock, GetAddressUtxosReply, RawTransaction, SubtreeRoot, TreeState};
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BridgeError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                BridgeError::Storage(format!("Failed to create storage directory: {e}"))
            })?;
        }
        let mut conn = Connection::open(path)
            .map_err(|e| BridgeError::Storage(format!("Failed to open SQLite database: {e}")))?;
        apply_migrations(&mut conn)
            .map_err(|e| BridgeError::Storage(format!("Failed to apply migrations: {e}")))?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    pub fn in_memory() -> Result<Self, BridgeError> {
        let mut conn = Connection::open_in_memory()
            .map_err(|e| BridgeError::Storage(format!("Failed to open in-memory SQLite: {e}")))?;
        apply_migrations(&mut conn)
            .map_err(|e| BridgeError::Storage(format!("Failed to apply migrations: {e}")))?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }
}

#[async_trait]
impl StorageBackend for SqliteStorage {
    async fn get_coverage_metadata(&self) -> Result<Option<CoverageMetadata>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            coverage::query_coverage_metadata(&conn)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn init_coverage(
        &self,
        network: Network,
        start_height: BlockHeight,
    ) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            coverage::initialize_coverage(&conn, network, start_height)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_latest_block(&self) -> Result<Option<(BlockHeight, BlockHash)>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            queries::query_latest_block(&conn)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_compact_block(
        &self,
        height: BlockHeight,
    ) -> Result<Option<CompactBlock>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            queries::query_compact_block(&conn, height)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_compact_block_range(
        &self,
        start: BlockHeight,
        end: BlockHeight,
    ) -> Result<Vec<CompactBlock>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            queries::query_compact_block_range(&conn, start, end)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_full_transaction(
        &self,
        txid: &TxId,
    ) -> Result<Option<RawTransaction>, BridgeError> {
        let conn = self.conn.clone();
        let txid_clone = *txid;
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            queries::query_full_transaction(&conn, &txid_clone)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_tree_state(&self, height: BlockHeight) -> Result<Option<TreeState>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            queries::query_tree_state(&conn, height)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_subtree_roots(
        &self,
        pool: i32,
        start_index: u32,
        max_entries: u32,
    ) -> Result<Vec<SubtreeRoot>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            queries::query_subtree_roots(&conn, pool, start_index, max_entries)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_address_utxos(
        &self,
        address: &TransparentAddress,
    ) -> Result<Vec<GetAddressUtxosReply>, BridgeError> {
        let conn = self.conn.clone();
        let addr = address.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            transparent::query_address_utxos(&conn, &addr)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_taddress_transactions(
        &self,
        address: &TransparentAddress,
        range: Option<IntervalRange>,
    ) -> Result<Vec<RawTransaction>, BridgeError> {
        let conn = self.conn.clone();
        let addr = address.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            transparent::query_taddress_transactions(&conn, &addr, range)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn commit_verified_interval(
        &self,
        batch: VerifiedIntervalBatch,
    ) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().unwrap();
            let tx = conn
                .transaction()
                .map_err(|e| BridgeError::Storage(format!("Failed to start transaction: {e}")))?;

            batch::insert_compact_blocks(&tx, &batch.blocks)?;
            batch::insert_full_transactions(&tx, &batch.transactions)?;
            batch::insert_transparent_outputs(&tx, &batch.transparent_outputs)?;
            batch::resolve_transparent_spends(&tx, &batch.transparent_spends)?;
            batch::insert_tree_states(&tx, &batch.tree_states)?;
            batch::update_coverage_checkpoint(&tx, batch.end_height, &batch.end_block_hash)?;

            tx.commit().map_err(|e| {
                BridgeError::Storage(format!("Failed to commit interval batch: {e}"))
            })?;

            Ok(())
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn handle_reorg(&self, fork_height: BlockHeight) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().unwrap();
            reorg::execute_reorg_rollback(&mut conn, fork_height)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn record_acquisition_failure(&self, error_msg: &str) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        let err_str = error_msg.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            coverage::record_failure(&conn, &err_str)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }
}
