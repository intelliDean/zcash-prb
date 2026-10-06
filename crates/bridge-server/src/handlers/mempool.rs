use super::blocks::ResponseStream;
use bridge_proto::{CompactTx, Empty, GetMempoolTxRequest, RawTransaction};
use tonic::{Request, Response, Status};

pub fn get_mempool_tx(
    _request: Request<GetMempoolTxRequest>,
) -> Result<Response<ResponseStream<CompactTx>>, Status> {
    Err(Status::unimplemented(
        "Bridge operates in confirmed-only profile. Mempool streaming is disabled.",
    ))
}

pub fn get_mempool_stream(
    _request: Request<Empty>,
) -> Result<Response<ResponseStream<RawTransaction>>, Status> {
    Err(Status::unimplemented(
        "Bridge operates in confirmed-only profile. Mempool streaming is disabled.",
    ))
}
