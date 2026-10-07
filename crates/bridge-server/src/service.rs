use crate::handlers::{blocks, info, mempool, transactions, transparent, trees};
use bridge_proto::compact_tx_streamer_server::CompactTxStreamer;
use bridge_proto::{
    Address, AddressList, Balance, BlockId, BlockRange, ChainSpec, CompactBlock, CompactTx,
    Duration as ProtoDuration, Empty, GetAddressUtxosArg, GetAddressUtxosReply,
    GetAddressUtxosReplyList, GetMempoolTxRequest, GetSubtreeRootsArg, LightdInfo, PingResponse,
    RawTransaction, SendResponse, SubtreeRoot, TransparentAddressBlockFilter, TreeState, TxFilter,
};
use bridge_storage::StorageBackend;
use std::sync::Arc;
use tonic::{Request, Response, Status};

pub struct BridgeGrpcService {
    storage: Arc<dyn StorageBackend>,
    network_name: String,
}

impl BridgeGrpcService {
    pub fn new(storage: Arc<dyn StorageBackend>, network_name: impl Into<String>) -> Self {
        Self {
            storage,
            network_name: network_name.into(),
        }
    }
}

#[tonic::async_trait]
impl CompactTxStreamer for BridgeGrpcService {
    type GetBlockRangeStream = blocks::ResponseStream<CompactBlock>;
    type GetBlockRangeNullifiersStream = blocks::ResponseStream<CompactBlock>;
    type GetTaddressTxidsStream = blocks::ResponseStream<RawTransaction>;
    type GetTaddressTransactionsStream = blocks::ResponseStream<RawTransaction>;
    type GetMempoolTxStream = blocks::ResponseStream<CompactTx>;
    type GetMempoolStreamStream = blocks::ResponseStream<RawTransaction>;
    type GetSubtreeRootsStream = blocks::ResponseStream<SubtreeRoot>;
    type GetAddressUtxosStreamStream = blocks::ResponseStream<GetAddressUtxosReply>;

    async fn get_latest_block(
        &self,
        _request: Request<ChainSpec>,
    ) -> Result<Response<BlockId>, Status> {
        blocks::get_latest_block(&self.storage).await
    }

    async fn get_block(&self, request: Request<BlockId>) -> Result<Response<CompactBlock>, Status> {
        blocks::get_block(&self.storage, request).await
    }

    async fn get_block_nullifiers(
        &self,
        _request: Request<BlockId>,
    ) -> Result<Response<CompactBlock>, Status> {
        Err(Status::unimplemented(
            "GetBlockNullifiers is deprecated; please use GetBlockRange with poolTypes",
        ))
    }

    async fn get_block_range(
        &self,
        request: Request<BlockRange>,
    ) -> Result<Response<Self::GetBlockRangeStream>, Status> {
        blocks::get_block_range(&self.storage, request).await
    }

    async fn get_block_range_nullifiers(
        &self,
        _request: Request<BlockRange>,
    ) -> Result<Response<Self::GetBlockRangeNullifiersStream>, Status> {
        Err(Status::unimplemented(
            "GetBlockRangeNullifiers is deprecated; please use GetBlockRange with poolTypes",
        ))
    }

    async fn get_transaction(
        &self,
        request: Request<TxFilter>,
    ) -> Result<Response<RawTransaction>, Status> {
        transactions::get_transaction(&self.storage, request).await
    }

    async fn send_transaction(
        &self,
        request: Request<RawTransaction>,
    ) -> Result<Response<SendResponse>, Status> {
        transactions::send_transaction(request)
    }

    async fn get_taddress_txids(
        &self,
        request: Request<TransparentAddressBlockFilter>,
    ) -> Result<Response<Self::GetTaddressTxidsStream>, Status> {
        transparent::get_taddress_txids(&self.storage, request).await
    }

    async fn get_taddress_transactions(
        &self,
        request: Request<TransparentAddressBlockFilter>,
    ) -> Result<Response<Self::GetTaddressTransactionsStream>, Status> {
        transparent::get_taddress_transactions(&self.storage, request).await
    }

    async fn get_taddress_balance(
        &self,
        request: Request<AddressList>,
    ) -> Result<Response<Balance>, Status> {
        transparent::get_taddress_balance(&self.storage, request).await
    }

    async fn get_taddress_balance_stream(
        &self,
        _request: Request<tonic::Streaming<Address>>,
    ) -> Result<Response<Balance>, Status> {
        Err(Status::unimplemented(
            "GetTaddressBalanceStream not implemented",
        ))
    }

    async fn get_mempool_tx(
        &self,
        request: Request<GetMempoolTxRequest>,
    ) -> Result<Response<Self::GetMempoolTxStream>, Status> {
        mempool::get_mempool_tx(&self.storage, request).await
    }

    async fn get_mempool_stream(
        &self,
        request: Request<Empty>,
    ) -> Result<Response<Self::GetMempoolStreamStream>, Status> {
        mempool::get_mempool_stream(&self.storage, request).await
    }

    async fn get_tree_state(
        &self,
        request: Request<BlockId>,
    ) -> Result<Response<TreeState>, Status> {
        trees::get_tree_state(&self.storage, request).await
    }

    async fn get_latest_tree_state(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<TreeState>, Status> {
        trees::get_latest_tree_state(&self.storage).await
    }

    async fn get_subtree_roots(
        &self,
        request: Request<GetSubtreeRootsArg>,
    ) -> Result<Response<Self::GetSubtreeRootsStream>, Status> {
        trees::get_subtree_roots(&self.storage, request).await
    }

    async fn get_address_utxos(
        &self,
        request: Request<GetAddressUtxosArg>,
    ) -> Result<Response<GetAddressUtxosReplyList>, Status> {
        transparent::get_address_utxos(&self.storage, request).await
    }

    async fn get_address_utxos_stream(
        &self,
        request: Request<GetAddressUtxosArg>,
    ) -> Result<Response<Self::GetAddressUtxosStreamStream>, Status> {
        transparent::get_address_utxos_stream(&self.storage, request).await
    }

    async fn get_lightd_info(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<LightdInfo>, Status> {
        info::get_lightd_info(&self.storage, &self.network_name).await
    }

    async fn ping(
        &self,
        request: Request<ProtoDuration>,
    ) -> Result<Response<PingResponse>, Status> {
        info::ping(request)
    }
}
