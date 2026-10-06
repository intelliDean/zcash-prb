# Client Compatibility Matrix & Verification Methodology

The Zcash Private Receive Bridge is designed to support existing, unmodified Zcash light wallets.

---

## 1. Supported Client Matrix

| Wallet Client | Target Component | Pinned Version / Commit | Connection Setting | Confirmed Profile Support |
| :--- | :--- | :--- | :--- | :---: |
| **Zkool (YWallet)** | Desktop / Android | `v1.5.0+` | Custom Server $\rightarrow$ `http://127.0.0.1:9067` | Supported |
| **Zingo** | `zingolib` / CLI | `v0.2.0` | `--server http://127.0.0.1:9067` | Supported |
| **Ledger Zcash** | Ledger Live / Sync Tool | `v2.70+` | Environment variable `LIGHTWALLETD_URI` | Supported |

---

## 2. Verification Protocol (Direct vs Bridge Sync)

To verify that an unchanged client synchronizes with 100% parity against the bridge compared to a direct remote server:

### Protocol Steps:

1. **Baseline Synchronization (Direct Server):**
   * Configure the client to connect directly to the upstream server (e.g. `https://mainnet.lightwalletd.com:9067`).
   * Perform initial scan across target interval $[H_{\text{start}}, H_{\text{end}}]$.
   * Record exported balance, transaction count, note commitments, and memos.

2. **Bridge Synchronization:**
   * Start `zcash-private-bridge` with `coverage_start_height = H_start`.
   * Wait until `status` reports `committed_height >= H_end`.
   * Clear the client's local wallet cache (keeping seed/keys identical).
   * Point the client to `http://127.0.0.1:9067`.
   * Perform wallet sync.

3. **Parity Assertions:**
   * $\text{Balance}_{\text{bridge}} == \text{Balance}_{\text{direct}}$
   * $\text{Notes}_{\text{bridge}} == \text{Notes}_{\text{direct}}$
   * $\text{Memos}_{\text{bridge}} == \text{Memos}_{\text{direct}}$
   * $\text{TxCount}_{\text{bridge}} == \text{TxCount}_{\text{direct}}$

4. **Restart & Repeat Check:**
   * Stop the bridge daemon (`zcash-private-bridge stop`).
   * Start the bridge daemon again (`zcash-private-bridge start`).
   * Trigger a rescan or query from the wallet client.
   * Verify all queries succeed immediately from local SQLite database without initiating new upstream network queries.
