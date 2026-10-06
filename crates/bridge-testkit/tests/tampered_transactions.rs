use bridge_core::{BlockHeight, BridgeConfig, Network};
use bridge_engine::worker::AcquisitionWorker;
use bridge_proto::{CompactBlock, CompactTx, RawTransaction};
use bridge_storage::{SqliteStorage, StorageBackend};
use bridge_testkit::{start_mock_upstream, MockUpstreamServer};
use std::sync::Arc;
use tokio::sync::watch;

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
