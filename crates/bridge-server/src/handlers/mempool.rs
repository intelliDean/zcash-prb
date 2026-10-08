use super::blocks::ResponseStream;
use bridge_proto::{CompactTx, Empty, GetMempoolTxRequest, RawTransaction};
use bridge_storage::StorageBackend;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub async fn get_mempool_tx(
    storage: &Arc<dyn StorageBackend>,
    _request: Request<GetMempoolTxRequest>,
) -> Result<Response<ResponseStream<CompactTx>>, Status> {
    let storage = storage.clone();
    let initial_tip = storage.get_latest_block().await.ok().flatten();

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if tx.is_closed() {
                break;
            }
            if let Ok(current_tip) = storage.get_latest_block().await
                && current_tip != initial_tip
            {
                // Committed public tip changed (height advance, rollback, or same-height replacement)
                break;
            }
        }
        drop(tx);
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}

pub async fn get_mempool_stream(
    storage: &Arc<dyn StorageBackend>,
    _request: Request<Empty>,
) -> Result<Response<ResponseStream<RawTransaction>>, Status> {
    let storage = storage.clone();
    let initial_tip = storage.get_latest_block().await.ok().flatten();

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if tx.is_closed() {
                break;
            }
            if let Ok(current_tip) = storage.get_latest_block().await
                && current_tip != initial_tip
            {
                // Committed public tip changed (height advance, rollback, or same-height replacement)
                break;
            }
        }
        drop(tx);
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
