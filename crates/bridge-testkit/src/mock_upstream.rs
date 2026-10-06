use bridge_proto::compact_tx_streamer_server::{CompactTxStreamer, CompactTxStreamerServer};
use bridge_proto::{
    Address, AddressList, Balance, BlockId, BlockRange, ChainSpec, CompactBlock, CompactTx,
    Duration as ProtoDuration, Empty, GetAddressUtxosArg, GetAddressUtxosReply,
    GetAddressUtxosReplyList, GetMempoolTxRequest, GetSubtreeRootsArg, LightdInfo, PingResponse,
    RawTransaction, SendResponse, SubtreeRoot, TransparentAddressBlockFilter, TreeState, TxFilter,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[derive(Clone, Default)]
pub struct MockUpstreamServer {
    pub blocks: Arc<Mutex<HashMap<u64, CompactBlock>>>,
    pub transactions: Arc<Mutex<HashMap<Vec<u8>, RawTransaction>>>,
    pub call_history: Arc<Mutex<Vec<String>>>,
    pub latest_height: Arc<Mutex<u64>>,
}

impl MockUpstreamServer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_block(&self, block: CompactBlock) {
        let mut blocks = self.blocks.lock().unwrap();
        let height = block.height;
        blocks.insert(height, block);

        let mut latest = self.latest_height.lock().unwrap();
        if height > *latest {
            *latest = height;
        }
    }

    pub fn add_transaction(&self, txid: &[u8], tx: RawTransaction) {
        let mut txs = self.transactions.lock().unwrap();
        txs.insert(txid.to_vec(), tx);
    }

    pub fn recorded_calls(&self) -> Vec<String> {
        self.call_history.lock().unwrap().clone()
    }
}

#[tonic::async_trait]
impl CompactTxStreamer for MockUpstreamServer {
    type GetBlockRangeStream = ResponseStream<CompactBlock>;
    type GetBlockRangeNullifiersStream = ResponseStream<CompactBlock>;
    type GetTaddressTxidsStream = ResponseStream<RawTransaction>;
    type GetTaddressTransactionsStream = ResponseStream<RawTransaction>;
    type GetMempoolTxStream = ResponseStream<CompactTx>;
    type GetMempoolStreamStream = ResponseStream<RawTransaction>;
    type GetSubtreeRootsStream = ResponseStream<SubtreeRoot>;
    type GetAddressUtxosStreamStream = ResponseStream<GetAddressUtxosReply>;

    async fn get_latest_block(&self, _request: Request<ChainSpec>) -> Result<Response<BlockId>, Status> {
        self.call_history.lock().unwrap().push("GetLatestBlock".to_string());
        let height = *self.latest_height.lock().unwrap();
        Ok(Response::new(BlockId {
            height,
            hash: vec![1u8; 32],
        }))
    }

    async fn get_block(&self, request: Request<BlockId>) -> Result<Response<CompactBlock>, Status> {
        let req = request.into_inner();
        self.call_history.lock().unwrap().push(format!("GetBlock({})", req.height));

        let blocks = self.blocks.lock().unwrap();
        let b = blocks
            .get(&req.height)
            .cloned()
            .ok_or_else(|| Status::not_found("Mock block not found"))?;

        Ok(Response::new(b))
    }

    async fn get_block_nullifiers(&self, _request: Request<BlockId>) -> Result<Response<CompactBlock>, Status> {
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_block_range(
        &self,
        request: Request<BlockRange>,
    ) -> Result<Response<Self::GetBlockRangeStream>, Status> {
        let req = request.into_inner();
        let start = req.start.unwrap().height;
        let end = req.end.unwrap().height;
        self.call_history.lock().unwrap().push(format!("GetBlockRange({}..={})", start, end));

        let blocks = self.blocks.lock().unwrap();
        let mut result = Vec::new();
        for h in start..=end {
            if let Some(b) = blocks.get(&h) {
                result.push(b.clone());
            }
        }

        let (tx, rx) = tokio::sync::mpsc::channel(result.len().max(1));
        tokio::spawn(async move {
            for b in result {
                let _ = tx.send(Ok(b)).await;
            }
        });

        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn get_block_range_nullifiers(
        &self,
        _request: Request<BlockRange>,
    ) -> Result<Response<Self::GetBlockRangeNullifiersStream>, Status> {
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_transaction(&self, request: Request<TxFilter>) -> Result<Response<RawTransaction>, Status> {
        let req = request.into_inner();
        let hex_hash = hex::encode(&req.hash);
        self.call_history.lock().unwrap().push(format!("GetTransaction({})", hex_hash));

        let txs = self.transactions.lock().unwrap();
        let tx = txs
            .get(&req.hash)
            .cloned()
            .ok_or_else(|| Status::not_found("Mock transaction not found"))?;

        Ok(Response::new(tx))
    }

    async fn send_transaction(&self, _request: Request<RawTransaction>) -> Result<Response<SendResponse>, Status> {
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_taddress_txids(
        &self,
        request: Request<TransparentAddressBlockFilter>,
    ) -> Result<Response<Self::GetTaddressTxidsStream>, Status> {
        let req = request.into_inner();
        self.call_history.lock().unwrap().push(format!("GetTaddressTxids({})", req.address));
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_taddress_transactions(
        &self,
        request: Request<TransparentAddressBlockFilter>,
    ) -> Result<Response<Self::GetTaddressTransactionsStream>, Status> {
        let req = request.into_inner();
        self.call_history.lock().unwrap().push(format!("GetTaddressTransactions({})", req.address));
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_taddress_balance(&self, _request: Request<AddressList>) -> Result<Response<Balance>, Status> {
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_taddress_balance_stream(
        &self,
        _request: Request<tonic::Streaming<Address>>,
    ) -> Result<Response<Balance>, Status> {
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_mempool_tx(
        &self,
        _request: Request<GetMempoolTxRequest>,
    ) -> Result<Response<Self::GetMempoolTxStream>, Status> {
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_mempool_stream(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::GetMempoolStreamStream>, Status> {
        Err(Status::unimplemented("unimplemented"))
    }

    async fn get_tree_state(&self, request: Request<BlockId>) -> Result<Response<TreeState>, Status> {
        let req = request.into_inner();
        self.call_history.lock().unwrap().push(format!("GetTreeState({})", req.height));
        Ok(Response::new(TreeState {
            network: "mainnet".to_string(),
            height: req.height,
            hash: hex::encode(vec![1u8; 32]),
            time: 1700000000,
            sapling_tree: "".to_string(),
            orchard_tree: "".to_string(),
            ironwood_tree: "".to_string(),
        }))
    }

    async fn get_latest_tree_state(&self, _request: Request<Empty>) -> Result<Response<TreeState>, Status> {
        Ok(Response::new(TreeState::default()))
    }

    async fn get_subtree_roots(
        &self,
        _request: Request<GetSubtreeRootsArg>,
    ) -> Result<Response<Self::GetSubtreeRootsStream>, Status> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn get_address_utxos(
        &self,
        request: Request<GetAddressUtxosArg>,
    ) -> Result<Response<GetAddressUtxosReplyList>, Status> {
        let req = request.into_inner();
        for a in req.addresses {
            self.call_history.lock().unwrap().push(format!("GetAddressUtxos({})", a));
        }
        Ok(Response::new(GetAddressUtxosReplyList::default()))
    }

    async fn get_address_utxos_stream(
        &self,
        _request: Request<GetAddressUtxosArg>,
    ) -> Result<Response<Self::GetAddressUtxosStreamStream>, Status> {
        let (_tx, rx) = tokio::sync::mpsc::channel(1);
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }

    async fn get_lightd_info(&self, _request: Request<Empty>) -> Result<Response<LightdInfo>, Status> {
        Ok(Response::new(LightdInfo {
            version: "0.1.0".to_string(),
            vendor: "mock-upstream".to_string(),
            ..Default::default()
        }))
    }

    async fn ping(&self, _request: Request<ProtoDuration>) -> Result<Response<PingResponse>, Status> {
        Ok(Response::new(PingResponse::default()))
    }
}

pub async fn start_mock_upstream(
    mock: MockUpstreamServer,
) -> Result<(SocketAddr, tokio::sync::oneshot::Sender<()>), Box<dyn std::error::Error>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();

    tokio::spawn(async move {
        tonic::transport::Server::builder()
            .add_service(CompactTxStreamerServer::new(mock))
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async {
                    let _ = shutdown_rx.await;
                },
            )
            .await
            .unwrap();
    });

    Ok((addr, shutdown_tx))
}
