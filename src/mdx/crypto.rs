//! MDict v2 keyword-index decryption.
//!
//! The format (from the MDict spec / canonical readers): when the header's
//! `Encrypted` attribute has bit 0x02 set, the compressed keyword-index block is
//! obfuscated. The 16-byte key is `RIPEMD128(adler_bytes ++ 0x95 0x36 0x00 0x00)`
//! where `adler_bytes` are the 4 big-endian checksum bytes stored at offset 4 of
//! the block, and `0x95 0x36 0x00 0x00` is the little-endian encoding of the
//! constant `0x3695`. Each payload byte is then transformed with a nibble-swap
//! XOR stream keyed on the byte index, the running previous ciphertext byte, and
//! the RIPEMD128 key.

use ripemd::{Digest, Ripemd128};

/// RIPEMD128 digest of `data` (16 bytes).
pub fn ripemd128(data: &[u8]) -> [u8; 16] {
    let mut hasher = Ripemd128::new();
    hasher.update(data);
    let out = hasher.finalize();
    let mut buf = [0u8; 16];
    buf.copy_from_slice(&out);
    buf
}

/// The fixed 4-byte salt appended to the block checksum before hashing.
/// `pack("<L", 0x3695)` == `[0x95, 0x36, 0x00, 0x00]`.
const SALT: [u8; 4] = [0x95, 0x36, 0x00, 0x00];

/// In-place nibble-swap XOR stream cipher used by MDict.
pub fn fast_decrypt(data: &mut [u8], key: &[u8]) {
    debug_assert!(!key.is_empty());
    let mut previous = 0x36u8;
    for (i, byte) in data.iter_mut().enumerate() {
        let cipher = *byte;
        let swapped = cipher.rotate_left(4);
        *byte = swapped ^ previous ^ (i as u8) ^ key[i % key.len()];
        previous = cipher;
    }
}

/// Decrypt the keyword-index block in place.
///
/// `block` is the raw `key_block_info` bytes: `[0..4]` compression type,
/// `[4..8]` the big-endian adler32 of the *decompressed* payload (also the key
/// material), `[8..]` the encrypted+compressed body. The header (`[0..8]`) is
/// left untouched; only the body is decrypted.
pub fn decrypt_key_index(block: &mut [u8]) {
    if block.len() < 8 {
        return;
    }
    let mut material = Vec::with_capacity(8);
    material.extend_from_slice(&block[4..8]);
    material.extend_from_slice(&SALT);
    let key = ripemd128(&material);
    fast_decrypt(&mut block[8..], &key);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn ripemd128_known_vectors() {
        assert_eq!(hex(&ripemd128(b"")), "cdf26213a150dc3ecb610f18f6b38b46");
        assert_eq!(hex(&ripemd128(b"a")), "86be7afa339d0fc7cfc785e72f578d33");
        assert_eq!(hex(&ripemd128(b"abc")), "c14a12199c66e4ba84636b0f69144c77");
    }

    #[test]
    fn fast_decrypt_is_self_inverse_with_encrypt() {
        // The matching encrypt transform: byte = rotl4(plain ^ prev ^ i ^ key).
        fn encrypt(data: &mut [u8], key: &[u8]) {
            let mut previous = 0x36u8;
            for (i, byte) in data.iter_mut().enumerate() {
                let plain = *byte;
                *byte = (plain ^ previous ^ (i as u8) ^ key[i % key.len()]).rotate_left(4);
                previous = *byte;
            }
        }
        let key = ripemd128(b"some key material");
        let original = b"the quick brown fox".to_vec();
        let mut buf = original.clone();
        encrypt(&mut buf, &key);
        assert_ne!(buf, original);
        fast_decrypt(&mut buf, &key);
        assert_eq!(buf, original);
    }
}
