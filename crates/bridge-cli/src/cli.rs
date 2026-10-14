use crate::commands::{
    StartArgs, run_benchmark, run_init_config, run_probe, run_start, run_status, run_stop,
};
use bridge_core::BridgeConfig;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};

#[derive(Parser, Debug)]
#[command(
    name = "zcash-private-bridge",
    version,
    about = "Zcash Private Receive Bridge Daemon"
)]
pub struct Cli {
    #[arg(short, long, global = true, help = "Path to bridge configuration file")]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
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

    /// Exercise a running bridge with wallet-shaped RPCs and failure paths
    Probe {
        #[arg(long, help = "Bridge gRPC endpoint (default: http://<bind_address>)")]
        endpoint: Option<String>,
    },

    /// Perform a full wallet scan, discover received notes, decrypt memos, and export wallet state
    WalletScan {
        #[arg(long, help = "gRPC endpoint to scan from (default: local bridge)")]
        endpoint: Option<String>,

        #[arg(long, help = "Watch-only viewing key string")]
        viewing_key: Option<String>,

        #[arg(long, default_value_t = 2500000, help = "Account birthday height")]
        birthday: u64,

        #[arg(long, help = "Optional scan end height")]
        scan_end: Option<u64>,

        #[arg(long, help = "Path to export SQLite wallet database")]
        export_db: Option<PathBuf>,

        #[arg(long, help = "Path to export structured JSON wallet state snapshot")]
        export_json: Option<PathBuf>,

        #[arg(long, help = "Path to export SQL table dump")]
        export_sql: Option<PathBuf>,
    },
}

impl Cli {
    /// Resolves the bridge configuration from the CLI flag or default paths.
    pub fn resolve_config(&self) -> anyhow::Result<BridgeConfig> {
        let config = match &self.config {
            Some(path) => BridgeConfig::from_file(path)?,
            None => {
                if Path::new("config/bridge.toml").exists() {
                    BridgeConfig::from_file("config/bridge.toml")?
                } else {
                    BridgeConfig::default()
                }
            }
        };
        Ok(config)
    }

    /// Dispatches the parsed CLI command to its dedicated command handler.
    pub async fn run(self) -> anyhow::Result<()> {
        let config = self.resolve_config()?;

        match self.command {
            Commands::InitConfig { output } => run_init_config(&output),
            Commands::Status => run_status(&config).await,
            Commands::Stop => run_stop(&config),
            Commands::Probe { endpoint } => run_probe(&config, endpoint).await,
            Commands::WalletScan {
                endpoint,
                viewing_key,
                birthday,
                scan_end,
                export_db,
                export_json,
                export_sql,
            } => {
                crate::commands::run_wallet_scan(
                    &config,
                    crate::commands::WalletScanArgs {
                        endpoint,
                        viewing_key,
                        birthday,
                        scan_end,
                        export_db,
                        export_json,
                        export_sql,
                    },
                )
                .await
            }
            Commands::Benchmark { blocks, provider } => {
                run_benchmark(&config, blocks, provider).await
            }
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
}
