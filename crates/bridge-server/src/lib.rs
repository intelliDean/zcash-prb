pub mod handlers;
pub mod service;

pub use service::BridgeGrpcService;

use bridge_proto::compact_tx_streamer_server::CompactTxStreamerServer;
use std::net::SocketAddr;
use tokio::sync::watch;
use tonic::transport::Server;
use tracing::info;

pub async fn run_server(
    addr: SocketAddr,
    service: BridgeGrpcService,
    mut shutdown_rx: watch::Receiver<bool>,
) -> Result<(), tonic::transport::Error> {
    info!("Starting local CompactTxStreamer gRPC server on {}", addr);

    Server::builder()
        .add_service(CompactTxStreamerServer::new(service))
        .serve_with_shutdown(addr, async move {
            while !*shutdown_rx.borrow() {
                if shutdown_rx.changed().await.is_err() {
                    break;
                }
            }
            info!("gRPC server received shutdown signal.");
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{BlockHeight, Network};
    use bridge_proto::compact_tx_streamer_server::CompactTxStreamer;
    use bridge_proto::{BlockId, CompactBlock, CompactTx, Empty, RawTransaction};
    use bridge_storage::{SqliteStorage, StorageBackend, VerifiedIntervalBatch};
    use std::sync::Arc;
    use tokio_stream::StreamExt;

    #[tokio::test]
    async fn test_cache_protection_missing_tx_returns_data_loss() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let txid_bytes = [0x55u8; 32];

        let b1 = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![CompactTx {
                index: 0,
                txid: txid_bytes.to_vec(),
                fee: 1000,
                spends: vec![],
                outputs: vec![],
                actions: vec![],
                ironwood_actions: vec![],
                vin: vec![],
                vout: vec![],
            }],
            chain_metadata: None,
        };

        let batch = VerifiedIntervalBatch {
            blocks: vec![b1],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(100),
            end_block_hash: bridge_core::BlockHash([1u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        let service = BridgeGrpcService::new(storage.clone(), "mainnet");

        let res = service
            .get_block(tonic::Request::new(BlockId {
                height: 100,
                hash: vec![],
            }))
            .await;

        assert!(res.is_err());
        assert_eq!(res.unwrap_err().code(), tonic::Code::DataLoss);
    }

    #[tokio::test]
    async fn test_cache_protection_corrupted_tx_payload_returns_data_loss() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        // Valid consensus tx
        let valid_tx_data = hex::decode("030000807082c4030002e7719811893e0000095200ac6551ac636565b2835a0805750200025151481cdd86b3cc431800").unwrap();
        let valid_txid = bridge_verifier::compute_raw_txid(&valid_tx_data).unwrap();

        // Commit valid block and transaction first
        let b1 = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![CompactTx {
                index: 0,
                txid: valid_txid.0.to_vec(),
                fee: 1000,
                spends: vec![],
                outputs: vec![],
                actions: vec![],
                ironwood_actions: vec![],
                vin: vec![],
                vout: vec![],
            }],
            chain_metadata: None,
        };
        let raw_tx = RawTransaction {
            data: valid_tx_data,
            height: 100,
        };
        let batch = VerifiedIntervalBatch {
            blocks: vec![b1],
            transactions: vec![raw_tx],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(100),
            end_block_hash: bridge_core::BlockHash([1u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        let service = BridgeGrpcService::new(storage.clone(), "mainnet");

        // 1. Verify that valid tx can initially be retrieved
        let valid_filter = tonic::Request::new(bridge_proto::TxFilter {
            block: None,
            index: 0,
            hash: valid_txid.0.to_vec(),
        });
        let initial_res = service.get_transaction(valid_filter).await;
        assert!(initial_res.is_ok());

        // 2. Tamper with the raw transaction bytes in storage (simulating disk bit rot / corruption)
        storage
            .tamper_transaction_raw(&valid_txid, vec![0xde, 0xad, 0xbe, 0xef])
            .unwrap();

        // 3. Querying the corrupted transaction directly returns DataLoss
        let corrupted_filter = tonic::Request::new(bridge_proto::TxFilter {
            block: None,
            index: 0,
            hash: valid_txid.0.to_vec(),
        });
        let corrupted_tx_res = service.get_transaction(corrupted_filter).await;
        assert!(corrupted_tx_res.is_err());
        assert_eq!(corrupted_tx_res.unwrap_err().code(), tonic::Code::DataLoss);

        // 4. Querying the block containing the corrupted transaction also returns DataLoss
        let block_res = service
            .get_block(tonic::Request::new(BlockId {
                height: 100,
                hash: vec![],
            }))
            .await;
        assert!(block_res.is_err());
        assert_eq!(block_res.unwrap_err().code(), tonic::Code::DataLoss);
    }

    #[tokio::test]
    async fn test_mempool_stream_completes_on_tip_advance() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let service = BridgeGrpcService::new(storage.clone(), "mainnet");
        let stream_res = service
            .get_mempool_stream(tonic::Request::new(Empty {}))
            .await
            .unwrap();
        let mut stream = stream_res.into_inner();

        // Advance storage tip
        let b1 = CompactBlock {
            height: 101,
            hash: vec![2u8; 32],
            prev_hash: vec![1u8; 32],
            time: 1700000075,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let batch = VerifiedIntervalBatch {
            blocks: vec![b1],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(101),
            end_block_hash: bridge_core::BlockHash([2u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        let next_item = tokio::time::timeout(std::time::Duration::from_secs(3), stream.next())
            .await
            .expect("Stream did not complete in time");
        assert!(
            next_item.is_none(),
            "Stream should close cleanly on tip advance"
        );
    }

    #[tokio::test]
    async fn test_get_subtree_roots_limits_and_pagination() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let root1 = bridge_proto::SubtreeRoot {
            root_hash: vec![1u8; 32],
            completing_block_hash: vec![10u8; 32],
            completing_block_height: 100,
        };
        let root2 = bridge_proto::SubtreeRoot {
            root_hash: vec![2u8; 32],
            completing_block_hash: vec![11u8; 32],
            completing_block_height: 101,
        };
        let root3 = bridge_proto::SubtreeRoot {
            root_hash: vec![3u8; 32],
            completing_block_hash: vec![12u8; 32],
            completing_block_height: 102,
        };

        let batch = VerifiedIntervalBatch {
            blocks: vec![],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![(0, root1), (0, root2), (0, root3)],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(102),
            end_block_hash: bridge_core::BlockHash([12u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        let service = BridgeGrpcService::new(storage.clone(), "mainnet");

        // 1. max_entries = 0 returns ALL roots
        let req_zero = tonic::Request::new(bridge_proto::GetSubtreeRootsArg {
            shielded_protocol: 0,
            start_index: 0,
            max_entries: 0,
        });
        let mut stream_zero = service
            .get_subtree_roots(req_zero)
            .await
            .unwrap()
            .into_inner();
        let mut results_zero = Vec::new();
        while let Some(item) = stream_zero.next().await {
            results_zero.push(item.unwrap());
        }
        assert_eq!(results_zero.len(), 3);
        assert_eq!(results_zero[0].root_hash, vec![1u8; 32]);
        assert_eq!(results_zero[2].root_hash, vec![3u8; 32]);

        // 2. positive limit (max_entries = 2) truncates output
        let req_limited = tonic::Request::new(bridge_proto::GetSubtreeRootsArg {
            shielded_protocol: 0,
            start_index: 0,
            max_entries: 2,
        });
        let mut stream_limited = service
            .get_subtree_roots(req_limited)
            .await
            .unwrap()
            .into_inner();
        let mut results_limited = Vec::new();
        while let Some(item) = stream_limited.next().await {
            results_limited.push(item.unwrap());
        }
        assert_eq!(results_limited.len(), 2);
        assert_eq!(results_limited[0].root_hash, vec![1u8; 32]);
        assert_eq!(results_limited[1].root_hash, vec![2u8; 32]);

        // 3. start_index offset with max_entries = 0
        let req_offset = tonic::Request::new(bridge_proto::GetSubtreeRootsArg {
            shielded_protocol: 0,
            start_index: 1,
            max_entries: 0,
        });
        let mut stream_offset = service
            .get_subtree_roots(req_offset)
            .await
            .unwrap()
            .into_inner();
        let mut results_offset = Vec::new();
        while let Some(item) = stream_offset.next().await {
            results_offset.push(item.unwrap());
        }
        assert_eq!(results_offset.len(), 2);
        assert_eq!(results_offset[0].root_hash, vec![2u8; 32]);
        assert_eq!(results_offset[1].root_hash, vec![3u8; 32]);
    }

    #[tokio::test]
    async fn test_mempool_stream_completes_on_same_height_replacement_and_reconnect() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let b1 = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let batch = VerifiedIntervalBatch {
            blocks: vec![b1],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(100),
            end_block_hash: bridge_core::BlockHash([1u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        let service = BridgeGrpcService::new(storage.clone(), "mainnet");

        // 1. Open stream and test client cancellation
        {
            let stream_res = service
                .get_mempool_stream(tonic::Request::new(Empty {}))
                .await
                .unwrap();
            let rx = stream_res.into_inner();
            drop(rx); // Client disconnects/cancels
        }

        // 2. Client reconnects cleanly
        let stream_res = service
            .get_mempool_stream(tonic::Request::new(Empty {}))
            .await
            .unwrap();
        let mut stream = stream_res.into_inner();

        // 3. Trigger same-height replacement (hash changes from 1u8 to 9u8 at height 100)
        let b1_replacement = CompactBlock {
            height: 100,
            hash: vec![9u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000001,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let batch_replacement = VerifiedIntervalBatch {
            blocks: vec![b1_replacement],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(100),
            end_block_hash: bridge_core::BlockHash([9u8; 32]),
        };
        storage
            .commit_verified_interval(batch_replacement)
            .await
            .unwrap();

        let next_item = tokio::time::timeout(std::time::Duration::from_secs(3), stream.next())
            .await
            .expect("Stream did not complete in time");
        assert!(
            next_item.is_none(),
            "Stream should complete cleanly on same-height block hash replacement"
        );
    }

    #[tokio::test]
    async fn test_mempool_stream_completes_on_rollback() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let b1 = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let b2 = CompactBlock {
            height: 101,
            hash: vec![2u8; 32],
            prev_hash: vec![1u8; 32],
            time: 1700000050,
            header: vec![0; 80],
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
            end_block_hash: bridge_core::BlockHash([2u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        let service = BridgeGrpcService::new(storage.clone(), "mainnet");
        let stream_res = service
            .get_mempool_stream(tonic::Request::new(Empty {}))
            .await
            .unwrap();
        let mut stream = stream_res.into_inner();

        // Roll back block 101 to height 100
        storage.handle_reorg(BlockHeight(101)).await.unwrap();

        let next_item = tokio::time::timeout(std::time::Duration::from_secs(3), stream.next())
            .await
            .expect("Stream did not complete in time");
        assert!(
            next_item.is_none(),
            "Stream should complete cleanly on rollback"
        );
    }

    #[tokio::test]
    async fn test_reorg_consistency_purges_tree_states_and_prevents_mixed_reads() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let b1 = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let b2 = CompactBlock {
            height: 101,
            hash: vec![2u8; 32],
            prev_hash: vec![1u8; 32],
            time: 1700000050,
            header: vec![0; 80],
            vtx: vec![],
            chain_metadata: None,
        };
        let ts1 = bridge_proto::TreeState {
            network: "mainnet".to_string(),
            height: 100,
            hash: hex::encode([1u8; 32]),
            time: 1700000000,
            sapling_tree: "sapling1".to_string(),
            orchard_tree: "orchard1".to_string(),
            ironwood_tree: "ironwood1".to_string(),
        };
        let ts2 = bridge_proto::TreeState {
            network: "mainnet".to_string(),
            height: 101,
            hash: hex::encode([2u8; 32]),
            time: 1700000050,
            sapling_tree: "sapling2".to_string(),
            orchard_tree: "orchard2".to_string(),
            ironwood_tree: "ironwood2".to_string(),
        };

        let batch = VerifiedIntervalBatch {
            blocks: vec![b1, b2],
            transactions: vec![],
            tree_states: vec![ts1, ts2],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![],
            end_height: BlockHeight(101),
            end_block_hash: bridge_core::BlockHash([2u8; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        let service = BridgeGrpcService::new(storage.clone(), "mainnet");

        // Verify initial state
        assert!(
            service
                .get_block(tonic::Request::new(BlockId {
                    height: 101,
                    hash: vec![]
                }))
                .await
                .is_ok()
        );
        assert!(
            service
                .get_tree_state(tonic::Request::new(BlockId {
                    height: 101,
                    hash: vec![]
                }))
                .await
                .is_ok()
        );

        // Roll back block 101
        storage.handle_reorg(BlockHeight(101)).await.unwrap();

        // Assert rolled back block and tree state are completely gone (not mixed)
        let block_101_res = service
            .get_block(tonic::Request::new(BlockId {
                height: 101,
                hash: vec![],
            }))
            .await;
        assert!(block_101_res.is_err());
        assert_eq!(block_101_res.unwrap_err().code(), tonic::Code::NotFound);

        let ts_101_res = service
            .get_tree_state(tonic::Request::new(BlockId {
                height: 101,
                hash: vec![],
            }))
            .await;
        assert!(ts_101_res.is_err());
        assert_eq!(ts_101_res.unwrap_err().code(), tonic::Code::NotFound);

        // Assert surviving block 100 and tree state 100 remain intact
        assert!(
            service
                .get_block(tonic::Request::new(BlockId {
                    height: 100,
                    hash: vec![]
                }))
                .await
                .is_ok()
        );
        assert!(
            service
                .get_tree_state(tonic::Request::new(BlockId {
                    height: 100,
                    hash: vec![]
                }))
                .await
                .is_ok()
        );
    }
}
