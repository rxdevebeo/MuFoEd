//! Huffman decode tables used by the RAR 5 decompressor.

use crate::bit_input::BitInput;

pub const MAX_QUICK_DECODE_BITS: u32 = 9;
pub const LARGEST_TABLE_SIZE: usize = 306;

#[derive(Clone)]
pub struct DecodeTable {
    pub max_num: usize,
    pub decode_len: [u32; 16],
    pub decode_pos: [u32; 16],
    pub quick_bits: u32,
    quick_len: [u8; 1 << MAX_QUICK_DECODE_BITS],
    quick_num: [u16; 1 << MAX_QUICK_DECODE_BITS],
    decode_num: [u16; LARGEST_TABLE_SIZE],
}

impl Default for DecodeTable {
    fn default() -> Self {
        DecodeTable {
            max_num: 0,
            decode_len: [0; 16],
            decode_pos: [0; 16],
            quick_bits: 0,
            quick_len: [0; 1 << MAX_QUICK_DECODE_BITS],
            quick_num: [0; 1 << MAX_QUICK_DECODE_BITS],
            decode_num: [0; LARGEST_TABLE_SIZE],
        }
    }
}

/// Build a decode table from a bit-length table (`LengthTable` in UnRAR).
pub fn make_decode_tables(length_table: &[u8], size: usize) -> DecodeTable {
    debug_assert!(size <= LARGEST_TABLE_SIZE);
    let mut dec = DecodeTable {
        max_num: size,
        ..DecodeTable::default()
    };

    let mut length_count = [0u32; 16];
    for &len in &length_table[..size] {
        length_count[(len & 0xf) as usize] += 1;
    }
    length_count[0] = 0;

    dec.decode_pos[0] = 0;
    dec.decode_len[0] = 0;
    let mut upper_limit: u32 = 0;
    for i in 1..16 {
        upper_limit += length_count[i];
        let left_aligned = upper_limit << (16 - i);
        upper_limit *= 2;
        dec.decode_len[i] = left_aligned;
        dec.decode_pos[i] = dec.decode_pos[i - 1].wrapping_add(length_count[i - 1]);
    }

    let mut copy_decode_pos = dec.decode_pos;
    for (i, &len) in length_table[..size].iter().enumerate() {
        let cur_bit_length = (len & 0xf) as usize;
        if cur_bit_length != 0 {
            let last_pos = copy_decode_pos[cur_bit_length] as usize;
            if last_pos < LARGEST_TABLE_SIZE {
                dec.decode_num[last_pos] = i as u16;
            }
            copy_decode_pos[cur_bit_length] = copy_decode_pos[cur_bit_length].wrapping_add(1);
        }
    }

    dec.quick_bits = if size == 306 || size == 298 || size == 299 {
        MAX_QUICK_DECODE_BITS
    } else {
        MAX_QUICK_DECODE_BITS.saturating_sub(3)
    };

    let quick_data_size = 1usize << dec.quick_bits;
    let mut cur_bit_length: usize = 1;
    for code in 0..quick_data_size {
        let bit_field = (code as u32) << (16 - dec.quick_bits);

        while cur_bit_length < 16 && bit_field >= dec.decode_len[cur_bit_length] {
            cur_bit_length += 1;
        }

        dec.quick_len[code] = cur_bit_length as u8;

        let dist = if cur_bit_length == 0 {
            bit_field
        } else {
            (bit_field.wrapping_sub(dec.decode_len[cur_bit_length - 1])) >> (16 - cur_bit_length)
        };
        let pos = dec.decode_pos[cur_bit_length.min(15)].wrapping_add(dist) as usize;
        if cur_bit_length < 16 && pos < size {
            dec.quick_num[code] = dec.decode_num[pos];
        } else {
            dec.quick_num[code] = 0;
        }
    }

    dec
}

#[inline]
pub fn decode_number(inp: &mut BitInput, dec: &DecodeTable) -> u32 {
    let bit_field = inp.getbits() & 0xfffe;

    if bit_field < dec.decode_len[dec.quick_bits as usize] {
        let code = (bit_field >> (16 - dec.quick_bits)) as usize;
        inp.addbits(dec.quick_len[code] as u32);
        return dec.quick_num[code] as u32;
    }

    let mut bits: usize = 15;
    let mut i = dec.quick_bits as usize + 1;
    while i < 15 {
        if bit_field < dec.decode_len[i] {
            bits = i;
            break;
        }
        i += 1;
    }

    inp.addbits(bits as u32);

    let mut dist = bit_field.wrapping_sub(dec.decode_len[bits - 1]);
    dist >>= 16 - bits;

    let mut pos = dec.decode_pos[bits].wrapping_add(dist) as usize;
    if pos >= dec.max_num {
        pos = 0;
    }
    dec.decode_num[pos] as u32
}
