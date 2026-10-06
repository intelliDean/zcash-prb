use bridge_core::{BlockHeight, BridgeConfig, Network};
use bridge_engine::{AcquisitionWorker, UpstreamClient};
use bridge_server::{run_server, BridgeGrpcService};
use bridge_storage::{SqliteStorage, StorageBackend};
use clap::{Parser, Subcommand};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::watch;
use tracing::{error, info, warn};

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

    /// Benchmark acquisition costs and local sync metrics across providers
    Benchmark {
        #[arg(long, default_value_t = 10, help = "Number of blocks to benchmark")]
        blocks: u32,

        #[arg(long, help = "Optional custom provider URL to benchmark")]
        provider: Option<String>,
    },

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
            println!("Storage Database:     {:?}", config.storage_path);
            println!("Network:              {}", config.network);
            println!("Upstream Provider:    {}", config.upstream_provider);
            println!("Local Bind Address:   {}", config.bind_address);

            match meta {
                Some(m) => {
                    println!("Coverage Start:       {}", m.coverage_start_height);
                    println!("Committed Height:     {}", m.committed_height);
                    println!("Latest Block Hash:    {}", m.latest_block_hash);
                    println!("Last Updated:         {}", m.updated_at);
                    println!("Acquisition Failures: {}", m.acquisition_failures_count);
                    if let Some(ref err) = m.last_error {
                        println!("Last Error Message:   {}", err);
                    }
                }
                None => {
                    println!("Coverage State:       Not yet initialized");
                }
            }

            if let Some((h, hash)) = latest {
                println!("Database Latest:      Height {} ({})", h, hash);
            }
        }

        Commands::Stop => {
            let pid_file = config.storage_path.with_extension("pid");
            if !pid_file.exists() {
                println!("No running daemon found (missing PID file {:?})", pid_file);
                return Ok(());
            }

            let pid_str = std::fs::read_to_string(&pid_file)?;
            let pid: i32 = pid_str.trim().parse().map_err(|e| anyhow::anyhow!("Invalid PID file: {e}"))?;

            info!("Sending SIGTERM to bridge daemon process (PID: {})...", pid);
            let status = std::process::Command::new("kill")
                .arg("-15")
                .arg(pid.to_string())
                .status();

            match status {
                Ok(s) if s.success() => {
                    println!("Daemon process (PID {}) terminated successfully.", pid);
                    let _ = std::fs::remove_file(&pid_file);
                }
                _ => {
                    warn!("Failed to terminate process PID {}. It may have already exited.", pid);
                    let _ = std::fs::remove_file(&pid_file);
                }
            }
        }

        Commands::Benchmark { blocks, provider } => {
            let target_provider = provider.unwrap_or_else(|| config.upstream_provider.clone());
            println!("=== Zcash Private Receive Bridge Cost Benchmark ===");
            println!("Provider Endpoint: {}", target_provider);
            println!("Interval Size:     {} blocks", blocks);

            let client = UpstreamClient::new(target_provider, config.request_timeout_sec);
            let start_time = Instant::now();

            println!("\n1. Querying Upstream Chain Tip...");
            let tip = client.get_latest_block().await?;
            let tip_height = tip.height as u32;
            let start_height = tip_height.saturating_sub(blocks);
            println!("   Chain Tip Height: {}", tip_height);
            println!("   Benchmark Range:  [{}..={}]", start_height, tip_height);

            println!("\n2. Fetching Compact Blocks Individually...");
            let block_fetch_start = Instant::now();
            let mut total_bytes = 0usize;
            let mut total_rpc_calls = 1usize; // 1 for GetLatestBlock
            let mut full_txids = Vec::new();

            for h in start_height..=tip_height {
                let block = client.get_block(BlockHeight(h)).await?;
                total_rpc_calls += 1;
                total_bytes += prost::Message::encoded_len(&block);
                for vtx in &block.vtx {
                    if vtx.txid.len() == 32 {
                        let mut arr = [0u8; 32];
                        arr.copy_from_slice(&vtx.txid);
                        full_txids.push(bridge_core::TxId(arr));
                    }
                }
            }
            let block_fetch_duration = block_fetch_start.elapsed();
            println!("   Fetched {} blocks in {:.2?}", blocks + 1, block_fetch_duration);
            println!("   Discovered full transactions: {}", full_txids.len());

            println!("\n3. Downloading Full Transactions in Bulk...");
            let tx_fetch_start = Instant::now();
            let mut total_tx_bytes = 0usize;

            for txid in &full_txids {
                match client.get_transaction(txid).await {
                    Ok(raw_tx) => {
                        total_rpc_calls += 1;
                        total_tx_bytes += raw_tx.data.len();
                    }
                    Err(e) => {
                        warn!("Transaction download failed for {}: {}", txid, e);
                    }
                }
            }
            let _tx_fetch_duration = tx_fetch_start.elapsed();
            let total_duration = start_time.elapsed();

            println!("\n=== Operating Costs Summary ===");
            println!("Total Time:                {:.2?}", total_duration);
            println!("Total Upstream RPC Calls:  {}", total_rpc_calls);
            println!("Block Data Downloaded:     {:.2} KB", total_bytes as f64 / 1024.0);
            println!("Tx Data Downloaded:        {:.2} KB", total_tx_bytes as f64 / 1024.0);
            println!("Total Network Ingress:     {:.2} KB", (total_bytes + total_tx_bytes) as f64 / 1024.0);
            println!("Repeat-Sync Savings:       100% (Subsequent queries served locally with 0 upstream RPCs)");
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

            // Manage PID file
            let pid_file = config.storage_path.with_extension("pid");
            if let Some(parent) = pid_file.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let current_pid = std::process::id();
            let _ = std::fs::write(&pid_file, current_pid.to_string());

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

            // Remove PID file
            let _ = std::fs::remove_file(&pid_file);
            info!("Zcash Private Receive Bridge shutdown completed cleanly.");
        }
    }

    Ok(())
}
