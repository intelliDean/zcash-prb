# Zcash Private Receive Bridge

> An open-source local service that lets supported Zcash wallets synchronize received payments without sending their selected transaction or transparent-address lookups to a remote lightwallet server.

---

## 1. Overview & Privacy Architecture

In the standard Zcash light-client model, trial decryption of compact blocks is private. However, whenever a wallet discovers an incoming shielded payment or syncs transparent address history, it queries the remote `lightwalletd` server for `GetTransaction(txid)` or `GetAddressUtxos(address)`. This directly exposes the user's IP address and transaction graph to the server operator.

The **Zcash Private Receive Bridge** solves this without modifying wallet clients:

1. **Bulk Fixed-Interval Acquisition:** The bridge scheduler fetches public block intervals and downloads **all full transactions** in bulk.
2. **Deterministic Cryptographic Verification:** All transaction IDs are recomputed using consensus hashing rules and verified against block commitments.
3. **Local Serving with Zero Selective Fallback:** The bridge runs a local `CompactTxStreamer` gRPC endpoint (`127.0.0.1:9067`) backed by an embedded SQLite database (WAL mode). Wallets point their custom server setting to `127.0.0.1:9067`. Missing or invalid data causes an explicit error—never a selective upstream fallback.
4. **Zero-Key Invariant:** The daemon never accepts, requests, or stores seeds, spending keys, or viewing keys.

```
Upstream (lightwalletd / Zaino)
       │ (Fixed public intervals: all blocks & full txs)
       ▼
[Interval Acquisition Scheduler]
       │
[Cryptographic TxID & Adjacency Verifier]
       │
[Transparent Spend & Outpoint Indexer]
       │ (Atomic SQLite Commit)
       ▼
[(Local SQLite DB - WAL Mode)]
       │
[Strict RPC Policy Engine] (Confirmed-only, mempool disabled, zero fallback)
       │
[CompactTxStreamer gRPC Server (127.0.0.1:9067)]
       ▲
       │ (Standard custom-server connection)
[Unchanged Client Wallets: Zkool, Zingo, Ledger]
```

---

## 2. Quickstart

### Build Requirements
* Rust 1.85+ (Rust 2024 edition)
* Protobuf compiler (`protoc`)

### Build
```bash
# Debug build
cargo build --workspace

# Release build
cargo build --release
```

### Run Tests
```bash
# Run unit and end-to-end privacy trace tests
cargo test --workspace
```

---

## 3. Configuration

Generate the default configuration file:
```bash
cargo run --bin zcash-private-bridge -- init-config --output config/bridge.toml
```

Example configuration (`config/bridge.toml`):
```toml
# Target network: mainnet, testnet, or regtest
network = "mainnet"

# Upstream public lightwalletd or Zaino provider
upstream_provider = "https://mainnet.lightwalletd.com:9067"

# Local gRPC endpoint for wallets (SECURITY: must be localhost only)
bind_address = "127.0.0.1:9067"

# Height from which verified coverage starts
coverage_start_height = 3000000

# Path to local durable SQLite database (WAL mode)
storage_path = "data/bridge.db"

# Maximum storage budget in Gigabytes (optional)
storage_limit_gb = 10

# Maximum concurrent transaction downloads from upstream
acquisition_concurrency = 8

# Batch size of blocks per verified interval
interval_size = 50

# Request timeout in seconds for upstream gRPC queries
request_timeout_sec = 15
```

---

## 4. Running the Daemon

### Start Bridge
```bash
# Start daemon using configuration file
cargo run --release --bin zcash-private-bridge -- start --config config/bridge.toml
```

### Check Status
```bash
cargo run --bin zcash-private-bridge -- status --config config/bridge.toml
```
Example output:
```
=== Zcash Private Receive Bridge Status ===
Storage Database:     "data/bridge.db"
Network:              mainnet
Upstream Provider:    https://mainnet.lightwalletd.com:9067
Local Bind Address:   127.0.0.1:9067
Coverage Start:       3000000
Committed Height:     3000500
Latest Block Hash:    0000000001a4...
Last Updated:         2026-10-06 02:30:00
Acquisition Failures: 0
```

### Stop Bridge
```bash
cargo run --bin zcash-private-bridge -- stop --config config/bridge.toml
```

### Operating Cost Benchmark
Benchmark network ingress, RPC call counts, and sync times:
```bash
cargo run --bin zcash-private-bridge -- benchmark --blocks 10
```

---

## 5. Security & Verification Documentation

* **[Threat Model & Invariants](docs/THREAT_MODEL.md):** Formal security model, zero-leakage proof, adversary assumptions, and non-goals.
* **[Client Compatibility Matrix](docs/COMPATIBILITY.md):** Pinned client versions (Zkool, Zingo, Ledger) and parity testing protocol.

---

## 5. Wallet Client Setup

Supported unchanged wallets can connect immediately by updating their custom server endpoint:

### Zkool (YWallet Successor)
1. Open **Settings** $\rightarrow$ **Server Settings**.
2. Set Server URL to: `http://127.0.0.1:9067`.
3. Enable confirmed-only synchronization.

### Zingo (`zingolib`)
Launch Zingo with the `--server` flag:
```bash
zingo-cli --server http://127.0.0.1:9067
```

### Ledger Companion Tool
Set the lightwalletd server environment variable or configuration endpoint to `http://127.0.0.1:9067`.

---

## 6. Strict RPC Policy

| RPC Method | Bridge Action | Upstream Leaks |
| :--- | :--- | :--- |
| `GetLatestBlock` | Return highest committed block in local storage | None |
| `GetBlock` | Serve `CompactBlock` from local DB | None |
| `GetBlockRange` | Stream `CompactBlock` range from local DB | None |
| `GetTransaction` | Look up `raw_data` in local DB. **Error if missing (Zero Fallback)** | None |
| `GetTreeState` | Return `TreeState` from local DB | None |
| `GetSubtreeRoots` | Stream subtree roots from local DB | None |
| `GetAddressUtxos` | Query UTXO index. Explicit error if pre-coverage spend exists | None |
| `GetTaddressTransactions` | Query address history from local DB | None |
| `GetMempoolTx` / `Stream` | **Reject with UNIMPLEMENTED** (confirmed-only profile) | None |
| `SendTransaction` | **Deny with PERMISSION_DENIED** (broadcast denied in MVP) | None |

---

## 7. Crate Architecture

* `bridge-core`: Domain models, types (`BlockHeight`, `BlockHash`, `TxId`), configuration, and `Redacted` privacy wrappers.
* `bridge-proto`: Protobuf compilation for `CompactTxStreamer` with `tonic` and `prost`.
* `bridge-storage`: `StorageBackend` trait, schema migrations, and high-performance `SqliteStorage` (WAL mode).
* `bridge-verifier`: Cryptographic BLAKE2b-256 TxID verification and block sequence adjacency validation.
* `bridge-engine`: Public interval scheduler, upstream client, and autonomous worker loop.
* `bridge-server`: Downstream `CompactTxStreamer` gRPC server enforcing the strict RPC policy.
* `bridge-cli`: `zcash-private-bridge` command-line binary (`start`, `status`, `stop`, `init-config`).
* `bridge-testkit`: Test fixtures, mock upstream server, and automated privacy trace verification suite.

---

## 8. License

Licensed under either of:
* Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
* MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
