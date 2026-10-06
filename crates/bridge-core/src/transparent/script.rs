use super::base58::base58check_encode;
use crate::types::{Network, TransparentAddress};

/// Resolves standard Zcash transparent scripts (P2PKH and P2SH) to transparent addresses.
pub fn script_pubkey_to_address(script: &[u8], network: Network) -> TransparentAddress {
    // 1. P2PKH script: OP_DUP OP_HASH160 <20 bytes> OP_EQUALVERIFY OP_CHECKSIG
    if let Some(hash160) = parse_p2pkh_hash160(script) {
        let prefix = network_p2pkh_prefix(network);
        return TransparentAddress::new(base58check_encode(prefix, &hash160));
    }

    // 2. P2SH script: OP_HASH160 <20 bytes> OP_EQUAL
    if let Some(hash160) = parse_p2sh_hash160(script) {
        let prefix = network_p2sh_prefix(network);
        return TransparentAddress::new(base58check_encode(prefix, &hash160));
    }

    // Non-standard or unrecognized transparent script: prefix with script:<hex>
    TransparentAddress::new(format!("script:{}", hex::encode(script)))
}

/// Identifies P2PKH script: OP_DUP(0x76) OP_HASH160(0xa9) 0x14 <20 bytes> OP_EQUALVERIFY(0x88) OP_CHECKSIG(0xac).
fn parse_p2pkh_hash160(script: &[u8]) -> Option<[u8; 20]> {
    if script.len() == 25
        && script[0] == 0x76
        && script[1] == 0xa9
        && script[2] == 0x14
        && script[23] == 0x88
        && script[24] == 0xac
    {
        let mut hash160 = [0u8; 20];
        hash160.copy_from_slice(&script[3..23]);
        Some(hash160)
    } else {
        None
    }
}

/// Identifies P2SH script: OP_HASH160(0xa9) 0x14 <20 bytes> OP_EQUAL(0x87).
fn parse_p2sh_hash160(script: &[u8]) -> Option<[u8; 20]> {
    if script.len() == 23 && script[0] == 0xa9 && script[1] == 0x14 && script[22] == 0x87 {
        let mut hash160 = [0u8; 20];
        hash160.copy_from_slice(&script[2..22]);
        Some(hash160)
    } else {
        None
    }
}

fn network_p2pkh_prefix(network: Network) -> [u8; 2] {
    match network {
        Network::Mainnet => [0x1c, 0xb8],                    // "t1..."
        Network::Testnet | Network::Regtest => [0x1d, 0x25], // "tm..."
    }
}

fn network_p2sh_prefix(network: Network) -> [u8; 2] {
    match network {
        Network::Mainnet => [0x1c, 0xbd],                    // "t3..."
        Network::Testnet | Network::Regtest => [0x1c, 0xba], // "t2..."
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
