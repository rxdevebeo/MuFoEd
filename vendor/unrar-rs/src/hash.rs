//! CRC32 and BLAKE2sp hashing used by the RAR 5 format.
//!
//! BLAKE2sp is not available in the usual `blake2` crates, so it is
//! implemented here directly from the reference algorithm. The parameter
//! block matches the one used by UnRAR (digest 32 bytes, fanout 8, depth 2).

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::error::{Error, Result};

pub const BLAKE2_DIGEST_SIZE: usize = 32;
type HmacSha256 = Hmac<Sha256>;

const BLAKE2S_BLOCKBYTES: usize = 64;
const BLAKE2S_OUTBYTES: usize = 32;
const PARALLELISM_DEGREE: usize = 8;

const IV: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];

const SIGMA: [[usize; 16]; 10] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
];

#[inline(always)]
fn g(v: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize, x: u32, y: u32) {
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
    v[d] = (v[d] ^ v[a]).rotate_right(16);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(12);
    v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
    v[d] = (v[d] ^ v[a]).rotate_right(8);
    v[c] = v[c].wrapping_add(v[d]);
    v[b] = (v[b] ^ v[c]).rotate_right(7);
}

#[derive(Clone)]
struct Blake2s {
    h: [u32; 8],
    t: [u32; 2],
    f: [u32; 2],
    buf: [u8; 2 * BLAKE2S_BLOCKBYTES],
    buflen: usize,
    last_node: bool,
}

impl Blake2s {
    fn init_param(node_offset: u32, node_depth: u32) -> Self {
        let mut state = Blake2s {
            h: IV,
            t: [0; 2],
            f: [0; 2],
            buf: [0; 2 * BLAKE2S_BLOCKBYTES],
            buflen: 0,
            last_node: false,
        };
        state.h[0] ^= 0x0208_0020;
        state.h[2] ^= node_offset;
        state.h[3] ^= (node_depth << 16) | 0x2000_0000;
        state
    }

    fn compress(&mut self, block: &[u8]) {
        debug_assert!(block.len() >= BLAKE2S_BLOCKBYTES);
        let mut m = [0u32; 16];
        for (i, word) in m.iter_mut().enumerate() {
            *word = u32::from_le_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }

        let mut v = [0u32; 16];
        v[..8].copy_from_slice(&self.h);
        v[8] = IV[0];
        v[9] = IV[1];
        v[10] = IV[2];
        v[11] = IV[3];
        v[12] = self.t[0] ^ IV[4];
        v[13] = self.t[1] ^ IV[5];
        v[14] = self.f[0] ^ IV[6];
        v[15] = self.f[1] ^ IV[7];

        for sigma in &SIGMA {
            g(&mut v, 0, 4, 8, 12, m[sigma[0]], m[sigma[1]]);
            g(&mut v, 1, 5, 9, 13, m[sigma[2]], m[sigma[3]]);
            g(&mut v, 2, 6, 10, 14, m[sigma[4]], m[sigma[5]]);
            g(&mut v, 3, 7, 11, 15, m[sigma[6]], m[sigma[7]]);
            g(&mut v, 0, 5, 10, 15, m[sigma[8]], m[sigma[9]]);
            g(&mut v, 1, 6, 11, 12, m[sigma[10]], m[sigma[11]]);
            g(&mut v, 2, 7, 8, 13, m[sigma[12]], m[sigma[13]]);
            g(&mut v, 3, 4, 9, 14, m[sigma[14]], m[sigma[15]]);
        }

        for i in 0..8 {
            self.h[i] ^= v[i] ^ v[i + 8];
        }
    }

    fn increment_counter(&mut self, inc: u32) {
        let (new, overflow) = self.t[0].overflowing_add(inc);
        self.t[0] = new;
        if overflow {
            self.t[1] = self.t[1].wrapping_add(1);
        }
    }

    fn update(&mut self, mut input: &[u8]) {
        while !input.is_empty() {
            let left = self.buflen;
            let fill = 2 * BLAKE2S_BLOCKBYTES - left;
            if input.len() > fill {
                self.buf[left..left + fill].copy_from_slice(&input[..fill]);
                self.buflen += fill;
                self.increment_counter(BLAKE2S_BLOCKBYTES as u32);
                let block = self.buf;
                self.compress(&block[..BLAKE2S_BLOCKBYTES]);
                self.buf
                    .copy_within(BLAKE2S_BLOCKBYTES..2 * BLAKE2S_BLOCKBYTES, 0);
                self.buflen -= BLAKE2S_BLOCKBYTES;
                input = &input[fill..];
            } else {
                self.buf[left..left + input.len()].copy_from_slice(input);
                self.buflen += input.len();
                return;
            }
        }
    }

    fn finalize(mut self, out: &mut [u8; BLAKE2S_OUTBYTES]) {
        if self.buflen > BLAKE2S_BLOCKBYTES {
            self.increment_counter(BLAKE2S_BLOCKBYTES as u32);
            let block = self.buf;
            self.compress(&block[..BLAKE2S_BLOCKBYTES]);
            self.buflen -= BLAKE2S_BLOCKBYTES;
            let len = self.buflen;
            self.buf
                .copy_within(BLAKE2S_BLOCKBYTES..BLAKE2S_BLOCKBYTES + len, 0);
        }

        self.increment_counter(self.buflen as u32);
        if self.last_node {
            self.f[1] = u32::MAX;
        }
        self.f[0] = u32::MAX;
        self.buf[self.buflen..].fill(0);
        let block = self.buf;
        self.compress(&block[..BLAKE2S_BLOCKBYTES]);

        for i in 0..8 {
            out[i * 4..i * 4 + 4].copy_from_slice(&self.h[i].to_le_bytes());
        }
    }
}

/// Streaming BLAKE2sp hash.
#[derive(Clone)]
pub struct Blake2sp {
    leaves: [Blake2s; PARALLELISM_DEGREE],
    root: Blake2s,
    buf: [u8; PARALLELISM_DEGREE * BLAKE2S_BLOCKBYTES],
    buflen: usize,
}

impl Default for Blake2sp {
    fn default() -> Self {
        Self::new()
    }
}

impl Blake2sp {
    pub fn new() -> Self {
        let mut leaves = std::array::from_fn(|_| Blake2s::init_param(0, 0));
        for (i, leaf) in leaves.iter_mut().enumerate() {
            *leaf = Blake2s::init_param(i as u32, 0);
        }
        let mut root = Blake2s::init_param(0, 1);
        root.last_node = true;
        leaves[PARALLELISM_DEGREE - 1].last_node = true;
        Blake2sp {
            leaves,
            root,
            buf: [0; PARALLELISM_DEGREE * BLAKE2S_BLOCKBYTES],
            buflen: 0,
        }
    }

    pub fn update(&mut self, mut input: &[u8]) {
        let mut left = self.buflen;
        let fill = self.buf.len() - left;
        if left > 0 && input.len() >= fill {
            self.buf[left..left + fill].copy_from_slice(&input[..fill]);
            for i in 0..PARALLELISM_DEGREE {
                let block = &self.buf[i * BLAKE2S_BLOCKBYTES..(i + 1) * BLAKE2S_BLOCKBYTES];
                self.leaves[i].update(block);
            }
            input = &input[fill..];
            left = 0;
        }

        let full = input.len() - input.len() % (PARALLELISM_DEGREE * BLAKE2S_BLOCKBYTES);
        for (i, leaf) in self.leaves.iter_mut().enumerate() {
            let mut pos = i * BLAKE2S_BLOCKBYTES;
            while pos < full {
                leaf.update(&input[pos..pos + BLAKE2S_BLOCKBYTES]);
                pos += PARALLELISM_DEGREE * BLAKE2S_BLOCKBYTES;
            }
        }

        let rem = input.len() - full;
        if rem > 0 {
            self.buf[left..left + rem].copy_from_slice(&input[full..]);
        }
        self.buflen = left + rem;
    }

    pub fn finalize(mut self) -> [u8; BLAKE2_DIGEST_SIZE] {
        let mut hashes = [[0u8; BLAKE2S_OUTBYTES]; PARALLELISM_DEGREE];
        for (i, hash) in hashes.iter_mut().enumerate() {
            if self.buflen > i * BLAKE2S_BLOCKBYTES {
                let mut left = self.buflen - i * BLAKE2S_BLOCKBYTES;
                if left > BLAKE2S_BLOCKBYTES {
                    left = BLAKE2S_BLOCKBYTES;
                }
                self.leaves[i]
                    .update(&self.buf[i * BLAKE2S_BLOCKBYTES..i * BLAKE2S_BLOCKBYTES + left]);
            }
            self.leaves[i].clone().finalize(hash);
        }
        for hash in &hashes {
            self.root.update(hash);
        }
        let mut out = [0u8; BLAKE2_DIGEST_SIZE];
        self.root.finalize(&mut out);
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HashType {
    None,
    Crc32,
    Blake2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HashValue {
    None,
    Crc32(u32),
    Blake2([u8; BLAKE2_DIGEST_SIZE]),
}

impl HashValue {
    pub fn hash_type(&self) -> HashType {
        match self {
            HashValue::None => HashType::None,
            HashValue::Crc32(_) => HashType::Crc32,
            HashValue::Blake2(_) => HashType::Blake2,
        }
    }
}

/// Incremental data hash, following UnRAR's `DataHash`.
#[derive(Clone)]
pub enum DataHash {
    None,
    Crc32(crc32fast::Hasher),
    Blake2(Box<Blake2sp>),
}

impl DataHash {
    pub fn new(hash_type: HashType) -> Self {
        match hash_type {
            HashType::None => DataHash::None,
            HashType::Crc32 => DataHash::Crc32(crc32fast::Hasher::new()),
            HashType::Blake2 => DataHash::Blake2(Box::default()),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            DataHash::None => {}
            DataHash::Crc32(hasher) => hasher.update(data),
            DataHash::Blake2(hasher) => hasher.update(data),
        }
    }

    pub fn finish(self) -> HashValue {
        match self {
            DataHash::None => HashValue::None,
            DataHash::Crc32(hasher) => HashValue::Crc32(hasher.finalize()),
            DataHash::Blake2(hasher) => HashValue::Blake2(hasher.finalize()),
        }
    }

    /// Compare with the expected value, applying the RAR 5 HMAC conversion
    /// when the file header asked for it.
    pub fn verify(self, expected: &HashValue, hash_key: Option<&[u8; 32]>) -> Result<()> {
        if matches!(expected, HashValue::None) {
            return Ok(());
        }
        let mut actual = self.finish();
        if let Some(key) = hash_key {
            actual = convert_hash_to_mac(&actual, key);
        }
        let expected = expected.clone();
        match (&actual, &expected) {
            (HashValue::Crc32(a), HashValue::Crc32(b)) if a == b => Ok(()),
            (HashValue::Blake2(a), HashValue::Blake2(b)) if a == b => Ok(()),
            (HashValue::None, _) => Ok(()),
            _ => Err(Error::Checksum("data checksum mismatch".into())),
        }
    }
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().into()
}

/// UnRAR converts stored checksums to HMAC values when `UseHashKey` is set.
pub fn convert_hash_to_mac(value: &HashValue, key: &[u8; 32]) -> HashValue {
    match value {
        HashValue::Crc32(crc) => {
            let digest = hmac_sha256(key, &crc.to_le_bytes());
            let mut out = 0u32;
            for (i, byte) in digest.iter().enumerate() {
                out ^= (*byte as u32) << ((i & 3) * 8);
            }
            HashValue::Crc32(out)
        }
        HashValue::Blake2(digest) => HashValue::Blake2(hmac_sha256(key, digest)),
        HashValue::None => HashValue::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc32_known_vector() {
        let mut h = DataHash::new(HashType::Crc32);
        h.update(b"testtesttest");
        assert_eq!(h.finish(), HashValue::Crc32(0x4460_8e84));
    }

    #[test]
    fn blake2sp_empty() {
        let digest = Blake2sp::new().finalize();
        assert_eq!(
            digest,
            [
                0xdd, 0x0e, 0x89, 0x17, 0x76, 0x93, 0x3f, 0x43, 0xc7, 0xd0, 0x32, 0xb0, 0x8a, 0x91,
                0x7e, 0x25, 0x74, 0x1f, 0x8a, 0xa9, 0xa1, 0x2c, 0x12, 0xe1, 0xca, 0xc8, 0x80, 0x15,
                0x00, 0xf2, 0xca, 0x4f
            ]
        );
    }
}
