#!/usr/bin/env bash
set -euo pipefail

echo "======================================================================"
echo "    Zcash Private Receive Bridge - Retrieval Privacy Audit Tool       "
echo "======================================================================"
echo "Testing: Multi-client disparate queries, cache misses, and failure"
echo "Verification: Upstream network RPC audit for selective leakage"
echo "----------------------------------------------------------------------"

# Run the automated retrieval privacy audit test
cargo test --test privacy_leakage -- --nocapture

echo ""
echo "======================================================================"
echo " AUDIT SUMMARY:"
echo " - Client A (tx1 hit):           Served locally from SQLite cache"
echo " - Client B (tx2 hit):           Served locally from SQLite cache"
echo " - Client C (un-synced txid):    Rejected with NOT_FOUND (0 upstream RPCs)"
echo " - Client D (un-synced block):   Rejected with NOT_FOUND (0 upstream RPCs)"
echo " - Client E (un-synced tree):    Rejected with NOT_FOUND (0 upstream RPCs)"
echo " - Upstream selective fallback:  0 calls (STRICT ZERO-FALLBACK ENFORCED)"
echo "======================================================================"
