use bridge_core::{BlockHash, BlockHeight, BridgeError, TxId};
use bridge_proto::{CompactBlock, RawTransaction, SubtreeRoot, TreeState};
use prost::Message;
use rusqlite::{Connection, OptionalExtension, params};

pub fn query_latest_block(
    conn: &Connection,
) -> Result<Option<(BlockHeight, BlockHash)>, BridgeError> {
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
}

pub fn query_compact_block(
    conn: &Connection,
    height: BlockHeight,
) -> Result<Option<CompactBlock>, BridgeError> {
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
}

pub fn query_height_by_hash(
    conn: &Connection,
    hash: &BlockHash,
) -> Result<Option<BlockHeight>, BridgeError> {
    let mut stmt = conn
        .prepare("SELECT height FROM compact_blocks WHERE block_hash = ?1")
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let h: Option<u32> = stmt
        .query_row(params![hash.as_bytes()], |row| row.get(0))
        .optional()
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    Ok(h.map(BlockHeight))
}

pub fn query_compact_block_range(
    conn: &Connection,
    start: BlockHeight,
    end: BlockHeight,
) -> Result<Vec<CompactBlock>, BridgeError> {
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
}

pub fn query_full_transaction(
    conn: &Connection,
    txid: &TxId,
) -> Result<Option<RawTransaction>, BridgeError> {
    let txid_bytes = txid.0.to_vec();
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
}

pub fn query_tree_state(
    conn: &Connection,
    height: BlockHeight,
) -> Result<Option<TreeState>, BridgeError> {
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
}

pub fn query_subtree_roots(
    conn: &Connection,
    pool: i32,
    start_index: u32,
    max_entries: u32,
) -> Result<Vec<SubtreeRoot>, BridgeError> {
    let sql = if max_entries == 0 {
        "SELECT root_hash, completing_block_hash, completing_height FROM subtree_roots 
         WHERE pool = ?1 AND subtree_index >= ?2 
         ORDER BY subtree_index ASC"
    } else {
        "SELECT root_hash, completing_block_hash, completing_height FROM subtree_roots 
         WHERE pool = ?1 AND subtree_index >= ?2 
         ORDER BY subtree_index ASC LIMIT ?3"
    };

    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let mut roots = Vec::new();
    if max_entries == 0 {
        let rows = stmt
            .query_map(params![pool, start_index], |row| {
                let root_hash: Vec<u8> = row.get(0)?;
                let completing_block_hash: Vec<u8> = row.get(1)?;
                let completing_h: u32 = row.get(2)?;
                Ok(SubtreeRoot {
                    root_hash,
                    completing_block_height: completing_h as u64,
                    completing_block_hash,
                })
            })
            .map_err(|e| BridgeError::Storage(e.to_string()))?;
        for r in rows {
            roots.push(r.map_err(|e| BridgeError::Storage(e.to_string()))?);
        }
    } else {
        let rows = stmt
            .query_map(params![pool, start_index, max_entries], |row| {
                let root_hash: Vec<u8> = row.get(0)?;
                let completing_block_hash: Vec<u8> = row.get(1)?;
                let completing_h: u32 = row.get(2)?;
                Ok(SubtreeRoot {
                    root_hash,
                    completing_block_height: completing_h as u64,
                    completing_block_hash,
                })
            })
            .map_err(|e| BridgeError::Storage(e.to_string()))?;
        for r in rows {
            roots.push(r.map_err(|e| BridgeError::Storage(e.to_string()))?);
        }
    }
    Ok(roots)
}
