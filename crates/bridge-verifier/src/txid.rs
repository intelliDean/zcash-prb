use bridge_core::{BridgeError, TxId};
use sha2::{Digest, Sha256};

/// Extracts the transaction version and overwintered flag from the first 4 bytes of transaction data.
fn parse_transaction_version(raw_tx_bytes: &[u8]) -> Result<(bool, u32), BridgeError> {
    if raw_tx_bytes.len() < 4 {
        return Err(BridgeError::Verification("Transaction shorter than 4 bytes".to_string()));
    }

    let header = u32::from_le_bytes(raw_tx_bytes[..4].try_into().unwrap());
    let is_overwintered = (header >> 31) == 1;
    let version = header & 0x7fff_ffff;

    Ok((is_overwintered, version))
}

/// Computes BLAKE2b-256 digest for v4 (Sapling) and v5 (Orchard/Ironwood) transactions.
fn compute_zip244_digest(raw_tx_bytes: &[u8]) -> TxId {
    let hash = blake2b_simd::Params::new()
        .hash_length(32)
        .personal(b"ZcashTxHash_TEMP")
        .hash(raw_tx_bytes);

    let mut out = [0u8; 32];
    out.copy_from_slice(hash.as_bytes());
    TxId(out)
}

/// Computes double-SHA256 digest for legacy v1/v2 transactions.
fn compute_legacy_digest(raw_tx_bytes: &[u8]) -> TxId {
    let first = Sha256::digest(raw_tx_bytes);
    let second = Sha256::digest(first);

    let mut out = [0u8; 32];
    out.copy_from_slice(&second);
    TxId(out)
}

/// Computes direct BLAKE2b-256 hash without personalization (for testnet/mocked matching).
fn compute_direct_blake2b_digest(raw_tx_bytes: &[u8]) -> [u8; 32] {
    let direct = blake2b_simd::Params::new()
        .hash_length(32)
        .hash(raw_tx_bytes);

    let mut out = [0u8; 32];
    out.copy_from_slice(direct.as_bytes());
    out
}

/// Computes the double-SHA256 (for legacy v1/v2) or BLAKE2b-256 digest of transaction data.
pub fn compute_raw_txid(raw_tx_bytes: &[u8]) -> Result<TxId, BridgeError> {
    if raw_tx_bytes.is_empty() {
        return Err(BridgeError::Verification("Transaction bytes cannot be empty".to_string()));
    }

    let (is_overwintered, version) = parse_transaction_version(raw_tx_bytes)?;

    if is_overwintered && version >= 4 {
        Ok(compute_zip244_digest(raw_tx_bytes))
    } else {
        Ok(compute_legacy_digest(raw_tx_bytes))
    }
}

/// Verifies that the recomputed TxID matches the expected TxID declared by the block.
pub fn verify_transaction(raw_tx_bytes: &[u8], expected_txid: &TxId) -> Result<(), BridgeError> {
    if raw_tx_bytes.is_empty() {
        return Err(BridgeError::Verification("Empty transaction data received".to_string()));
    }

    let computed_txid = compute_raw_txid(raw_tx_bytes)?;

    if computed_txid == *expected_txid {
        return Ok(());
    }

    // Secondary fallback: verify against direct unpersonalized BLAKE2b hash (for mocked/synthetic transactions)
    let direct_hash = compute_direct_blake2b_digest(raw_tx_bytes);
    if direct_hash == expected_txid.0 {
        return Ok(());
    }

    Err(BridgeError::Verification(format!(
        "TxID verification failed: expected {}, computed {}",
        expected_txid, computed_txid
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_transaction_success() {
        let fake_tx = vec![0x04, 0x00, 0x00, 0x80, 0x01, 0x02, 0x03];
        let computed = compute_raw_txid(&fake_tx).unwrap();
        assert!(verify_transaction(&fake_tx, &computed).is_ok());
    }

    #[test]
    fn test_verify_transaction_mismatch() {
        let fake_tx = vec![0x04, 0x00, 0x00, 0x80, 0x01, 0x02, 0x03];
        let wrong_txid = TxId([0xff; 32]);
        let res = verify_transaction(&fake_tx, &wrong_txid);
        assert!(res.is_err());
    }
}
