#!/usr/bin/env bash
set -euo pipefail

BLOCKS="${1:-10}"
CONFIG_FILE="${2:-config/bridge.default.toml}"
OUTPUT_DIR="artifacts"
RAW_LOG="${OUTPUT_DIR}/benchmark_raw_output.txt"
TIME_LOG="${OUTPUT_DIR}/benchmark_time_output.txt"

mkdir -p "${OUTPUT_DIR}"

echo "=========================================================================="
echo "    Zcash Private Receive Bridge - Operating Advantage Benchmark Tool    "
echo "=========================================================================="
echo "Target Evaluation Window: ${BLOCKS} blocks"
echo "Configuration:            ${CONFIG_FILE}"
echo "Preserving raw logs to:   ${RAW_LOG}"
echo "--------------------------------------------------------------------------"

# Ensure release binary is compiled; exit immediately on failure
cargo build --release --bin zcash-private-bridge

# Execute benchmark with resource tracking using python3 standard library; DO NOT mask failures
echo "Running benchmark measurement..."
set +e
python3 -c '
import subprocess, resource, sys, time

args = ["./target/release/zcash-private-bridge", "benchmark", "--blocks", sys.argv[1], "--config", sys.argv[2]]
start = time.time()
proc = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
elapsed = time.time() - start
usage = resource.getrusage(resource.RUSAGE_CHILDREN)

with open(sys.argv[3], "w") as f:
    f.write(proc.stdout)

with open(sys.argv[4], "w") as f:
    f.write(proc.stderr)
    f.write(f"\nElapsed (seconds): {elapsed:.2f}\n")
    f.write(f"Maximum resident set size (kbytes): {usage.ru_maxrss}\n")

sys.exit(proc.returncode)
' "${BLOCKS}" "${CONFIG_FILE}" "${RAW_LOG}" "${TIME_LOG}"
BENCH_STATUS=$?
set -e

if [ ${BENCH_STATUS} -ne 0 ]; then
    echo "ERROR: Benchmark execution failed with exit code ${BENCH_STATUS}!" >&2
    echo "--- Standard Error Output ---" >&2
    cat "${TIME_LOG}" >&2
    echo "--- Standard Output ---" >&2
    cat "${RAW_LOG}" >&2
    exit ${BENCH_STATUS}
fi

# Parse empirical metrics directly from output
cat "${RAW_LOG}"

PEAK_RSS_KB=$(grep "Maximum resident set size (kbytes):" "${TIME_LOG}" | awk '{print $NF}')
if [ -z "${PEAK_RSS_KB}" ]; then
    echo "ERROR: Failed to extract peak RSS from time output!" >&2
    exit 1
fi
PEAK_RSS_MB=$(echo "scale=2; ${PEAK_RSS_KB} / 1024" | bc)

DB_PATH=$(grep "storage_path" "${CONFIG_FILE}" | cut -d'=' -f2 | tr -d ' "')
if [ -z "${DB_PATH}" ]; then
    DB_PATH="data/bridge.db"
fi

DB_SIZE_BYTES=0
for f in "${DB_PATH}" "${DB_PATH}-wal" "${DB_PATH}-shm"; do
    if [ -f "${f}" ]; then
        SZ=$(stat -c %s "${f}")
        DB_SIZE_BYTES=$((DB_SIZE_BYTES + SZ))
    fi
done
DB_SIZE_MB=$(echo "scale=2; ${DB_SIZE_BYTES} / (1024 * 1024)" | bc)

# Check for local running Zebra instance
ZEBRA_RUNNING=0
ZEBRA_RSS_MB="N/A"
if pgrep -x zebrad > /dev/null 2>&1; then
    ZEBRA_RUNNING=1
    ZEBRA_PID=$(pgrep -x zebrad | head -n1)
    ZEBRA_RSS_KB=$(ps -p "${ZEBRA_PID}" -o rss= | tr -d ' ')
    ZEBRA_RSS_MB=$(echo "scale=2; ${ZEBRA_RSS_KB} / 1024" | bc)
fi

echo ""
echo "=========================================================================="
echo " EMPIRICAL RESOURCE MEASUREMENT: LOCAL PRB STACK (LIVE RUN)"
echo "=========================================================================="
printf "%-32s | %-32s\n" "Measured Metric" "Private Receive Bridge (Empirical)"
echo "---------------------------------+---------------------------------"
printf "%-32s | %-32s\n" "Peak Process RSS" "${PEAK_RSS_MB} MB"
printf "%-32s | %-32s\n" "Local SQLite Storage (DB+WAL)" "${DB_SIZE_MB} MB (${DB_SIZE_BYTES} bytes)"
printf "%-32s | %-32s\n" "Blocks Evaluated" "${BLOCKS} blocks"
echo "=========================================================================="

echo ""
echo "=========================================================================="
echo " COMPARATIVE CONTEXT (Documented Zebra Specs vs Local PRB Empirical)"
echo "=========================================================================="
if [ ${ZEBRA_RUNNING} -eq 1 ]; then
    echo "Detected running local zebrad process (PID ${ZEBRA_PID}, RSS: ${ZEBRA_RSS_MB} MB)."
else
    echo "Note: No local Zebra daemon was detected running on this host."
    echo "Baseline figures below cite published Zcash Foundation specifications (Documented Specs)."
fi
echo "--------------------------------------------------------------------------"
printf "%-26s | %-28s | %-22s\n" "Dimension" "Zebra (Documented Baseline)" "PRB (Empirical Window)"
echo "---------------------------+------------------------------+-----------------------"
printf "%-26s | %-28s | %-22s\n" "Storage Architecture" "Full Chain (~200 GB SSD)" "Windowed SQLite (${DB_SIZE_MB} MB)"
printf "%-26s | %-28s | %-22s\n" "Memory Footprint (RSS)" "4,096 - 8,192 MB (spec)" "${PEAK_RSS_MB} MB (measured)"
printf "%-26s | %-28s | %-22s\n" "Daemon Architecture" "2 daemons + RPC wiring" "1 standalone binary"
printf "%-26s | %-28s | %-22s\n" "Consensus Validation" "Full PoW consensus rules" "Adjacency + Tx hash checks"
printf "%-26s | %-28s | %-22s\n" "Retrieval Privacy Model" "Full validation / local" "Unconditional batch interval"
echo "=========================================================================="
echo "Raw benchmark logs saved to: ${RAW_LOG} and ${TIME_LOG}"

