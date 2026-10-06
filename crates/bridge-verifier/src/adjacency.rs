use bridge_core::{BlockHash, BlockHeight, BridgeError};
use bridge_proto::CompactBlock;

/// Validates that a slice of CompactBlocks forms a strictly contiguous, unbroken chain.
pub fn validate_block_sequence(
    blocks: &[CompactBlock],
    expected_start_height: BlockHeight,
    expected_prev_hash: Option<&BlockHash>,
) -> Result<(), BridgeError> {
    if blocks.is_empty() {
        return Ok(());
    }

    // 1. Verify first block height
    let first_height = BlockHeight(blocks[0].height as u32);
    if first_height != expected_start_height {
        return Err(BridgeError::Verification(format!(
            "Block sequence height mismatch: expected start height {}, got {}",
            expected_start_height, first_height
        )));
    }

    // 2. Verify link to previous committed block
    if let Some(expected_prev) = expected_prev_hash {
        let actual_prev = &blocks[0].prev_hash;
        if actual_prev.as_slice() != expected_prev.as_bytes().as_slice() {
            return Err(BridgeError::ReorgDetected {
                height: blocks[0].height as u32,
                expected: expected_prev.to_hex(),
                actual: hex::encode(actual_prev),
            });
        }
    }

    // 3. Verify internal adjacency
    for i in 1..blocks.len() {
        let prev = &blocks[i - 1];
        let curr = &blocks[i];

        if curr.height != prev.height + 1 {
            return Err(BridgeError::Verification(format!(
                "Non-sequential block heights: block {} immediately followed block {}",
                curr.height, prev.height
            )));
        }

        if curr.prev_hash != prev.hash {
            return Err(BridgeError::ReorgDetected {
                height: curr.height as u32,
                expected: hex::encode(&prev.hash),
                actual: hex::encode(&curr.prev_hash),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_adjacency() {
        let b1 = CompactBlock {
            height: 100,
            hash: vec![1; 32],
            prev_hash: vec![0; 32],
            time: 1000,
            header: vec![],
            vtx: vec![],
            chain_metadata: None,
        };
        let b2 = CompactBlock {
            height: 101,
            hash: vec![2; 32],
            prev_hash: vec![1; 32],
            time: 1075,
            header: vec![],
            vtx: vec![],
            chain_metadata: None,
        };

        let prev = BlockHash([0; 32]);
        assert!(validate_block_sequence(&[b1, b2], BlockHeight(100), Some(&prev)).is_ok());
    }

    #[test]
    fn test_reorg_detected_on_first_block() {
        let b1 = CompactBlock {
            height: 100,
            hash: vec![1; 32],
            prev_hash: vec![99; 32],
            time: 1000,
            header: vec![],
            vtx: vec![],
            chain_metadata: None,
        };

        let prev = BlockHash([0; 32]);
        let res = validate_block_sequence(&[b1], BlockHeight(100), Some(&prev));
        assert!(matches!(res, Err(BridgeError::ReorgDetected { .. })));
    }
}
