//! MSB-first bit reader matching UnRAR's `BitInput`.

use crate::error::{Error, Result};

pub struct BitInput {
    data: Vec<u8>,
    len: usize,
    pub in_addr: usize,
    pub in_bit: u32,
}

impl BitInput {
    pub fn new() -> Self {
        BitInput {
            data: Vec::new(),
            len: 0,
            in_addr: 0,
            in_bit: 0,
        }
    }

    /// Replace the input buffer. The buffer is zero padded so the 64-bit
    /// lookahead reads used by the decoder are always in bounds.
    pub fn set_data(&mut self, data: &[u8]) {
        self.data.clear();
        self.data.extend_from_slice(data);
        self.len = self.data.len();
        self.data.resize(self.len + 16, 0);
        self.in_addr = 0;
        self.in_bit = 0;
    }

    pub fn init(&mut self) {
        self.in_addr = 0;
        self.in_bit = 0;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Return an error once the decoder has consumed substantially more
    /// input than was supplied (only possible with corrupt data).
    pub fn check_bounds(&self) -> Result<()> {
        if self.in_addr > self.len + 8 {
            return Err(Error::Corrupt("compressed data ended unexpectedly".into()));
        }
        Ok(())
    }

    #[inline]
    pub fn addbits(&mut self, bits: u32) {
        let total = bits + self.in_bit;
        self.in_addr += (total >> 3) as usize;
        self.in_bit = total & 7;
    }

    #[inline]
    pub fn getbits(&self) -> u32 {
        let a = self.in_addr;
        let bf = u32::from_be_bytes([
            self.data[a],
            self.data[a + 1],
            self.data[a + 2],
            self.data[a + 3],
        ]);
        (bf >> (16 - self.in_bit)) & 0xffff
    }

    #[inline]
    pub fn getbits32(&self) -> u32 {
        let a = self.in_addr;
        let bf = u32::from_be_bytes([
            self.data[a],
            self.data[a + 1],
            self.data[a + 2],
            self.data[a + 3],
        ]);
        (bf << self.in_bit) | ((self.data[a + 4] as u32) >> (8 - self.in_bit))
    }

    #[inline]
    pub fn getbits64(&self) -> u64 {
        let a = self.in_addr;
        let bf = u64::from_be_bytes([
            self.data[a],
            self.data[a + 1],
            self.data[a + 2],
            self.data[a + 3],
            self.data[a + 4],
            self.data[a + 5],
            self.data[a + 6],
            self.data[a + 7],
        ]);
        (bf << self.in_bit) | ((self.data[a + 8] as u64) >> (8 - self.in_bit))
    }
}

impl Default for BitInput {
    fn default() -> Self {
        Self::new()
    }
}
