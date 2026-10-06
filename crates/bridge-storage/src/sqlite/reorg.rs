use bridge_core::{BlockHeight, BridgeError};
use rusqlite::{params, Connection, OptionalExtension};

pub fn execute_reorg_rollback(
    conn: &mut Connection,
    fork_height: BlockHeight,
) -> Result<(), BridgeError> {
    let tx = conn
        .transaction()
        .map_err(|e| BridgeError::Storage(format!("Failed to start transaction: {e}")))?;

    // 1. Delete rolled back compact blocks
    tx.execute("DELETE FROM compact_blocks WHERE height >= ?1", params![fork_height.0])
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    // 2. Unspend transparent outputs that were spent at or after fork_height
    tx.execute(
        "UPDATE transparent_outputs 
         SET spent_by_txid = NULL, spent_at_height = NULL 
         WHERE spent_at_height >= ?1",
        params![fork_height.0],
    )
    .map_err(|e| BridgeError::Storage(e.to_string()))?;

    // 3. Delete pre-coverage spends created at or after fork_height
    tx.execute(
        "DELETE FROM pre_coverage_spends WHERE spending_height >= ?1",
        params![fork_height.0],
    )
    .map_err(|e| BridgeError::Storage(e.to_string()))?;

    // 4. Recompute latest committed block
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
}
