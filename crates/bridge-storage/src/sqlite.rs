use crate::migrations::apply_migrations;
use crate::traits::{StorageBackend, VerifiedIntervalBatch};
use async_trait::async_trait;
use bridge_core::{
    BlockHash, BlockHeight, BridgeError, CoverageMetadata, IntervalRange, Network, TransparentAddress, TxId,
};
use bridge_proto::{
    CompactBlock, GetAddressUtxosReply, RawTransaction, SubtreeRoot, TreeState,
};
use prost::Message;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
pub struct SqliteStorage {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteStorage {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BridgeError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| BridgeError::Storage(format!("Failed to create storage directory: {e}")))?;
        }
        let mut conn = Connection::open(path)
            .map_err(|e| BridgeError::Storage(format!("Failed to open SQLite database: {e}")))?;
        apply_migrations(&mut conn)
            .map_err(|e| BridgeError::Storage(format!("Failed to apply migrations: {e}")))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn in_memory() -> Result<Self, BridgeError> {
        let mut conn = Connection::open_in_memory()
            .map_err(|e| BridgeError::Storage(format!("Failed to open in-memory SQLite: {e}")))?;
        apply_migrations(&mut conn)
            .map_err(|e| BridgeError::Storage(format!("Failed to apply migrations: {e}")))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }
}

#[async_trait]
impl StorageBackend for SqliteStorage {
    async fn get_coverage_metadata(&self) -> Result<Option<CoverageMetadata>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare(
                    "SELECT network, coverage_start_height, committed_height, latest_block_hash, updated_at, 
                            acquisition_failures_count, last_error
                     FROM coverage_metadata WHERE id = 1",
                )
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let res = stmt
                .query_row([], |row| {
                    let network_str: String = row.get(0)?;
                    let start_h: u32 = row.get(1)?;
                    let committed_h: u32 = row.get(2)?;
                    let hash_bytes: Vec<u8> = row.get(3)?;
                    let updated_at: String = row.get(4)?;
                    let failures_count: u64 = row.get(5)?;
                    let last_error: Option<String> = row.get(6)?;

                    let mut hash_arr = [0u8; 32];
                    if hash_bytes.len() == 32 {
                        hash_arr.copy_from_slice(&hash_bytes);
                    }

                    let network = match network_str.as_str() {
                        "testnet" => Network::Testnet,
                        "regtest" => Network::Regtest,
                        _ => Network::Mainnet,
                    };

                    Ok(CoverageMetadata {
                        network,
                        coverage_start_height: BlockHeight(start_h),
                        committed_height: BlockHeight(committed_h),
                        latest_block_hash: BlockHash(hash_arr),
                        updated_at,
                        acquisition_failures_count: failures_count,
                        last_error,
                    })
                })
                .optional()
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            Ok(res)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn init_coverage(&self, network: Network, start_height: BlockHeight) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let empty_hash = [0u8; 32];
            conn.execute(
                "INSERT INTO coverage_metadata (id, network, coverage_start_height, committed_height, latest_block_hash)
                 VALUES (1, ?1, ?2, ?3, ?4)
                 ON CONFLICT(id) DO UPDATE SET network=?1, coverage_start_height=?2",
                params![
                    network.to_string(),
                    start_height.0,
                    start_height.0.saturating_sub(1),
                    &empty_hash[..],
                ],
            )
            .map_err(|e| BridgeError::Storage(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_latest_block(&self) -> Result<Option<(BlockHeight, BlockHash)>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT height, block_hash FROM compact_blocks ORDER BY height DESC LIMIT 1")
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let res = stmt
                .query_row([], |row| {
                    let h: u32 = row.get(0)?;
                    let hash_bytes: Vec<u8> = row.get(1)?;
                    let mut hash_arr = [0u8; 32];
                    if hash_bytes.len() == 32 {
                        hash_arr.copy_from_slice(&hash_bytes);
                    }
                    Ok((BlockHeight(h), BlockHash(hash_arr)))
                })
                .optional()
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            Ok(res)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_compact_block(&self, height: BlockHeight) -> Result<Option<CompactBlock>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT compact_block_proto FROM compact_blocks WHERE height = ?1")
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let bytes_opt: Option<Vec<u8>> = stmt
                .query_row(params![height.0], |row| row.get(0))
                .optional()
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            match bytes_opt {
                Some(b) => CompactBlock::decode(b.as_slice())
                    .map(Some)
                    .map_err(|e| BridgeError::Storage(format!("Protobuf decode error: {e}"))),
                None => Ok(None),
            }
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
            let mut stmt = conn
                .prepare(
                    "SELECT compact_block_proto FROM compact_blocks 
                     WHERE height >= ?1 AND height <= ?2 
                     ORDER BY height ASC",
                )
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(params![start.0, end.0], |row| row.get::<_, Vec<u8>>(0))
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let mut blocks = Vec::new();
            for r in rows {
                let bytes = r.map_err(|e| BridgeError::Storage(e.to_string()))?;
                let block = CompactBlock::decode(bytes.as_slice())
                    .map_err(|e| BridgeError::Storage(format!("Protobuf decode error: {e}")))?;
                blocks.push(block);
            }
            Ok(blocks)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_full_transaction(&self, txid: &TxId) -> Result<Option<RawTransaction>, BridgeError> {
        let conn = self.conn.clone();
        let txid_bytes = txid.0.to_vec();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT raw_data, height FROM full_transactions WHERE txid = ?1")
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let res = stmt
                .query_row(params![txid_bytes], |row| {
                    let raw_data: Vec<u8> = row.get(0)?;
                    let height: u32 = row.get(1)?;
                    Ok(RawTransaction {
                        data: raw_data,
                        height: height as u64,
                    })
                })
                .optional()
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            Ok(res)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_tree_state(&self, height: BlockHeight) -> Result<Option<TreeState>, BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = conn
                .prepare("SELECT tree_state_proto FROM tree_states WHERE height = ?1")
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let bytes_opt: Option<Vec<u8>> = stmt
                .query_row(params![height.0], |row| row.get(0))
                .optional()
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            match bytes_opt {
                Some(b) => TreeState::decode(b.as_slice())
                    .map(Some)
                    .map_err(|e| BridgeError::Storage(format!("TreeState decode error: {e}"))),
                None => Ok(None),
            }
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
            let mut stmt = conn
                .prepare(
                    "SELECT root_hash, completing_height FROM subtree_roots 
                     WHERE pool = ?1 AND subtree_index >= ?2 
                     ORDER BY subtree_index ASC LIMIT ?3",
                )
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(params![pool, start_index, max_entries], |row| {
                    let root_hash: Vec<u8> = row.get(0)?;
                    let completing_h: u32 = row.get(1)?;
                    Ok(SubtreeRoot {
                        root_hash,
                        completing_block_height: completing_h as u64,
                        completing_block_hash: vec![],
                    })
                })
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let mut roots = Vec::new();
            for r in rows {
                roots.push(r.map_err(|e| BridgeError::Storage(e.to_string()))?);
            }
            Ok(roots)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn get_address_utxos(&self, address: &TransparentAddress) -> Result<Vec<GetAddressUtxosReply>, BridgeError> {
        let conn = self.conn.clone();
        let addr_str = address.as_str().to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();

            // Spec requirement: Incomplete history must return an error—not an empty history or zero balance.
            // Check if this address has unresolved pre-coverage spends
            let has_pre_coverage_spend: bool = conn
                .query_row(
                    "SELECT 1 FROM pre_coverage_spends WHERE address = ?1 LIMIT 1",
                    params![addr_str],
                    |_| Ok(true),
                )
                .unwrap_or(false);

            if has_pre_coverage_spend {
                return Err(BridgeError::IncompleteHistory {
                    txid: "unknown".to_string(),
                    vout: 0,
                    coverage_start: 0,
                });
            }

            let mut stmt = conn
                .prepare(
                    "SELECT txid, vout, address, value_zat, script_pubkey, height 
                     FROM transparent_outputs 
                     WHERE address = ?1 AND spent_by_txid IS NULL",
                )
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(params![addr_str], |row| {
                    let txid: Vec<u8> = row.get(0)?;
                    let vout: u32 = row.get(1)?;
                    let addr: String = row.get(2)?;
                    let value: u64 = row.get(3)?;
                    let script: Vec<u8> = row.get(4)?;
                    let height: u32 = row.get(5)?;

                    Ok(GetAddressUtxosReply {
                        txid,
                        index: vout as i32,
                        script,
                        value_zat: value as i64,
                        height: height as u64,
                        address: addr,
                    })
                })
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let mut utxos = Vec::new();
            for r in rows {
                utxos.push(r.map_err(|e| BridgeError::Storage(e.to_string()))?);
            }
            Ok(utxos)
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
        let addr_str = address.as_str().to_string();

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let (query, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match range {
                Some(r) => (
                    "SELECT DISTINCT ft.raw_data, ft.height 
                     FROM full_transactions ft
                     JOIN transparent_outputs t_out ON ft.txid = t_out.txid
                     WHERE t_out.address = ?1 AND ft.height >= ?2 AND ft.height <= ?3
                     ORDER BY ft.height ASC".to_string(),
                    vec![
                        Box::new(addr_str),
                        Box::new(r.start.0),
                        Box::new(r.end.0),
                    ],
                ),
                None => (
                    "SELECT DISTINCT ft.raw_data, ft.height 
                     FROM full_transactions ft
                     JOIN transparent_outputs t_out ON ft.txid = t_out.txid
                     WHERE t_out.address = ?1
                     ORDER BY ft.height ASC".to_string(),
                    vec![Box::new(addr_str)],
                ),
            };

            let rusqlite_params: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
            let mut stmt = conn.prepare(&query).map_err(|e| BridgeError::Storage(e.to_string()))?;

            let rows = stmt
                .query_map(rusqlite_params.as_slice(), |row| {
                    let raw_data: Vec<u8> = row.get(0)?;
                    let height: u32 = row.get(1)?;
                    Ok(RawTransaction {
                        data: raw_data,
                        height: height as u64,
                    })
                })
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            let mut txs = Vec::new();
            for r in rows {
                txs.push(r.map_err(|e| BridgeError::Storage(e.to_string()))?);
            }
            Ok(txs)
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn commit_verified_interval(&self, batch: VerifiedIntervalBatch) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().unwrap();
            let tx = conn
                .transaction()
                .map_err(|e| BridgeError::Storage(format!("Failed to start transaction: {e}")))?;

            // 1. Insert CompactBlocks
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT OR REPLACE INTO compact_blocks (height, block_hash, prev_hash, time, header, compact_block_proto)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    )
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;

                for b in &batch.blocks {
                    let mut proto_bytes = Vec::new();
                    b.encode(&mut proto_bytes)
                        .map_err(|e| BridgeError::Storage(format!("Failed to encode block: {e}")))?;

                    stmt.execute(params![
                        b.height as u32,
                        b.hash,
                        b.prev_hash,
                        b.time,
                        b.header,
                        proto_bytes,
                    ])
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;
                }
            }

            // 2. Insert FullTransactions
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT OR REPLACE INTO full_transactions (txid, height, block_time, raw_data)
                         VALUES (?1, ?2, ?3, ?4)",
                    )
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;

                for raw_tx in &batch.transactions {
                    // Extract or hash txid from raw_tx or use existing
                    // In batch, txids are keyed in raw_tx data or verified
                    // For durability, compute 32-byte hash
                    let tx_hash = blake2b_simd::Params::new()
                        .hash_length(32)
                        .personal(b"ZcashTxHash_TEMP")
                        .hash(&raw_tx.data);

                    stmt.execute(params![
                        tx_hash.as_bytes(),
                        raw_tx.height as u32,
                        0u32, // block_time
                        raw_tx.data,
                    ])
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;
                }
            }

            // 3. Insert TransparentOutputs
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT OR REPLACE INTO transparent_outputs 
                         (txid, vout, address, value_zat, script_pubkey, height, spent_by_txid, spent_at_height)
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)",
                    )
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;

                for out in &batch.transparent_outputs {
                    stmt.execute(params![
                        out.txid.0.as_slice(),
                        out.vout,
                        out.address.as_str(),
                        out.value_zat,
                        out.script_pubkey,
                        out.height.0,
                    ])
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;
                }
            }

            // 4. Resolve TransparentSpends
            {
                let mut update_stmt = tx
                    .prepare(
                        "UPDATE transparent_outputs 
                         SET spent_by_txid = ?1, spent_at_height = ?2 
                         WHERE txid = ?3 AND vout = ?4",
                    )
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;

                let mut pre_coverage_stmt = tx
                    .prepare(
                        "INSERT OR IGNORE INTO pre_coverage_spends (spending_txid, spending_height, prev_txid, prev_vout, address)
                         VALUES (?1, ?2, ?3, ?4, NULL)",
                    )
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;

                for spend in &batch.transparent_spends {
                    let rows_affected = update_stmt
                        .execute(params![
                            spend.spending_txid.0.as_slice(),
                            spend.spending_height.0,
                            spend.prev_txid.0.as_slice(),
                            spend.prev_vout,
                        ])
                        .map_err(|e| BridgeError::Storage(e.to_string()))?;

                    if rows_affected == 0 {
                        // The output being spent was created prior to coverage start or is untracked
                        pre_coverage_stmt
                            .execute(params![
                                spend.spending_txid.0.as_slice(),
                                spend.spending_height.0,
                                spend.prev_txid.0.as_slice(),
                                spend.prev_vout,
                            ])
                            .map_err(|e| BridgeError::Storage(e.to_string()))?;
                    }
                }
            }

            // 5. Insert TreeStates
            {
                let mut stmt = tx
                    .prepare(
                        "INSERT OR REPLACE INTO tree_states (height, block_hash, sapling_tree_hex, orchard_tree_hex, tree_state_proto)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                    )
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;

                for ts in &batch.tree_states {
                    let mut proto_bytes = Vec::new();
                    ts.encode(&mut proto_bytes)
                        .map_err(|e| BridgeError::Storage(format!("TreeState encode error: {e}")))?;

                    let mut hash_arr = [0u8; 32];
                    if let Ok(decoded) = hex::decode(&ts.hash) {
                        if decoded.len() == 32 {
                            hash_arr.copy_from_slice(&decoded);
                        }
                    }

                    stmt.execute(params![
                        ts.height as u32,
                        hash_arr.as_slice(),
                        ts.sapling_tree,
                        ts.orchard_tree,
                        proto_bytes,
                    ])
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;
                }
            }

            // 6. Update Coverage Metadata
            tx.execute(
                "UPDATE coverage_metadata 
                 SET committed_height = ?1, latest_block_hash = ?2, updated_at = CURRENT_TIMESTAMP
                 WHERE id = 1",
                params![batch.end_height.0, batch.end_block_hash.0.as_slice()],
            )
            .map_err(|e| BridgeError::Storage(e.to_string()))?;

            tx.commit()
                .map_err(|e| BridgeError::Storage(format!("Failed to commit interval batch: {e}")))?;

            Ok(())
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn handle_reorg(&self, fork_height: BlockHeight) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().unwrap();
            let tx = conn
                .transaction()
                .map_err(|e| BridgeError::Storage(format!("Failed to start transaction: {e}")))?;

            tx.execute(
                "DELETE FROM compact_blocks WHERE height >= ?1",
                params![fork_height.0],
            )
            .map_err(|e| BridgeError::Storage(e.to_string()))?;

            // Unspend transparent outputs that were spent at or after fork_height
            tx.execute(
                "UPDATE transparent_outputs 
                 SET spent_by_txid = NULL, spent_at_height = NULL 
                 WHERE spent_at_height >= ?1",
                params![fork_height.0],
            )
            .map_err(|e| BridgeError::Storage(e.to_string()))?;

            tx.execute(
                "DELETE FROM pre_coverage_spends WHERE spending_height >= ?1",
                params![fork_height.0],
            )
            .map_err(|e| BridgeError::Storage(e.to_string()))?;

            // Recompute latest committed block
            let new_latest: Option<(u32, Vec<u8>)> = tx
                .query_row(
                    "SELECT height, block_hash FROM compact_blocks ORDER BY height DESC LIMIT 1",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
                .map_err(|e| BridgeError::Storage(e.to_string()))?;

            if let Some((h, hash)) = new_latest {
                tx.execute(
                    "UPDATE coverage_metadata 
                     SET committed_height = ?1, latest_block_hash = ?2, updated_at = CURRENT_TIMESTAMP
                     WHERE id = 1",
                    params![h, hash],
                )
                .map_err(|e| BridgeError::Storage(e.to_string()))?;
            }

            tx.commit()
                .map_err(|e| BridgeError::Storage(format!("Failed to commit reorg rollback: {e}")))?;

            Ok(())
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }

    async fn record_acquisition_failure(&self, error_msg: &str) -> Result<(), BridgeError> {
        let conn = self.conn.clone();
        let err_str = error_msg.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.execute(
                "UPDATE coverage_metadata 
                 SET acquisition_failures_count = acquisition_failures_count + 1, 
                     last_error = ?1, 
                     updated_at = CURRENT_TIMESTAMP 
                 WHERE id = 1",
                params![err_str],
            )
            .map_err(|e| BridgeError::Storage(e.to_string()))?;
            Ok(())
        })
        .await
        .map_err(|e| BridgeError::Storage(format!("Join error: {e}")))?
    }
}
