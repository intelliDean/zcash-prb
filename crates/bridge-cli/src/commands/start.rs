use crate::pid::PidFile;
use anyhow::Result;
use bridge_core::{BridgeConfig, Network};
use bridge_engine::AcquisitionWorker;
use bridge_server::{BridgeGrpcService, run_server};
use bridge_storage::SqliteStorage;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info};

pub struct StartArgs {
    pub bind: Option<String>,
    pub provider: Option<String>,
    pub coverage_start: Option<u32>,
    pub network: Option<String>,
}

pub async fn run_start(mut config: BridgeConfig, args: StartArgs) -> Result<()> {
    if let Some(b) = args.bind {
        config.bind_address = b;
    }
    if let Some(p) = args.provider {
        config.upstream_provider = p;
    }
    if let Some(cs) = args.coverage_start {
        config.coverage_start_height = cs;
    }
    if let Some(net) = args.network {
        config.network = match net.to_lowercase().as_str() {
            "testnet" => Network::Testnet,
            "regtest" => Network::Regtest,
            _ => Network::Mainnet,
        };
    }

    // Enforce localhost-only security invariant
    config.validate()?;

    info!(
        "Starting Zcash Private Receive Bridge on {} (Network: {})",
        config.bind_address, config.network
    );

    // Manage PID file
    let pid_file = PidFile::from_storage_path(&config.storage_path);
    pid_file.write_current()?;

    let socket_addr: SocketAddr = config.bind_address.parse()?;
    let storage = Arc::new(SqliteStorage::open(&config.storage_path)?);

    // Shutdown coordination channel
    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // 1. Spawn Acquisition Worker
    let worker = AcquisitionWorker::new(config.clone(), storage.clone(), shutdown_rx.clone());
    let worker_handle = tokio::spawn(async move {
        worker.run().await;
    });

    // 2. Spawn local gRPC server
    let service = BridgeGrpcService::new(storage.clone(), config.network.to_string());
    let server_handle = tokio::spawn(async move {
        if let Err(e) = run_server(socket_addr, service, shutdown_rx).await {
            error!("gRPC server failed: {e}");
        }
    });

    // 3. Wait for Ctrl+C signal
    tokio::signal::ctrl_c().await?;
    info!("Shutdown signal received (Ctrl+C). Initiating graceful shutdown...");
    let _ = shutdown_tx.send(true);

    // Wait for tasks to terminate cleanly
    let _ = tokio::join!(worker_handle, server_handle);

    // Remove PID file
    pid_file.clean();
    info!("Zcash Private Receive Bridge shutdown completed cleanly.");

    Ok(())
}
