use bridge_core::{BlockHeight, BridgeConfig, Network};
use bridge_engine::worker::AcquisitionWorker;
use bridge_proto::compact_tx_streamer_client::CompactTxStreamerClient;
use bridge_proto::compact_tx_streamer_server::CompactTxStreamerServer;
use bridge_proto::{CompactBlock, CompactTx, TxFilter};
use bridge_server::BridgeGrpcService;
use bridge_storage::{SqliteStorage, StorageBackend};
use bridge_testkit::{create_test_tx, start_mock_upstream, MockUpstreamServer};
use std::sync::Arc;
use tokio::sync::watch;
use tonic::transport::Channel;

#[tokio::test]
async fn test_privacy_trace_proves_zero_selected_upstream_leakage() {
    // 1. Setup mock upstream server with 2 blocks and 2 transactions
    let mock = MockUpstreamServer::new();

    let tx1_data = vec![0x04, 0x00, 0x00, 0x80, 0x11, 0x22, 0x33];
    let (raw_tx1, tx1_hash) = create_test_tx(tx1_data.clone(), 100);

    let tx2_data = vec![0x04, 0x00, 0x00, 0x80, 0x44, 0x55, 0x66];
    let (raw_tx2, tx2_hash) = create_test_tx(tx2_data.clone(), 101);

    mock.add_transaction(&tx1_hash, raw_tx1);
    mock.add_transaction(&tx2_hash, raw_tx2);

    let b1 = CompactBlock {
        height: 100,
        hash: vec![1u8; 32],
        prev_hash: vec![0u8; 32],
        time: 1700000000,
        header: vec![0; 80],
        vtx: vec![CompactTx {
            index: 0,
            txid: tx1_hash.to_vec(),
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
            txid: tx2_hash.to_vec(),
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
    storage.init_coverage(Network::Mainnet, BlockHeight(100)).await.unwrap();

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

    tokio::spawn(async move {
        worker.run().await;
    });

    // Wait until storage reaches height 101
    for _ in 0..50 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Ok(Some((h, _))) = storage.get_latest_block().await
            && h == BlockHeight(101)
        {
            break;
        }
    }

    let _ = shutdown_tx.send(true);

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
    let channel =
        Channel::from_shared(format!("http://{}", local_addr)).unwrap().connect().await.unwrap();
    let mut client = CompactTxStreamerClient::new(channel);

    // Client A queries tx1
    let res1 = client
        .get_transaction(TxFilter { block: None, index: 0, hash: tx1_hash.to_vec() })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(res1.data, tx1_data);

    // Client B queries tx2
    let res2 = client
        .get_transaction(TxFilter { block: None, index: 0, hash: tx2_hash.to_vec() })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(res2.data, tx2_data);

    // 5. PRIVACY PROOF: Zero additional calls to upstream during client queries
    let final_calls = mock.recorded_calls();
    assert_eq!(
        final_calls.len(),
        calls_after_acquisition,
        "Privacy leak detected! Upstream received calls during client queries: {:?}",
        &final_calls[calls_after_acquisition..]
    );

    for call in &final_calls[calls_after_acquisition..] {
        assert!(!call.contains(&hex::encode(tx1_hash)));
        assert!(!call.contains(&hex::encode(tx2_hash)));
    }
}
