//! `probe`: a client that exercises a *running* bridge daemon over gRPC with the
//! RPC sequence a light wallet issues during sync, plus explicit failure paths.
//!
//! Every request/response outcome is printed as one JSON line so the output can
//! be preserved as raw evidence. The command exits non-zero if any expectation
//! fails, so scripts cannot silently report success.

use anyhow::{Result, bail};
use bridge_core::BridgeConfig;
use bridge_proto::compact_tx_streamer_client::CompactTxStreamerClient;
use bridge_proto::{
    BlockId, BlockRange, ChainSpec, Empty, GetSubtreeRootsArg, RawTransaction, TxFilter,
};
use std::time::Instant;
use tonic::Code;
use tonic::transport::Channel;

struct Recorder {
    failures: usize,
}

impl Recorder {
    fn emit(&mut self, rpc: &str, arg: &str, expect: &str, outcome: &str, ok: bool, ms: u128) {
        if !ok {
            self.failures += 1;
        }
        println!(
            "{{\"rpc\":\"{rpc}\",\"arg\":\"{arg}\",\"expect\":\"{expect}\",\"outcome\":\"{}\",\"pass\":{ok},\"elapsed_ms\":{ms}}}",
            outcome.replace('"', "'")
        );
    }
}

pub async fn run_probe(config: &BridgeConfig, endpoint: Option<String>) -> Result<()> {
    let endpoint = endpoint.unwrap_or_else(|| format!("http://{}", config.bind_address));
    let channel = Channel::from_shared(endpoint.clone())?.connect().await?;
    let mut client = CompactTxStreamerClient::new(channel);
    let mut rec = Recorder { failures: 0 };

    // 1. GetLightdInfo
    let t = Instant::now();
    let info = client.get_lightd_info(Empty {}).await?.into_inner();
    rec.emit(
        "GetLightdInfo",
        "",
        "OK",
        &format!(
            "chain={} branch={} upgrade={} height={}",
            info.chain_name, info.consensus_branch_id, info.upgrade_name, info.block_height
        ),
        true,
        t.elapsed().as_millis(),
    );

    // 2. GetLatestBlock
    let t = Instant::now();
    let tip = client.get_latest_block(ChainSpec {}).await?.into_inner();
    rec.emit(
        "GetLatestBlock",
        "",
        "OK",
        &format!("height={}", tip.height),
        true,
        t.elapsed().as_millis(),
    );

    let start = config.coverage_start_height as u64;
    if tip.height < start {
        bail!(
            "bridge tip {} is below coverage_start_height {start}; nothing committed yet",
            tip.height
        );
    }

    // 3. GetBlockRange over full committed coverage
    let t = Instant::now();
    let mut stream = client
        .get_block_range(BlockRange {
            start: Some(BlockId {
                height: start,
                hash: vec![],
            }),
            end: Some(BlockId {
                height: tip.height,
                hash: vec![],
            }),
            pool_types: vec![],
        })
        .await?
        .into_inner();
    let mut txids: Vec<Vec<u8>> = Vec::new();
    let mut blocks = 0u64;
    let mut sapling_outputs = 0usize;
    let mut orchard_actions = 0usize;
    while let Some(b) = stream.message().await? {
        blocks += 1;
        for tx in b.vtx {
            sapling_outputs += tx.outputs.len();
            orchard_actions += tx.actions.len();
            txids.push(tx.txid);
        }
    }
    let expected_blocks = tip.height - start + 1;
    rec.emit(
        "GetBlockRange",
        &format!("{start}..={}", tip.height),
        &format!("{expected_blocks} blocks"),
        &format!(
            "blocks={blocks} txs={} sapling_outputs={sapling_outputs} orchard_actions={orchard_actions}",
            txids.len()
        ),
        blocks == expected_blocks,
        t.elapsed().as_millis(),
    );

    // 4. GetTreeState at coverage start (birthday) and tip
    for h in [start, tip.height] {
        let t = Instant::now();
        let r = client
            .get_tree_state(BlockId {
                height: h,
                hash: vec![],
            })
            .await;
        let (ok, outcome) = match r {
            Ok(ts) => {
                let ts = ts.into_inner();
                (
                    true,
                    format!(
                        "height={} sapling_tree_len={} orchard_tree_len={}",
                        ts.height,
                        ts.sapling_tree.len(),
                        ts.orchard_tree.len()
                    ),
                )
            }
            Err(e) => (false, format!("{:?}: {}", e.code(), e.message())),
        };
        rec.emit(
            "GetTreeState",
            &format!("height={h}"),
            "OK",
            &outcome,
            ok,
            t.elapsed().as_millis(),
        );
    }

    // 5. GetSubtreeRoots, both pools, max_entries=0 (all)
    for pool in [0i32, 1] {
        let t = Instant::now();
        let r = client
            .get_subtree_roots(GetSubtreeRootsArg {
                shielded_protocol: pool,
                start_index: 0,
                max_entries: 0,
            })
            .await;
        let (ok, outcome) = match r {
            Ok(s) => {
                let mut s = s.into_inner();
                let mut n = 0usize;
                while let Some(_r) = s.message().await? {
                    n += 1;
                }
                (true, format!("roots={n}"))
            }
            Err(e) => (false, format!("{:?}: {}", e.code(), e.message())),
        };
        rec.emit(
            "GetSubtreeRoots",
            &format!("pool={pool},start=0,max=0"),
            "OK",
            &outcome,
            ok,
            t.elapsed().as_millis(),
        );
    }

    // 6. GetTransaction for every txid in coverage
    let t = Instant::now();
    let mut served = 0usize;
    let mut tx_bytes = 0usize;
    let mut tx_failures = Vec::new();
    for id in &txids {
        match client
            .get_transaction(TxFilter {
                block: None,
                index: 0,
                hash: id.clone(),
            })
            .await
        {
            Ok(r) => {
                served += 1;
                tx_bytes += r.into_inner().data.len();
            }
            Err(e) => tx_failures.push(format!("{:?}", e.code())),
        }
    }
    rec.emit(
        "GetTransaction",
        &format!("all {} txids in coverage", txids.len()),
        "all served locally",
        &format!(
            "served={served} bytes={tx_bytes} failures={}",
            tx_failures.len()
        ),
        tx_failures.is_empty(),
        t.elapsed().as_millis(),
    );

    // ---- Failure paths: must fail locally, never fall back upstream ----
    let t = Instant::now();
    let r = client
        .get_transaction(TxFilter {
            block: None,
            index: 0,
            hash: vec![0xEE; 32],
        })
        .await;
    let (ok, outcome) = expect_code(r.map(|_| ()), Code::NotFound);
    rec.emit(
        "GetTransaction",
        "txid=eeee..ee (not in coverage)",
        "NotFound",
        &outcome,
        ok,
        t.elapsed().as_millis(),
    );

    let t = Instant::now();
    let r = client
        .get_block(BlockId {
            height: tip.height + 1_000,
            hash: vec![],
        })
        .await;
    let (ok, outcome) = expect_code(r.map(|_| ()), Code::NotFound);
    rec.emit(
        "GetBlock",
        &format!("height={} (above tip)", tip.height + 1_000),
        "NotFound",
        &outcome,
        ok,
        t.elapsed().as_millis(),
    );

    if start > 0 {
        let t = Instant::now();
        let r = client
            .get_tree_state(BlockId {
                height: start - 1,
                hash: vec![],
            })
            .await;
        let (ok, outcome) = expect_code(r.map(|_| ()), Code::NotFound);
        rec.emit(
            "GetTreeState",
            &format!("height={} (below coverage)", start - 1),
            "NotFound",
            &outcome,
            ok,
            t.elapsed().as_millis(),
        );
    }

    let t = Instant::now();
    let r = client
        .send_transaction(RawTransaction {
            data: vec![0u8; 8],
            height: 0,
        })
        .await;
    let (ok, outcome) = expect_code(r.map(|_| ()), Code::PermissionDenied);
    rec.emit(
        "SendTransaction",
        "8 zero bytes",
        "PermissionDenied",
        &outcome,
        ok,
        t.elapsed().as_millis(),
    );

    if rec.failures > 0 {
        bail!("probe: {} expectation(s) failed", rec.failures);
    }
    Ok(())
}

fn expect_code(r: Result<(), tonic::Status>, want: Code) -> (bool, String) {
    match r {
        Ok(()) => (false, "unexpected OK".to_string()),
        Err(e) => (e.code() == want, format!("{:?}: {}", e.code(), e.message())),
    }
}
