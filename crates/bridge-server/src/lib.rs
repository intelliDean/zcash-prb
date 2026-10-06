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
