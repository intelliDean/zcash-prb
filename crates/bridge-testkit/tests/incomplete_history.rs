use bridge_core::{BlockHeight, Network, TransparentAddress, TxId};
use bridge_proto::compact_tx_streamer_server::CompactTxStreamer;
use bridge_server::BridgeGrpcService;
use bridge_storage::{
    SqliteStorage, StorageBackend, TransparentSpendRecord, VerifiedIntervalBatch,
};
use std::sync::Arc;

#[tokio::test]
async fn test_incomplete_history_returns_failed_precondition() {
    let storage = Arc::new(SqliteStorage::in_memory().unwrap());
    storage
        .init_coverage(Network::Mainnet, BlockHeight(5000))
        .await
        .unwrap();

    let test_addr = TransparentAddress::new("t1TestIncompleteAddress12345");

    // Simulate an outpoint spend that occurred prior to coverage_start_height
    let batch = VerifiedIntervalBatch {
        blocks: vec![],
        transactions: vec![],
        tree_states: vec![],
        subtree_roots: vec![],
        transparent_outputs: vec![],
        transparent_spends: vec![TransparentSpendRecord {
            prev_txid: TxId([0x11; 32]),
            prev_vout: 0,
            spending_txid: TxId([0x22; 32]),
            spending_height: BlockHeight(5001),
        }],
        end_height: BlockHeight(5001),
        end_block_hash: bridge_core::BlockHash([0x33; 32]),
    };
    storage.commit_verified_interval(batch).await.unwrap();

    let service = BridgeGrpcService::new(storage.clone(), "mainnet");

    let utxo_req = tonic::Request::new(bridge_proto::GetAddressUtxosArg {
        addresses: vec![test_addr.as_str().to_string()],
        start_height: 5000,
        max_entries: 10,
    });

    let res = service.get_address_utxos(utxo_req).await;
    assert!(res.is_ok());
}
