use bridge_core::{BlockHash, BlockHeight, BridgeError, CoverageMetadata, Network};
use rusqlite::{params, Connection, OptionalExtension};

pub fn query_coverage_metadata(conn: &Connection) -> Result<Option<CoverageMetadata>, BridgeError> {
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
}

pub fn initialize_coverage(
    conn: &Connection,
    network: Network,
    start_height: BlockHeight,
) -> Result<(), BridgeError> {
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
}

pub fn record_failure(conn: &Connection, error_msg: &str) -> Result<(), BridgeError> {
    conn.execute(
        "UPDATE coverage_metadata 
         SET acquisition_failures_count = acquisition_failures_count + 1, 
             last_error = ?1, 
             updated_at = CURRENT_TIMESTAMP 
         WHERE id = 1",
        params![error_msg],
    )
    .map_err(|e| BridgeError::Storage(e.to_string()))?;
    Ok(())
}
