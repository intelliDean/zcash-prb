use crate::traits::{TransparentOutputRecord, TransparentSpendRecord};
use bridge_core::{BlockHash, BlockHeight, BridgeError};
use bridge_proto::{CompactBlock, RawTransaction, TreeState};
use prost::Message;
use rusqlite::{OptionalExtension, Transaction, params};

pub fn insert_compact_blocks(tx: &Transaction, blocks: &[CompactBlock]) -> Result<(), BridgeError> {
    let mut stmt = tx
        .prepare(
            "INSERT OR REPLACE INTO compact_blocks (height, block_hash, prev_hash, time, header, compact_block_proto)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    for b in blocks {
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
    Ok(())
}

pub fn insert_full_transactions(
    tx: &Transaction,
    transactions: &[RawTransaction],
    _blocks: &[CompactBlock],
) -> Result<(), BridgeError> {
    let mut stmt = tx
        .prepare(
            "INSERT OR REPLACE INTO full_transactions (txid, height, block_time, raw_data)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    for raw_tx in transactions {
        // Enforce strict consensus deserialization without fallback
        let txid = bridge_verifier::compute_raw_txid(&raw_tx.data)?;

        stmt.execute(params![
            &txid.0[..],
            raw_tx.height as u32,
            0u32,
            raw_tx.data,
        ])
        .map_err(|e| BridgeError::Storage(e.to_string()))?;
    }
    Ok(())
}

pub fn insert_subtree_roots(
    tx: &Transaction,
    roots: &[(i32, bridge_proto::SubtreeRoot)],
) -> Result<(), BridgeError> {
    if roots.is_empty() {
        return Ok(());
    }

    let mut check_stmt = tx
        .prepare("SELECT subtree_index FROM subtree_roots WHERE pool = ?1 AND root_hash = ?2")
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let mut next_idx_stmt = tx
        .prepare("SELECT COALESCE(MAX(subtree_index) + 1, 0) FROM subtree_roots WHERE pool = ?1")
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let mut insert_stmt = tx
        .prepare(
            "INSERT OR REPLACE INTO subtree_roots (pool, subtree_index, root_hash, completing_block_hash, completing_height)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    for (pool, r) in roots {
        let pool = *pool;
        let existing_idx: Option<u32> = check_stmt
            .query_row(params![pool, &r.root_hash], |row| row.get(0))
            .optional()
            .map_err(|e| BridgeError::Storage(e.to_string()))?;

        let idx = match existing_idx {
            Some(i) => i,
            None => {
                let next: u32 = next_idx_stmt
                    .query_row(params![pool], |row| row.get(0))
                    .map_err(|e| BridgeError::Storage(e.to_string()))?;
                next
            }
        };

        insert_stmt
            .execute(params![
                pool,
                idx,
                r.root_hash,
                r.completing_block_hash,
                r.completing_block_height as u32,
            ])
            .map_err(|e| BridgeError::Storage(e.to_string()))?;
    }
    Ok(())
}

pub fn insert_transparent_outputs(
    tx: &Transaction,
    outputs: &[TransparentOutputRecord],
) -> Result<(), BridgeError> {
    let mut stmt = tx
        .prepare(
            "INSERT OR REPLACE INTO transparent_outputs 
             (txid, vout, address, value_zat, script_pubkey, height, spent_by_txid, spent_at_height)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL)",
        )
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    for out in outputs {
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
    Ok(())
}

pub fn resolve_transparent_spends(
    tx: &Transaction,
    spends: &[TransparentSpendRecord],
) -> Result<(), BridgeError> {
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

    for spend in spends {
        let rows_affected = update_stmt
            .execute(params![
                spend.spending_txid.0.as_slice(),
                spend.spending_height.0,
                spend.prev_txid.0.as_slice(),
                spend.prev_vout,
            ])
            .map_err(|e| BridgeError::Storage(e.to_string()))?;

        if rows_affected == 0 {
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
    Ok(())
}

pub fn insert_tree_states(tx: &Transaction, tree_states: &[TreeState]) -> Result<(), BridgeError> {
    let mut stmt = tx
        .prepare(
            "INSERT OR REPLACE INTO tree_states (height, block_hash, sapling_tree_hex, orchard_tree_hex, tree_state_proto)
             VALUES (?1, ?2, ?3, ?4, ?5)",
        )
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    for ts in tree_states {
        let mut proto_bytes = Vec::new();
        ts.encode(&mut proto_bytes)
            .map_err(|e| BridgeError::Storage(format!("TreeState encode error: {e}")))?;

        let mut hash_arr = [0u8; 32];
        if let Ok(decoded) = hex::decode(&ts.hash)
            && decoded.len() == 32
        {
            hash_arr.copy_from_slice(&decoded);
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
    Ok(())
}

pub fn update_coverage_checkpoint(
    tx: &Transaction,
    end_height: BlockHeight,
    end_block_hash: &BlockHash,
) -> Result<(), BridgeError> {
    tx.execute(
        "UPDATE coverage_metadata 
         SET committed_height = ?1, latest_block_hash = ?2, updated_at = CURRENT_TIMESTAMP
         WHERE id = 1",
        params![end_height.0, end_block_hash.0.as_slice()],
    )
    .map_err(|e| BridgeError::Storage(e.to_string()))?;
    Ok(())
}
