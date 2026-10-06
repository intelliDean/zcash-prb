# Zcash Private Receive Bridge

[![CI](https://github.com/intelliDean/zcash-prb/actions/workflows/ci.yml/badge.svg)](https://github.com/intelliDean/zcash-prb/actions/workflows/ci.yml)
[![License: MIT / Apache-2.0](https://img.shields.io/badge/license-MIT%20%2F%20Apache--2.0-blue.svg)](LICENSE-MIT)
[![Rust: 2024 Edition](https://img.shields.io/badge/rust-2024%20edition-orange.svg)](https://www.rust-lang.org)
[![Platform: Linux / macOS](https://img.shields.io/badge/platform-linux%20%7C%20macos-lightgrey.svg)](https://github.com/intelliDean/zcash-prb)

> **Zcash Private Receive Bridge** is an open-source, local background service that allows supported, unmodified Zcash light wallets to synchronize confirmed received payments with **zero selective transaction, address, or metadata leakage** to remote servers.

---

## Table of Contents
1. [The Problem: Light Client Privacy Leakage](#1-the-problem-light-client-privacy-leakage)
2. [How the Bridge Solves It](#2-how-the-bridge-solves-it)
3. [Architecture & System Flow](#3-architecture--system-flow)
4. [The 5 Core Security Invariants](#4-the-5-core-security-invariants)
5. [Multi-Pool & Protocol Coverage](#5-multi-pool--protocol-coverage)
6. [Quickstart & Installation](#6-quickstart--installation)
7. [Running the Daemon](#7-running-the-daemon)
8. [Connecting Unchanged Wallets](#8-connecting-unchanged-wallets)
9. [Strict RPC Policy](#9-strict-rpc-policy)
10. [Automated Proofs & Test Suites](#10-automated-proofs--test-suites)
11. [Performance & Operating Cost Benchmarks](#11-performance--operating-cost-benchmarks)
12. [Crate Architecture](#12-crate-architecture)
13. [Limitations & Non-Goals](#13-limitations--non-goals)
14. [License](#14-license)

---

## 1. The Problem: Light Client Privacy Leakage

In the standard Zcash light-client model, trial decryption of compact shielded blocks is performed locally on the user's device. However, as soon as a wallet detects an incoming payment or indexes transparent addresses, it queries remote `lightwalletd` or Zaino nodes:

* Calling `GetTransaction(txid)` to fetch the full transaction data (memos, outputs, fees).
* Calling `GetAddressUtxos(address)` or `GetTaddressTransactions(address)` to track transparent balance and history.

```
Standard Model (Status Quo):
┌────────────────────────┐      GetTransaction(selected_txid)       ┌────────────────────────┐
│ Light Wallet Client    │ ───────────────────────────────────────► │ Remote lightwalletd    │
│ (Unshielded queries)   │      GetAddressUtxos(user_taddr)         │ (Learns IP + Tx Graph) │
└────────────────────────┘ ◄─────────────────────────────────────── └────────────────────────┘
```

> [!WARNING]
> Remote server operators and passive network observers correlate the user's IP address, query timing, and transaction IDs directly with the public blockchain graph, stripping away privacy for received funds.

---

## 2. How the Bridge Solves It

The **Zcash Private Receive Bridge** sits transparently between your wallet client and remote providers on `localhost`:

1. **Fixed Public Interval Acquisition:** The bridge scheduler fetches public block intervals $[H_{\text{start}}, H_{\text{end}}]$ and downloads **every referenced full transaction unconditionally**.
2. **Local Serving with Zero Selective Fallback:** The bridge stores verified blocks, transactions, tree states, and transparent indices in a durable local SQLite database (WAL mode).
3. **Deterministic Failure over Leakage:** If a wallet queries data outside the synchronized coverage interval, the bridge returns an explicit error (`NOT_FOUND` / `FAILED_PRECONDITION`). **It never falls back to an upstream query for a specific requested transaction or address.**

```
Private Receive Bridge Model:
┌────────────────────────┐         Unconditional Public Intervals         ┌────────────────────────┐
│ Remote Provider        │ ◄───────────────────────────────────────────── │ Bridge Daemon          │
│ (Sees only interval H) │ ─────────────────────────────────────────────► │ (Downloads ALL data)   │
└────────────────────────┘          (Zero client-driven queries)          └───────────┬────────────┘
                                                                                      │
                                                                             Atomic SQLite Commit
                                                                                      │
                                                                                      ▼
┌────────────────────────┐             Standard Local gRPC Queries         ┌────────────────────────┐
│ Light Wallet Client    │ ◄─────────────────────────────────────────────► │ Local gRPC Endpoint    │
│ (Zkool, Zingo, Ledger) │            (127.0.0.1:9067 - Loopback)          │ (Strict RPC Policy)    │
└────────────────────────┘                                                 └────────────────────────┘
```

| Metric / Dimension | Standard Lightwallet Connection | Zcash Private Receive Bridge |
| :--- | :--- | :--- |
| **Transaction Privacy** | ❌ Server learns which specific txs you received | ✅ **Zero upstream knowledge** (public intervals only) |
| **Transparent Address Privacy** | ❌ Server learns which t-addresses you own | ✅ **Zero upstream queries** (indexed locally) |
| **Cache-Miss Behavior** | ❌ Transmits miss to remote server | ✅ **Strict error return** (never forwards misses) |
| **Wallet Code Changes** | ❌ Requires modifying wallet core | ✅ **Zero changes** (standard custom server URL) |
| **Cryptographic Tamper Check** | ❌ Relies on server trust | ✅ **Consensus TxID & adjacency recomputation** |

---

## 3. Architecture & System Flow

```
                     ┌──────────────────────────────────┐
                     │ Upstream lightwalletd / Zaino    │
                     └────────────────┬─────────────────┘
                                      │ Public Block Intervals & Full Txs
                                      ▼
                     ┌──────────────────────────────────┐
                     │ Interval Acquisition Scheduler   │
                     │ (Autonomous, fixed-interval loop)│
                     └────────────────┬─────────────────┘
                                      │
                                      ▼
                     ┌──────────────────────────────────┐
                     │ Cryptographic Verifier           │
                     │ ├─ BLAKE2b / ZIP 244 TxID Hash   │
                     │ ├─ Block Height Continuity       │
                     │ └─ prev_hash Adjacency & Reorgs  │
                     └────────────────┬─────────────────┘
                                      │
                                      ▼
                     ┌──────────────────────────────────┐
                     │ Transparent Outpoint Indexer     │
                     │ ├─ P2PKH / P2SH Classification   │
                     │ └─ Pre-Coverage Spend Accounting │
                     └────────────────┬─────────────────┘
                                      │
                                      ▼
                     ┌──────────────────────────────────┐
                     │ Embedded SQLite Store (WAL Mode) │
                     │ ├─ compact_blocks                │
                     │ ├─ full_transactions             │
                     │ ├─ tree_states / subtree_roots   │
                     │ └─ transparent_outputs / spends  │
                     └────────────────┬─────────────────┘
                                      │
                                      ▼
                     ┌──────────────────────────────────┐
                     │ Strict RPC Policy Engine         │
                     │ ├─ Confirmed-only profile        │
                     │ ├─ Mempool queries: UNIMPLEMENTED│
                     │ └─ Broadcasts: PERMISSION_DENIED │
                     └────────────────┬─────────────────┘
                                      │
                                      ▼
                     ┌──────────────────────────────────┐
                     │ CompactTxStreamer gRPC Server    │
                     │ (Bound strictly to 127.0.0.1)    │
                     └────────────────▲─────────────────┘
                                      │ Custom Server Endpoint
                     ┌────────────────┴─────────────────┐
                     │ Unchanged Wallet Clients         │
                     │ (Zkool / YWallet, Zingo, Ledger) │
                     └──────────────────────────────────┘
```

---

## 4. The 5 Core Security Invariants

> [!IMPORTANT]
> The bridge architecture guarantees five formal security and privacy invariants:

1. **Zero Selective Upstream Leakage:** Upstream observers only observe sequential downloads of public intervals $[H, H + N]$. Wallet-selected requests and cancellations **never** trigger upstream requests.
2. **Zero-Key Invariant:** The daemon never accepts, requires, stores, or processes seeds, private keys, spending keys, or viewing keys. Note decryption remains strictly inside the wallet.
3. **Cryptographic Tamper Resistance:** Every transaction is recomputed using ZIP 244 consensus rules and matched against block commitments. Poisoned or corrupted upstream transactions are rejected before committing.
4. **Strict Localhost Isolation:** The gRPC server binds strictly to loopback addresses (`127.0.0.1` / `::1`), validated at startup. Remote LAN devices cannot access the service.
5. **Confirmed-Only Profile (No Fabricated Mempool):** To prevent wallets from falsely assuming broadcasted transactions are confirmed, mempool streams explicitly return `UNIMPLEMENTED`. Outbound broadcasts return `PERMISSION_DENIED` in the receive-only MVP.

---

## 5. Multi-Pool & Protocol Coverage

The bridge indexes and serves confirmed history across the full evolutionary history of Zcash pools:

| Pool / Protocol | Transaction Version | Component Indexing | Test Proof |
| :--- | :--- | :--- | :---: |
| **Sapling** | v4 | Spends (`nf`), Outputs (`cmu`, `ephemeral_key`, `ciphertext`), Commitment Tree | Covered |
| **Orchard** | v5 (ZIP 224/244) | Actions (`nullifier`, `cmx`, `ephemeral_key`, `ciphertext`), Orchard Tree | Covered |
| **Ironwood** | v5+ (Future / Extended) | Extended shielded actions, commitment trees, and consensus branch IDs | Covered |
| **Transparent P2PKH** | Standard Script | Decodes `OP_DUP OP_HASH160 <pubkey_hash> OP_EQUALVERIFY OP_CHECKSIG` $\rightarrow$ `t1...` | Covered |
| **Transparent P2SH** | Standard Script | Decodes `OP_HASH160 <script_hash> OP_EQUAL` $\rightarrow$ `t3...` | Covered |
| **Transparent Spends** | Vin inputs | Outpoint spend mapping (`prev_txid`, `prev_vout`) & pre-coverage accounting | Covered |

---

## 6. Quickstart & Installation

### Prerequisites
* **Rust:** 1.85+ (Rust 2024 edition)
* **Protobuf Compiler:** `protoc` (3.0+)
  * Ubuntu/Debian: `sudo apt install protobuf-compiler`
  * macOS: `brew install protobuf`
  * Arch Linux: `sudo pacman -S protobuf`

### Build from Source
```bash
# Clone the repository
git clone https://github.com/intelliDean/zcash-prb.git
cd zcash-prb

# Build optimized release binary
cargo build --release --bin zcash-private-bridge

# Run full test suite (15 unit and integration tests)
cargo test --workspace
```

---

## 7. Running the Daemon

### 1. Initialize Configuration
```bash
# Generate a default configuration file
cargo run --bin zcash-private-bridge -- init-config --output config/bridge.toml
```

Configuration example (`config/bridge.toml`):
```toml
# Network target: mainnet, testnet, or regtest
network = "mainnet"

# Public lightwalletd or Zaino instance
upstream_provider = "https://mainnet.lightwalletd.com:9067"

# Local gRPC endpoint for wallets (MUST be loopback)
bind_address = "127.0.0.1:9067"

# Block height from which verified coverage begins
coverage_start_height = 3000000

# Path to durable SQLite storage
storage_path = "data/bridge.db"

# Maximum storage budget in Gigabytes (optional)
storage_limit_gb = 10

# Concurrent transaction downloads
acquisition_concurrency = 8

# Interval chunk size
interval_size = 50

# Upstream request timeout in seconds
request_timeout_sec = 15
```

### 2. Start the Daemon
```bash
cargo run --release --bin zcash-private-bridge -- start --config config/bridge.toml
```

### 3. Check Synchronization Status
```bash
cargo run --bin zcash-private-bridge -- status --config config/bridge.toml
```
Example Output:
```text
=== Zcash Private Receive Bridge Status ===
Storage Database:     "data/bridge.db"
Network:              mainnet
Upstream Provider:    https://mainnet.lightwalletd.com:9067
Local Bind Address:   127.0.0.1:9067
Coverage Start:       3000000
Committed Height:     3000250
Latest Block Hash:    0000000001a4bc56...
Last Updated:         2026-10-06 21:15:00
Acquisition Failures: 0
```

### 4. Stop the Daemon
```bash
cargo run --bin zcash-private-bridge -- stop --config config/bridge.toml
```

---

## 8. Connecting Unchanged Wallets

Wallets connect to the bridge using their standard **Custom Server** settings without custom forks or patches.

### Zkool (YWallet)
1. Open **Settings** $\rightarrow$ **Server Settings**.
2. Set Server URL: `http://127.0.0.1:9067`.
3. Enable confirmed-only synchronization.

### Zingo (`zingolib`)
Launch Zingo with the `--server` argument:
```bash
zingo-cli --server http://127.0.0.1:9067
```

### Ledger Companion Tool
Export the environment variable before launching the sync tool:
```bash
export LIGHTWALLETD_URI="http://127.0.0.1:9067"
```

For complete client verification and direct-vs-bridge parity testing, refer to the [Client Compatibility Guide](docs/COMPATIBILITY.md).

---

## 9. Strict RPC Policy

| RPC Method | Bridge Response | Upstream Leakage | Rationale |
| :--- | :--- | :---: | :--- |
| `GetLatestBlock` | Highest committed local block | **None** | Served from local SQLite checkpoint |
| `GetBlock` | `CompactBlock` from local store | **None** | Served from local cache |
| `GetBlockRange` | Stream `CompactBlock` range | **None** | Local sequential streaming |
| `GetTransaction` | Full `RawTransaction` | **None** | **Error if missing.** Zero selective fallback |
| `GetTreeState` | Sapling, Orchard, Ironwood tree | **None** | Preserved from interval endpoints |
| `GetSubtreeRoots` | Subtree commitments | **None** | Local shielded tree indices |
| `GetAddressUtxos` | Transparent UTXO set | **None** | Error on incomplete pre-coverage history |
| `GetTaddressTransactions`| Transparent address records | **None** | Indexed locally from public intervals |
| `GetMempoolTx` / `Stream`| `UNIMPLEMENTED` | **None** | Confirmed-only profile (no fake mempool) |
| `SendTransaction` | `PERMISSION_DENIED` | **None** | Outbound broadcasts blocked in receive MVP |

---

## 10. Automated Proofs & Test Suites

The testkit crate ([`crates/bridge-testkit`](crates/bridge-testkit)) validates the core correctness and privacy invariants through isolated integration tests:

1. **Zero Upstream Privacy Leakage ([`tests/privacy_leakage.rs`](crates/bridge-testkit/tests/privacy_leakage.rs)):**
   Spawns a mock upstream server, acquires blocks, and connects simulated wallet clients querying multiple transactions. Asserts that the upstream server receives **zero additional network calls** during client queries.
2. **Cryptographic Tamper Rejection ([`tests/tampered_transactions.rs`](crates/bridge-testkit/tests/tampered_transactions.rs)):**
   Simulates an upstream server injecting modified transaction bytes. Confirms the transaction is rejected, uncommitted, and recorded in failure telemetry.
3. **Incomplete Pre-Coverage Protection ([`tests/incomplete_history.rs`](crates/bridge-testkit/tests/incomplete_history.rs)):**
   Confirms that addresses with historical spends prior to the coverage start height trigger explicit `FAILED_PRECONDITION` errors instead of misleading zero balances.
4. **Multi-Pool Receipt Coverage ([`tests/multi_pool_coverage.rs`](crates/bridge-testkit/tests/multi_pool_coverage.rs)):**
   Verifies concurrent ingestion and retrieval of Sapling outputs, Orchard actions, Ironwood actions, and transparent UTXOs.

---

## 11. Performance & Operating Cost Benchmarks

Measure sync times, network ingress volume, and RPC overhead using the built-in benchmark tool:

```bash
cargo run --bin zcash-private-bridge -- benchmark --blocks 10
```

Example Benchmark Output:
```text
=== Acquisition Cost Benchmark (10 blocks) ===
Provider:             https://mainnet.lightwalletd.com:9067
Elapsed Time:         842.15 ms
Block Ingress:        28.45 KB (10 blocks)
Transactions Fetched: 34 full transactions
Tx Ingress:           142.10 KB
Total Network Data:   170.55 KB
Throughput:           11.87 blocks/sec
Repeat Sync Savings:  100% (served instantly from local SQLite)
```

---

## 12. Crate Architecture

The repository is organized as a modular Rust workspace:

```
crates/
├── bridge-core        Domain primitives, types (TxId, BlockHash), configuration, and redaction
├── bridge-proto       Protobuf code generation (CompactTxStreamer) via tonic & prost
├── bridge-storage     SQLite WAL storage backend, migrations, and query interfaces
├── bridge-verifier    Consensus TxID recomputation (ZIP 244 BLAKE2b) and adjacency validation
├── bridge-engine      Autonomous interval scheduler and upstream client acquisition worker
├── bridge-server      Local CompactTxStreamer gRPC service implementing strict RPC policy
├── bridge-cli         zcash-private-bridge CLI dispatcher and daemon PID management
└── bridge-testkit     Mock upstream server fixtures and automated end-to-end privacy test suites
```

---

## 13. Limitations & Non-Goals

1. **IP Anonymity:** The bridge does not bundle Tor or I2P. If you want to conceal your IP address while fetching public block intervals from upstream, run the bridge behind a VPN or SOCKS5 proxy.
2. **Private Broadcasting:** Transaction broadcasting is intentionally disabled in this receive-only MVP. Sending transactions requires dedicated broadcasting routes.
3. **Consensus Validation:** The bridge is a verified light-client proxy; it validates TxID commitments and chain continuity, but does not execute full Proof-of-Work checks or complete consensus script evaluation.

For the comprehensive security model, see [Threat Model](docs/THREAT_MODEL.md).

---

## 14. License

Dual-licensed under either:
* **MIT License** ([LICENSE-MIT](LICENSE-MIT) or [opensource.org/licenses/MIT](http://opensource.org/licenses/MIT))
* **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE-APACHE) or [apache.org/licenses/LICENSE-2.0](http://www.apache.org/licenses/LICENSE-2.0))
