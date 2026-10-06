use bridge_core::BlockHeight;
use bridge_proto::{BlockId, BlockRange, CompactBlock};
use bridge_storage::StorageBackend;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

pub type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

pub async fn get_latest_block(
    storage: &Arc<dyn StorageBackend>,
) -> Result<Response<BlockId>, Status> {
    let (height, hash) = storage
        .get_latest_block()
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::unavailable("Bridge coverage not yet initialized"))?;

    Ok(Response::new(BlockId { height: height.0 as u64, hash: hash.as_bytes().to_vec() }))
}

pub async fn get_block(
    storage: &Arc<dyn StorageBackend>,
    request: Request<BlockId>,
) -> Result<Response<CompactBlock>, Status> {
    let req = request.into_inner();
    let height = BlockHeight(req.height as u32);

    let block = storage
        .get_compact_block(height)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| {
            Status::not_found(format!(
                "Block at height {} not found in local verified storage",
                height.0
            ))
        })?;

    Ok(Response::new(block))
}

pub async fn get_block_range(
    storage: &Arc<dyn StorageBackend>,
    request: Request<BlockRange>,
) -> Result<Response<ResponseStream<CompactBlock>>, Status> {
    let req = request.into_inner();
    let start_id = req.start.ok_or_else(|| Status::invalid_argument("Missing start BlockID"))?;
    let end_id = req.end.ok_or_else(|| Status::invalid_argument("Missing end BlockID"))?;

    let start = BlockHeight(start_id.height as u32);
    let end = BlockHeight(end_id.height as u32);

    let blocks = storage
        .get_compact_block_range(start, end)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let (tx, rx) = tokio::sync::mpsc::channel(blocks.len().max(1));
    tokio::spawn(async move {
        for block in blocks {
            if tx.send(Ok(block)).await.is_err() {
                break;
            }
        }
    });

    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
