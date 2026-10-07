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
    use bridge_proto::{BlockId, CompactBlock, CompactTx, Empty};
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
}
