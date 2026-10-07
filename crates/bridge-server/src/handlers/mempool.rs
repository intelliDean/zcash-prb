use super::blocks::ResponseStream;
use bridge_proto::{CompactTx, Empty, GetMempoolTxRequest, RawTransaction};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

pub fn get_mempool_tx(
    _request: Request<GetMempoolTxRequest>,
) -> Result<Response<ResponseStream<CompactTx>>, Status> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        drop(tx);
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}

pub fn get_mempool_stream(
    _request: Request<Empty>,
) -> Result<Response<ResponseStream<RawTransaction>>, Status> {
    let (tx, rx) = tokio::sync::mpsc::channel(1);
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
        drop(tx);
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
