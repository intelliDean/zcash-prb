use super::blocks::ResponseStream;
use bridge_proto::{BlockId, GetSubtreeRootsArg, SubtreeRoot, TreeState};
use bridge_storage::StorageBackend;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub async fn get_tree_state(
    storage: &Arc<dyn StorageBackend>,
    request: Request<BlockId>,
) -> Result<Response<TreeState>, Status> {
    let req = request.into_inner();
    let height = super::blocks::resolve_block_height(storage, &req).await?;

    let tree_state = storage
        .get_tree_state(height)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| {
            Status::not_found(format!(
                "Tree state at height {} not found in local verified storage",
                height.0
            ))
        })?;

    Ok(Response::new(tree_state))
}

pub async fn get_latest_tree_state(
    storage: &Arc<dyn StorageBackend>,
) -> Result<Response<TreeState>, Status> {
    let (height, _) = storage
        .get_latest_block()
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::unavailable("Bridge coverage not yet initialized"))?;

    let tree_state = storage
        .get_tree_state(height)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| Status::not_found("Latest tree state not found"))?;

    Ok(Response::new(tree_state))
}

pub async fn get_subtree_roots(
    storage: &Arc<dyn StorageBackend>,
    request: Request<GetSubtreeRootsArg>,
) -> Result<Response<ResponseStream<SubtreeRoot>>, Status> {
    let req = request.into_inner();
    let roots = storage
        .get_subtree_roots(req.shielded_protocol, req.start_index, req.max_entries)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let (tx, rx) = tokio::sync::mpsc::channel(roots.len().max(1));
    tokio::spawn(async move {
        for r in roots {
            if tx.send(Ok(r)).await.is_err() {
                break;
            }
        }
    });

    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
