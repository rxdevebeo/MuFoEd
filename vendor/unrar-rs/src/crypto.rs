//! RAR 5 key derivation and AES-256-CBC decryption.

use aes::cipher::{block_padding::NoPadding, BlockDecryptMut, KeyIvInit};
use hmac::{Hmac, Mac};
use sha1::Sha1;
use sha2::{Digest, Sha256};

use crate::error::{Error, Result};

type Aes256CbcDec = cbc::Decryptor<aes::Aes256>;
type HmacSha256 = Hmac<Sha256>;

pub const SALT_SIZE: usize = 16;
pub const INIT_V_SIZE: usize = 16;
pub const PSW_CHECK_SIZE: usize = 8;
pub const HASH_KEY_SIZE: usize = 32;
pub const AES_BLOCK_SIZE: usize = 16;
pub const MAX_KDF_LG2_COUNT: u8 = 24;
const MAX_PASSWORD_CHARS: usize = 127;

#[derive(Clone)]
pub struct Rar5Key {
    pub key: [u8; 32],
    pub hash_key: [u8; HASH_KEY_SIZE],
    pub psw_check: [u8; PSW_CHECK_SIZE],
}

impl std::fmt::Debug for Rar5Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Rar5Key { .. }")
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

fn normalized_password(password: &str) -> &[u8] {
    match password.char_indices().nth(MAX_PASSWORD_CHARS) {
        Some((idx, _)) => &password.as_bytes()[..idx],
        None => password.as_bytes(),
    }
}

/// PBKDF2-HMAC-SHA256 as implemented by UnRAR. In addition to the 32 byte
/// key it also derives the hash key (16 extra iterations) and the 64 bit
/// password check value (16 more iterations) from the same chain.
fn pbkdf2_rar5(password: &[u8], salt: &[u8], rounds: u32) -> ([u8; 32], [u8; 32], [u8; 32]) {
    const MAX_SALT: usize = 64;
    let salt_len = salt.len().min(MAX_SALT);
    let mut salt_data = Vec::with_capacity(salt_len + 4);
    salt_data.extend_from_slice(&salt[..salt_len]);
    salt_data.extend_from_slice(&[0, 0, 0, 1]);

    let mut u = hmac_sha256(password, &salt_data);
    let mut f = u;

    let mut key = [0u8; 32];
    let mut hash_key = [0u8; 32];
    let mut psw_check_raw = [0u8; 32];

    let counts = [rounds.saturating_sub(1), 16, 16];
    let outputs = [&mut key, &mut hash_key, &mut psw_check_raw];
    for (count, out) in counts.into_iter().zip(outputs) {
        for _ in 0..count {
            u = hmac_sha256(password, &u);
            for (dst, src) in f.iter_mut().zip(u.iter()) {
                *dst ^= *src;
            }
        }
        *out = f;
    }

    (key, hash_key, psw_check_raw)
}

pub fn fold_psw_check(raw: &[u8; 32]) -> [u8; PSW_CHECK_SIZE] {
    let mut out = [0u8; PSW_CHECK_SIZE];
    for (i, byte) in raw.iter().enumerate() {
        out[i % PSW_CHECK_SIZE] ^= byte;
    }
    out
}

/// Derive the RAR 5 key material from a password and salt.
pub fn derive_key(password: &str, salt: &[u8], lg2_count: u8) -> Result<Rar5Key> {
    if lg2_count > MAX_KDF_LG2_COUNT {
        return Err(Error::Unsupported(format!(
            "PBKDF2 iteration count 2^{lg2_count} exceeds the supported maximum"
        )));
    }
    let rounds = 1u32 << lg2_count;
    let (key, hash_key, psw_check_raw) = pbkdf2_rar5(normalized_password(password), salt, rounds);
    let psw_check = fold_psw_check(&psw_check_raw);
    Ok(Rar5Key {
        key,
        hash_key,
        psw_check,
    })
}

/// Decrypt a buffer in place using AES-256-CBC with no padding.
pub fn decrypt_cbc(key: &[u8; 32], iv: &[u8; 16], data: &mut [u8]) -> Result<()> {
    if data.len() % AES_BLOCK_SIZE != 0 {
        return Err(Error::Corrupt(
            "encrypted data length is not a multiple of the AES block size".into(),
        ));
    }
    if data.is_empty() {
        return Ok(());
    }
    let decryptor = Aes256CbcDec::new(key.into(), iv.into());
    decryptor
        .decrypt_padded_mut::<NoPadding>(data)
        .map_err(|_| Error::Corrupt("AES-CBC decryption failed".into()))?;
    Ok(())
}

#[derive(Clone)]
pub struct Rar3Key {
    pub key: [u8; 16],
    pub iv: [u8; 16],
}

impl std::fmt::Debug for Rar3Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Rar3Key { .. }")
    }
}

/// RAR 3.x key derivation: 0x40000 SHA-1 rounds over UTF-16LE password plus
/// optional salt. Every 1/16 of the rounds also contributes one byte of IV.
pub fn derive_key_rar3(password: &str, salt: Option<&[u8; 8]>) -> Rar3Key {
    let pwd = normalized_password(password);
    let text = std::str::from_utf8(pwd).unwrap_or("");
    let mut raw = Vec::with_capacity(text.len() * 2 + 8);
    for unit in text.encode_utf16() {
        raw.push(unit as u8);
        raw.push((unit >> 8) as u8);
    }
    if let Some(salt) = salt {
        raw.extend_from_slice(salt);
    }

    const ROUNDS: u32 = 0x40000;
    const STEP: u32 = ROUNDS / 16;
    let mut sha = Sha1::new();
    let mut iv = [0u8; 16];
    for round in 0..ROUNDS {
        sha.update(&raw);
        sha.update([round as u8, (round >> 8) as u8, (round >> 16) as u8]);
        if round % STEP == 0 {
            let digest = sha.clone().finalize();
            iv[(round / STEP) as usize] = digest[19];
        }
    }
    let digest = sha.finalize();
    // UnRAR extracts the AES key in little-endian word order, which reverses
    // the bytes inside every 4-byte SHA-1 word.
    let mut key = [0u8; 16];
    for word in 0..4 {
        for byte in 0..4 {
            key[word * 4 + byte] = digest[word * 4 + (3 - byte)];
        }
    }
    Rar3Key { key, iv }
}

/// Decrypt a buffer in place using AES-128-CBC with no padding.
pub fn decrypt_cbc_128(key: &[u8; 16], iv: &[u8; 16], data: &mut [u8]) -> Result<()> {
    if data.len() % AES_BLOCK_SIZE != 0 {
        return Err(Error::Corrupt(
            "encrypted data length is not a multiple of the AES block size".into(),
        ));
    }
    if data.is_empty() {
        return Ok(());
    }
    let decryptor = cbc::Decryptor::<aes::Aes128>::new(key.into(), iv.into());
    decryptor
        .decrypt_padded_mut::<NoPadding>(data)
        .map_err(|_| Error::Corrupt("AES-CBC decryption failed".into()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pbkdf2_known_vector() {
        // RFC 7914 style PBKDF2-HMAC-SHA256 vector; only the main key.
        let (key, _, _) = pbkdf2_rar5(b"passwd", b"salt", 1);
        assert_eq!(
            key,
            [
                0x55, 0xac, 0x04, 0x6e, 0x56, 0xe3, 0x08, 0x9f, 0xec, 0x16, 0x91, 0xc2, 0x25, 0x44,
                0xb6, 0x05, 0xf9, 0x41, 0x85, 0x21, 0x6d, 0xde, 0x04, 0x65, 0xe6, 0x8b, 0x9d, 0x57,
                0xc2, 0x0d, 0xac, 0xbc
            ]
        );
    }
}
