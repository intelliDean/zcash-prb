use super::blocks::ResponseStream;
use bridge_core::{BlockHeight, TransparentAddress};
use bridge_proto::{
    AddressList, Balance, GetAddressUtxosArg, GetAddressUtxosReply, GetAddressUtxosReplyList,
    RawTransaction, TransparentAddressBlockFilter,
};
use bridge_storage::StorageBackend;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub async fn get_taddress_txids(
    storage: &Arc<dyn StorageBackend>,
    request: Request<TransparentAddressBlockFilter>,
) -> Result<Response<ResponseStream<RawTransaction>>, Status> {
    let req = request.into_inner();
    let addr = TransparentAddress::new(req.address);

    let txs = storage
        .get_taddress_transactions(&addr, None)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let (tx, rx) = tokio::sync::mpsc::channel(txs.len().max(1));
    tokio::spawn(async move {
        for raw_tx in txs {
            if tx.send(Ok(raw_tx)).await.is_err() {
                break;
            }
        }
    });

    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}

pub async fn get_taddress_transactions(
    storage: &Arc<dyn StorageBackend>,
    request: Request<TransparentAddressBlockFilter>,
) -> Result<Response<ResponseStream<RawTransaction>>, Status> {
    let req = request.into_inner();
    let addr = TransparentAddress::new(req.address);

    let range = match (
        req.range.as_ref().and_then(|r| r.start.as_ref()),
        req.range.as_ref().and_then(|r| r.end.as_ref()),
    ) {
        (Some(s), Some(e)) => Some(bridge_core::IntervalRange::new(
            BlockHeight(s.height as u32),
            BlockHeight(e.height as u32),
        )),
        _ => None,
    };

    let txs = storage
        .get_taddress_transactions(&addr, range)
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

    let (tx, rx) = tokio::sync::mpsc::channel(txs.len().max(1));
    tokio::spawn(async move {
        for raw_tx in txs {
            if tx.send(Ok(raw_tx)).await.is_err() {
                break;
            }
        }
    });

    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}

pub async fn get_taddress_balance(
    storage: &Arc<dyn StorageBackend>,
    request: Request<AddressList>,
) -> Result<Response<Balance>, Status> {
    let req = request.into_inner();
    let mut total_zat: i64 = 0;

    for addr_str in req.addresses {
        let addr = TransparentAddress::new(addr_str);
        let utxos = storage
            .get_address_utxos(&addr)
            .await
            .map_err(|e| match e {
                bridge_core::BridgeError::IncompleteHistory { .. } => Status::failed_precondition(
                    "Incomplete transparent history: prior output out of coverage",
                ),
                _ => Status::internal(e.to_string()),
            })?;

        for u in utxos {
            total_zat += u.value_zat;
        }
    }

    Ok(Response::new(Balance {
        value_zat: total_zat,
    }))
}

pub async fn get_address_utxos(
    storage: &Arc<dyn StorageBackend>,
    request: Request<GetAddressUtxosArg>,
) -> Result<Response<GetAddressUtxosReplyList>, Status> {
    let req = request.into_inner();
    let mut all_utxos = Vec::new();

    for addr_str in req.addresses {
        let addr = TransparentAddress::new(addr_str);
        let utxos = storage
            .get_address_utxos(&addr)
            .await
            .map_err(|e| match e {
                bridge_core::BridgeError::IncompleteHistory { .. } => Status::failed_precondition(
                    "Incomplete transparent history: prior output out of coverage",
                ),
                _ => Status::internal(e.to_string()),
            })?;
        all_utxos.extend(utxos);
    }

    Ok(Response::new(GetAddressUtxosReplyList {
        address_utxos: all_utxos,
    }))
}

pub async fn get_address_utxos_stream(
    storage: &Arc<dyn StorageBackend>,
    request: Request<GetAddressUtxosArg>,
) -> Result<Response<ResponseStream<GetAddressUtxosReply>>, Status> {
    let req = request.into_inner();
    let mut all_utxos = Vec::new();

    for addr_str in req.addresses {
        let addr = TransparentAddress::new(addr_str);
        let utxos = storage
            .get_address_utxos(&addr)
            .await
            .map_err(|e| match e {
                bridge_core::BridgeError::IncompleteHistory { .. } => Status::failed_precondition(
                    "Incomplete transparent history: prior output out of coverage",
                ),
                _ => Status::internal(e.to_string()),
            })?;
        all_utxos.extend(utxos);
    }

    let (tx, rx) = tokio::sync::mpsc::channel(all_utxos.len().max(1));
    tokio::spawn(async move {
        for u in all_utxos {
            if tx.send(Ok(u)).await.is_err() {
                break;
            }
        }
    });

    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
