use serde::{Deserialize, Serialize};
use std::fmt;

/// Block height represented as a type-safe 32-bit unsigned integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct BlockHeight(pub u32);

impl fmt::Display for BlockHeight {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u32> for BlockHeight {
    fn from(val: u32) -> Self {
        Self(val)
    }
}

impl From<u64> for BlockHeight {
    fn from(val: u64) -> Self {
        Self(val as u32)
    }
}

impl From<BlockHeight> for u32 {
    fn from(h: BlockHeight) -> Self {
        h.0
    }
}

impl From<BlockHeight> for u64 {
    fn from(h: BlockHeight) -> Self {
        h.0 as u64
    }
}

/// 32-byte block hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlockHash(pub [u8; 32]);

impl BlockHash {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(s, &mut bytes)?;
        Ok(Self(bytes))
    }
}

impl fmt::Debug for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BlockHash({})", self.to_hex())
    }
}

impl fmt::Display for BlockHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// 32-byte Transaction ID (TxID).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TxId(pub [u8; 32]);

impl TxId {
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    pub fn from_hex(s: &str) -> Result<Self, hex::FromHexError> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(s, &mut bytes)?;
        Ok(Self(bytes))
    }
}

impl fmt::Debug for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TxId({})", self.to_hex())
    }
}

impl fmt::Display for TxId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Transparent Zcash address (e.g. t1..., t3...).
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransparentAddress(pub String);

impl TransparentAddress {
    pub fn new(addr: impl Into<String>) -> Self {
        Self(addr.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for TransparentAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Redacted by default to prevent address leaks
        write!(f, "TransparentAddress([REDACTED])")
    }
}

impl fmt::Display for TransparentAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Contiguous block interval [start, end] inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IntervalRange {
    pub start: BlockHeight,
    pub end: BlockHeight,
}

impl IntervalRange {
    pub fn new(start: BlockHeight, end: BlockHeight) -> Self {
        Self { start, end }
    }

    pub fn len(&self) -> u32 {
        if self.end >= self.start {
            self.end.0 - self.start.0 + 1
        } else {
            0
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Network specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Network {
    Mainnet,
    Testnet,
    Regtest,
}

impl fmt::Display for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Mainnet => write!(f, "mainnet"),
            Self::Testnet => write!(f, "testnet"),
            Self::Regtest => write!(f, "regtest"),
        }
    }
}

/// Committed coverage boundaries and checkpoints.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageMetadata {
    pub network: Network,
    pub coverage_start_height: BlockHeight,
    pub committed_height: BlockHeight,
    pub latest_block_hash: BlockHash,
    pub updated_at: String,
    pub acquisition_failures_count: u64,
    pub last_error: Option<String>,
}
