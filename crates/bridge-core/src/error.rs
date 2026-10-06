use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Verification error: {0}")]
    Verification(String),

    #[error("Upstream network error: {0}")]
    Upstream(String),

    #[error("Policy violation: {0}")]
    Policy(String),

    #[error("Chain reorganization detected at height {height}: expected prev_hash {expected}, got {actual}")]
    ReorgDetected { height: u32, expected: String, actual: String },

    #[error("Pre-coverage history error: transparent output ({txid}:{vout}) was created before coverage start {coverage_start}")]
    IncompleteHistory { txid: String, vout: u32, coverage_start: u32 },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
