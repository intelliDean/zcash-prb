use anyhow::Result;
use bridge_core::BridgeConfig;
use crate::pid::PidFile;
use tracing::warn;

pub fn run_stop(config: &BridgeConfig) -> Result<()> {
    let pid_file = PidFile::from_storage_path(&config.storage_path);
    if !pid_file.path().exists() {
        println!("No running daemon found (missing PID file {:?})", pid_file.path());
        return Ok(());
    }

    match pid_file.terminate_process()? {
        true => println!("Daemon process terminated successfully."),
        false => {
            warn!("Failed to cleanly terminate daemon. It may have already exited.");
            println!("Daemon stop signal dispatched, cleaned PID file.");
        }
    }

    Ok(())
}
