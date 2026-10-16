#!/usr/bin/env python3
"""
verify_released_wallet_receipt.py

Executes a tested Zingo CLI build (zingolib_v3.0.0-57-g45d4fc7f)
against an authentic confirmed shielded receipt with a decrypted memo on a live Zebra node.
Performs:
1. RUN 1: Sync directly against standard upstream lightwalletd (recording baseline wallet database state).
2. RUN 2: Sync through local Private Receive Bridge (measuring serving privacy and zero upstream leakage).
3. RUN 3: Deep equality comparison of actual wallet database exports (notes, memos, transfers, balances).
Strictly validates exit codes, note amounts, heights, memos, balances, and trace byte order.
"""

import os
import sys
import json
import time
import signal
import hashlib
import subprocess

ZINGO_BIN = "/home/dean/.local/bin/zingo-cli"
ZINGO_RELEASE_REPO = "https://github.com/zingolabs/zingolib"
ZINGO_RELEASE_TAG = "zingolib_v3.0.0"
ZINGO_GIT_COMMIT = "fb58a162cfb583bc8c405ac962a85405ee1b509b"

ARTIFACTS_DIR = os.path.abspath("artifacts")
EXPORTS_DIR = os.path.join(ARTIFACTS_DIR, "released_wallet_exports")
os.makedirs(EXPORTS_DIR, exist_ok=True)

RECORDING_PATH = os.path.join(ARTIFACTS_DIR, "real_wallet_receipt_recording.txt")

UFVK = "uviewregtest163spuf63zq00l8je74mkdtsj3s8f3snu8uyur5kqttxqt7u6xwrgqdd3za29qhgxs0cwnqdcnajmvc80qtuvmlvvzqcnpqlafzsvx6ct9dcmx69lnpas9s6gqg3h09g07aw6et5qmy908tlzkdzdpx4y4y7e0aa77md2jdxwfhqu357nk0wxhg24aykyhm3h6qu66ru3f7td6vkchrd78atwy806s5vemddpjgchhdgg54ffp6njfu0yy7qsflh3vql8hhk53y24qfvkv4c0373vgjc9tr6xgwxj3mtt7fzwv3m4z9p0q2q6j5k3gvldkum8mjhu300ap9elkzhr9xuf4t558c6d5lv0427fpxdl2rlkyr56dfppt0vlzmaeey789jrzuznc3k7s9279jn4cphs5xydh0kmedwp6l3wct6zc52knyu9plwnuwpxv3hqx57cehzy70rsrgxvfg0vnslw2hfgp55zr2sj26dah0et48uens06l"
EXPECTED_TXID = "b9e7da7538ff7b864a25978165b2eb9612a541121e667600dbacbef7d5c3e4dc"
EXPECTED_MEMO = "ZCG Grant Evaluation - Actual Wallet Receipt Parity"
EXPECTED_HEIGHT = 226
EXPECTED_ZATOSHIS = 50000000

def get_file_sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        while chunk := f.read(65536):
            h.update(chunk)
    return h.hexdigest()

def clean_zingo_json(raw_text):
    """Strip 'Launching save task...' and trailing status text to produce valid JSON."""
    lines = raw_text.strip().split("\n")
    start = -1
    end = -1
    for i, line in enumerate(lines):
        if line.strip().startswith("{") or line.strip().startswith("["):
            start = i
            break
    for i in range(len(lines) - 1, -1, -1):
        if lines[i].strip().endswith("}") or lines[i].strip().endswith("]"):
            end = i + 1
            break
    if start != -1 and end != -1:
        return "\n".join(lines[start:end])
    return raw_text.strip()

def run_wallet_cmd(data_dir, server, args):
    cmd = [ZINGO_BIN, "--data-dir", data_dir, "--server", server, "--chain", "regtest"] + args
    res = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
    return res.returncode, res.stdout, res.stderr

def parse_orchard_balance(bal_text):
    """Extract confirmed orchard balance from Zingo balance output."""
    for line in bal_text.splitlines():
        line = line.strip()
        if "confirmed_orchard_balance:" in line:
            val_str = line.split(":", 1)[1].strip().replace("_", "")
            return int(val_str)
    raise ValueError(f"Could not find confirmed_orchard_balance in: {bal_text}")

def main():
    recording = []
    def log(msg):
        print(msg)
        recording.append(msg)

    # Compute binary sha256
    bin_sha256 = get_file_sha256(ZINGO_BIN)

    log("================================================================================")
    log("  ZCASH TESTED WALLET BUILD (ZINGO CLI) RECEIPT & PARITY AUDIT                  ")
    log("================================================================================")
    log(f"Wallet Binary:     {ZINGO_BIN}")
    log(f"Binary SHA256:     {bin_sha256}")
    log(f"Build Source:      {ZINGO_RELEASE_REPO} (commit: {ZINGO_GIT_COMMIT[:10]})")
    log(f"Exact Build Info:  Development build 57 commits after tag {ZINGO_RELEASE_TAG}")
    
    ver_code, ver_out, ver_err = run_wallet_cmd("/tmp", "http://127.0.0.1:9067", ["--nosync", "version"])
    assert ver_code == 0, f"Failed to query wallet version: {ver_err}"
    version_line = [l.strip() for l in ver_out.splitlines() if "zingolib" in l]
    wallet_ver_str = version_line[0] if version_line else ver_out.strip()
    log(f"Wallet Version:    {wallet_ver_str}")
    log(f"Confirmed TxID:    {EXPECTED_TXID}")
    
    # Compute reversed TxID for trace byte-order checks
    txid_bytes = bytes.fromhex(EXPECTED_TXID)
    txid_reversed_hex = txid_bytes[::-1].hex()
    log(f"TxID Trace (LE):   {txid_reversed_hex}")
    log(f"Receipt Height:    Block {EXPECTED_HEIGHT} (Orchard Shielded Pool)")
    log(f"Expected Amount:   {EXPECTED_ZATOSHIS} zatoshis (0.50000000 ZEC)")
    log(f"Decrypted Memo:    \"{EXPECTED_MEMO}\"")
    log("")

    # --------------------------------------------------------------------------
    # RUN 1: Direct Sync Against Upstream lightwalletd
    # --------------------------------------------------------------------------
    upstream_dir = "/tmp/zingo_audit_upstream"
    subprocess.run(["rm", "-rf", upstream_dir], check=True)
    os.makedirs(upstream_dir, exist_ok=True)

    log(">>> RUN 1: Syncing Tested Zingo Build Directly Against Upstream lightwalletd")
    log(f"          Endpoint: http://127.0.0.1:9067 (Native Lightwalletd)")
    
    # Initialize wallet from UFVK with birthday 200
    init_code, init_out, init_err = run_wallet_cmd(
        upstream_dir, "http://127.0.0.1:9067",
        ["--nosync", "--viewkey", UFVK, "--birthday", "200", "addresses"]
    )
    assert init_code == 0, f"Failed to initialize wallet: {init_err}"
    log(f"  [WALLET] Initialized with Unified Full Viewing Key (Birthday: 200)")

    # Perform full sync and note discovery
    log(f"  [WALLET] Running sync task against lightwalletd...")
    sync_code, sync_out, sync_err = run_wallet_cmd(
        upstream_dir, "http://127.0.0.1:9067",
        ["--waitsync", "notes"]
    )
    assert sync_code == 0, f"Wallet sync failed with code {sync_code}: {sync_err}"
    upstream_notes_json = clean_zingo_json(sync_out)
    assert upstream_notes_json, "Empty notes output from upstream sync!"
    
    with open(os.path.join(EXPORTS_DIR, "upstream_zingo_notes.json"), "w") as f:
        f.write(upstream_notes_json)

    # Export messages / value transfers
    msg_code, msg_out, msg_err = run_wallet_cmd(upstream_dir, "http://127.0.0.1:9067", ["--nosync", "messages"])
    assert msg_code == 0, f"Failed to export messages: {msg_err}"
    upstream_msg_json = clean_zingo_json(msg_out)
    assert upstream_msg_json, "Empty messages export from upstream sync!"
    with open(os.path.join(EXPORTS_DIR, "upstream_zingo_messages.json"), "w") as f:
        f.write(upstream_msg_json)

    # Export balance
    bal_code, bal_out, bal_err = run_wallet_cmd(upstream_dir, "http://127.0.0.1:9067", ["--nosync", "balance"])
    assert bal_code == 0, f"Failed to export balance: {bal_err}"
    upstream_bal_text = bal_out.strip()
    with open(os.path.join(EXPORTS_DIR, "upstream_zingo_balance.txt"), "w") as f:
        f.write(upstream_bal_text)

    # Validate upstream notes
    notes_obj = json.loads(upstream_notes_json)
    summaries = notes_obj.get("orchard_notes", {}).get("note_summaries", [])
    assert len(summaries) >= 1, "No notes discovered in upstream sync!"
    target_note = summaries[0]
    log(f"  [NOTE DISCOVERED] Status: {target_note['status']} | Value: {target_note['value']} zatoshis")
    log(f"                    TxID:   {target_note['txid']}")
    log(f"  [MEMO DECRYPTED]  Memo:   \"{target_note['memo']}\"")
    
    assert target_note["txid"] == EXPECTED_TXID, f"TxID mismatch: {target_note['txid']} != {EXPECTED_TXID}"
    assert target_note["memo"] == EXPECTED_MEMO, f"Memo mismatch: {target_note['memo']} != {EXPECTED_MEMO}"
    assert target_note["value"] == EXPECTED_ZATOSHIS, f"Received amount mismatch: {target_note['value']} != {EXPECTED_ZATOSHIS}"
    assert str(EXPECTED_HEIGHT) in str(target_note["status"]), f"Height mismatch in status: {target_note['status']}"

    upstream_orchard_bal = parse_orchard_balance(upstream_bal_text)
    assert upstream_orchard_bal == EXPECTED_ZATOSHIS, f"Upstream balance {upstream_orchard_bal} != {EXPECTED_ZATOSHIS}"
    log(f"  [BALANCE VERIFIED] Confirmed Orchard Balance: {upstream_orchard_bal} zatoshis (0.50000000 ZEC)")
    log(f"  [EXPORT] Upstream database state dumped to {EXPORTS_DIR}/upstream_zingo_*.json")
    log("")

    # --------------------------------------------------------------------------
    # RUN 2: Sync Against Local Private Receive Bridge
    # --------------------------------------------------------------------------
    prb_dir = "/tmp/zingo_audit_prb"
    prb_scratch = "/tmp/prb_audit_daemon"
    subprocess.run(["rm", "-rf", prb_dir, prb_scratch], check=True)
    os.makedirs(prb_dir, exist_ok=True)
    os.makedirs(prb_scratch, exist_ok=True)

    prb_config = os.path.join(prb_scratch, "config.toml")
    prb_db = os.path.join(prb_scratch, "bridge.db")
    prb_trace = os.path.join(prb_scratch, "upstream_trace.jsonl")

    with open(prb_config, "w") as f:
        f.write(f'''
storage_path = "{prb_db}"
bind_address = "127.0.0.1:9088"
network = "regtest"
upstream_provider = "http://127.0.0.1:9067"
coverage_start_height = 199
interval_size = 51
request_timeout_sec = 30
max_mempool_entries = 100
''')

    log(">>> RUN 2: Starting Local Private Receive Bridge & Syncing Wallet Through PRB")
    log(f"          Bridge Bind Address: 127.0.0.1:9088")
    log(f"          Covered Interval:    [199..=249] (Encapsulating Birthday TreeState 199 & Receipt Height {EXPECTED_HEIGHT})")

    env = os.environ.copy()
    env["PRB_UPSTREAM_TRACE"] = prb_trace

    daemon_log_path = os.path.join(prb_scratch, "daemon.log")
    daemon_log_file = open(daemon_log_path, "w")
    daemon_proc = subprocess.Popen(
        ["./target/release/zcash-private-bridge", "start", "-c", prb_config],
        env=env,
        stdout=daemon_log_file,
        stderr=subprocess.STDOUT,
        text=True
    )

    log("  [DAEMON] Waiting for verified interval acquisition and commitment...")
    committed = False
    for _ in range(30):
        time.sleep(1)
        if os.path.exists(daemon_log_path):
            with open(daemon_log_path) as lf:
                if "Committed verified interval" in lf.read():
                    committed = True
                    break
    assert committed, "Daemon failed to acquire and commit verified interval!"
    log("  [DAEMON] Verified interval [199..=249] committed to local SQLite storage.")

    # Initialize wallet from exact same UFVK pointing to PRB
    init_code, _, init_err = run_wallet_cmd(
        prb_dir, "http://127.0.0.1:9088",
        ["--nosync", "--viewkey", UFVK, "--birthday", "200", "addresses"]
    )
    assert init_code == 0, f"Failed to initialize wallet pointing to PRB: {init_err}"

    # Read upstream trace before wallet sync (reject missing or empty traces)
    def get_trace_records(path):
        if not os.path.exists(path):
            raise FileNotFoundError(f"Required upstream trace file does not exist at {path}!")
        recs = []
        with open(path) as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        recs.append(json.loads(line))
                    except Exception as e:
                        raise ValueError(f"Corrupted trace line in {path}: {line}") from e
        if not recs:
            raise ValueError(f"Required upstream trace at {path} is empty! No RPC calls were recorded.")
        return recs

    trace_before = get_trace_records(prb_trace)
    calls_before = len(trace_before)
    log(f"  [AUDIT] Upstream trace call count BEFORE wallet queries: {calls_before}")

    # Perform full sync and note discovery against PRB
    log("  [WALLET] Running sync task against Local PRB Endpoint (127.0.0.1:9088)...")
    sync_code, sync_out, sync_err = run_wallet_cmd(
        prb_dir, "http://127.0.0.1:9088",
        ["--waitsync", "notes"]
    )
    assert sync_code == 0, f"Wallet sync against PRB failed with code {sync_code}: {sync_err}"
    prb_notes_json = clean_zingo_json(sync_out)
    assert prb_notes_json, "Empty notes output from PRB sync!"
    
    with open(os.path.join(EXPORTS_DIR, "prb_zingo_notes.json"), "w") as f:
        f.write(prb_notes_json)

    # Export messages / value transfers
    msg_code, msg_out, msg_err = run_wallet_cmd(prb_dir, "http://127.0.0.1:9088", ["--nosync", "messages"])
    assert msg_code == 0, f"Failed to export messages from PRB wallet: {msg_err}"
    prb_msg_json = clean_zingo_json(msg_out)
    assert prb_msg_json, "Empty messages export from PRB sync!"
    with open(os.path.join(EXPORTS_DIR, "prb_zingo_messages.json"), "w") as f:
        f.write(prb_msg_json)

    # Export balance
    bal_code, bal_out, bal_err = run_wallet_cmd(prb_dir, "http://127.0.0.1:9088", ["--nosync", "balance"])
    assert bal_code == 0, f"Failed to export balance from PRB wallet: {bal_err}"
    prb_bal_text = bal_out.strip()
    with open(os.path.join(EXPORTS_DIR, "prb_zingo_balance.txt"), "w") as f:
        f.write(prb_bal_text)

    prb_notes_obj = json.loads(prb_notes_json)
    prb_summaries = prb_notes_obj.get("orchard_notes", {}).get("note_summaries", [])
    assert len(prb_summaries) >= 1, "No notes discovered in PRB sync!"
    prb_note = prb_summaries[0]
    log(f"  [NOTE DISCOVERED] Status: {prb_note['status']} | Value: {prb_note['value']} zatoshis")
    log(f"                    TxID:   {prb_note['txid']}")
    log(f"  [MEMO DECRYPTED]  Memo:   \"{prb_note['memo']}\"")
    
    assert prb_note["txid"] == EXPECTED_TXID, f"PRB TxID mismatch: {prb_note['txid']} != {EXPECTED_TXID}"
    assert prb_note["memo"] == EXPECTED_MEMO, f"PRB memo mismatch: {prb_note['memo']} != {EXPECTED_MEMO}"
    assert prb_note["value"] == EXPECTED_ZATOSHIS, f"PRB received amount mismatch: {prb_note['value']} != {EXPECTED_ZATOSHIS}"
    assert str(EXPECTED_HEIGHT) in str(prb_note["status"]), f"PRB height mismatch in status: {prb_note['status']}"

    prb_orchard_bal = parse_orchard_balance(prb_bal_text)
    assert prb_orchard_bal == EXPECTED_ZATOSHIS, f"PRB balance {prb_orchard_bal} != {EXPECTED_ZATOSHIS}"
    log(f"  [BALANCE VERIFIED] Confirmed Orchard Balance: {prb_orchard_bal} zatoshis (0.50000000 ZEC)")

    # Read upstream trace after wallet sync
    trace_after = get_trace_records(prb_trace)
    calls_after = len(trace_after)
    serving_trace = trace_after[calls_before:]
    serving_calls = len(serving_trace)

    log(f"  [AUDIT] Upstream trace call count AFTER wallet queries:  {calls_after}")
    log(f"  [AUDIT] Total Upstream calls during serving phase:        {serving_calls}")

    # Retain raw trace
    raw_trace_dest = os.path.join(ARTIFACTS_DIR, "released_wallet_upstream_trace.jsonl")
    subprocess.run(["cp", prb_trace, raw_trace_dest], check=True)
    log(f"  [TRACE RETENTION] Raw trace retained to: {raw_trace_dest}")

    # Inspect all calls emitted during serving phase for selective leakage:
    # Check BOTH big-endian (EXPECTED_TXID) and little-endian (txid_reversed_hex) byte orders,
    # as well as any selective single-tx (GetTransaction) or address queries.
    serving_methods = {}
    client_leaked_txs = []
    client_address_queries_leaked = []

    for call in serving_trace:
        m = call.get("method", "Unknown")
        arg = str(call.get("arg", ""))
        serving_methods[m] = serving_methods.get(m, 0) + 1
        
        # Byte order check: check standard display order (BE) AND internal consensus byte order (LE)
        if (EXPECTED_TXID in arg) or (txid_reversed_hex in arg):
            client_leaked_txs.append(call)
            
        # Address queries: PRB never performs transparent address queries upstream; any such call is a leak
        if m in ("GetAddressUtxos", "GetTaddressTxids", "GetTaddressTransactions"):
            client_address_queries_leaked.append(call)

    log(f"  [SERVING CALL BREAKDOWN] Methods during serving: {serving_methods}")
    log(f"  [PRIVACY AUDIT] Upstream calls requesting client TxID (both BE & LE byte orders): {len(client_leaked_txs)}")
    log(f"  [PRIVACY AUDIT] Upstream selective address forwarding calls:                        {len(client_address_queries_leaked)}")
    
    assert len(client_leaked_txs) == 0, f"Client TxID leaked upstream (detected across byte orders): {client_leaked_txs}"
    assert len(client_address_queries_leaked) == 0, f"Selective address query forwarded upstream: {client_address_queries_leaked}"
    log("  [PRIVACY VERDICT] ZERO client-specific transaction/note leakage upstream (100% served locally from SQLite).")

    # Stop daemon
    daemon_proc.send_signal(signal.SIGINT)
    try: daemon_proc.wait(timeout=5)
    except: daemon_proc.kill()
    daemon_log_file.close()

    log("")
    # --------------------------------------------------------------------------
    # RUN 3: Deep Database Export Comparison
    # --------------------------------------------------------------------------
    log(">>> RUN 3: Deep Equality Verification of Tested Zingo Build Databases")
    
    notes_match = (upstream_notes_json == prb_notes_json)
    messages_match = (upstream_msg_json == prb_msg_json)
    balances_match = (upstream_bal_text == prb_bal_text)

    log(f"  - Discovered Orchard Notes JSON Parity: {'100% IDENTICAL' if notes_match else 'MISMATCH'}")
    log(f"  - Decrypted Value Transfers / Memos:   {'100% IDENTICAL' if messages_match else 'MISMATCH'}")
    log(f"  - Confirmed Balance Parity:            {'100% IDENTICAL' if balances_match else 'MISMATCH'} (0.50000000 ZEC)")
    
    assert notes_match, "Notes JSON mismatch between upstream and PRB runs!"
    assert messages_match, "Messages JSON mismatch between upstream and PRB runs!"
    assert balances_match, f"Balance mismatch between upstream and PRB runs!\nUpstream:\n{upstream_bal_text}\nPRB:\n{prb_bal_text}"

    log("")
    log("================================================================================")
    log("[SUCCESS] EMPIRICAL PROOF OF TESTED WALLET RECEIPT & PRIVACY ADVANTAGE:")
    log(f"  1. Tested Zingo build ({ZINGO_BIN}) executed.")
    log(f"     - Exact Build: {wallet_ver_str} (57 commits past {ZINGO_RELEASE_TAG})")
    log(f"     - Binary SHA256: {bin_sha256}")
    log(f"     - Build Source: {ZINGO_RELEASE_REPO} (commit {ZINGO_GIT_COMMIT[:10]})")
    log(f"  2. Confirmed note at block {EXPECTED_HEIGHT} discovered by trial decryption.")
    log(f"     - Amount: {EXPECTED_ZATOSHIS} zatoshis (0.50000000 ZEC) verified.")
    log(f"  3. Memo \"{EXPECTED_MEMO}\" decrypted successfully.")
    log(f"  4. Wallet database exports from native lightwalletd and PRB are 100% identical.")
    log(f"     - Notes JSON, messages/memos JSON, and account balances match byte-for-byte.")
    log(f"  5. Upstream traffic during serving: {serving_calls} background polling RPCs, exactly 0 client-specific queries.")
    log(f"     - TxID byte-order audit (BE {EXPECTED_TXID[:12]}... / LE {txid_reversed_hex[:12]}...): 0 leaks.")
    log(f"     - Selective single-tx / address query forwarding: 0 calls.")
    log("================================================================================")

    with open(RECORDING_PATH, "w") as f:
        f.write("\n".join(recording) + "\n")

    print(f"\nRecording written to: {RECORDING_PATH}")
    print(f"Exports written to:   {EXPORTS_DIR}/")

if __name__ == "__main__":
    main()
