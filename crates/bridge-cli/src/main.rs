mod commands;
mod pid;

use bridge_core::BridgeConfig;
use clap::{Parser, Subcommand};
use commands::{run_benchmark, run_init_config, run_start, run_status, run_stop, StartArgs};
use std::path::PathBuf;

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
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let cli = Cli::parse();

    let config = match &cli.config {
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
        Commands::InitConfig { output } => run_init_config(&output),
        Commands::Status => run_status(&config).await,
        Commands::Stop => run_stop(&config),
        Commands::Benchmark { blocks, provider } => run_benchmark(&config, blocks, provider).await,
        Commands::Start {
            bind,
            provider,
            coverage_start,
            network,
        } => {
            run_start(
                config,
                StartArgs {
                    bind,
                    provider,
                    coverage_start,
                    network,
                },
            )
            .await
        }
    }
}
