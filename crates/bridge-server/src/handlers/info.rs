use bridge_core::BlockHeight;
use bridge_proto::{Duration as ProtoDuration, LightdInfo, PingResponse};
use bridge_storage::StorageBackend;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::debug;

pub async fn get_lightd_info(
    storage: &Arc<dyn StorageBackend>,
    network_name: &str,
) -> Result<Response<LightdInfo>, Status> {
    let meta = storage
        .get_coverage_metadata()
        .await
        .map_err(|e| Status::internal(e.to_string()))?
        .unwrap_or(bridge_core::CoverageMetadata {
            network: bridge_core::Network::Mainnet,
            coverage_start_height: BlockHeight(0),
            committed_height: BlockHeight(0),
            latest_block_hash: bridge_core::BlockHash([0; 32]),
            updated_at: String::new(),
            acquisition_failures_count: 0,
            last_error: None,
        });

    Ok(Response::new(LightdInfo {
        version: "0.1.0".to_string(),
        vendor: "zcash-private-receive-bridge".to_string(),
        taddr_support: true,
        chain_name: match network_name {
            "mainnet" => "main".to_string(),
            "testnet" => "test".to_string(),
            "regtest" => "regtest".to_string(),
            other => other.to_string(),
        },
        sapling_activation_height: match network_name {
            "testnet" => 280000,
            _ => 419200,
        },
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

pub fn ping(request: Request<ProtoDuration>) -> Result<Response<PingResponse>, Status> {
    let dur = request.into_inner();
    debug!("Ping received with interval: {} us", dur.interval_us);
    Ok(Response::new(PingResponse { entry: 1, exit: 0 }))
}
