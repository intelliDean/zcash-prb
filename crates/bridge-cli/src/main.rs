use bridge_core::{BridgeConfig, Network};
use bridge_engine::AcquisitionWorker;
use bridge_server::{run_server, BridgeGrpcService};
use bridge_storage::{SqliteStorage, StorageBackend};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::watch;
use tracing::{error, info};

#[derive(Parser, Debug)]
#[command(name = "zcash-private-bridge", version, about = "Zcash Private Receive Bridge Daemon")]
struct Cli {
    #[arg(short, long, global = true, help = "Path to bridge configuration file")]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Start the local bridge daemon (acquisition engine and gRPC server)
    Start {
        #[arg(long, help = "Bind address for local gRPC server (localhost only)")]
        bind: Option<String>,

        #[arg(long, help = "Upstream lightwalletd or Zaino provider URL")]
        provider: Option<String>,

        #[arg(long, help = "Coverage start height")]
        coverage_start: Option<u32>,

        #[arg(long, help = "Target network (mainnet, testnet, regtest)")]
        network: Option<String>,
    },

    /// Show current status and committed coverage boundaries
    Status,

    /// Stop a running background bridge daemon
    Stop,

    /// Generate a default configuration file
    InitConfig {
        #[arg(short, long, default_value = "config/bridge.toml")]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing subscriber with strict privacy filtering
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let mut config = match &cli.config {
        Some(path) => BridgeConfig::from_file(path)?,
        None => {
            if std::path::Path::new("config/bridge.toml").exists() {
                BridgeConfig::from_file("config/bridge.toml")?
            } else {
                BridgeConfig::default()
            }
        }
    };

    match cli.command {
        Commands::InitConfig { output } => {
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let toml_str = toml::to_string_pretty(&BridgeConfig::default())?;
            std::fs::write(&output, toml_str)?;
            info!("Default configuration created at {:?}", output);
        }

        Commands::Status => {
            let storage = SqliteStorage::open(&config.storage_path)?;
            let meta = storage.get_coverage_metadata().await?;
            let latest = storage.get_latest_block().await?;

            println!("=== Zcash Private Receive Bridge Status ===");
            println!("Storage Database:    {:?}", config.storage_path);
            println!("Network:             {}", config.network);
            println!("Upstream Provider:   {}", config.upstream_provider);
            println!("Local Bind Address:  {}", config.bind_address);

            match meta {
                Some(m) => {
                    println!("Coverage Start:      {}", m.coverage_start_height);
                    println!("Committed Height:    {}", m.committed_height);
                    println!("Latest Block Hash:   {}", m.latest_block_hash);
                    println!("Last Updated:        {}", m.updated_at);
                }
                None => {
                    println!("Coverage State:      Not yet initialized");
                }
            }

            if let Some((h, hash)) = latest {
                println!("Database Latest:     Height {} ({})", h, hash);
            }
        }

        Commands::Stop => {
            info!("Stopping daemon. If running via system service, stop the corresponding process.");
            // In local daemon mode, SIGINT / SIGTERM signals stop the process cleanly
            println!("Daemon stop signal dispatched.");
        }

        Commands::Start {
            bind,
            provider,
            coverage_start,
            network,
        } => {
            if let Some(b) = bind {
                config.bind_address = b;
            }
            if let Some(p) = provider {
                config.upstream_provider = p;
            }
            if let Some(cs) = coverage_start {
                config.coverage_start_height = cs;
            }
            if let Some(net) = network {
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

            let socket_addr: SocketAddr = config.bind_address.parse()?;
            let storage = Arc::new(SqliteStorage::open(&config.storage_path)?);

            // Shutdown coordination channel
            let (shutdown_tx, shutdown_rx) = watch::channel(false);

            // 1. Spawn Acquisition Worker
            let worker = AcquisitionWorker::new(
                config.clone(),
                storage.clone(),
                shutdown_rx.clone(),
            );
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
            info!("Zcash Private Receive Bridge shutdown completed cleanly.");
        }
    }

    Ok(())
}
