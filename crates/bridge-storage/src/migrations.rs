use rusqlite::Connection;

pub const SCHEMA_SQL: &str = r#"
-- Enable WAL mode for high concurrent read performance
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

-- 1. Metadata and Coverage Tracking
CREATE TABLE IF NOT EXISTS coverage_metadata (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    network TEXT NOT NULL,
    coverage_start_height INTEGER NOT NULL,
    committed_height INTEGER NOT NULL,
    latest_block_hash BLOB NOT NULL,
    updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
    acquisition_failures_count INTEGER NOT NULL DEFAULT 0,
    last_error TEXT NULL
);

-- 2. Compact Blocks
CREATE TABLE IF NOT EXISTS compact_blocks (
    height INTEGER PRIMARY KEY,
    block_hash BLOB NOT NULL UNIQUE,
    prev_hash BLOB NOT NULL,
    time INTEGER NOT NULL,
    header BLOB NOT NULL,
    compact_block_proto BLOB NOT NULL
);

-- 3. Full Transactions
CREATE TABLE IF NOT EXISTS full_transactions (
    txid BLOB PRIMARY KEY,
    height INTEGER NOT NULL,
    block_time INTEGER NOT NULL,
    raw_data BLOB NOT NULL,
    FOREIGN KEY(height) REFERENCES compact_blocks(height) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_tx_height ON full_transactions(height);

-- 4. Transparent Outputs (UTXOs and Historical Spends)
CREATE TABLE IF NOT EXISTS transparent_outputs (
    txid BLOB NOT NULL,
    vout INTEGER NOT NULL,
    address TEXT NOT NULL,
    value_zat INTEGER NOT NULL,
    script_pubkey BLOB NOT NULL,
    height INTEGER NOT NULL,
    spent_by_txid BLOB NULL,
    spent_at_height INTEGER NULL,
    PRIMARY KEY (txid, vout),
    FOREIGN KEY(txid) REFERENCES full_transactions(txid) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_t_address ON transparent_outputs(address);
CREATE INDEX IF NOT EXISTS idx_t_spent ON transparent_outputs(spent_by_txid);

-- 5. Outpoints spent whose creation was before coverage_start_height
CREATE TABLE IF NOT EXISTS pre_coverage_spends (
    spending_txid BLOB NOT NULL,
    spending_height INTEGER NOT NULL,
    prev_txid BLOB NOT NULL,
    prev_vout INTEGER NOT NULL,
    address TEXT NULL,
    PRIMARY KEY (spending_txid, prev_txid, prev_vout)
);
CREATE INDEX IF NOT EXISTS idx_pre_coverage_addr ON pre_coverage_spends(address);

-- 6. Tree States and Subtrees
CREATE TABLE IF NOT EXISTS tree_states (
    height INTEGER PRIMARY KEY,
    block_hash BLOB NOT NULL,
    sapling_tree_hex TEXT NOT NULL,
    orchard_tree_hex TEXT NOT NULL,
    tree_state_proto BLOB NOT NULL,
    FOREIGN KEY(height) REFERENCES compact_blocks(height) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS subtree_roots (
    pool INTEGER NOT NULL,
    subtree_index INTEGER NOT NULL,
    root_hash BLOB NOT NULL,
    completing_height INTEGER NOT NULL,
    PRIMARY KEY(pool, subtree_index)
);
"#;

pub fn apply_migrations(conn: &mut Connection) -> Result<(), rusqlite::Error> {
    conn.execute_batch(SCHEMA_SQL)?;

    // Backward-compatible schema evolution for existing databases
    let _ = conn.execute(
        "ALTER TABLE coverage_metadata ADD COLUMN acquisition_failures_count INTEGER NOT NULL DEFAULT 0",
        [],
    );
    let _ = conn.execute(
        "ALTER TABLE coverage_metadata ADD COLUMN last_error TEXT NULL",
        [],
    );

    Ok(())
}
