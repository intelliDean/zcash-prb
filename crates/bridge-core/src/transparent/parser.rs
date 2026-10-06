use super::script::script_pubkey_to_address;
use crate::error::BridgeError;
use crate::types::{Network, TransparentAddress, TxId};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTransparentInput {
    pub prev_txid: TxId,
    pub prev_vout: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedTransparentOutput {
    pub vout: u32,
    pub value_zat: u64,
    pub address: TransparentAddress,
    pub script_pubkey: Vec<u8>,
}

/// Reads a compact size integer (VarInt) from a byte slice.
pub fn read_varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, BridgeError> {
    if *cursor >= bytes.len() {
        return Err(BridgeError::Verification(
            "Unexpected end of transaction".into(),
        ));
    }
    let first = bytes[*cursor];
    *cursor += 1;

    match first {
        0..=0xfc => Ok(first as u64),
        0xfd => {
            if *cursor + 2 > bytes.len() {
                return Err(BridgeError::Verification("Truncated varint".into()));
            }
            let val = u16::from_le_bytes(bytes[*cursor..*cursor + 2].try_into().unwrap());
            *cursor += 2;
            Ok(val as u64)
        }
        0xfe => {
            if *cursor + 4 > bytes.len() {
                return Err(BridgeError::Verification("Truncated varint".into()));
            }
            let val = u32::from_le_bytes(bytes[*cursor..*cursor + 4].try_into().unwrap());
            *cursor += 4;
            Ok(val as u64)
        }
        0xff => {
            if *cursor + 8 > bytes.len() {
                return Err(BridgeError::Verification("Truncated varint".into()));
            }
            let val = u64::from_le_bytes(bytes[*cursor..*cursor + 8].try_into().unwrap());
            *cursor += 8;
            Ok(val)
        }
    }
}

/// Skips the Zcash transaction header, handling overwintered and v5 header structures.
fn skip_transaction_header(raw_tx_bytes: &[u8], cursor: &mut usize) {
    if raw_tx_bytes.len() < 4 {
        return;
    }

    let header = u32::from_le_bytes(raw_tx_bytes[0..4].try_into().unwrap());
    *cursor += 4;

    let is_overwintered = (header >> 31) == 1;
    let version = header & 0x7fff_ffff;

    if is_overwintered {
        if version == 3 || version == 4 {
            // Overwinter or Sapling: 4-byte version_group_id
            *cursor += 4;
        } else if version >= 5 {
            // Orchard / Ironwood (v5):
            // version_group_id (4), consensus_branch_id (4), lock_time (4), expiry_height (4)
            *cursor += 16;
        }
    }
}

/// Parses a single transparent input from the transaction byte stream.
/// Returns None if the input is a coinbase input (prev_txid is all zeros).
fn parse_single_transparent_input(
    raw_tx_bytes: &[u8],
    cursor: &mut usize,
) -> Result<Option<ParsedTransparentInput>, BridgeError> {
    if *cursor + 36 > raw_tx_bytes.len() {
        return Err(BridgeError::Verification("Truncated input in tx".into()));
    }

    let mut prev_txid_bytes = [0u8; 32];
    prev_txid_bytes.copy_from_slice(&raw_tx_bytes[*cursor..*cursor + 32]);
    *cursor += 32;

    let prev_vout = u32::from_le_bytes(raw_tx_bytes[*cursor..*cursor + 4].try_into().unwrap());
    *cursor += 4;

    let script_len = read_varint(raw_tx_bytes, cursor)? as usize;
    if *cursor + script_len + 4 > raw_tx_bytes.len() {
        return Err(BridgeError::Verification(
            "Truncated script_sig in tx".into(),
        ));
    }
    *cursor += script_len; // skip script_sig
    *cursor += 4; // skip sequence

    if prev_txid_bytes == [0u8; 32] {
        Ok(None) // Coinbase input
    } else {
        Ok(Some(ParsedTransparentInput {
            prev_txid: TxId(prev_txid_bytes),
            prev_vout,
        }))
    }
}

/// Parses a single transparent output from the transaction byte stream.
fn parse_single_transparent_output(
    raw_tx_bytes: &[u8],
    cursor: &mut usize,
    vout_idx: u32,
    network: Network,
) -> Result<ParsedTransparentOutput, BridgeError> {
    if *cursor + 8 > raw_tx_bytes.len() {
        return Err(BridgeError::Verification(
            "Truncated output value in tx".into(),
        ));
    }

    let value_zat = u64::from_le_bytes(raw_tx_bytes[*cursor..*cursor + 8].try_into().unwrap());
    *cursor += 8;

    let script_len = read_varint(raw_tx_bytes, cursor)? as usize;
    if *cursor + script_len > raw_tx_bytes.len() {
        return Err(BridgeError::Verification(
            "Truncated script_pubkey in tx".into(),
        ));
    }

    let script_pubkey = raw_tx_bytes[*cursor..*cursor + script_len].to_vec();
    *cursor += script_len;

    let address = script_pubkey_to_address(&script_pubkey, network);
    Ok(ParsedTransparentOutput {
        vout: vout_idx,
        value_zat,
        address,
        script_pubkey,
    })
}

/// Parses transparent inputs and outputs from raw Zcash transaction bytes.
pub fn parse_transparent_transaction(
    raw_tx_bytes: &[u8],
    network: Network,
) -> Result<(Vec<ParsedTransparentInput>, Vec<ParsedTransparentOutput>), BridgeError> {
    if raw_tx_bytes.len() < 4 {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut cursor = 0;
    skip_transaction_header(raw_tx_bytes, &mut cursor);

    if cursor > raw_tx_bytes.len() {
        return Ok((Vec::new(), Vec::new()));
    }

    // 1. Parse Transparent Inputs (vin)
    let vin_count = read_varint(raw_tx_bytes, &mut cursor)?;
    let mut inputs = Vec::with_capacity(vin_count as usize);

    for _ in 0..vin_count {
        if let Some(input) = parse_single_transparent_input(raw_tx_bytes, &mut cursor)? {
            inputs.push(input);
        }
    }

    // 2. Parse Transparent Outputs (vout)
    let vout_count = read_varint(raw_tx_bytes, &mut cursor)?;
    let mut outputs = Vec::with_capacity(vout_count as usize);

    for vout_idx in 0..vout_count {
        let output =
            parse_single_transparent_output(raw_tx_bytes, &mut cursor, vout_idx as u32, network)?;
        outputs.push(output);
    }

    Ok((inputs, outputs))
}
