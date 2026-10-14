#!/usr/bin/env python3
"""
measure_running_daemon.py

Empirical performance, verification, storage (DB+WAL), and serving latency
measurement tool for the running zcash-private-bridge daemon.
Evaluates both older historical coverage and recent mainnet coverage.
"""

import os
import sys
import time
import glob
import json
import signal
import subprocess
import urllib.request

ARTIFACTS_DIR = os.path.abspath("artifacts")
os.makedirs(ARTIFACTS_DIR, exist_ok=True)

MEASUREMENTS_JSONL = os.path.join(ARTIFACTS_DIR, "daemon_running_measurements.jsonl")
STORAGE_BREAKDOWN_TXT = os.path.join(ARTIFACTS_DIR, "daemon_storage_breakdown.txt")
COVERAGE_COMPARISON_JSON = os.path.join(ARTIFACTS_DIR, "recent_vs_older_coverage.json")

def get_dir_size(path_prefix):
    total = 0
    breakdown = {}
    for f in glob.glob(path_prefix + "*"):
        sz = os.path.getsize(f)
        total += sz
        breakdown[os.path.basename(f)] = sz
    return total, breakdown

def run_daemon_measurement(coverage_name, start_height, block_count, port=9088):
    print(f"\n=======================================================")
    print(f" MEASURING RUNNING DAEMON: {coverage_name}")
    print(f" Height Range: [{start_height}..={start_height + block_count - 1}] ({block_count} blocks)")
    print(f" Port: {port}")
    print(f"=======================================================")

    scratch_dir = os.path.abspath(f"scratch/daemon_meas_{coverage_name}")
    os.makedirs(scratch_dir, exist_ok=True)
    db_path = os.path.join(scratch_dir, "bridge.db")
    for f in glob.glob(db_path + "*"):
        try: os.remove(f)
        except: pass

    config_path = os.path.join(scratch_dir, "config.toml")
    with open(config_path, "w") as f:
        f.write(f'''
storage_path = "{db_path}"
bind_address = "127.0.0.1:{port}"
network = "mainnet"
upstream_provider = "https://zec.rocks:443"
coverage_start_height = {start_height}
interval_size = {block_count}
request_timeout_sec = 30
max_mempool_entries = 100
''')

    log_path = os.path.join(scratch_dir, "daemon.log")
    log_file = open(log_path, "w")

    start_time = time.time()
    daemon_proc = subprocess.Popen(
        ["./target/release/zcash-private-bridge", "start", "-c", config_path],
        stdout=log_file,
        stderr=subprocess.STDOUT,
        text=True
    )

    # Monitor until interval committed
    committed = False
    peak_rss_kb = 0
    sync_duration = 0.0

    print("Syncing interval and verifying transactions...")
    for _ in range(60):
        time.sleep(1)
        if daemon_proc.poll() is not None:
            print("Daemon exited prematurely!")
            break

        # Check RSS via /proc
        try:
            with open(f"/proc/{daemon_proc.pid}/status") as s:
                for line in s:
                    if line.startswith("VmRSS:"):
                        rss = int(line.split()[1])
                        if rss > peak_rss_kb:
                            peak_rss_kb = rss
        except Exception:
            pass

        # Check if committed interval is in logs
        if os.path.exists(log_path):
            with open(log_path) as lf:
                content = lf.read()
                if "Committed verified interval" in content:
                    committed = True
                    sync_duration = time.time() - start_time
                    print(f"Interval committed in {sync_duration:.2f}s!")
                    break

    if not committed:
        print("Failed to sync within timeout! Checking log:")
        with open(log_path) as lf:
            print(lf.read()[-1000:])
        daemon_proc.terminate()
        log_file.close()
        return None

    # Measure storage (DB, WAL, SHM)
    total_db_bytes, storage_breakdown = get_dir_size(db_path)
    print(f"Storage Breakdown: {storage_breakdown} (Total: {total_db_bytes} bytes)")

    # Measure serving latency by executing probe against running daemon port
    print("Measuring gRPC serving latency against live daemon...")
    probe_start = time.time()
    probe_run = subprocess.run(
        ["./target/release/zcash-private-bridge", "probe", "-c", config_path, "--endpoint", f"http://127.0.0.1:{port}"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True
    )
    probe_duration = time.time() - probe_start

    probe_lines = []
    latencies = []
    for line in probe_run.stdout.strip().split("\n"):
        if line.strip().startswith("{"):
            try:
                rec = json.loads(line)
                probe_lines.append(rec)
                if "elapsed_ms" in rec:
                    latencies.append(rec["elapsed_ms"])
            except Exception:
                pass

    # Stop daemon gracefully
    print("Initiating graceful shutdown (SIGINT)...")
    daemon_proc.send_signal(signal.SIGINT)
    try:
        daemon_proc.wait(timeout=10)
    except subprocess.TimeoutExpired:
        daemon_proc.kill()
    log_file.close()

    # Re-check storage post-shutdown (checkpointed)
    total_post_bytes, storage_post_breakdown = get_dir_size(db_path)

    avg_latency_ms = sum(latencies) / len(latencies) if latencies else 0.0
    max_latency_ms = max(latencies) if latencies else 0.0

    result = {
        "coverage_name": coverage_name,
        "start_height": start_height,
        "end_height": start_height + block_count - 1,
        "block_count": block_count,
        "sync_and_verification_seconds": round(sync_duration, 2),
        "peak_rss_mb": round(peak_rss_kb / 1024.0, 2),
        "pre_shutdown_storage_bytes": total_db_bytes,
        "pre_shutdown_breakdown": storage_breakdown,
        "post_shutdown_storage_bytes": total_post_bytes,
        "post_shutdown_breakdown": storage_post_breakdown,
        "probe_executed_rpcs": len(probe_lines),
        "serving_avg_latency_ms": round(avg_latency_ms, 2),
        "serving_max_latency_ms": max_latency_ms,
        "probe_all_passed": all(r.get("pass", False) for r in probe_lines),
        "probe_details": probe_lines,
    }

    return result

def main():
    # 1. Historical Older Coverage (e.g. Block 2500000..=2500009)
    older_result = run_daemon_measurement("older_historical_nu5", 2500000, 10, port=9088)

    # 2. Recent Mainnet Coverage (e.g. Block 3516600..=3516609)
    recent_result = run_daemon_measurement("recent_mainnet_tip", 3516600, 10, port=9089)

    results = {
        "timestamp_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "evaluations": [older_result, recent_result]
    }

    with open(COVERAGE_COMPARISON_JSON, "w") as f:
        json.dump(results, f, indent=2)

    with open(MEASUREMENTS_JSONL, "w") as f:
        if older_result:
            for p in older_result.get("probe_details", []):
                p["coverage"] = "older_historical_nu5"
                f.write(json.dumps(p) + "\n")
        if recent_result:
            for p in recent_result.get("probe_details", []):
                p["coverage"] = "recent_mainnet_tip"
                f.write(json.dumps(p) + "\n")

    with open(STORAGE_BREAKDOWN_TXT, "w") as f:
        f.write("=== ZCASH PRIVATE RECEIVE BRIDGE: LIVE DAEMON STORAGE & WAL BREAKDOWN ===\n\n")
        for res in [older_result, recent_result]:
            if not res: continue
            name = res["coverage_name"]
            f.write(f"--- Coverage Window: {name} (Blocks {res['start_height']}..={res['end_height']}) ---\n")
            f.write(f"Sync & Verification Duration:  {res['sync_and_verification_seconds']} s\n")
            f.write(f"Peak Process RSS:              {res['peak_rss_mb']} MB\n")
            f.write(f"Storage Pre-Shutdown (Active WAL):\n")
            for k, v in res["pre_shutdown_breakdown"].items():
                f.write(f"  - {k}: {v} bytes ({v/1024:.2f} KB)\n")
            f.write(f"  Total Pre-Shutdown: {res['pre_shutdown_storage_bytes']} bytes ({res['pre_shutdown_storage_bytes']/1024:.2f} KB)\n")
            f.write(f"Storage Post-Shutdown (Checkpointed):\n")
            for k, v in res["post_shutdown_breakdown"].items():
                f.write(f"  - {k}: {v} bytes ({v/1024:.2f} KB)\n")
            f.write(f"  Total Post-Shutdown: {res['post_shutdown_storage_bytes']} bytes ({res['post_shutdown_storage_bytes']/1024:.2f} KB)\n")
            f.write(f"Serving Average Latency:       {res['serving_avg_latency_ms']} ms\n")
            f.write(f"Serving Max Latency:           {res['serving_max_latency_ms']} ms\n\n")

    print("\nMeasurements completed successfully!")
    print(f"Comparison saved to: {COVERAGE_COMPARISON_JSON}")
    print(f"Storage breakdown saved to: {STORAGE_BREAKDOWN_TXT}")

if __name__ == "__main__":
    main()
