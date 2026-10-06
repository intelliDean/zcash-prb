use bridge_core::{BlockHeight, Network, TxId};
use bridge_proto::{
    CompactBlock, CompactOrchardAction, CompactSaplingOutput, CompactSaplingSpend, CompactTx,
    RawTransaction,
};
use bridge_storage::{
    SqliteStorage, StorageBackend, TransparentOutputRecord, VerifiedIntervalBatch,
};
use bridge_testkit::create_test_tree_state;
use std::sync::Arc;

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
    let txid = TxId(txid_arr);

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
            spends: vec![CompactSaplingSpend { nf: vec![0x01; 32] }],
            outputs: vec![CompactSaplingOutput {
                cmu: vec![0x02; 32],
                ephemeral_key: vec![0x03; 32],
                ciphertext: vec![0; 52],
            }],
            actions: vec![CompactOrchardAction {
                nullifier: vec![0x04; 32],
                cmx: vec![0x05; 32],
                ephemeral_key: vec![0x06; 32],
                ciphertext: vec![0; 52],
            }],
            ironwood_actions: vec![CompactOrchardAction {
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

    let batch = VerifiedIntervalBatch {
        blocks: vec![block.clone()],
        transactions: vec![RawTransaction {
            data: tx_data,
            height: 1000,
        }],
        tree_states: vec![create_test_tree_state(1000, &hex::encode(vec![0xaa; 32]))],
        subtree_roots: vec![],
        transparent_outputs: vec![TransparentOutputRecord {
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
    let retrieved_block = storage
        .get_compact_block(BlockHeight(1000))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        retrieved_block.vtx[0].outputs.len(),
        1,
        "Sapling output preserved"
    );
    assert_eq!(
        retrieved_block.vtx[0].actions.len(),
        1,
        "Orchard action preserved"
    );
    assert_eq!(
        retrieved_block.vtx[0].ironwood_actions.len(),
        1,
        "Ironwood action preserved"
    );

    // 2. Verify retrieval of transparent UTXO
    let utxos = storage.get_address_utxos(&t_addr).await.unwrap();
    assert_eq!(utxos.len(), 1);
    assert_eq!(utxos[0].value_zat, 500_000_000);

    // 3. Verify tree state preservation for all shielded pools
    let ts = storage
        .get_tree_state(BlockHeight(1000))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ts.sapling_tree, "sapling_tree_state_data");
    assert_eq!(ts.orchard_tree, "orchard_tree_state_data");
    assert_eq!(ts.ironwood_tree, "ironwood_tree_state_data");
}
