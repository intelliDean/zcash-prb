#!/usr/bin/env bash
set -euo pipefail

OUTPUT_DIR="artifacts"
UPSTREAM_TRACE="${OUTPUT_DIR}/upstream_rpc_trace.jsonl"
mkdir -p "${OUTPUT_DIR}"

echo "======================================================================"
echo "    Zcash Private Receive Bridge - Retrieval Privacy Audit Tool       "
echo "======================================================================"
echo "Testing: Multi-client disparate queries, cache misses, and failure paths"
echo "Verification: Real Upstream Network RPC Audit Log"
echo "Trace log output: ${UPSTREAM_TRACE}"
echo "----------------------------------------------------------------------"

# Reset trace log
rm -f "${UPSTREAM_TRACE}"
export PRB_UPSTREAM_TRACE="$(pwd)/${UPSTREAM_TRACE}"

# Run the automated retrieval privacy audit test
cargo test --test privacy_leakage -- --nocapture

if [ ! -f "${UPSTREAM_TRACE}" ]; then
    echo "ERROR: Upstream RPC trace log was not generated!" >&2
    exit 1
fi

TOTAL_UPSTREAM_CALLS=$(wc -l < "${UPSTREAM_TRACE}")
GET_TX_CALLS=$(grep -c '"method":"GetTransaction"' "${UPSTREAM_TRACE}" || true)
UNSYNCED_LEAKS=$(grep -c "99999999" "${UPSTREAM_TRACE}" || true)
FORWARDED_SELECTIVE_CALLS=$(grep -c -E '"method":"(GetAddressUtxos|GetTaddressTxids|GetTaddressTransactions)"' "${UPSTREAM_TRACE}" || true)

echo ""
echo "======================================================================"
echo " EMPIRICAL AUDIT SUMMARY FROM RAW RPC TRACE:"
echo " - Total Upstream RPC Calls (Ingestion): ${TOTAL_UPSTREAM_CALLS}"
echo " - Upstream Transactions Fetched:        ${GET_TX_CALLS} (Known public batch)"
echo " - Client-Triggered Upstream Forwarding: ${FORWARDED_SELECTIVE_CALLS} calls (measured from raw trace)"
echo " - Selective Queries Leaked to Upstream: ${UNSYNCED_LEAKS} (measured from raw trace)"
echo "======================================================================"
echo "Raw upstream trace preserved at: ${UPSTREAM_TRACE}"

