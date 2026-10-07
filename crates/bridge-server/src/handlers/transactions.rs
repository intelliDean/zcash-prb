use bridge_core::TxId;
use bridge_proto::{RawTransaction, SendResponse, TxFilter};
use bridge_storage::StorageBackend;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::warn;

pub async fn get_transaction(
    storage: &Arc<dyn StorageBackend>,
    request: Request<TxFilter>,
) -> Result<Response<RawTransaction>, Status> {
    let req = request.into_inner();
    if req.hash.len() != 32 {
        return Err(Status::invalid_argument("TxFilter hash must be 32 bytes"));
    }

    let mut hash_arr = [0u8; 32];
    hash_arr.copy_from_slice(&req.hash);
    let txid = TxId(hash_arr);

    // Strict RPC Policy: Zero selective fallback. Look up in local storage ONLY.
    let raw_tx = storage
        .get_full_transaction(&txid)
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .ok_or_else(|| {
            Status::not_found(format!(
                "Transaction {} not found in local verified coverage. Zero-fallback policy active.",
                txid
            ))
        })?;

    // Strict cache integrity check: verify transaction payload matches requested TxID
    bridge_verifier::verify_transaction(&raw_tx.data, &txid).map_err(|e| {
        Status::data_loss(format!(
            "Corrupted full transaction in cache for txid {}: {}",
            txid, e
        ))
    })?;

    Ok(Response::new(raw_tx))
}

pub fn send_transaction(
    _request: Request<RawTransaction>,
) -> Result<Response<SendResponse>, Status> {
    // Enforce strict MVP policy: deny unshielded transaction broadcasts
    warn!("Blocked SendTransaction call: private receive bridge denies broadcasts in MVP profile");
    Err(Status::permission_denied(
        "Private receive bridge does not support transaction broadcasting in confirmed-receive MVP profile.",
    ))
}
