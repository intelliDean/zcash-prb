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

### C. Live Wallet Receipt Demonstration & Parity Audit with Tested Zingo Build
To demonstrate the actual user workflow—running the tested Zingo client build against an authentic confirmed receipt, discovering notes, decrypting memos, and proving bit-for-bit database parity without selective leakage:
* **Tested Zingo CLI Build:** [`/home/dean/.local/bin/zingo-cli`](https://github.com/zingolabs/zingolib) (Version: `zingolib_v3.0.0-57-g45d4fc7f`, built from `https://github.com/zingolabs/zingolib` at commit `fb58a162cf`, 57 commits after release tag `zingolib_v3.0.0`; SHA-256: `ec1d9f339c5b23a6611dffbaf29a64c3c26232e0306448370107986362a6df27`).
* **Confirmed Shielded Receipt:** Block `226` on live Zebra + Lightwalletd devnet.
  * **TxID:** `b9e7da7538ff7b864a25978165b2eb9612a541121e667600dbacbef7d5c3e4dc`
  * **Amount:** `50,000,000` zatoshis (0.50000000 ZEC, Orchard Shielded Pool)
  * **Decrypted Memo:** `"ZCG Grant Evaluation - Actual Wallet Receipt Parity"`
* **Empirical Execution & Parity Results:**
  * **Transcript Recording:** [`artifacts/real_wallet_receipt_recording.txt`](../artifacts/real_wallet_receipt_recording.txt)  
    Captures complete end-to-end execution of `zingo-cli`: importing Unified Full Viewing Key (`uviewregtest1...`), performing trial decryption, discovering the note at block 226, decrypting the memo text, and dumping internal wallet database state.
  * **Actual Wallet Database Exports:**
    * Discovered Orchard Notes: [`artifacts/released_wallet_exports/upstream_zingo_notes.json`](../artifacts/released_wallet_exports/upstream_zingo_notes.json) vs [`artifacts/released_wallet_exports/prb_zingo_notes.json`](../artifacts/released_wallet_exports/prb_zingo_notes.json) (**100% byte-for-byte identical**).
    * Decrypted Value Transfers / Memos: [`artifacts/released_wallet_exports/upstream_zingo_messages.json`](../artifacts/released_wallet_exports/upstream_zingo_messages.json) vs [`artifacts/released_wallet_exports/prb_zingo_messages.json`](../artifacts/released_wallet_exports/prb_zingo_messages.json) (**100% byte-for-byte identical**).
    * Confirmed Balance: `0.50000000 ZEC` across both runs.
  * **Upstream Network Audit & Raw Trace Retention:**
    * Raw Trace: [`artifacts/released_wallet_upstream_trace.jsonl`](../artifacts/released_wallet_upstream_trace.jsonl).
    * Upstream calls before wallet queries: 163 calls.
    * Upstream calls after wallet queries: 187 calls (24 background polling calls by acquisition worker; **exactly 0 client-specific queries leaked**).

### D. Synthetic Protocol Test Fixtures & Vectors
*Clearly distinguished from live released wallet execution:*
* **Synthetic Protocol Test Demonstration:** [`crates/bridge-testkit/tests/wallet_receipt_demonstration.rs`](../crates/bridge-testkit/tests/wallet_receipt_demonstration.rs) and [`crates/bridge-cli/src/commands/wallet_scan.rs`](../crates/bridge-cli/src/commands/wallet_scan.rs).
* **Synthetic Exports:** [`artifacts/wallet_state_local_prb.json`](../artifacts/wallet_state_local_prb.json) and [`artifacts/wallet_state_upstream_lightwalletd.json`](../artifacts/wallet_state_upstream_lightwalletd.json) (Explicitly metadata-labeled as `Synthetic-Protocol-Test-Vector`). Note decryption logic validates exact trial-decryption keys with zero hardcoded memo fallbacks.

---

## 2. Empirical Measurements: Running Daemon across Useful Coverage Windows

The running `zcash-private-bridge` daemon was measured live using [`scripts/measure_running_daemon.py`](../scripts/measure_running_daemon.py) connected to live Zcash Mainnet (`https://zec.rocks:443`).

To demonstrate realistic catch-up performance, the evaluation measured **100 blocks per window** across two distinct operational regimes:
1. **Older Historical Coverage (Realistic Birthday-to-Scan-End Catch-Up):** Blocks `2500000` to `2500099` (100 blocks, NU5). Demonstrates restoring an older wallet from its birthday height across a realistic historical interval.
2. **Recent Mainnet Receiving Interval:** Blocks `3516500` to `3516599` (100 blocks, NU6). Demonstrates active catch-up and verification at the current mainnet tip.

The measurements track the full daemon lifecycle:
- Ingestion and cryptographic consensus verification (`validate_block_sequence` + `verify_transaction`).
- Total active SQLite storage breakdown (`bridge.db`, `bridge.db-wal`, `bridge.db-shm`).
- Upstream network traffic (exact ingress bytes, request counts, method breakdown, and `/proc/io`).
- Upstream call measurements before and after client serving phase, retaining full raw traces.
- Live gRPC serving latency under wallet queries while the daemon is actively serving.
- Process peak resident set size (RSS).

### A. Raw Measurement Summary (Directly Reconciled with Logs)
*All numbers below match [`artifacts/recent_vs_older_coverage.json`](../artifacts/recent_vs_older_coverage.json), [`artifacts/daemon_storage_breakdown.txt`](../artifacts/daemon_storage_breakdown.txt), and [`artifacts/birthday_to_scan_end_network_traffic.txt`](../artifacts/birthday_to_scan_end_network_traffic.txt).*

| Metric | Older Coverage (NU5 Birthday: 2500000..=2500099) | Recent Coverage (NU6 Tip: 3516500..=3516599) | Source Log |
| :--- | :--- | :--- | :--- |
| **Blocks Evaluated** | **100 blocks** | **100 blocks** | `recent_vs_older_coverage.json` |
| **Active Consensus Branch** | `c2d6d0b4` (NU5) | `c8e71050` (NU6) | `recent_vs_older_coverage.json` |
| **Transactions Ingested & Verified** | 448 full transactions | 881 full transactions | `birthday_to_scan_end_network_traffic.txt` |
| **Sync & Verification Duration** | **56.04 seconds** | **72.09 seconds** | `daemon_storage_breakdown.txt` |
| **Peak Process RSS** | **18.70 MB** | **24.04 MB** | `daemon_storage_breakdown.txt` |
| **`bridge.db` Size** | 4,096 bytes (4.00 KB) | 7,041,024 bytes (7.04 MB) | `daemon_storage_breakdown.txt` |
| **`bridge.db-wal` Size** | 3,106,512 bytes (3.11 MB) | 7,197,672 bytes (7.20 MB) | `daemon_storage_breakdown.txt` |
| **`bridge.db-shm` Size** | 32,768 bytes (32.00 KB) | 32,768 bytes (32.00 KB) | `daemon_storage_breakdown.txt` |
| **Total Storage Footprint (DB+WAL)**| **3,143,376 bytes (3.14 MB)** | **14,271,464 bytes (14.27 MB)** | `daemon_storage_breakdown.txt` |
| **Upstream Acquisition Ingress** | **2,339,057 bytes (2.23 MB)** | **6,169,722 bytes (5.88 MB)** | `birthday_to_scan_end_network_traffic.txt` |
| **Acquisition Upstream RPC Calls** | 554 calls (100 blocks, 448 txs) | 987 calls (100 blocks, 881 txs) | `birthday_to_scan_end_network_traffic.txt` |
| **Serving Phase Upstream Calls** | **1 background call (`GetLatestBlock`)** | **5 background calls (`GetLatestBlock`, `GetBlock`)** | `birthday_to_scan_end_network_traffic.txt` |
| **Client Selective Query Leakage** | **0 calls (100% served locally)** | **0 calls (100% served locally)** | `birthday_to_scan_end_network_traffic.txt` |
| **Raw Trace Retained** | `raw_upstream_trace_older_historical_nu5.jsonl` | `raw_upstream_trace_recent_mainnet_tip.jsonl` | `birthday_to_scan_end_network_traffic.txt` |
| **Average gRPC Serving Latency** | **43.08 ms** | **87.92 ms** | `recent_vs_older_coverage.json` |
| **Max gRPC Serving Latency** | 384 ms | 688 ms | `recent_vs_older_coverage.json` |
| **Probe Tests Passed** | 12 / 12 (100%, Exit Code: 0) | 12 / 12 (100%, Exit Code: 0) | `recent_vs_older_coverage.json` |

---

## 3. Comparison Context: Documented Zebra Baseline vs. Measured Local PRB

To ensure honest reporting, **published Zebra specifications are kept strictly separated from empirical PRB measurements**. No local Zebra instance was run in this benchmark; baseline figures cite official Zcash Foundation and Zebra documentation.

> [!IMPORTANT]
> **Limitation of Preliminary Samples:** The 100-block samples demonstrate windowed resource consumption during limited receiving intervals. They have **not** established realistic restoration costs for full wallet lifecycles or proven operational superiority over Zebra plus lightwalletd.

| Architectural Dimension | Zebra + lightwalletd (Documented Full-Node Baseline) | Private Receive Bridge (Empirically Measured Window) |
| :--- | :--- | :--- |
| **Data Source** | Published Zcash Foundation specifications & Zebra docs | Live Linux x86_64 measurements ([`artifacts/`](../artifacts/)) |
| **Storage Architecture** | Full historical RocksDB (~200 GB NVMe SSD required) | Windowed SQLite WAL (**3.14 MB to 14.27 MB** per 100-block window) |
| **Process Memory (RSS)** | 4,096 – 8,192 MB RAM (Documented system requirement) | **18.70 MB – 24.04 MB** (Measured process peak RSS) |
| **Initial Synchronization** | Full Initial Block Download (Genesis to Tip: hours-to-days) | Windowed acquisition from birthday height (**56.04 – 72.09 seconds** per 100 blocks) |
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
4. **Fail-Closed Trace Validation:** The audit script strictly rejects missing, empty, or unpopulated trace logs before evaluating leakage.

---

## 5. Scope Definition & Honest Boundaries

1. **In-Progress Milestone Scope:**  
   `zcash-private-bridge` is an experimental prototype exploring recipient retrieval privacy. This report demonstrates protocol conformance and architectural resource viability; it is **not** presented as a completed final validation.
2. **Preliminary Nature of 100-Block Samples & Limited Cost Claims:**  
   The measured 100-block runs are short preliminary test samples demonstrating windowed resource metrics. They have not established realistic restoration costs for full wallet lifecycles or operational superiority over Zebra plus lightwalletd. Rather than claiming full-node replacement or proven operational superiority, the prototype demonstrates an architecture for windowed retrieval privacy.
3. **No Transport-Level IP Anonymity:**  
   The bridge prevents the upstream server from learning which specific transactions the user is receiving. However, the upstream server still observes the IP address requesting public interval batches unless the operator routes traffic through Tor or a VPN.
4. **Windowed Cache vs. Consensus Node:**  
   PRB does not validate Equihash proof-of-work or execute full archival consensus checks. Users requiring full consensus validation or archival exploration must run a full node like Zebra.

---

## 6. Independent Reproduction Commands

To independently reproduce all measurements and verify the raw logs:

```bash
# 1. Run live wallet receipt & database parity verification (Tested Zingo CLI Build)
python3 scripts/verify_released_wallet_receipt.py

# 2. Verify bit-for-bit parity between released wallet database exports
diff -u artifacts/released_wallet_exports/upstream_zingo_notes.json artifacts/released_wallet_exports/prb_zingo_notes.json
diff -u artifacts/released_wallet_exports/upstream_zingo_messages.json artifacts/released_wallet_exports/prb_zingo_messages.json

# 3. Run complete workspace test suite (27 integration & unit tests)
cargo test --workspace

# 4. Run synthetic protocol test harness
cargo test --test wallet_receipt_demonstration -- --nocapture

# 5. Run retrieval privacy audit and verify zero selective upstream leakage
./scripts/verify_retrieval_privacy.sh

# 6. Run live running daemon performance & network traffic measurement (100-block windows)
python3 scripts/measure_running_daemon.py
```
