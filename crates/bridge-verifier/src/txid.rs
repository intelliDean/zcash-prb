use bridge_core::{BridgeError, TxId};
use zebra_chain::serialization::ZcashDeserialize;
use zebra_chain::transaction::Transaction;

/// Computes the consensus transaction ID (TxId) by deserializing the transaction
/// using Zcash consensus rules (ZIP 244 for v5, SHA256d for v1-v4).
pub fn compute_raw_txid(raw_tx_bytes: &[u8]) -> Result<TxId, BridgeError> {
    if raw_tx_bytes.is_empty() {
        return Err(BridgeError::Verification(
            "TxID verification failed: transaction bytes cannot be empty".to_string(),
        ));
    }

    let tx = Transaction::zcash_deserialize(raw_tx_bytes).map_err(|e| {
        BridgeError::Verification(format!("TxID verification failed: failed to parse consensus transaction: {e}"))
    })?;

    Ok(TxId(tx.hash().0))
}

/// Verifies that the recomputed consensus TxID matches the expected TxID declared by the block.
pub fn verify_transaction(raw_tx_bytes: &[u8], expected_txid: &TxId) -> Result<(), BridgeError> {
    if raw_tx_bytes.is_empty() {
        return Err(BridgeError::Verification(
            "Empty transaction data received".to_string(),
        ));
    }

    let computed_txid = compute_raw_txid(raw_tx_bytes)?;

    if computed_txid == *expected_txid {
        Ok(())
    } else {
        Err(BridgeError::Verification(format!(
            "TxID verification failed: expected {}, computed {}",
            expected_txid, computed_txid
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Official valid consensus test transaction hex
    const VALID_TX_HEX: &str = "030000807082c4030002e7719811893e0000095200ac6551ac636565b2835a0805750200025151481cdd86b3cc431800";

    #[test]
    fn test_verify_transaction_success() {
        let valid_tx = hex::decode(VALID_TX_HEX).unwrap();
        let computed = compute_raw_txid(&valid_tx).unwrap();
        assert!(verify_transaction(&valid_tx, &computed).is_ok());
    }

    #[test]
    fn test_verify_transaction_mismatch() {
        let valid_tx = hex::decode(VALID_TX_HEX).unwrap();
        let wrong_txid = TxId([0xff; 32]);
        let res = verify_transaction(&valid_tx, &wrong_txid);
        assert!(res.is_err());
    }

    #[test]
    fn test_verify_transaction_malformed() {
        let bad_tx = vec![0x04, 0x00, 0x00, 0x80];
        let wrong_txid = TxId([0x00; 32]);
        assert!(verify_transaction(&bad_tx, &wrong_txid).is_err());
    }
}
