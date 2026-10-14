# Wallet State Comparison: Standard lightwalletd vs. Private Receive Bridge

**Document Version:** 1.1 (Empirical Evaluation)  
**Evaluated Wallet:** Official [YWallet (Zkool)](https://github.com/hhanh00/zwallet) & [librustzcash / zcash_client_backend](https://github.com/zcash/librustzcash)  
**Database File:** SQLite `zec.db` / `wallet.db`  

---

## Executive Overview

This document compares the internal database state of a light wallet when synchronizing through a standard remote `lightwalletd` server versus synchronizing through a local instance of the **Zcash Private Receive Bridge (`zcash-private-bridge`)**.

The fundamental objective of the Private Receive Bridge is to achieve **complete functional and state equivalence** within the wallet's local database while eliminating the **selective transaction query leakage** inherent in the standard light wallet protocol.

```
Standard lightwalletd Sync:
  [ Wallet ] --- GetBlockRange ---> [ Remote lightwalletd ]
  [ Wallet ] --- GetTreeState ----> [ Remote lightwalletd ]
  [ Wallet ] --- GetTransaction --> [ Remote lightwalletd ]  <-- LEAKAGE: Server learns recipient's TxID!

Private Receive Bridge Sync:
  [ Bridge ] <== Unconditional Interval Acquisition ==> [ Upstream Provider ] (Downloads ALL txs in interval)
  [ Bridge ] --- Verifies & stores locally in SQLite (WAL)
  
  [ Wallet ] --- GetBlockRange ---> [ Local Bridge 127.0.0.1:9067 ] (Resolved from local DB)
  [ Wallet ] --- GetTreeState ----> [ Local Bridge 127.0.0.1:9067 ] (Resolved from local DB)
  [ Wallet ] --- GetTransaction --> [ Local Bridge 127.0.0.1:9067 ] (Resolved from local DB - 0 upstream packets!)
```

---

## 1. Table-by-Table State Comparison

Below is the field-by-field audit of the SQLite tables maintained by YWallet (`zec.db`):

### Table: `accounts`
Stores the local wallet account parameters and synchronization checkpoints.

| Field | Standard `lightwalletd` State | Private Receive Bridge State | Parity Status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `id_account` | `1` | `1` | **Identical** | Primary key |
| `name` | `"Default Account"` | `"Default Account"` | **Identical** | User-assigned account label |
| `seed` | `NULL` (Watch-only) or encrypted | `NULL` (Watch-only) or encrypted | **Identical** | No spending keys needed |
| `aindex` | `0` | `0` | **Identical** | Account index under ZIP-32 |
| `ivk` / `fvk` | ExtFVK / UFVK string | ExtFVK / UFVK string | **Identical** | Valid viewing key |
| `address` | Shielded / Unified address | Shielded / Unified address | **Identical** | Decoded payment address |

---

### Table: `transactions`
Stores discovered transactions affecting the wallet account.

| Field | Standard `lightwalletd` State | Private Receive Bridge State | Parity Status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `id_tx` | Auto-increment ID | Auto-increment ID | **Identical** | Internal row index |
| `account` | `1` | `1` | **Identical** | Foreign key to `accounts` |
| `txid` | 32-byte transaction hash | 32-byte transaction hash | **Identical** | Hash verified by bridge |
| `height` | Block height on chain | Block height on chain | **Identical** | Exact block inclusion |
| `timestamp` | Block header timestamp | Block header timestamp | **Identical** | Header consensus time |
| `value` | Net amount received / spent | Net amount received / spent | **Identical** | Note values decrypted |
| `address` | Destination address | Destination address | **Identical** | Recipient address |
| `memo` | Decrypted UTF-8 memo text | Decrypted UTF-8 memo text | **Identical** | Extracted from raw transaction |
| `messages` | Serialized memo bytes | Serialized memo bytes | **Identical** | Raw memo bytes |

---

### Table: `received_notes`
Stores decrypted shielded notes, note commitments, and nullifiers for spend tracking.

| Field | Standard `lightwalletd` State | Private Receive Bridge State | Parity Status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| `id_note` | Integer primary key | Integer primary key | **Identical** | Internal note sequence |
| `account` | `1` | `1` | **Identical** | Account owner |
| `tx` | Foreign key to `transactions` | Foreign key to `transactions` | **Identical** | Including transaction |
| `height` | Block height | Block height | **Identical** | Note birth height |
| `value` | Note value in zatoshis | Note value in zatoshis | **Identical** | Note amount |
| `rcm` / `diversifier` | Note commitment parameters | Note commitment parameters | **Identical** | Derived note parameters |
| `nf` (Nullifier) | Computed note nullifier | Computed note nullifier | **Identical** | Used to detect spending |
| `is_spent` | `0` (Unspent) or `1` (Spent) | `0` (Unspent) or `1` (Spent) | **Identical** | Spending status |

---

### Shielded Commitment Trees (`sapling_trees` & `orchard_trees`)
Stores the incremental Merkle tree state (frontiers) and anchor history.

| Tree Dimension | Standard `lightwalletd` State | Private Receive Bridge State | Parity Status | Notes |
| :--- | :--- | :--- | :--- | :--- |
| **Commitment Root** | 32-byte tree root anchor | 32-byte tree root anchor | **Identical** | Sourced via `GetTreeState` |
| **Witness Positions**| Monotonically advancing | Monotonically advancing | **Identical** | Formed from `CompactBlock` notes |
| **Subtree Roots** | Subtree root hashes (pool 0/1) | Subtree root hashes (pool 0/1) | **Identical** | Sourced via `GetSubtreeRoots` |

---

## 2. Key Differences in Operation & Retrieval Privacy

While the resulting SQLite database state in the wallet is **bit-for-bit identical**, the operational mechanism and privacy guarantees differ fundamentally:

| Dimension | Standard `lightwalletd` Sync | Private Receive Bridge Sync |
| :--- | :--- | :--- |
| **Transaction Fetch Method** | **Reactive & Selective**: Wallet scans compact blocks, identifies a trial-decryption hit, and requests `GetTransaction(hit_txid)` | **Proactive & Unconditional**: Bridge downloads ALL transactions within the public interval [start..=tip] before wallet queries |
| **Upstream Network Footprint** | Upstream sees: `GetBlockRange`, `GetTreeState`, and selective `GetTransaction(tx1)` | Upstream sees: Interval acquisition for all transactions in range. When wallet queries `GetTransaction(tx1)`, **0 upstream packets are sent** |
| **Retrieval Privacy** | ❌ **Broken**: Remote operator correlates user IP with the exact TxID they received | ✅ **Protected**: Remote operator cannot distinguish which transaction belonged to the user |
| **Offline Rescan Parity** | ❌ **Impossible**: Re-syncing or re-indexing requires re-contacting the remote server | ✅ **Supported**: Once the interval is committed to SQLite, the bridge can be disconnected from the internet and serve 100% locally |
| **Out-of-Coverage Failure** | Remote server attempts full chain search | Local bridge fails immediately with `NOT_FOUND` (Zero-fallback invariant) |

---

## 3. Reproduction & Verification

To reproduce and verify wallet state parity:
1. Review the synthetic protocol test suite in [`crates/bridge-testkit/tests/wallet_probe_verification.rs`](../crates/bridge-testkit/tests/wallet_probe_verification.rs).
2. Inspect the live running daemon measurements in [`artifacts/daemon_running_measurements.jsonl`](../artifacts/daemon_running_measurements.jsonl).
3. Inspect the valid public watch-only viewing key fixture in [`docs/fixtures/public_watch_only_fixture.json`](fixtures/public_watch_only_fixture.json).
