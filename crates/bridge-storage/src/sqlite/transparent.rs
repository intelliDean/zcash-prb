use bridge_core::{BridgeError, IntervalRange, TransparentAddress};
use bridge_proto::{GetAddressUtxosReply, RawTransaction};
use rusqlite::{Connection, params};

pub fn query_address_utxos(
    conn: &Connection,
    address: &TransparentAddress,
) -> Result<Vec<GetAddressUtxosReply>, BridgeError> {
    let addr_str = address.as_str().to_string();

    // Check if this address or general coverage has unresolved pre-coverage spends
    let has_pre_coverage_spend: bool = conn
        .query_row(
            "SELECT 1 FROM pre_coverage_spends WHERE address = ?1 OR address IS NULL LIMIT 1",
            params![addr_str],
            |_| Ok(true),
        )
        .unwrap_or(false);

    if has_pre_coverage_spend {
        return Err(BridgeError::IncompleteHistory {
            txid: "unknown".to_string(),
            vout: 0,
            coverage_start: 0,
        });
    }

    let mut stmt = conn
        .prepare(
            "SELECT txid, vout, address, value_zat, script_pubkey, height 
             FROM transparent_outputs 
             WHERE address = ?1 AND spent_by_txid IS NULL",
        )
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let rows = stmt
        .query_map(params![addr_str], |row| {
            let txid: Vec<u8> = row.get(0)?;
            let vout: u32 = row.get(1)?;
            let addr: String = row.get(2)?;
            let value: u64 = row.get(3)?;
            let script: Vec<u8> = row.get(4)?;
            let height: u32 = row.get(5)?;

            Ok(GetAddressUtxosReply {
                txid,
                index: vout as i32,
                script,
                value_zat: value as i64,
                height: height as u64,
                address: addr,
            })
        })
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let mut utxos = Vec::new();
    for r in rows {
        utxos.push(r.map_err(|e| BridgeError::Storage(e.to_string()))?);
    }
    Ok(utxos)
}

pub fn query_taddress_transactions(
    conn: &Connection,
    address: &TransparentAddress,
    range: Option<IntervalRange>,
) -> Result<Vec<RawTransaction>, BridgeError> {
    let addr_str = address.as_str().to_string();

    let (query, params_vec): (String, Vec<Box<dyn rusqlite::ToSql>>) = match range {
        Some(r) => (
            "SELECT DISTINCT ft.raw_data, ft.height 
             FROM full_transactions ft
             JOIN transparent_outputs t_out ON ft.txid = t_out.txid
             WHERE t_out.address = ?1 AND ft.height >= ?2 AND ft.height <= ?3
             ORDER BY ft.height ASC"
                .to_string(),
            vec![Box::new(addr_str), Box::new(r.start.0), Box::new(r.end.0)],
        ),
        None => (
            "SELECT DISTINCT ft.raw_data, ft.height 
             FROM full_transactions ft
             JOIN transparent_outputs t_out ON ft.txid = t_out.txid
             WHERE t_out.address = ?1
             ORDER BY ft.height ASC"
                .to_string(),
            vec![Box::new(addr_str)],
        ),
    };

    let rusqlite_params: Vec<&dyn rusqlite::ToSql> =
        params_vec.iter().map(|b| b.as_ref()).collect();
    let mut stmt = conn
        .prepare(&query)
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let rows = stmt
        .query_map(rusqlite_params.as_slice(), |row| {
            let raw_data: Vec<u8> = row.get(0)?;
            let height: u32 = row.get(1)?;
            Ok(RawTransaction {
                data: raw_data,
                height: height as u64,
            })
        })
        .map_err(|e| BridgeError::Storage(e.to_string()))?;

    let mut txs = Vec::new();
    for r in rows {
        txs.push(r.map_err(|e| BridgeError::Storage(e.to_string()))?);
    }
    Ok(txs)
}
