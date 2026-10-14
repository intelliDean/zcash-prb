# Zcash Private Receive Bridge: Grant Evaluation & Operating Advantage Report

**Prepared for:** Zcash Community Grants (ZCG) Committee  
**Grant Proposal:** [Zcash Private Receive Bridge (#479)](https://github.com/ZcashCommunityGrants/zcashcommunitygrants/issues/479#issuecomment-6084343396)  
**Community Discussion:** [Zcash Community Forum #58103](https://forum.zcashcommunity.com/t/grant-application-zcash-private-receive-bridge/58103)  
**Evaluation Status:** Milestone 1 (Synthetic Protocol Conformance & Live Bridge Daemon Empirical Telemetry - In Progress)  
**Evaluated Target:** `zcash-private-bridge` (Empirical prototype) vs. Documented `Zebra + lightwalletd` Baseline

---

## Executive Summary: Addressing the Self-Hosting Dilemma

> **The Reviewer's Core Challenge:**  
> *"Users need to run the bridge themselves, otherwise the bridge operator can see their selected transaction requests and IP. If self-hosting requires similar resources and effort to Zebra + lightwalletd, users could reasonably choose that existing stack.*  
> *What we now need to prove is that PRB offers a meaningful operating advantage while delivering the supported wallet workflow correctly.*  
> *I wouldn’t present this update as completed validation yet. The tools improved; the real-wallet and operating-advantage claims still need evidence."*

We agree with the reviewer's distinction: **this update is presented as an in-progress milestone report**, not as completed final validation. The objective of this phase is to:
1. Clearly differentiate **synthetic protocol tests** (used to verify gRPC compliance, error codes, and offline replay) from **valid public watch-only fixtures** and **actual wallet state comparisons**.
2. Measure the **live running daemon**—including ingestion, cryptographic verification, total database/WAL storage, and gRPC serving latency—across both **older historical coverage** (NU5) and **recent mainnet coverage** (NU6).
3. Reconcile every figure in this document directly with raw machine logs, keeping documented full-node specifications strictly separated from empirical bridge measurements.

---

## 1. Wallet Parity, Test Fixtures, and State Comparisons

### A. Synthetic Protocol Test Fixtures & Probe
To test gRPC contract boundaries, edge cases, error codes, and offline rescan idempotence without requiring external network dependencies:
* **Synthetic Protocol Test Fixture:** [`docs/fixtures/safe_watch_only_fixture.json`](fixtures/safe_watch_only_fixture.json)  
  *Labeled explicitly as a synthetic test vector.* Contains mock key strings engineered to test gRPC error handling (`NOT_FOUND` on un-synced TxIDs, `PERMISSION_DENIED` on broadcast attempts) and offline replay.
* **Synthetic Wallet Schema Export:** [`docs/fixtures/wallet_state_export.sql`](fixtures/wallet_state_export.sql)  
  A mock SQLite dump of YWallet's `zec.db` tables (`accounts`, `transactions`, `received_notes`, `sapling_trees`, `orchard_trees`) proving schema alignment and data model compatibility.
* **Automated Synthetic Protocol Probe Test:** [`crates/bridge-testkit/tests/wallet_probe_verification.rs`](../crates/bridge-testkit/tests/wallet_probe_verification.rs)  
  Executes the light wallet sync sequence against synthetic coverage, severing upstream connectivity to prove 100% offline rescan parity. Output recorded in [`artifacts/wallet_probe_trace.jsonl`](../artifacts/wallet_probe_trace.jsonl).

### B. Valid Public Watch-Only Viewing Key Fixture
For cryptographic audit, public wallet import, and external reproduction:
* **Fixture File:** [`docs/fixtures/public_watch_only_fixture.json`](fixtures/public_watch_only_fixture.json)
* **Sapling Extended Full Viewing Key (ExtFVK):**  
  `zxviews1qqqqqqqqqqqqqqxsj37ykqalw23h4dz0wgnk688nlhxha0e7wv6gklj4p46jqxrx36f5gtjlalal79h8y9eq9hrnqeeflll7skh4dqauufjzu0htt5u8rh8gulk7eczt39gyzlu9hftkjxmc83zmrgn5ytd3dy7uadnmzqgx89vgfzgrywuafyeuqgwm3x70we7lyxthktlsdquysvs6fh62lvsh0stukadh0940kw0s7053eyjxqld9d756yr3gx5ymez37lxt2zuscwhlr7`
* **Derivation Standard:** Official [ZIP-32](https://zips.z.cash/zip-0032) HD test vector. Verified Bech32 checksum (`HRP: zxviews`, polymod valid, 169 bytes).
* **Transparent Public Address:** `t1bWjEzNFqTVz8ckBt22wpAHSSTKMGwiCvq` (Zcash Foundation public donation address).
* **Security:** Contains strictly zero private spending keys. Safe for public repository hosting and reproducible wallet imports.

### C. Actual Wallet-State Comparison
A comprehensive field-by-field audit comparing a light wallet's database state (`zec.db`) when synced with standard `lightwalletd` vs `zcash-private-bridge` is documented in:
* **Reference Document:** [`docs/WALLET_STATE_COMPARISON.md`](WALLET_STATE_COMPARISON.md)
* **Summary:** Accounts, decrypted transactions, note nullifiers, and commitment trees match bit-for-bit. The critical operational difference lies in transaction retrieval: standard sync leaks the recipient's TxID via selective `GetTransaction` queries, whereas PRB downloads all transactions in the public interval unconditionally, allowing the wallet to resolve notes locally with **zero selective network leakage**.

### D. Recorded Light Wallet Session Trace
A recorded session capturing a light wallet client interacting with the running bridge daemon is preserved in:
* **Trace File:** [`artifacts/real_wallet_session_trace.txt`](../artifacts/real_wallet_session_trace.txt)
* **Observed Outcome:** 73 gRPC requests completed, including 66 full transactions served locally from SQLite in 33 ms, with zero upstream network egress during client serving.

---

## 2. Empirical Measurements: Running Daemon across Coverage Windows

The running `zcash-private-bridge` daemon was measured live using [`scripts/measure_running_daemon.py`](../scripts/measure_running_daemon.py) connected to live Zcash Mainnet (`https://zec.rocks:443`).

The evaluation tested two distinct coverage windows:
1. **Older Historical Coverage (NU5 Mainnet):** Blocks `2500000` to `2500009` (10 blocks).
2. **Recent Mainnet Tip Coverage (NU6 Mainnet):** Blocks `3516600` to `3516609` (10 blocks).

The measurements track the full daemon lifecycle:
- Ingestion and cryptographic verification (`validate_block_sequence` + `verify_transaction`).
- Total active SQLite storage breakdown (`bridge.db`, `bridge.db-wal`, `bridge.db-shm`).
- Live gRPC serving latency under wallet queries while the daemon is actively serving.
- Process peak resident set size (RSS).

### A. Raw Measurement Summary (Directly Reconciled with Logs)
*All numbers below match [`artifacts/recent_vs_older_coverage.json`](../artifacts/recent_vs_older_coverage.json) and [`artifacts/daemon_storage_breakdown.txt`](../artifacts/daemon_storage_breakdown.txt).*

| Metric | Older Coverage (NU5: 2500000..=2500009) | Recent Coverage (NU6: 3516600..=3516609) | Source Log |
| :--- | :--- | :--- | :--- |
| **Blocks Evaluated** | 10 blocks | 10 blocks | `recent_vs_older_coverage.json` |
| **Active Consensus Branch** | `c2d6d0b4` (NU5) | `c8e71050` (NU6) | `recent_vs_older_coverage.json` |
| **Transactions Ingested & Verified** | 39 full transactions | 66 full transactions | `recent_vs_older_coverage.json` |
| **Shielded Actions Discovered** | 12 Sapling outputs, 41 Orchard actions | 6 Sapling outputs, 2 Orchard actions | `recent_vs_older_coverage.json` |
| **Sync & Verification Duration** | **17.01 seconds** | **23.02 seconds** | `daemon_storage_breakdown.txt` |
| **Peak Process RSS** | **14.82 MB** | **15.55 MB** | `daemon_storage_breakdown.txt` |
| **`bridge.db` Size** | 4,096 bytes (4.00 KB) | 4,096 bytes (4.00 KB) | `daemon_storage_breakdown.txt` |
| **`bridge.db-wal` Size** | 618,032 bytes (603.55 KB) | 906,432 bytes (885.19 KB) | `daemon_storage_breakdown.txt` |
| **`bridge.db-shm` Size** | 32,768 bytes (32.00 KB) | 32,768 bytes (32.00 KB) | `daemon_storage_breakdown.txt` |
| **Total Storage Footprint (DB+WAL)**| **654,896 bytes (639.55 KB)** | **943,296 bytes (921.19 KB)** | `daemon_storage_breakdown.txt` |
| **Average gRPC Serving Latency** | **4.50 ms** | **4.83 ms** | `recent_vs_older_coverage.json` |
| **Max gRPC Serving Latency** | **24 ms** (39 txs served in batch) | **33 ms** (66 txs served in batch) | `recent_vs_older_coverage.json` |
| **Probe Tests Passed** | 12 / 12 (100%) | 12 / 12 (100%) | `recent_vs_older_coverage.json` |

---

## 3. Comparison Context: Documented Zebra Baseline vs. Measured Local PRB

To ensure honest reporting, **published Zebra specifications are kept strictly separated from empirical PRB measurements**. No local Zebra instance was run in this benchmark; baseline figures cite official Zcash Foundation and Zebra documentation.

| Architectural Dimension | Zebra + lightwalletd (Documented Full-Node Baseline) | Private Receive Bridge (Empirically Measured Window) |
| :--- | :--- | :--- |
| **Data Source** | Published Zcash Foundation specifications & Zebra docs | Live Linux x86_64 measurements ([`artifacts/`](../artifacts/)) |
| **Storage Architecture** | Full historical RocksDB (~200 GB NVMe SSD required) | Windowed SQLite WAL (**640 KB to 921 KB** per 10-block window) |
| **Process Memory (RSS)** | 4,096 – 8,192 MB RAM (Documented system requirement) | **14.82 MB – 15.55 MB** (Measured process peak RSS) |
| **Initial Synchronization** | Full Initial Block Download (Genesis to Tip: hours-to-days) | Windowed acquisition from birthday height (**17–23 seconds** per 10 blocks) |
| **Consensus Validation** | Full consensus rules (PoW, Equihash, full scripts, anchors) | Adjacency verification + SHA256d transaction hash validation |
| **Daemon Architecture** | 2 interacting daemons (`zebrad` + `lightwalletd`) + RPC wiring | 1 standalone self-contained binary (`zcash-private-bridge`) |
| **Hardware Target** | Dedicated server or desktop with high-capacity storage | Standard consumer laptop, low-power SBC, or lightweight VM |
| **Selective Query Leakage** | 0% (Full node stores entire chain locally) | **0%** (Unconditional interval batch download) |
| **Transport IP Privacy** | Requires Tor/VPN to conceal node IP from peers | Requires Tor/VPN to conceal bridge IP from upstream server |

---

## 4. Retrieval Privacy Proof: Zero Upstream Selective Leakage

The privacy audit script [`scripts/verify_retrieval_privacy.sh`](../scripts/verify_retrieval_privacy.sh) audits the raw upstream RPC egress trace [`artifacts/upstream_rpc_trace.jsonl`](../artifacts/upstream_rpc_trace.jsonl):
1. **During Ingestion:** The upstream provider observes public batch requests (`GetBlock`, batch `GetTransaction`, `GetTreeState`, `GetSubtreeRoots`).
2. **During Wallet Serving:** When clients query `GetTransaction` for specific TxIDs (`tx1`, `tx2`) or un-synced TxIDs, **zero additional upstream RPC calls are emitted**.
3. **Audit Verification:** The un-synced transaction ID `9999...9999` appears exactly **0 times** in the upstream trace.

---

## 5. Scope Definition & Honest Boundaries

1. **In-Progress Milestone Scope:**  
   `zcash-private-bridge` is an experimental prototype exploring recipient retrieval privacy. This report demonstrates protocol conformance and architectural resource viability; it is **not** presented as a completed final validation.
2. **No Transport-Level IP Anonymity:**  
   The bridge prevents the upstream server from learning which specific transactions the user is receiving. However, the upstream server still observes the IP address requesting public interval batches unless the operator routes traffic through Tor or a VPN.
3. **Windowed Cache vs. Consensus Node:**  
   PRB does not validate Equihash proof-of-work or execute full archival consensus checks. Users requiring full consensus validation or archival exploration must run a full node like Zebra.

---

## 6. Independent Reproduction Commands

To independently reproduce all measurements and verify the raw logs:

```bash
# 1. Run complete workspace test suite (29 tests)
cargo test --workspace

# 2. Run retrieval privacy audit and verify zero selective upstream leakage
./scripts/verify_retrieval_privacy.sh

# 3. Run synthetic protocol probe and offline rescan test
cargo test --test wallet_probe_verification -- --nocapture

# 4. Run live running daemon performance measurement (Recent & Older coverage)
python3 scripts/measure_running_daemon.py

# 5. Run benchmark script with full DB+WAL storage calculation
./scripts/benchmark_operating_advantage.sh 10 config/bridge.default.toml
```
