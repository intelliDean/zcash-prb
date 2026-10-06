use crate::types::{Network, TransparentAddress, TxId};
use crate::error::BridgeError;
use sha2::{Digest, Sha256};

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

/// Base58 encoding alphabet used by Bitcoin and Zcash.
const BASE58_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Encodes 22-byte payload (2-byte prefix + 20-byte hash160) into base58check with double-SHA256 checksum.
pub fn base58check_encode(prefix: [u8; 2], hash160: &[u8; 20]) -> String {
    let mut payload = [0u8; 26];
    payload[0] = prefix[0];
    payload[1] = prefix[1];
    payload[2..22].copy_from_slice(hash160);

    // Double SHA256 checksum
    let first = Sha256::digest(&payload[0..22]);
    let second = Sha256::digest(&first);
    payload[22..26].copy_from_slice(&second[0..4]);

    // Base58 encode
    let mut num = num_bigint(payload.as_slice());
    let mut encoded = Vec::new();
    let zero = 0u8;

    while !num.iter().all(|&b| b == 0) {
        let rem = div_mod_58(&mut num);
        encoded.push(BASE58_ALPHABET[rem as usize]);
    }

    // Preserve leading zeros as '1'
    for &b in &payload {
        if b == zero {
            encoded.push(BASE58_ALPHABET[0]);
        } else {
            break;
        }
    }

    encoded.reverse();
    String::from_utf8(encoded).unwrap_or_default()
}

fn num_bigint(bytes: &[u8]) -> Vec<u8> {
    bytes.to_vec()
}

fn div_mod_58(num: &mut [u8]) -> u32 {
    let mut rem = 0u32;
    for byte in num.iter_mut() {
        let temp = (rem << 8) + (*byte as u32);
        *byte = (temp / 58) as u8;
        rem = temp % 58;
    }
    rem
}

/// Resolves standard Zcash transparent scripts (P2PKH and P2SH) to transparent addresses.
pub fn script_pubkey_to_address(script: &[u8], network: Network) -> TransparentAddress {
    // 1. P2PKH: OP_DUP(0x76) OP_HASH160(0xa9) 0x14 <20 bytes> OP_EQUALVERIFY(0x88) OP_CHECKSIG(0xac)
    if script.len() == 25
        && script[0] == 0x76
        && script[1] == 0xa9
        && script[2] == 0x14
        && script[23] == 0x88
        && script[24] == 0xac
    {
        let mut hash160 = [0u8; 20];
        hash160.copy_from_slice(&script[3..23]);

        let prefix = match network {
            Network::Mainnet => [0x1c, 0xb8],        // Produces "t1..."
            Network::Testnet | Network::Regtest => [0x1d, 0x25], // Produces "tm..."
        };
        return TransparentAddress::new(base58check_encode(prefix, &hash160));
    }

    // 2. P2SH: OP_HASH160(0xa9) 0x14 <20 bytes> OP_EQUAL(0x87)
    if script.len() == 23 && script[0] == 0xa9 && script[1] == 0x14 && script[22] == 0x87 {
        let mut hash160 = [0u8; 20];
        hash160.copy_from_slice(&script[2..22]);

        let prefix = match network {
            Network::Mainnet => [0x1c, 0xbd],        // Produces "t3..."
            Network::Testnet | Network::Regtest => [0x1c, 0xba], // Produces "t2..."
        };
        return TransparentAddress::new(base58check_encode(prefix, &hash160));
    }

    // Unsupported or non-standard transparent script
    TransparentAddress::new(format!("script:{}", hex::encode(script)))
}

/// Reads a compact size integer (VarInt) from bytes slice.
fn read_varint(bytes: &[u8], cursor: &mut usize) -> Result<u64, BridgeError> {
    if *cursor >= bytes.len() {
        return Err(BridgeError::Verification("Unexpected end of transaction".into()));
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

/// Parses transparent inputs and outputs from raw Zcash transaction bytes.
pub fn parse_transparent_transaction(
    raw_tx_bytes: &[u8],
    network: Network,
) -> Result<(Vec<ParsedTransparentInput>, Vec<ParsedTransparentOutput>), BridgeError> {
    if raw_tx_bytes.len() < 4 {
        return Ok((Vec::new(), Vec::new()));
    }

    let mut cursor = 0;
    let header = u32::from_le_bytes(raw_tx_bytes[0..4].try_into().unwrap());
    cursor += 4;

    let is_overwintered = (header >> 31) == 1;
    let version = header & 0x7fff_ffff;

    if is_overwintered {
        if version == 3 || version == 4 {
            // Overwinter or Sapling: 4-byte version_group_id
            cursor += 4;
        } else if version >= 5 {
            // Orchard / Ironwood (v5):
            // version_group_id (4), consensus_branch_id (4), lock_time (4), expiry_height (4)
            cursor += 16;
        }
    }

    if cursor > raw_tx_bytes.len() {
        return Ok((Vec::new(), Vec::new()));
    }

    // 1. Parse Transparent Inputs (vin)
    let vin_count = read_varint(raw_tx_bytes, &mut cursor)?;
    let mut inputs = Vec::with_capacity(vin_count as usize);

    for _ in 0..vin_count {
        if cursor + 36 > raw_tx_bytes.len() {
            return Err(BridgeError::Verification("Truncated input in tx".into()));
        }

        let mut prev_txid_bytes = [0u8; 32];
        prev_txid_bytes.copy_from_slice(&raw_tx_bytes[cursor..cursor + 32]);
        cursor += 32;

        let prev_vout = u32::from_le_bytes(raw_tx_bytes[cursor..cursor + 4].try_into().unwrap());
        cursor += 4;

        let script_len = read_varint(raw_tx_bytes, &mut cursor)? as usize;
        if cursor + script_len + 4 > raw_tx_bytes.len() {
            return Err(BridgeError::Verification("Truncated script_sig in tx".into()));
        }
        cursor += script_len; // skip script_sig
        cursor += 4; // skip sequence

        // Filter out coinbase input (prev_txid is all zeros)
        if prev_txid_bytes != [0u8; 32] {
            inputs.push(ParsedTransparentInput {
                prev_txid: TxId(prev_txid_bytes),
                prev_vout,
            });
        }
    }

    // 2. Parse Transparent Outputs (vout)
    let vout_count = read_varint(raw_tx_bytes, &mut cursor)?;
    let mut outputs = Vec::with_capacity(vout_count as usize);

    for vout_idx in 0..vout_count {
        if cursor + 8 > raw_tx_bytes.len() {
            return Err(BridgeError::Verification("Truncated output value in tx".into()));
        }

        let value_zat = u64::from_le_bytes(raw_tx_bytes[cursor..cursor + 8].try_into().unwrap());
        cursor += 8;

        let script_len = read_varint(raw_tx_bytes, &mut cursor)? as usize;
        if cursor + script_len > raw_tx_bytes.len() {
            return Err(BridgeError::Verification("Truncated script_pubkey in tx".into()));
        }

        let script_pubkey = raw_tx_bytes[cursor..cursor + script_len].to_vec();
        cursor += script_len;

        let address = script_pubkey_to_address(&script_pubkey, network);
        outputs.push(ParsedTransparentOutput {
            vout: vout_idx as u32,
            value_zat,
            address,
            script_pubkey,
        });
    }

    Ok((inputs, outputs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base58check_p2pkh() {
        let dummy_hash = [0x42u8; 20];
        let addr_mainnet = base58check_encode([0x1c, 0xb8], &dummy_hash);
        assert!(addr_mainnet.starts_with("t1"), "Mainnet P2PKH should start with t1: {}", addr_mainnet);

        let addr_testnet = base58check_encode([0x1d, 0x25], &dummy_hash);
        assert!(addr_testnet.starts_with("tm"), "Testnet P2PKH should start with tm: {}", addr_testnet);
    }

    #[test]
    fn test_script_pubkey_p2pkh_resolution() {
        let dummy_hash = [0x55u8; 20];
        let mut script = vec![0x76, 0xa9, 0x14];
        script.extend_from_slice(&dummy_hash);
        script.extend_from_slice(&[0x88, 0xac]);

        let addr = script_pubkey_to_address(&script, Network::Mainnet);
        assert!(addr.as_str().starts_with("t1"));
    }

    #[test]
    fn test_script_pubkey_p2sh_resolution() {
        let dummy_hash = [0xaau8; 20];
        let mut script = vec![0xa9, 0x14];
        script.extend_from_slice(&dummy_hash);
        script.push(0x87);

        let addr = script_pubkey_to_address(&script, Network::Mainnet);
        assert!(addr.as_str().starts_with("t3"));
    }
}
