use anyhow::Result;
use bridge_core::BridgeConfig;
use bridge_storage::{SqliteStorage, StorageBackend};

pub async fn run_status(config: &BridgeConfig) -> Result<()> {
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

    Ok(())
}
