use bridge_core::BlockHeight;
use bridge_proto::{BlockId, BlockRange, CompactBlock};
use bridge_storage::StorageBackend;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
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

    Ok(Response::new(BlockId {
        height: height.0 as u64,
        hash: hash.as_bytes().to_vec(),
    }))
}

pub async fn verify_block_integrity(
    storage: &Arc<dyn StorageBackend>,
    block: &CompactBlock,
) -> Result<(), Status> {
    if block.hash.len() != 32 {
        return Err(Status::data_loss(format!(
            "Corrupted block hash in cache for block {}",
            block.height
        )));
    }

    for vtx in &block.vtx {
        if vtx.txid.len() != 32 {
            return Err(Status::data_loss(format!(
                "Malformed transaction ID in block {}",
                block.height
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&vtx.txid);
        let txid = bridge_core::TxId(arr);
        match storage.get_full_transaction(&txid).await {
            Ok(Some(raw_tx)) => {
                bridge_verifier::verify_transaction(&raw_tx.data, &txid).map_err(|e| {
                    Status::data_loss(format!(
                        "Corrupted transaction {} in cache for block {}: {}",
                        txid, block.height, e
                    ))
                })?;
            }
            Ok(None) => {
                return Err(Status::data_loss(format!(
                    "Referenced full transaction missing for block {}",
                    block.height
                )));
            }
            Err(e) => {
                return Err(Status::internal(e.to_string()));
            }
        }
    }
    Ok(())
}

pub async fn resolve_block_height(
    storage: &Arc<dyn StorageBackend>,
    id: &BlockId,
) -> Result<BlockHeight, Status> {
    if id.height > 0 {
        Ok(BlockHeight(id.height as u32))
    } else if id.hash.len() == 32 {
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&id.hash);
        storage
            .find_block_height_by_hash(&bridge_core::BlockHash(arr))
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| {
                Status::not_found(format!(
                    "Block with hash {} not found in local verified storage",
                    hex::encode(&id.hash)
                ))
            })
    } else {
        Ok(BlockHeight(id.height as u32))
    }
}

pub async fn get_block(
    storage: &Arc<dyn StorageBackend>,
    request: Request<BlockId>,
) -> Result<Response<CompactBlock>, Status> {
    let req = request.into_inner();
    let height = resolve_block_height(storage, &req).await?;

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

    verify_block_integrity(storage, &block).await?;

    Ok(Response::new(block))
}

pub async fn get_block_range(
    storage: &Arc<dyn StorageBackend>,
    request: Request<BlockRange>,
) -> Result<Response<ResponseStream<CompactBlock>>, Status> {
    let req = request.into_inner();
    let start_id = req
        .start
        .ok_or_else(|| Status::invalid_argument("Missing start BlockID"))?;
    let end_id = req
        .end
        .ok_or_else(|| Status::invalid_argument("Missing end BlockID"))?;

    let start = BlockHeight(start_id.height as u32);
    let end = BlockHeight(end_id.height as u32);

    if start.0 > end.0 {
        return Err(Status::invalid_argument(
            "start height cannot exceed end height",
        ));
    }

    let expected_count = (end.0 - start.0 + 1) as usize;

    let blocks = storage
        .get_compact_block_range(start, end)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let storage_clone = storage.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(blocks.len().max(1) + 1);

    tokio::spawn(async move {
        let is_incomplete = blocks.len() != expected_count;

        for block in blocks {
            if let Err(status) = verify_block_integrity(&storage_clone, &block).await {
                let _ = tx.send(Err(status)).await;
                return;
            }

            if tx.send(Ok(block)).await.is_err() {
                return;
            }
        }

        if is_incomplete {
            let _ = tx
                .send(Err(Status::data_loss(
                    "Incomplete coverage or missing blocks for requested block range",
                )))
                .await;
        }
    });

    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
