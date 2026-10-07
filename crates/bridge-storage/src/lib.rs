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

        let tx_data = hex::decode("030000807082c4030002e7719811893e0000095200ac6551ac636565b2835a0805750200025151481cdd86b3cc431800").unwrap();
        let raw_tx = RawTransaction {
            data: tx_data,
            height: 100,
        };

        let tx_id = bridge_verifier::compute_raw_txid(&raw_tx.data).unwrap();
        let tx_arr = tx_id.0;

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

    #[tokio::test]
    async fn test_subtree_roots_multibatch_and_reorg() {
        let storage = SqliteStorage::in_memory().expect("in-memory db failed");
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let root1 = bridge_proto::SubtreeRoot {
            root_hash: vec![0x11; 32],
            completing_block_hash: vec![0xaa; 32],
            completing_block_height: 100,
        };
        let b1 = CompactBlock {
            height: 100,
            hash: vec![0xaa; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let batch1 = VerifiedIntervalBatch {
            blocks: vec![b1],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![(0, root1)],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(100),
            end_block_hash: BlockHash([0xaa; 32]),
        };
        storage.commit_verified_interval(batch1).await.unwrap();

        let root2 = bridge_proto::SubtreeRoot {
            root_hash: vec![0x22; 32],
            completing_block_hash: vec![0xbb; 32],
            completing_block_height: 101,
        };
        let root_orchard = bridge_proto::SubtreeRoot {
            root_hash: vec![0x33; 32],
            completing_block_hash: vec![0xbb; 32],
            completing_block_height: 101,
        };
        let b2 = CompactBlock {
            height: 101,
            hash: vec![0xbb; 32],
            prev_hash: vec![0xaa; 32],
            time: 1700000075,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let batch2 = VerifiedIntervalBatch {
            blocks: vec![b2],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![(0, root2), (1, root_orchard)],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(101),
            end_block_hash: BlockHash([0xbb; 32]),
        };
        storage.commit_verified_interval(batch2).await.unwrap();

        // Verify both pools preserved independently across batches
        let sapling_roots = storage.get_subtree_roots(0, 0, 10).await.unwrap();
        assert_eq!(sapling_roots.len(), 2);
        assert_eq!(sapling_roots[0].root_hash, vec![0x11; 32]);
        assert_eq!(sapling_roots[0].completing_block_hash, vec![0xaa; 32]);
        assert_eq!(sapling_roots[1].root_hash, vec![0x22; 32]);
        assert_eq!(sapling_roots[1].completing_block_hash, vec![0xbb; 32]);

        let orchard_roots = storage.get_subtree_roots(1, 0, 10).await.unwrap();
        assert_eq!(orchard_roots.len(), 1);
        assert_eq!(orchard_roots[0].root_hash, vec![0x33; 32]);
        assert_eq!(orchard_roots[0].completing_block_hash, vec![0xbb; 32]);

        // Reorg back block 101
        storage.handle_reorg(BlockHeight(101)).await.unwrap();
        let roots_after = storage.get_subtree_roots(0, 0, 10).await.unwrap();
        assert_eq!(roots_after.len(), 1);
        assert_eq!(roots_after[0].root_hash, vec![0x11; 32]);

        let orchard_after = storage.get_subtree_roots(1, 0, 10).await.unwrap();
        assert_eq!(orchard_after.len(), 0);
    }
}
