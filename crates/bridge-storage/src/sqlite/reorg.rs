use bridge_core::{BlockHeight, BridgeError};
use rusqlite::{Connection, OptionalExtension, params};

pub fn execute_reorg_rollback(
    conn: &mut Connection,
    fork_height: BlockHeight,
) -> Result<(), BridgeError> {
    let tx = conn
        .transaction()
        .map_err(|e| BridgeError::Storage(format!("Failed to start transaction: {e}")))?;

    // 1. Delete rolled back compact blocks
    tx.execute(
        "DELETE FROM compact_blocks WHERE height >= ?1",
        params![fork_height.0],
    )
    .map_err(|e| BridgeError::Storage(e.to_string()))?;

    // 2. Delete rolled back tree states
    tx.execute(
        "DELETE FROM tree_states WHERE height >= ?1",
        params![fork_height.0],
    )
    .map_err(|e| BridgeError::Storage(e.to_string()))?;

    // 3. Delete transparent outputs created at or after fork_height
    tx.execute(
        "DELETE FROM transparent_outputs WHERE height >= ?1",
        params![fork_height.0],
    )
    .map_err(|e| BridgeError::Storage(e.to_string()))?;

    // 4. Unspend transparent outputs that were spent at or after fork_height
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

    // 4. Delete subtree roots completing at or after fork_height
    tx.execute(
        "DELETE FROM subtree_roots WHERE completing_height >= ?1",
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
    } else {
        let start_h: u32 = tx
            .query_row(
                "SELECT coverage_start_height FROM coverage_metadata WHERE id = 1",
                [],
                |row| row.get(0),
            )
            .map_err(|e| BridgeError::Storage(e.to_string()))?;

        let reset_h = start_h.saturating_sub(1);
        let empty_hash = vec![0u8; 32];
        tx.execute(
            "UPDATE coverage_metadata 
             SET committed_height = ?1, latest_block_hash = ?2, updated_at = CURRENT_TIMESTAMP
             WHERE id = 1",
            params![reset_h, empty_hash],
        )
        .map_err(|e| BridgeError::Storage(e.to_string()))?;
    }

    tx.commit()
        .map_err(|e| BridgeError::Storage(format!("Failed to commit reorg rollback: {e}")))?;

    Ok(())
}
