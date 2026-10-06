# Threat Model & Security Invariants

The **Zcash Private Receive Bridge** is engineered to eliminate network-level transaction and address linkage when light clients synchronize received funds from untrusted remote servers.

---

## 1. Adversary Model

* **Adversary Identity:** The operator of the upstream lightwalletd or Zaino instance, or a network passive/active eavesdropper between the bridge and upstream.
* **Adversary Capabilities:**
  * Observes all inbound gRPC requests, timing, headers, and IP addresses.
  * Can return arbitrary, malicious, or malformed responses (tampered transactions, withheld blocks, mutated chain histories, out-of-order blocks).
  * Colludes with network telemetry or chain analytics firms.

---

## 2. Security & Privacy Invariants

### Invariant 1: Zero Selective Upstream Leakage
* **Definition:** An upstream observer learning the bridge's queries learns *only* that a node is syncing a public interval $[H_{\text{start}}, H_{\text{end}}]$.
* **Enforcement:**
  * Client wallet requests never trigger, alter, or prioritize upstream downloads.
  * All full transactions referenced in the public interval are fetched unconditionally.
  * Cache misses are rejected with explicit errors (`NOT_FOUND` / `FAILED_PRECONDITION`); **the bridge never performs selective on-demand fallback queries.**

### Invariant 2: Zero Key Invariant
* **Definition:** The bridge daemon never accepts, requires, stores, or processes private keys, viewing keys, or recovery seeds.
* **Enforcement:**
  * Key management and trial decryption of compact notes remain strictly within the wallet client.
  * The bridge exposes only the public `CompactTxStreamer` interface.

### Invariant 3: Cryptographic Tamper Resistance
* **Definition:** Upstream servers cannot inject substituted or counterfeit transactions.
* **Enforcement:**
  * Every downloaded raw transaction is hashed using consensus TxID rules and compared against the TxID committed in the corresponding `CompactBlock`.
  * Mismatched transactions are rejected with `Verification` errors, preventing poisoned history.

### Invariant 4: Strict Localhost Isolation
* **Definition:** Downstream RPC endpoints bind exclusively to `127.0.0.1` or `::1`.
* **Enforcement:**
  * Configuration validation rejects any non-loopback bind address at startup.
  * Remote nodes on the local LAN cannot query the bridge.

### Invariant 5: Confirmed-Only Profile (No Fake Mempool)
* **Definition:** Unconfirmed transaction streams (`GetMempoolTx`, `GetMempoolStream`) return `UNIMPLEMENTED`.
* **Enforcement:**
  * The daemon does not return a fabricated empty mempool (which could deceive wallets into assuming all broadcasts are confirmed).
  * Outbound unshielded broadcasts (`SendTransaction`) are blocked with `PERMISSION_DENIED` in the MVP.

---

## 3. Explicit Non-Goals & Limitations

1. **IP Anonymity:** The bridge does not bundle Tor or I2P. If the user wishes to conceal their IP from upstream, they should run the bridge behind a SOCKS5 proxy or VPN.
2. **Private Broadcasting:** Sending transactions requires separate routing (e.g. Tor or dedicated broadcast proxies).
3. **Consensus Validation:** The bridge is a light client intermediary; it verifies TxID hashes and sequence adjacency, but does not execute full node script validation or Proof-of-Work checks.
