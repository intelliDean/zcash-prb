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
    let initial_tip = storage
        .get_latest_block()
        .await
        .ok()
        .flatten()
        .map(|(h, _)| h);

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if tx.is_closed() {
                break;
            }
            if let Ok(Some((current_h, _))) = storage.get_latest_block().await
                && Some(current_h) != initial_tip
            {
                // Committed public tip changed; complete stream cleanly
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
    let initial_tip = storage
        .get_latest_block()
        .await
        .ok()
        .flatten()
        .map(|(h, _)| h);

    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            if tx.is_closed() {
                break;
            }
            if let Ok(Some((current_h, _))) = storage.get_latest_block().await
                && Some(current_h) != initial_tip
            {
                // Committed public tip changed; complete stream cleanly
                break;
            }
        }
        drop(tx);
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
