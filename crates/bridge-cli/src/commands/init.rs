use anyhow::Result;
use bridge_core::BridgeConfig;
use std::path::Path;
use tracing::info;

pub fn run_init_config(output: &Path) -> Result<()> {
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let toml_str = toml::to_string_pretty(&BridgeConfig::default())?;
    std::fs::write(output, toml_str)?;
    info!("Default configuration created at {:?}", output);
    println!("Created default configuration at {:?}", output);
    Ok(())
}
