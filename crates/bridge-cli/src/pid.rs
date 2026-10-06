use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tracing::info;

pub struct PidFile {
    path: PathBuf,
}

impl PidFile {
    pub fn from_storage_path(storage_path: &Path) -> Self {
        Self {
            path: storage_path.with_extension("pid"),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn write_current(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let current_pid = std::process::id();
        std::fs::write(&self.path, current_pid.to_string())
            .with_context(|| format!("Failed to write PID file {:?}", self.path))?;
        Ok(())
    }

    pub fn read_pid(&self) -> Result<Option<i32>> {
        if !self.path.exists() {
            return Ok(None);
        }
        let content = std::fs::read_to_string(&self.path)
            .with_context(|| format!("Failed to read PID file {:?}", self.path))?;
        let pid: i32 = content
            .trim()
            .parse()
            .with_context(|| format!("Invalid PID format in {:?}", self.path))?;
        Ok(Some(pid))
    }

    pub fn terminate_process(&self) -> Result<bool> {
        let pid_opt = self.read_pid()?;
        match pid_opt {
            Some(pid) => {
                info!("Sending SIGTERM to bridge daemon process (PID: {})...", pid);
                let status = std::process::Command::new("kill")
                    .arg("-15")
                    .arg(pid.to_string())
                    .status();

                self.clean();

                match status {
                    Ok(s) if s.success() => Ok(true),
                    _ => Ok(false),
                }
            }
            None => Ok(false),
        }
    }

    pub fn clean(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
