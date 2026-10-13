use bridge_core::{BlockHeight, BridgeConfig, Network};
use bridge_engine::worker::AcquisitionWorker;
use bridge_proto::compact_tx_streamer_client::CompactTxStreamerClient;
use bridge_proto::compact_tx_streamer_server::CompactTxStreamerServer;
use bridge_proto::{
    BlockId, BlockRange, ChainSpec, CompactBlock, CompactTx, Empty, GetSubtreeRootsArg,
    RawTransaction, TxFilter,
};
use bridge_server::BridgeGrpcService;
use bridge_storage::{SqliteStorage, StorageBackend};
use bridge_testkit::{MockUpstreamServer, create_test_tx, start_mock_upstream};
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;
use tonic::Code;
use tonic::transport::Channel;

#[tokio::test]
async fn test_wallet_probe_online_and_offline_rescan_parity() {
    let _ = std::fs::create_dir_all("artifacts");
    let db_path = PathBuf::from("target/wallet_probe_parity.db");
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(format!("{}-wal", db_path.display()));
    let _ = std::fs::remove_file(format!("{}-shm", db_path.display()));

    // 1. Setup mock upstream server with 5 blocks [2500000..=2500004]
    let mock = MockUpstreamServer::new();

    let tx1_data = hex::decode("030000807082c4030002e7719811893e0000095200ac6551ac636565b2835a0805750200025151481cdd86b3cc431800").unwrap();
    let (raw_tx1, tx1_hash) = create_test_tx(tx1_data.clone(), 2500000);

    let tx2_data = hex::decode("030000807082c4030187daa731f570a7a4060af0ce700d31bca7e74b3e3ba3d0e8a6392a062b8e86d9d7d00b21026553062e06b1013011ff08f083050009636a52635163006aac9abcef2a9908731900").unwrap();
    let (raw_tx2, tx2_hash) = create_test_tx(tx2_data.clone(), 2500001);

    mock.add_transaction(&tx1_hash, raw_tx1);
    mock.add_transaction(&tx2_hash, raw_tx2);

    let mut prev = vec![0u8; 32];
    for h in 2500000..=2500004 {
        let vtx = if h == 2500000 {
            vec![CompactTx {
                index: 0,
                txid: tx1_hash.to_vec(),
                fee: 1000,
                spends: vec![],
                outputs: vec![],
                actions: vec![],
                ironwood_actions: vec![],
                vin: vec![],
                vout: vec![],
            }]
        } else if h == 2500001 {
            vec![CompactTx {
                index: 0,
                txid: tx2_hash.to_vec(),
                fee: 1000,
                spends: vec![],
                outputs: vec![],
                actions: vec![],
                ironwood_actions: vec![],
                vin: vec![],
                vout: vec![],
            }]
        } else {
            vec![]
        };

        let curr_hash = vec![(h % 250) as u8; 32];
        let block = CompactBlock {
            height: h,
            hash: curr_hash.clone(),
            prev_hash: prev.clone(),
            time: 1700000000 + (h as u32),
            header: vec![0; 80],
            vtx,
            chain_metadata: None,
        };
        prev = curr_hash;
        mock.add_block(block);
    }

    let (upstream_addr, upstream_shutdown) = start_mock_upstream(mock.clone()).await.unwrap();

    // 2. Start bridge daemon with durable SQLite storage
    let storage = Arc::new(SqliteStorage::open(&db_path).unwrap());
    let config = BridgeConfig {
        network: Network::Mainnet,
        upstream_provider: format!("http://{}", upstream_addr),
        bind_address: "127.0.0.1:0".to_string(),
        coverage_start_height: 2500000,
        storage_path: db_path.clone(),
        storage_limit_gb: Some(1),
        acquisition_concurrency: 4,
        interval_size: 10,
        request_timeout_sec: 5,
    };

    let (worker_shutdown_tx, worker_shutdown_rx) = watch::channel(false);
    let worker = AcquisitionWorker::new(config.clone(), storage.clone(), worker_shutdown_rx);
    let worker_handle = tokio::spawn(async move {
        worker.run().await;
    });

    // Wait until storage reaches height 2500004
    for _ in 0..60 {
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        if let Ok(Some((h, _))) = storage.get_latest_block().await
            && h == BlockHeight(2500004)
        {
            break;
        }
    }

    let _ = worker_shutdown_tx.send(true);
    let _ = worker_handle.await;

    // 3. Start local gRPC server (Online Phase)
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let local_addr = listener.local_addr().unwrap();
    let service = BridgeGrpcService::new(storage.clone(), "mainnet");

    let server_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(CompactTxStreamerServer::new(service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
            .await
            .unwrap();
    });

    // 4. Run probe against online bridge & record trace
    let channel = Channel::from_shared(format!("http://{}", local_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();

    let workspace_artifacts = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("artifacts");
    let _ = std::fs::create_dir_all(&workspace_artifacts);
    let online_probe_trace = workspace_artifacts.join("wallet_probe_trace.jsonl");
    let probe_pass = execute_and_record_probe(
        channel.clone(),
        2500000,
        2500004,
        vec![tx1_hash.to_vec(), tx2_hash.to_vec()],
        online_probe_trace.to_str().unwrap(),
    )
    .await;
    assert!(probe_pass, "Online probe failed!");

    // 5. SHUT DOWN UPSTREAM COMPLETELY (Simulate network disconnection)
    drop(upstream_shutdown);
    server_handle.abort();
    drop(storage);

    // 6. RESTART BRIDGE DAEMON IN FULLY OFFLINE MODE
    // Reopen persisted SQLite storage from disk
    let offline_storage = Arc::new(SqliteStorage::open(&db_path).unwrap());
    let offline_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let offline_addr = offline_listener.local_addr().unwrap();
    let offline_service = BridgeGrpcService::new(offline_storage.clone(), "mainnet");

    let offline_server_handle = tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(CompactTxStreamerServer::new(offline_service))
            .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(
                offline_listener,
            ))
            .await
            .unwrap();
    });

    // 7. Run probe against offline restarted bridge (Offline Rescan)
    let offline_channel = Channel::from_shared(format!("http://{}", offline_addr))
        .unwrap()
        .connect()
        .await
        .unwrap();

    let offline_probe_trace = workspace_artifacts.join("offline_rescan_probe_trace.jsonl");
    let offline_pass = execute_and_record_probe(
        offline_channel,
        2500000,
        2500004,
        vec![tx1_hash.to_vec(), tx2_hash.to_vec()],
        offline_probe_trace.to_str().unwrap(),
    )
    .await;
    assert!(offline_pass, "Offline rescan probe failed!");

    offline_server_handle.abort();
}

async fn execute_and_record_probe(
    channel: Channel,
    start_height: u64,
    tip_height: u64,
    expected_txids: Vec<Vec<u8>>,
    trace_path: &str,
) -> bool {
    let mut client = CompactTxStreamerClient::new(channel);
    let mut log = Vec::new();
    let mut all_ok = true;

    // Helper to log probe line
    let mut record = |rpc: &str, arg: &str, expect: &str, outcome: &str, ok: bool, ms: u128| {
        if !ok {
            all_ok = false;
        }
        let line = format!(
            "{{\"rpc\":\"{rpc}\",\"arg\":\"{arg}\",\"expect\":\"{expect}\",\"outcome\":\"{}\",\"pass\":{ok},\"elapsed_ms\":{ms}}}\n",
            outcome.replace('"', "'")
        );
        log.push(line);
    };

    // 1. GetLightdInfo
    let t = Instant::now();
    let info = client.get_lightd_info(Empty {}).await.unwrap().into_inner();
    let is_nu5 = info.consensus_branch_id == "c2d6d0b4"
        && info.upgrade_name.eq_ignore_ascii_case("nu5");
    record(
        "GetLightdInfo",
        "",
        "Dynamic NU5 consensus branch c2d6d0b4",
        &format!("chain={} branch={} upgrade={} height={}", info.chain_name, info.consensus_branch_id, info.upgrade_name, info.block_height),
        is_nu5,
        t.elapsed().as_millis(),
    );

    // 2. GetLatestBlock
    let t = Instant::now();
    let tip = client.get_latest_block(ChainSpec {}).await.unwrap().into_inner();
    record(
        "GetLatestBlock",
        "",
        &format!("height={tip_height}"),
        &format!("height={}", tip.height),
        tip.height == tip_height,
        t.elapsed().as_millis(),
    );

    // 3. GetBlockRange
    let t = Instant::now();
    let mut stream = client
        .get_block_range(BlockRange {
            start: Some(BlockId { height: start_height, hash: vec![] }),
            end: Some(BlockId { height: tip_height, hash: vec![] }),
            pool_types: vec![],
        })
        .await
        .unwrap()
        .into_inner();

    let mut blocks = 0u64;
    while let Some(_) = stream.message().await.unwrap() {
        blocks += 1;
    }
    let want_blocks = tip_height - start_height + 1;
    record(
        "GetBlockRange",
        &format!("{start_height}..={tip_height}"),
        &format!("{want_blocks} blocks"),
        &format!("blocks={blocks}"),
        blocks == want_blocks,
        t.elapsed().as_millis(),
    );

    // 4. GetTreeState at start and tip
    for h in [start_height, tip_height] {
        let t = Instant::now();
        let res = client.get_tree_state(BlockId { height: h, hash: vec![] }).await;
        let (ok, out) = match res {
            Ok(ts) => (true, format!("height={} sapling_len={}", ts.get_ref().height, ts.get_ref().sapling_tree.len())),
            Err(e) => (false, format!("{:?}: {}", e.code(), e.message())),
        };
        record("GetTreeState", &format!("height={h}"), "OK", &out, ok, t.elapsed().as_millis());
    }

    // 5. GetSubtreeRoots
    for pool in [0i32, 1] {
        let t = Instant::now();
        let res = client.get_subtree_roots(GetSubtreeRootsArg {
            shielded_protocol: pool,
            start_index: 0,
            max_entries: 0,
        }).await;
        let (ok, out) = match res {
            Ok(_) => (true, "roots stream OK".to_string()),
            Err(e) => (false, format!("{:?}: {}", e.code(), e.message())),
        };
        record("GetSubtreeRoots", &format!("pool={pool}"), "OK", &out, ok, t.elapsed().as_millis());
    }

    // 6. GetTransaction for coverage txids
    for txid in &expected_txids {
        let t = Instant::now();
        let res = client.get_transaction(TxFilter {
            block: None,
            index: 0,
            hash: txid.clone(),
        }).await;
        let (ok, out) = match res {
            Ok(tx) => (true, format!("data_len={}", tx.get_ref().data.len())),
            Err(e) => (false, format!("{:?}: {}", e.code(), e.message())),
        };
        record("GetTransaction", &format!("txid={}", hex::encode(txid)), "OK", &out, ok, t.elapsed().as_millis());
    }

    // Failure Path 1: Out of range TxID -> NotFound
    let t = Instant::now();
    let un_synced_txid = vec![0x99u8; 32];
    let res = client.get_transaction(TxFilter {
        block: None,
        index: 0,
        hash: un_synced_txid.clone(),
    }).await;
    let ok = matches!(&res, Err(e) if e.code() == Code::NotFound);
    record("GetTransaction", "txid=9999..99 (un-synced)", "NotFound (zero-fallback)", &format!("{:?}", res.err().map(|e| e.code())), ok, t.elapsed().as_millis());

    // Failure Path 2: Block height above tip -> NotFound
    let t = Instant::now();
    let res = client.get_block(BlockId { height: tip_height + 1000, hash: vec![] }).await;
    let ok = matches!(&res, Err(e) if e.code() == Code::NotFound);
    record("GetBlock", "height=above_tip", "NotFound", &format!("{:?}", res.err().map(|e| e.code())), ok, t.elapsed().as_millis());

    // Failure Path 3: TreeState below coverage -> NotFound
    let t = Instant::now();
    let res = client.get_tree_state(BlockId { height: start_height.saturating_sub(1), hash: vec![] }).await;
    let ok = matches!(&res, Err(e) if e.code() == Code::NotFound);
    record("GetTreeState", "height=below_coverage", "NotFound", &format!("{:?}", res.err().map(|e| e.code())), ok, t.elapsed().as_millis());

    // Failure Path 4: SendTransaction -> PermissionDenied
    let t = Instant::now();
    let res = client.send_transaction(RawTransaction { data: vec![0u8; 8], height: 0 }).await;
    let ok = matches!(&res, Err(e) if e.code() == Code::PermissionDenied);
    record("SendTransaction", "dummy raw bytes", "PermissionDenied", &format!("{:?}", res.err().map(|e| e.code())), ok, t.elapsed().as_millis());

    if let Ok(mut f) = File::create(trace_path) {
        for line in &log {
            let _ = f.write_all(line.as_bytes());
        }
    }

    all_ok
}
