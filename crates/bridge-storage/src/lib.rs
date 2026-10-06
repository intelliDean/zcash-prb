pub mod migrations;
pub mod sqlite;
pub mod traits;

pub use sqlite::SqliteStorage;
pub use traits::{
    StorageBackend, TransparentOutputRecord, TransparentSpendRecord, VerifiedIntervalBatch,
};

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{BlockHash, BlockHeight, Network, TransparentAddress, TxId};
    use bridge_proto::{CompactBlock, RawTransaction};

    #[tokio::test]
    async fn test_sqlite_in_memory_initialization_and_coverage() {
        let storage = SqliteStorage::in_memory().expect("in-memory db failed");

        // Check initial coverage
        let meta = storage.get_coverage_metadata().await.expect("query failed");
        assert!(meta.is_none());

        storage
            .init_coverage(Network::Mainnet, BlockHeight(3_000_000))
            .await
            .expect("init coverage failed");

        let meta = storage
            .get_coverage_metadata()
            .await
            .expect("query failed")
            .unwrap();
        assert_eq!(meta.network, Network::Mainnet);
        assert_eq!(meta.coverage_start_height, BlockHeight(3_000_000));
        assert_eq!(meta.committed_height, BlockHeight(2_999_999));
    }

    #[tokio::test]
    async fn test_commit_interval_and_query() {
        let storage = SqliteStorage::in_memory().expect("in-memory db failed");
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let block = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0xaa; 80],
            vtx: vec![],
            chain_metadata: None,
        };

        let raw_tx = RawTransaction {
            data: vec![0xde, 0xad, 0xbe, 0xef],
            height: 100,
        };

        let tx_hash = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashTxHash_TEMP")
            .hash(&raw_tx.data);
        let mut tx_arr = [0u8; 32];
        tx_arr.copy_from_slice(tx_hash.as_bytes());

        let out = TransparentOutputRecord {
            txid: TxId(tx_arr),
            vout: 0,
            address: TransparentAddress::new("t1TestAddress"),
            value_zat: 50_000_000,
            script_pubkey: vec![0x76, 0xa9],
            height: BlockHeight(100),
        };

        let batch = VerifiedIntervalBatch {
            blocks: vec![block],
            transactions: vec![raw_tx],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![out],
            transparent_spends: vec![],
            end_height: BlockHeight(100),
            end_block_hash: BlockHash([1u8; 32]),
        };

        storage.commit_verified_interval(batch).await.unwrap();

        // Verify committed block
        let (latest_h, latest_hash) = storage.get_latest_block().await.unwrap().unwrap();
        assert_eq!(latest_h, BlockHeight(100));
        assert_eq!(latest_hash, BlockHash([1u8; 32]));

        // Query UTXO
        let utxos = storage
            .get_address_utxos(&TransparentAddress::new("t1TestAddress"))
            .await
            .unwrap();
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].value_zat, 50_000_000);
        assert_eq!(utxos[0].address, "t1TestAddress");
    }

    #[tokio::test]
    async fn test_reorg_handling() {
        let storage = SqliteStorage::in_memory().expect("in-memory db failed");
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let b1 = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0xaa; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let b2 = CompactBlock {
            height: 101,
            hash: vec![2u8; 32],
            prev_hash: vec![1u8; 32],
            time: 1700000075,
            header: vec![0xbb; 80],
            vtx: vec![],
            chain_metadata: None,
        };

        let batch = VerifiedIntervalBatch {
            blocks: vec![b1, b2],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(101),
            end_block_hash: BlockHash([2u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        // Roll back block 101
        storage.handle_reorg(BlockHeight(101)).await.unwrap();

        let (latest_h, latest_hash) = storage.get_latest_block().await.unwrap().unwrap();
        assert_eq!(latest_h, BlockHeight(100));
        assert_eq!(latest_hash, BlockHash([1u8; 32]));
    }
}
