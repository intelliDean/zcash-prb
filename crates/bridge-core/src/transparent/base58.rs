use sha2::{Digest, Sha256};

/// Base58 encoding alphabet used by Bitcoin and Zcash.
const BASE58_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Encodes a 22-byte payload (2-byte prefix + 20-byte hash160) into base58check with double-SHA256 checksum.
pub fn base58check_encode(prefix: [u8; 2], hash160: &[u8; 20]) -> String {
    let mut payload = [0u8; 26];
    payload[0] = prefix[0];
    payload[1] = prefix[1];
    payload[2..22].copy_from_slice(hash160);

    // Double SHA-256 checksum (first 4 bytes of SHA256(SHA256(payload)))
    let first = Sha256::digest(&payload[0..22]);
    let second = Sha256::digest(first);
    payload[22..26].copy_from_slice(&second[0..4]);

    // Base58 encode
    let mut num = payload.to_vec();
    let mut encoded = Vec::new();

    while !num.iter().all(|&b| b == 0) {
        let rem = div_mod_58(&mut num);
        encoded.push(BASE58_ALPHABET[rem as usize]);
    }

    // Preserve leading zeros as '1'
    for &b in &payload {
        if b == 0 {
            encoded.push(BASE58_ALPHABET[0]);
        } else {
            break;
        }
    }

    encoded.reverse();
    String::from_utf8(encoded).unwrap_or_default()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base58check_encoding() {
        let dummy_hash = [0x42u8; 20];
        let addr_mainnet = base58check_encode([0x1c, 0xb8], &dummy_hash);
        assert!(
            addr_mainnet.starts_with("t1"),
            "Mainnet P2PKH should start with t1: {}",
            addr_mainnet
        );

        let addr_testnet = base58check_encode([0x1d, 0x25], &dummy_hash);
        assert!(
            addr_testnet.starts_with("tm"),
            "Testnet P2PKH should start with tm: {}",
            addr_testnet
        );
    }
}
