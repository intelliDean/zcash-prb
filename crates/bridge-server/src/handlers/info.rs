use bridge_core::BlockHeight;
use bridge_proto::{Duration as ProtoDuration, LightdInfo, PingResponse};
use bridge_storage::StorageBackend;
use std::sync::Arc;
use tonic::{Request, Response, Status};
use tracing::debug;

pub struct ConsensusBranchInfo {
    pub branch_id: &'static str,
    pub upgrade_name: &'static str,
    pub activation_height: u64,
}

pub fn get_consensus_info(network_name: &str, height: u64) -> ConsensusBranchInfo {
    match network_name {
        "testnet" => {
            if height >= 2975000 {
                ConsensusBranchInfo {
                    branch_id: "c8e71050",
                    upgrade_name: "NU6",
                    activation_height: 2975000,
                }
            } else if height >= 1842420 {
                ConsensusBranchInfo {
                    branch_id: "c2d6d0b4",
                    upgrade_name: "NU5",
                    activation_height: 1842420,
                }
            } else if height >= 1028500 {
                ConsensusBranchInfo {
                    branch_id: "e9ff75a6",
                    upgrade_name: "Canopy",
                    activation_height: 1028500,
                }
            } else if height >= 903000 {
                ConsensusBranchInfo {
                    branch_id: "f5b9230b",
                    upgrade_name: "Heartwood",
                    activation_height: 903000,
                }
            } else if height >= 584000 {
                ConsensusBranchInfo {
                    branch_id: "2bb40e60",
                    upgrade_name: "Blossom",
                    activation_height: 584000,
                }
            } else if height >= 280000 {
                ConsensusBranchInfo {
                    branch_id: "76b809bb",
                    upgrade_name: "Sapling",
                    activation_height: 280000,
                }
            } else if height >= 207500 {
                ConsensusBranchInfo {
                    branch_id: "5ba81b19",
                    upgrade_name: "Overwinter",
                    activation_height: 207500,
                }
            } else {
                ConsensusBranchInfo {
                    branch_id: "00000000",
                    upgrade_name: "Sprout",
                    activation_height: 0,
                }
            }
        }
        _ => {
            // Mainnet (and default for others like regtest)
            if height >= 2726400 {
                ConsensusBranchInfo {
                    branch_id: "c8e71050",
                    upgrade_name: "NU6",
                    activation_height: 2726400,
                }
            } else if height >= 1687106 {
                ConsensusBranchInfo {
                    branch_id: "c2d6d0b4",
                    upgrade_name: "NU5",
                    activation_height: 1687106,
                }
            } else if height >= 1046400 {
                ConsensusBranchInfo {
                    branch_id: "e9ff75a6",
                    upgrade_name: "Canopy",
                    activation_height: 1046400,
                }
            } else if height >= 903800 {
                ConsensusBranchInfo {
                    branch_id: "f5b9230b",
                    upgrade_name: "Heartwood",
                    activation_height: 903800,
                }
            } else if height >= 653600 {
                ConsensusBranchInfo {
                    branch_id: "2bb40e60",
                    upgrade_name: "Blossom",
                    activation_height: 653600,
                }
            } else if height >= 419200 {
                ConsensusBranchInfo {
                    branch_id: "76b809bb",
                    upgrade_name: "Sapling",
                    activation_height: 419200,
                }
            } else if height >= 347500 {
                ConsensusBranchInfo {
                    branch_id: "5ba81b19",
                    upgrade_name: "Overwinter",
                    activation_height: 347500,
                }
            } else {
                ConsensusBranchInfo {
                    branch_id: "00000000",
                    upgrade_name: "Sprout",
                    activation_height: 0,
                }
            }
        }
    }
}

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

    let current_height = if meta.committed_height.0 > 0 {
        meta.committed_height.0 as u64
    } else {
        meta.coverage_start_height.0 as u64
    };

    let consensus_info = get_consensus_info(network_name, current_height);

    Ok(Response::new(LightdInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
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
        consensus_branch_id: consensus_info.branch_id.to_string(),
        block_height: meta.committed_height.0 as u64,
        git_commit: option_env!("GIT_COMMIT").unwrap_or("release").to_string(),
        branch: "main".to_string(),
        build_date: option_env!("BUILD_DATE").unwrap_or("2026-10").to_string(),
        build_user: "bridge".to_string(),
        estimated_height: meta.committed_height.0 as u64,
        zcashd_build: "bridge-local".to_string(),
        zcashd_subversion: format!("/zcash-private-bridge:{}/", env!("CARGO_PKG_VERSION")),
        donation_address: String::new(),
        upgrade_name: consensus_info.upgrade_name.to_string(),
        upgrade_height: consensus_info.activation_height,
        lightwallet_protocol_version: "2.0".to_string(),
    }))
}

pub fn ping(request: Request<ProtoDuration>) -> Result<Response<PingResponse>, Status> {
    let dur = request.into_inner();
    debug!("Ping received with interval: {} us", dur.interval_us);
    Ok(Response::new(PingResponse { entry: 1, exit: 0 }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bridge_storage::SqliteStorage;

    #[test]
    fn test_mainnet_consensus_branch_ids() {
        assert_eq!(get_consensus_info("mainnet", 0).branch_id, "00000000");
        assert_eq!(get_consensus_info("mainnet", 347499).branch_id, "00000000");
        assert_eq!(get_consensus_info("mainnet", 347500).branch_id, "5ba81b19");
        assert_eq!(get_consensus_info("mainnet", 419200).branch_id, "76b809bb");
        assert_eq!(get_consensus_info("mainnet", 653600).branch_id, "2bb40e60");
        assert_eq!(get_consensus_info("mainnet", 903800).branch_id, "f5b9230b");
        assert_eq!(get_consensus_info("mainnet", 1046400).branch_id, "e9ff75a6");
        assert_eq!(get_consensus_info("mainnet", 1687106).branch_id, "c2d6d0b4");
        assert_eq!(get_consensus_info("mainnet", 2500000).branch_id, "c2d6d0b4");
        assert_eq!(get_consensus_info("mainnet", 2500000).upgrade_name, "NU5");
        assert_eq!(get_consensus_info("mainnet", 2726400).branch_id, "c8e71050");
        assert_eq!(get_consensus_info("mainnet", 2726400).upgrade_name, "NU6");
    }

    #[test]
    fn test_testnet_consensus_branch_ids() {
        assert_eq!(get_consensus_info("testnet", 0).branch_id, "00000000");
        assert_eq!(get_consensus_info("testnet", 207500).branch_id, "5ba81b19");
        assert_eq!(get_consensus_info("testnet", 280000).branch_id, "76b809bb");
        assert_eq!(get_consensus_info("testnet", 584000).branch_id, "2bb40e60");
        assert_eq!(get_consensus_info("testnet", 903000).branch_id, "f5b9230b");
        assert_eq!(get_consensus_info("testnet", 1028500).branch_id, "e9ff75a6");
        assert_eq!(get_consensus_info("testnet", 1842420).branch_id, "c2d6d0b4");
        assert_eq!(get_consensus_info("testnet", 2975000).branch_id, "c8e71050");
    }

    #[tokio::test]
    async fn test_get_lightd_info_dynamic_metadata() {
        let storage = Arc::new(SqliteStorage::in_memory().unwrap());
        storage
            .init_coverage(bridge_core::Network::Mainnet, BlockHeight(2500000))
            .await
            .unwrap();

        let resp = get_lightd_info(&(storage as Arc<dyn StorageBackend>), "mainnet")
            .await
            .unwrap()
            .into_inner();

        assert_eq!(resp.consensus_branch_id, "c2d6d0b4");
        assert_eq!(resp.upgrade_name, "NU5");
        assert_eq!(resp.upgrade_height, 1687106);
        assert_eq!(resp.chain_name, "main");
        assert_eq!(resp.sapling_activation_height, 419200);
        assert!(resp.taddr_support);
    }
}
