use bridge_proto::{CompactBlock, CompactTx, RawTransaction, TreeState};

/// Creates a test CompactBlock with given height, previous hash, and transactions.
pub fn create_test_compact_block(
    height: u64,
    prev_hash: Vec<u8>,
    vtx: Vec<CompactTx>,
) -> CompactBlock {
    CompactBlock {
        height,
        hash: vec![(height as u8).wrapping_add(1); 32],
        prev_hash,
        time: (1_700_000_000 + height * 75) as u32,
        header: vec![0; 80],
        vtx,
        chain_metadata: None,
    }
}

/// Creates a test transaction with computed ZIP 244 temporary BLAKE2b digest.
pub fn create_test_tx(data: Vec<u8>, height: u64) -> (RawTransaction, [u8; 32]) {
    let hash = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(b"ZcashTxHash_TEMP")
        .hash(&data);
    let mut txid = [0u8; 32];
    txid.copy_from_slice(hash.as_bytes());

    (RawTransaction { data, height }, txid)
}

/// Creates a test TreeState with placeholder shielded commitment trees.
pub fn create_test_tree_state(height: u64, hash_hex: &str) -> TreeState {
    TreeState {
        network: "mainnet".to_string(),
        height,
        hash: hash_hex.to_string(),
        time: (1_700_000_000 + height * 75) as u32,
        sapling_tree: "sapling_tree_state_data".to_string(),
        orchard_tree: "orchard_tree_state_data".to_string(),
        ironwood_tree: "ironwood_tree_state_data".to_string(),
    }
}
