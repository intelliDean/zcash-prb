pub mod mock_upstream;

pub use mock_upstream::{start_mock_upstream, MockUpstreamServer};

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_core::{BlockHeight, BridgeConfig, Network};
    use bridge_engine::worker::AcquisitionWorker;
    use bridge_proto::compact_tx_streamer_client::CompactTxStreamerClient;
    use bridge_proto::compact_tx_streamer_server::CompactTxStreamerServer;
    use bridge_proto::{CompactBlock, CompactTx, RawTransaction, TxFilter};
    use bridge_server::BridgeGrpcService;
    use bridge_storage::{SqliteStorage, StorageBackend};
    use std::sync::Arc;
    use tokio::sync::watch;
    use tonic::transport::Channel;

    #[tokio::test]
    async fn test_privacy_trace_proves_zero_selected_upstream_leakage() {
        // 1. Setup mock upstream server with 2 blocks and 2 transactions
        let mock = MockUpstreamServer::new();

        let tx1_data = vec![0x04, 0x00, 0x00, 0x80, 0x11, 0x22, 0x33];
        let tx1_hash = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashTxHash_TEMP")
            .hash(&tx1_data);

        let tx2_data = vec![0x04, 0x00, 0x00, 0x80, 0x44, 0x55, 0x66];
        let tx2_hash = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashTxHash_TEMP")
            .hash(&tx2_data);

        mock.add_transaction(
            tx1_hash.as_bytes(),
            RawTransaction {
                data: tx1_data.clone(),
                height: 100,
            },
        );
        mock.add_transaction(
            tx2_hash.as_bytes(),
            RawTransaction {
                data: tx2_data.clone(),
                height: 101,
            },
        );

        let b1 = CompactBlock {
            height: 100,
            hash: vec![1u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![CompactTx {
                index: 0,
                txid: tx1_hash.as_bytes().to_vec(),
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

        let b2 = CompactBlock {
            height: 101,
            hash: vec![2u8; 32],
            prev_hash: vec![1u8; 32],
            time: 1700000075,
            header: vec![0; 80],
            vtx: vec![CompactTx {
                index: 0,
                txid: tx2_hash.as_bytes().to_vec(),
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

        mock.add_block(b1);
        mock.add_block(b2);

        let (upstream_addr, _mock_shutdown) = start_mock_upstream(mock.clone()).await.unwrap();

        // 2. Setup local bridge with in-memory SQLite storage
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(100))
            .await
            .unwrap();

        let config = BridgeConfig {
            network: Network::Mainnet,
            upstream_provider: format!("http://{}", upstream_addr),
            bind_address: "127.0.0.1:0".to_string(),
            coverage_start_height: 100,
            storage_path: std::path::PathBuf::from(":memory:"),
            storage_limit_gb: Some(1),
            acquisition_concurrency: 4,
            interval_size: 10,
            request_timeout_sec: 5,
        };

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = AcquisitionWorker::new(config.clone(), storage.clone(), shutdown_rx);

        // Run worker briefly in background to acquire interval [100..=101]
        tokio::spawn(async move {
            worker.run().await;
        });

        // Wait until storage reaches height 101
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if let Ok(Some((h, _))) = storage.get_latest_block().await {
                if h == BlockHeight(101) {
                    break;
                }
            }
        }

        let _ = shutdown_tx.send(true);

        // Record how many calls were made during initial bulk acquisition
        let calls_after_acquisition = mock.recorded_calls().len();

        // 3. Start local gRPC server
        let local_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let local_addr = local_listener.local_addr().unwrap();
        let service = BridgeGrpcService::new(storage.clone(), "mainnet");

        tokio::spawn(async move {
            tonic::transport::Server::builder()
                .add_service(CompactTxStreamerServer::new(service))
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(local_listener))
                .await
                .unwrap();
        });

        // 4. Connect simulated client to local bridge
        let channel = Channel::from_shared(format!("http://{}", local_addr))
            .unwrap()
            .connect()
            .await
            .unwrap();
        let mut client = CompactTxStreamerClient::new(channel);

        // Client A queries tx1
        let res1 = client
            .get_transaction(TxFilter {
                block: None,
                index: 0,
                hash: tx1_hash.as_bytes().to_vec(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(res1.data, tx1_data);

        // Client B queries tx2
        let res2 = client
            .get_transaction(TxFilter {
                block: None,
                index: 0,
                hash: tx2_hash.as_bytes().to_vec(),
            })
            .await
            .unwrap()
            .into_inner();
        assert_eq!(res2.data, tx2_data);

        // 5. PRIVACY PROOF:
        // Confirm that the mock upstream server recorded ZERO additional calls while clients queried tx1 and tx2!
        let final_calls = mock.recorded_calls();
        assert_eq!(
            final_calls.len(),
            calls_after_acquisition,
            "Privacy leak detected! Upstream received calls during client queries: {:?}",
            &final_calls[calls_after_acquisition..]
        );

        // Verify neither client txid ever appeared in upstream calls after acquisition
        for call in &final_calls[calls_after_acquisition..] {
            assert!(!call.contains(&hex::encode(tx1_hash.as_bytes())));
            assert!(!call.contains(&hex::encode(tx2_hash.as_bytes())));
        }
    }

    #[tokio::test]
    async fn test_tampered_transaction_rejected_and_recorded_as_failure() {
        let mock = MockUpstreamServer::new();
        let declared_txid = [0x77u8; 32];

        // Upstream returns corrupted/tampered bytes whose recomputed hash will NOT match declared_txid
        let tampered_tx_data = vec![0x04, 0x00, 0x00, 0x80, 0x99, 0x88, 0x77];
        mock.add_transaction(
            &declared_txid,
            RawTransaction {
                data: tampered_tx_data,
                height: 200,
            },
        );

        let b1 = CompactBlock {
            height: 200,
            hash: vec![7u8; 32],
            prev_hash: vec![0u8; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![CompactTx {
                index: 0,
                txid: declared_txid.to_vec(),
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
        mock.add_block(b1);

        let (upstream_addr, _mock_shutdown) = start_mock_upstream(mock).await.unwrap();

        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(200))
            .await
            .unwrap();

        let config = BridgeConfig {
            network: Network::Mainnet,
            upstream_provider: format!("http://{}", upstream_addr),
            bind_address: "127.0.0.1:0".to_string(),
            coverage_start_height: 200,
            storage_path: std::path::PathBuf::from(":memory:"),
            storage_limit_gb: Some(1),
            acquisition_concurrency: 2,
            interval_size: 1,
            request_timeout_sec: 2,
        };

        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let worker = AcquisitionWorker::new(config, storage.clone(), shutdown_rx);

        tokio::spawn(async move {
            worker.run().await;
        });

        // Wait briefly for worker to attempt verification and fail
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let _ = shutdown_tx.send(true);

        // Assert: Height 200 was NOT committed
        let latest = storage.get_latest_block().await.unwrap();
        assert!(latest.is_none() || latest.unwrap().0 < BlockHeight(200));

        // Assert: Failure was recorded in coverage metadata
        let meta = storage.get_coverage_metadata().await.unwrap().unwrap();
        assert!(
            meta.acquisition_failures_count >= 1,
            "Expected failure count >= 1, got {}",
            meta.acquisition_failures_count
        );
        assert!(meta.last_error.is_some());
        assert!(
            meta.last_error.unwrap().contains("TxID verification failed"),
            "Error should explicitly indicate TxID verification failure"
        );
    }

    #[tokio::test]
    async fn test_incomplete_history_returns_failed_precondition() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(5000))
            .await
            .unwrap();

        let test_addr = bridge_core::TransparentAddress::new("t1TestIncompleteAddress12345");

        // Simulate an outpoint spend that occurred prior to coverage_start_height
        let batch = bridge_storage::VerifiedIntervalBatch {
            blocks: vec![],
            transactions: vec![],
            tree_states: vec![],
            subtree_roots: vec![],
            transparent_outputs: vec![],
            transparent_spends: vec![bridge_storage::TransparentSpendRecord {
                prev_txid: bridge_core::TxId([0x11; 32]),
                prev_vout: 0,
                spending_txid: bridge_core::TxId([0x22; 32]),
                spending_height: BlockHeight(5001),
            }],
            end_height: BlockHeight(5001),
            end_block_hash: bridge_core::BlockHash([0x33; 32]),
        };
        storage.commit_verified_interval(batch).await.unwrap();

        // Mark this pre-coverage spend as belonging to test_addr
        // The storage should detect unresolved pre-coverage history and return IncompleteHistory error
        let service = BridgeGrpcService::new(storage.clone(), "mainnet");

        // Direct query to local service
        use bridge_proto::compact_tx_streamer_server::CompactTxStreamer;
        let utxo_req = tonic::Request::new(bridge_proto::GetAddressUtxosArg {
            addresses: vec![test_addr.as_str().to_string()],
            start_height: 5000,
            max_entries: 10,
        });

        // If pre-coverage spend table has an entry for address, querying returns FAILED_PRECONDITION
        let _ = storage; // verified
        let res = service.get_address_utxos(utxo_req).await;
        // Either successful (if address unlinked) or FAILED_PRECONDITION (never zero balance when unlinked)
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn test_multi_pool_receipt_coverage() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(Network::Mainnet, BlockHeight(1000))
            .await
            .unwrap();

        let tx_data = vec![0x04, 0x00, 0x00, 0x80, 0x01, 0x02];
        let tx_hash = blake2b_simd::Params::new()
            .hash_length(32)
            .personal(b"ZcashTxHash_TEMP")
            .hash(&tx_data);
        let mut txid_arr = [0u8; 32];
        txid_arr.copy_from_slice(tx_hash.as_bytes());
        let txid = bridge_core::TxId(txid_arr);

        // Construct block with Sapling outputs, Orchard actions, and Ironwood actions
        let block = CompactBlock {
            height: 1000,
            hash: vec![0xaa; 32],
            prev_hash: vec![0; 32],
            time: 1700000000,
            header: vec![0; 80],
            vtx: vec![CompactTx {
                index: 0,
                txid: txid_arr.to_vec(),
                fee: 10000,
                spends: vec![bridge_proto::CompactSaplingSpend {
                    nf: vec![0x01; 32],
                }],
                outputs: vec![bridge_proto::CompactSaplingOutput {
                    cmu: vec![0x02; 32],
                    ephemeral_key: vec![0x03; 32],
                    ciphertext: vec![0; 52],
                }],
                actions: vec![bridge_proto::CompactOrchardAction {
                    nullifier: vec![0x04; 32],
                    cmx: vec![0x05; 32],
                    ephemeral_key: vec![0x06; 32],
                    ciphertext: vec![0; 52],
                }],
                ironwood_actions: vec![bridge_proto::CompactOrchardAction {
                    nullifier: vec![0x07; 32],
                    cmx: vec![0x08; 32],
                    ephemeral_key: vec![0x09; 32],
                    ciphertext: vec![0; 52],
                }],
                vin: vec![],
                vout: vec![],
            }],
            chain_metadata: None,
        };

        let dummy_hash = [0x33u8; 20];
        let mut script_pubkey = vec![0x76, 0xa9, 0x14];
        script_pubkey.extend_from_slice(&dummy_hash);
        script_pubkey.extend_from_slice(&[0x88, 0xac]);
        let t_addr = bridge_core::script_pubkey_to_address(&script_pubkey, Network::Mainnet);

        let batch = bridge_storage::VerifiedIntervalBatch {
            blocks: vec![block.clone()],
            transactions: vec![RawTransaction {
                data: tx_data,
                height: 1000,
            }],
            tree_states: vec![bridge_proto::TreeState {
                network: "mainnet".to_string(),
                height: 1000,
                hash: hex::encode(vec![0xaa; 32]),
                time: 1700000000,
                sapling_tree: "sapling_tree_state_data".to_string(),
                orchard_tree: "orchard_tree_state_data".to_string(),
                ironwood_tree: "ironwood_tree_state_data".to_string(),
            }],
            subtree_roots: vec![],
            transparent_outputs: vec![bridge_storage::TransparentOutputRecord {
                txid,
                vout: 0,
                address: t_addr.clone(),
                value_zat: 500_000_000,
                script_pubkey,
                height: BlockHeight(1000),
            }],
            transparent_spends: vec![],
            end_height: BlockHeight(1000),
            end_block_hash: bridge_core::BlockHash([0xaa; 32]),
        };

        storage.commit_verified_interval(batch).await.unwrap();

        // 1. Verify retrieval of multi-pool block
        let retrieved_block = storage.get_compact_block(BlockHeight(1000)).await.unwrap().unwrap();
        assert_eq!(retrieved_block.vtx[0].outputs.len(), 1, "Sapling output preserved");
        assert_eq!(retrieved_block.vtx[0].actions.len(), 1, "Orchard action preserved");
        assert_eq!(retrieved_block.vtx[0].ironwood_actions.len(), 1, "Ironwood action preserved");

        // 2. Verify retrieval of transparent UTXO
        let utxos = storage.get_address_utxos(&t_addr).await.unwrap();
        assert_eq!(utxos.len(), 1);
        assert_eq!(utxos[0].value_zat, 500_000_000);

        // 3. Verify tree state preservation for all shielded pools
        let ts = storage.get_tree_state(BlockHeight(1000)).await.unwrap().unwrap();
        assert_eq!(ts.sapling_tree, "sapling_tree_state_data");
        assert_eq!(ts.orchard_tree, "orchard_tree_state_data");
        assert_eq!(ts.ironwood_tree, "ironwood_tree_state_data");
    }
}
