use bridge_core::{BlockHeight, TransparentAddress, TxId};
use bridge_proto::compact_tx_streamer_server::CompactTxStreamer;
use bridge_proto::{
    Address, AddressList, Balance, BlockId, BlockRange, ChainSpec, CompactBlock, CompactTx,
    Duration as ProtoDuration, Empty, GetAddressUtxosArg, GetAddressUtxosReply,
    GetAddressUtxosReplyList, GetMempoolTxRequest, GetSubtreeRootsArg, LightdInfo, PingResponse,
    RawTransaction, SendResponse, SubtreeRoot, TransparentAddressBlockFilter, TreeState, TxFilter,
};
use bridge_storage::StorageBackend;
use std::pin::Pin;
use std::sync::Arc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::Stream;
use tonic::{Request, Response, Status};
use tracing::{debug, warn};

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

type ResponseStream<T> = Pin<Box<dyn Stream<Item = Result<T, Status>> + Send + 'static>>;

#[tonic::async_trait]
impl CompactTxStreamer for BridgeGrpcService {
    type GetBlockRangeStream = ResponseStream<CompactBlock>;
    type GetBlockRangeNullifiersStream = ResponseStream<CompactBlock>;
    type GetTaddressTxidsStream = ResponseStream<RawTransaction>;
    type GetTaddressTransactionsStream = ResponseStream<RawTransaction>;
    type GetMempoolTxStream = ResponseStream<CompactTx>;
    type GetMempoolStreamStream = ResponseStream<RawTransaction>;
    type GetSubtreeRootsStream = ResponseStream<SubtreeRoot>;
    type GetAddressUtxosStreamStream = ResponseStream<GetAddressUtxosReply>;

    async fn get_latest_block(&self, _request: Request<ChainSpec>) -> Result<Response<BlockId>, Status> {
        let (height, hash) = self
            .storage
            .get_latest_block()
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::unavailable("Bridge coverage not yet initialized"))?;

        Ok(Response::new(BlockId {
            height: height.0 as u64,
            hash: hash.as_bytes().to_vec(),
        }))
    }

    async fn get_block(&self, request: Request<BlockId>) -> Result<Response<CompactBlock>, Status> {
        let req = request.into_inner();
        let height = BlockHeight(req.height as u32);

        let block = self
            .storage
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

    async fn get_block_nullifiers(&self, _request: Request<BlockId>) -> Result<Response<CompactBlock>, Status> {
        Err(Status::unimplemented(
            "GetBlockNullifiers is deprecated; please use GetBlockRange with poolTypes",
        ))
    }

    async fn get_block_range(
        &self,
        request: Request<BlockRange>,
    ) -> Result<Response<Self::GetBlockRangeStream>, Status> {
        let req = request.into_inner();
        let start_id = req.start.ok_or_else(|| Status::invalid_argument("Missing start BlockID"))?;
        let end_id = req.end.ok_or_else(|| Status::invalid_argument("Missing end BlockID"))?;

        let start = BlockHeight(start_id.height as u32);
        let end = BlockHeight(end_id.height as u32);

        let blocks = self
            .storage
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

    async fn get_block_range_nullifiers(
        &self,
        _request: Request<BlockRange>,
    ) -> Result<Response<Self::GetBlockRangeNullifiersStream>, Status> {
        Err(Status::unimplemented(
            "GetBlockRangeNullifiers is deprecated; please use GetBlockRange with poolTypes",
        ))
    }

    async fn get_transaction(&self, request: Request<TxFilter>) -> Result<Response<RawTransaction>, Status> {
        let req = request.into_inner();
        if req.hash.len() != 32 {
            return Err(Status::invalid_argument("TxFilter hash must be 32 bytes"));
        }

        let mut hash_arr = [0u8; 32];
        hash_arr.copy_from_slice(&req.hash);
        let txid = TxId(hash_arr);

        // Strict RPC Policy: Zero selective fallback. Look up in local storage ONLY.
        let raw_tx = self
            .storage
            .get_full_transaction(&txid)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| {
                Status::not_found(format!(
                    "Transaction {} not found in local verified coverage. Zero-fallback policy active.",
                    txid
                ))
            })?;

        Ok(Response::new(raw_tx))
    }

    async fn send_transaction(&self, _request: Request<RawTransaction>) -> Result<Response<SendResponse>, Status> {
        // Enforce strict MVP policy: deny unshielded transaction broadcasts
        warn!("Blocked SendTransaction call: private receive bridge denies broadcasts in MVP profile");
        Err(Status::permission_denied(
            "Private receive bridge does not support transaction broadcasting in confirmed-receive MVP profile.",
        ))
    }

    async fn get_taddress_txids(
        &self,
        request: Request<TransparentAddressBlockFilter>,
    ) -> Result<Response<Self::GetTaddressTxidsStream>, Status> {
        let req = request.into_inner();
        let addr = TransparentAddress::new(req.address);

        let txs = self
            .storage
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

    async fn get_taddress_transactions(
        &self,
        request: Request<TransparentAddressBlockFilter>,
    ) -> Result<Response<Self::GetTaddressTransactionsStream>, Status> {
        let req = request.into_inner();
        let addr = TransparentAddress::new(req.address);

        let range = match (req.range.as_ref().and_then(|r| r.start.as_ref()), req.range.as_ref().and_then(|r| r.end.as_ref())) {
            (Some(s), Some(e)) => Some(bridge_core::IntervalRange::new(
                BlockHeight(s.height as u32),
                BlockHeight(e.height as u32),
            )),
            _ => None,
        };

        let txs = self
            .storage
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

    async fn get_taddress_balance(&self, request: Request<AddressList>) -> Result<Response<Balance>, Status> {
        let req = request.into_inner();
        let mut total_zat: i64 = 0;

        for addr_str in req.addresses {
            let addr = TransparentAddress::new(addr_str);
            let utxos = self
                .storage
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

        Ok(Response::new(Balance { value_zat: total_zat }))
    }

    async fn get_taddress_balance_stream(
        &self,
        _request: Request<tonic::Streaming<Address>>,
    ) -> Result<Response<Balance>, Status> {
        Err(Status::unimplemented("GetTaddressBalanceStream not implemented"))
    }

    async fn get_mempool_tx(
        &self,
        _request: Request<GetMempoolTxRequest>,
    ) -> Result<Response<Self::GetMempoolTxStream>, Status> {
        // Spec invariant: Keep first profile confirmed-only, with mempool disabled.
        // Do not present a fabricated empty mempool!
        Err(Status::unimplemented(
            "Bridge operates in confirmed-only profile. Mempool streaming is disabled.",
        ))
    }

    async fn get_mempool_stream(
        &self,
        _request: Request<Empty>,
    ) -> Result<Response<Self::GetMempoolStreamStream>, Status> {
        Err(Status::unimplemented(
            "Bridge operates in confirmed-only profile. Mempool streaming is disabled.",
        ))
    }

    async fn get_tree_state(&self, request: Request<BlockId>) -> Result<Response<TreeState>, Status> {
        let req = request.into_inner();
        let height = BlockHeight(req.height as u32);

        let tree_state = self
            .storage
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

    async fn get_latest_tree_state(&self, _request: Request<Empty>) -> Result<Response<TreeState>, Status> {
        let (height, _) = self
            .storage
            .get_latest_block()
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::unavailable("Bridge coverage not yet initialized"))?;

        let tree_state = self
            .storage
            .get_tree_state(height)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("Latest tree state not found"))?;

        Ok(Response::new(tree_state))
    }

    async fn get_subtree_roots(
        &self,
        request: Request<GetSubtreeRootsArg>,
    ) -> Result<Response<Self::GetSubtreeRootsStream>, Status> {
        let req = request.into_inner();
        let roots = self
            .storage
            .get_subtree_roots(req.shielded_protocol, req.start_index as u32, req.max_entries as u32)
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

    async fn get_address_utxos(
        &self,
        request: Request<GetAddressUtxosArg>,
    ) -> Result<Response<GetAddressUtxosReplyList>, Status> {
        let req = request.into_inner();
        let mut all_utxos = Vec::new();

        for addr_str in req.addresses {
            let addr = TransparentAddress::new(addr_str);
            let utxos = self
                .storage
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

    async fn get_address_utxos_stream(
        &self,
        request: Request<GetAddressUtxosArg>,
    ) -> Result<Response<Self::GetAddressUtxosStreamStream>, Status> {
        let req = request.into_inner();
        let mut all_utxos = Vec::new();

        for addr_str in req.addresses {
            let addr = TransparentAddress::new(addr_str);
            let utxos = self
                .storage
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

    async fn get_lightd_info(&self, _request: Request<Empty>) -> Result<Response<LightdInfo>, Status> {
        let meta = self
            .storage
            .get_coverage_metadata()
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .unwrap_or(bridge_core::CoverageMetadata {
                network: bridge_core::Network::Mainnet,
                coverage_start_height: BlockHeight(0),
                committed_height: BlockHeight(0),
                latest_block_hash: bridge_core::BlockHash([0; 32]),
                updated_at: String::new(),
            });

        Ok(Response::new(LightdInfo {
            version: "0.1.0".to_string(),
            vendor: "zcash-private-receive-bridge".to_string(),
            taddr_support: true,
            chain_name: self.network_name.clone(),
            sapling_activation_height: 419200,
            consensus_branch_id: "c2d6d0b4".to_string(),
            block_height: meta.committed_height.0 as u64,
            git_commit: "main".to_string(),
            branch: "main".to_string(),
            build_date: "2026-10-06".to_string(),
            build_user: "bridge".to_string(),
            estimated_height: meta.committed_height.0 as u64,
            zcashd_build: "bridge-local".to_string(),
            zcashd_subversion: "/zcash-private-bridge:0.1.0/".to_string(),
            donation_address: String::new(),
            upgrade_name: String::new(),
            upgrade_height: 0,
            lightwallet_protocol_version: "2.0".to_string(),
        }))
    }

    async fn ping(&self, request: Request<ProtoDuration>) -> Result<Response<PingResponse>, Status> {
        let dur = request.into_inner();
        debug!("Ping received with interval: {} us", dur.interval_us);
        Ok(Response::new(PingResponse {
            entry: 1,
            exit: 0,
        }))
    }
}
