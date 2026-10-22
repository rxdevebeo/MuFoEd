//! RAR 5 decompressor (port of `unpack50.cpp` and the shared Huffman
//! decoding code from `unpack.cpp`).

use crate::bit_input::BitInput;
use crate::error::{Error, Result};
use crate::huffman::{decode_number, make_decode_tables, DecodeTable};

pub const NC: usize = 306;
pub const DCB: usize = 64;
pub const DCX: usize = 80;
pub const LDC: usize = 16;
pub const RC: usize = 44;
pub const BC: usize = 20;

const MAX_INC_LZ_MATCH: usize = 0x1004;
const MAX_UNPACK_FILTERS: usize = 8192;
const MAX_FILTER_BLOCK_SIZE: u32 = 0x40_0000;
const UNPACK_MAX_WRITE: usize = 0x40_0000;
const MIN_ALLOC_SIZE: u64 = 0x4_0000;
const MAX_WINDOW_ALLOC: u64 = 1 << 36;
const MAX3_INC_LZ_MATCH: usize = 0x104;
const LOW_DIST_REP_COUNT: i32 = 16;

pub const NC30: usize = 299;
pub const DC30: usize = 60;
pub const LDC30: usize = 17;
pub const RC30: usize = 28;
pub const BC30: usize = 20;
pub const HUFF_TABLE_SIZE30: usize = NC30 + DC30 + LDC30 + RC30;
const BLOCK_LZ: u8 = 0;
const BLOCK_PPM: u8 = 1;

const L_DECODE: [u8; 28] = [
    0, 1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96, 112, 128,
    160, 192, 224,
];
const L_BITS: [u8; 28] = [
    0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5,
];
const SD_DECODE: [u8; 8] = [0, 4, 8, 16, 32, 64, 128, 192];
const SD_BITS: [u8; 8] = [2, 2, 3, 4, 5, 6, 6, 6];

const VM_MEM_SIZE: usize = 0x40000;
const MAX3_UNPACK_FILTERS: usize = 8192;
const MAX3_UNPACK_CHANNELS: u32 = 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VmFilterType {
    None,
    E8,
    E8E9,
    Itanium,
    Rgb,
    Audio,
    Delta,
}

#[derive(Clone, Copy)]
struct VmPreparedProgram {
    typ: VmFilterType,
    init_r: [u32; 7],
}

#[derive(Clone, Copy)]
struct VmFilter {
    block_start: u32,
    block_length: u32,
    next_window: bool,
    parent: usize,
    prg: VmPreparedProgram,
}

pub const FILTER_DELTA: u8 = 0;
pub const FILTER_E8: u8 = 1;
pub const FILTER_E8E9: u8 = 2;
pub const FILTER_ARM: u8 = 3;
pub const FILTER_NONE: u8 = 10;

#[derive(Default)]
struct BlockHeader {
    block_size: i64,
    block_bit_size: u32,
    block_start: usize,
    header_size: u32,
    last_block: bool,
    table_present: bool,
}

#[derive(Default)]
struct BlockTables {
    ld: DecodeTable,
    dd: DecodeTable,
    ldd: DecodeTable,
    rd: DecodeTable,
    bd: DecodeTable,
}

#[derive(Clone, Copy)]
struct UnpackFilter {
    typ: u8,
    channels: u8,
    next_window: bool,
    block_start: usize,
    block_length: u32,
}

/// RAR 5 sliding-window decompressor. One instance can be reused for a whole
/// solid stream; call [`Unpacker::init`] before each file and
/// [`Unpacker::unpack5`] with that file's packed data.
pub struct Unpacker {
    window: Vec<u8>,
    alloc_win_size: usize,
    max_win_size: usize,
    unp_ptr: usize,
    wr_ptr: usize,
    prev_ptr: usize,
    first_win_done: bool,
    write_border: usize,
    old_dist: [usize; 4],
    last_length: usize,
    filters: Vec<UnpackFilter>,
    written_file_size: u64,
    dest_unp_size: i64,
    block_header: BlockHeader,
    tables: BlockTables,
    tables_read5: bool,
    extra_dist: bool,
    inp: BitInput,
    file_extracted: bool,

    // RAR 3.x state.
    rar3_old_table: [u8; HUFF_TABLE_SIZE30],
    rar3_tables_read: bool,
    rar3_block_type: u8,
    prev_low_dist: i32,
    low_dist_rep_count: i32,
    rar3_filters: Vec<VmFilter>,
    rar3_prg_stack: Vec<Option<VmFilter>>,
    rar3_old_filter_lengths: Vec<u32>,
    rar3_last_filter: usize,
    vm_mem: Vec<u8>,
}

impl Default for Unpacker {
    fn default() -> Self {
        Self::new()
    }
}

impl Unpacker {
    pub fn new() -> Self {
        Unpacker {
            window: Vec::new(),
            alloc_win_size: 0,
            max_win_size: 0,
            unp_ptr: 0,
            wr_ptr: 0,
            prev_ptr: 0,
            first_win_done: false,
            write_border: 0,
            old_dist: [usize::MAX; 4],
            last_length: 0,
            filters: Vec::new(),
            written_file_size: 0,
            dest_unp_size: 0,
            block_header: BlockHeader {
                block_size: -1,
                ..BlockHeader::default()
            },
            tables: BlockTables::default(),
            tables_read5: false,
            extra_dist: false,
            inp: BitInput::new(),
            file_extracted: false,
            rar3_old_table: [0; HUFF_TABLE_SIZE30],
            rar3_tables_read: false,
            rar3_block_type: BLOCK_LZ,
            prev_low_dist: 0,
            low_dist_rep_count: 0,
            rar3_filters: Vec::new(),
            rar3_prg_stack: Vec::new(),
            rar3_old_filter_lengths: Vec::new(),
            rar3_last_filter: 0,
            vm_mem: Vec::new(),
        }
    }

    /// Prepare the sliding window for a file. `win_size` is the dictionary
    /// size from the file header and `solid` indicates whether the previous
    /// file's dictionary must be preserved.
    pub fn init(&mut self, win_size: u64, solid: bool) -> Result<()> {
        let win_size = win_size.max(MIN_ALLOC_SIZE);
        if win_size > MAX_WINDOW_ALLOC {
            return Err(Error::Unsupported(format!(
                "dictionary size {win_size} exceeds the supported maximum"
            )));
        }
        let win_size = win_size as usize;

        if !solid || self.window.is_empty() {
            self.max_win_size = win_size;
        }

        if win_size <= self.alloc_win_size && !self.window.is_empty() {
            return Ok(());
        }

        if solid && !self.window.is_empty() {
            return Err(Error::Corrupt(
                "solid stream uses a dictionary larger than the initial one".into(),
            ));
        }

        let mut window = Vec::new();
        window.try_reserve_exact(win_size).map_err(|_| {
            Error::Unsupported(format!("cannot allocate {win_size} byte dictionary"))
        })?;
        window.resize(win_size, 0);
        self.window = window;
        self.alloc_win_size = win_size;
        Ok(())
    }

    pub fn set_dest_size(&mut self, size: u64) {
        self.dest_unp_size = size as i64;
        self.file_extracted = false;
    }

    pub fn set_extra_dist(&mut self, extra_dist: bool) {
        self.extra_dist = extra_dist;
    }

    pub fn is_file_extracted(&self) -> bool {
        self.file_extracted
    }

    /// Decode one file. `write` receives unpacked data in order.
    pub fn unpack5(
        &mut self,
        input: &[u8],
        solid: bool,
        write: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        self.file_extracted = true;
        self.inp.set_data(input);
        self.unp_init_data(solid);

        if input.is_empty() {
            if self.dest_unp_size == 0 {
                return Ok(());
            }
            return Err(Error::Corrupt("compressed file has no data".into()));
        }

        if !self.read_block_header()? || !self.read_tables()? || !self.tables_read5 {
            return Err(Error::Corrupt("invalid compressed block header".into()));
        }

        loop {
            self.unp_ptr = self.wrap_up(self.unp_ptr);
            self.first_win_done |= self.prev_ptr > self.unp_ptr;
            self.prev_ptr = self.unp_ptr;

            self.inp.check_bounds()?;

            let end_minus_one =
                self.block_header.block_start as i64 + self.block_header.block_size - 1;
            let mut at_end = (self.inp.in_addr as i64) > end_minus_one
                || ((self.inp.in_addr as i64) == end_minus_one
                    && self.inp.in_bit >= self.block_header.block_bit_size);

            if at_end {
                let mut file_done = false;
                loop {
                    if self.block_header.last_block {
                        file_done = true;
                        break;
                    }
                    if !self.read_block_header()? || !self.read_tables()? {
                        return Err(Error::Corrupt("invalid compressed block header".into()));
                    }
                    let end_minus_one =
                        self.block_header.block_start as i64 + self.block_header.block_size - 1;
                    at_end = (self.inp.in_addr as i64) > end_minus_one
                        || ((self.inp.in_addr as i64) == end_minus_one
                            && self.inp.in_bit >= self.block_header.block_bit_size);
                    if !at_end {
                        break;
                    }
                }
                if file_done {
                    break;
                }
            }

            if self.wrap_down(self.write_border.wrapping_sub(self.unp_ptr)) <= MAX_INC_LZ_MATCH
                && self.write_border != self.unp_ptr
            {
                self.unp_write_buf(write)?;
                if self.written_file_size > self.dest_unp_size as u64 {
                    return Ok(());
                }
            }

            let main_slot = decode_number(&mut self.inp, &self.tables.ld);
            if main_slot < 256 {
                self.window[self.unp_ptr] = main_slot as u8;
                self.unp_ptr += 1;
                continue;
            }

            if main_slot >= 262 {
                let length = self.slot_to_length(main_slot - 262);
                let mut distance: usize = 1;
                let dist_slot = decode_number(&mut self.inp, &self.tables.dd);

                let dbits = if dist_slot < 4 {
                    distance += dist_slot as usize;
                    0
                } else {
                    let dbits = dist_slot / 2 - 1;
                    distance += ((2 | (dist_slot & 1)) as usize) << dbits;
                    dbits
                };

                if dbits > 0 {
                    if dbits >= 4 {
                        if dbits > 4 {
                            if dbits > 36 {
                                distance += ((self.inp.getbits64() >> (68 - dbits)) as usize) << 4;
                            } else {
                                distance += ((self.inp.getbits32() >> (36 - dbits)) as usize) << 4;
                            }
                            self.inp.addbits(dbits - 4);
                        }
                        let low_dist = decode_number(&mut self.inp, &self.tables.ldd) as usize;
                        distance += low_dist;
                    } else {
                        distance += (self.inp.getbits() >> (16 - dbits)) as usize;
                        self.inp.addbits(dbits);
                    }
                }

                let mut length = length;
                if distance > 0x100 {
                    // Longer distances imply a minimum match length.
                    length += 1;
                    if distance > 0x2000 {
                        length += 1;
                        if distance > 0x40000 {
                            length += 1;
                        }
                    }
                }

                self.insert_old_dist(distance);
                self.last_length = length;
                self.copy_string(length, distance);
                continue;
            }

            match main_slot {
                256 => {
                    let filter = self.read_filter();
                    if !self.add_filter(filter, write)? {
                        break;
                    }
                }
                257 => {
                    if self.last_length != 0 {
                        let distance = self.old_dist[0];
                        self.copy_string(self.last_length, distance);
                    }
                }
                258..=261 => {
                    let dist_num = (main_slot - 258) as usize;
                    let distance = self.old_dist[dist_num];
                    for i in (1..=dist_num).rev() {
                        self.old_dist[i] = self.old_dist[i - 1];
                    }
                    self.old_dist[0] = distance;

                    let length_slot = decode_number(&mut self.inp, &self.tables.rd);
                    let length = self.slot_to_length(length_slot);
                    self.last_length = length;
                    self.copy_string(length, distance);
                }
                _ => unreachable!(),
            }
        }

        self.unp_write_buf(write)?;
        Ok(())
    }

    /// Decode a RAR 3.x file (compression method 29). PPM blocks and VM
    /// filters are rejected; the LZ path covers the vast majority of RAR 3.x
    /// archives that do not contain executables.
    pub fn unpack29(
        &mut self,
        input: &[u8],
        solid: bool,
        write: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        self.file_extracted = true;
        self.inp.set_data(input);
        self.unp_init_data(solid);
        self.rar3_init_data(solid);

        if input.is_empty() {
            if self.dest_unp_size == 0 {
                return Ok(());
            }
            return Err(Error::Corrupt("compressed file has no data".into()));
        }

        if (!solid || !self.rar3_tables_read) && !self.read_tables30()? {
            return Err(Error::Corrupt("invalid RAR 3.x Huffman tables".into()));
        }

        let (decode, bits) = build_rar3_distance_tables();
        let mask = self.max_win_size - 1;

        loop {
            self.unp_ptr &= mask;
            self.first_win_done |= self.prev_ptr > self.unp_ptr;
            self.prev_ptr = self.unp_ptr;
            self.inp.check_bounds()?;

            if ((self.wr_ptr.wrapping_sub(self.unp_ptr)) & mask) <= MAX3_INC_LZ_MATCH
                && self.wr_ptr != self.unp_ptr
            {
                self.unp_write_buf30(write)?;
                if self.written_file_size > self.dest_unp_size as u64 {
                    return Ok(());
                }
            }

            if self.rar3_block_type == BLOCK_PPM {
                return Err(Error::Unsupported(
                    "RAR 3.x PPM compression is not supported".into(),
                ));
            }

            let number = decode_number(&mut self.inp, &self.tables.ld);
            if number < 256 {
                self.window[self.unp_ptr] = number as u8;
                self.unp_ptr += 1;
                continue;
            }

            if number >= 271 {
                let n = (number - 271) as usize;
                let mut length = L_DECODE[n] as usize + 3;
                let lbits = L_BITS[n];
                if lbits > 0 {
                    length += (self.inp.getbits() >> (16 - lbits)) as usize;
                    self.inp.addbits(lbits as u32);
                }

                let dist_number = decode_number(&mut self.inp, &self.tables.dd) as usize;
                let mut distance = decode[dist_number] as usize + 1;
                let dbits = bits[dist_number];
                if dbits > 0 {
                    if dist_number > 9 {
                        if dbits > 4 {
                            distance += ((self.inp.getbits() >> (20 - dbits)) as usize) << 4;
                            self.inp.addbits((dbits - 4) as u32);
                        }
                        if self.low_dist_rep_count > 0 {
                            self.low_dist_rep_count -= 1;
                            distance = distance.wrapping_add(self.prev_low_dist as usize);
                        } else {
                            let low = decode_number(&mut self.inp, &self.tables.ldd);
                            if low == 16 {
                                self.low_dist_rep_count = LOW_DIST_REP_COUNT - 1;
                                distance = distance.wrapping_add(self.prev_low_dist as usize);
                            } else {
                                distance += low as usize;
                                self.prev_low_dist = low as i32;
                            }
                        }
                    } else {
                        distance += (self.inp.getbits() >> (16 - dbits)) as usize;
                        self.inp.addbits(dbits as u32);
                    }
                }

                if distance >= 0x2000 {
                    length += 1;
                    if distance >= 0x40000 {
                        length += 1;
                    }
                }

                self.insert_old_dist(distance);
                self.last_length = length;
                self.copy_string(length, distance);
                continue;
            }

            match number {
                256 => {
                    if !self.read_end_of_block()? {
                        break;
                    }
                }
                257 => {
                    if !self.read_vm_code()? {
                        break;
                    }
                }
                258 => {
                    if self.last_length != 0 {
                        let distance = self.old_dist[0];
                        self.copy_string(self.last_length, distance);
                    }
                }
                259..=262 => {
                    let dist_num = (number - 259) as usize;
                    let distance = self.old_dist[dist_num];
                    for i in (1..=dist_num).rev() {
                        self.old_dist[i] = self.old_dist[i - 1];
                    }
                    self.old_dist[0] = distance;

                    let length_number = decode_number(&mut self.inp, &self.tables.rd) as usize;
                    let mut length = L_DECODE[length_number] as usize + 2;
                    let lbits = L_BITS[length_number];
                    if lbits > 0 {
                        length += (self.inp.getbits() >> (16 - lbits)) as usize;
                        self.inp.addbits(lbits as u32);
                    }
                    self.last_length = length;
                    self.copy_string(length, distance);
                }
                263..=270 => {
                    let n = (number - 263) as usize;
                    let mut distance = SD_DECODE[n] as usize + 1;
                    let sbits = SD_BITS[n];
                    if sbits > 0 {
                        distance += (self.inp.getbits() >> (16 - sbits)) as usize;
                        self.inp.addbits(sbits as u32);
                    }
                    self.insert_old_dist(distance);
                    self.last_length = 2;
                    self.copy_string(2, distance);
                }
                _ => {}
            }
        }

        self.unp_write_buf30(write)?;
        Ok(())
    }

    fn rar3_init_data(&mut self, solid: bool) {
        if !solid {
            self.rar3_tables_read = false;
            self.rar3_old_table = [0; HUFF_TABLE_SIZE30];
            self.rar3_block_type = BLOCK_LZ;
            self.rar3_filters.clear();
            self.rar3_old_filter_lengths.clear();
            self.rar3_last_filter = 0;
        }
        self.rar3_prg_stack.clear();
    }

    fn read_tables30(&mut self) -> Result<bool> {
        let align = (8 - self.inp.in_bit) & 7;
        self.inp.addbits(align);
        let bit_field = self.inp.getbits();
        if bit_field & 0x8000 != 0 {
            self.rar3_block_type = BLOCK_PPM;
            return Err(Error::Unsupported(
                "RAR 3.x PPM compression is not supported".into(),
            ));
        }
        self.rar3_block_type = BLOCK_LZ;
        self.prev_low_dist = 0;
        self.low_dist_rep_count = 0;

        if bit_field & 0x4000 == 0 {
            self.rar3_old_table.fill(0);
        }
        self.inp.addbits(2);

        let mut bit_length = [0u8; BC30];
        let mut i = 0;
        while i < BC30 {
            let length = (self.inp.getbits() >> 12) as u8;
            self.inp.addbits(4);
            if length == 15 {
                let zero_count = (self.inp.getbits() >> 12) as u8;
                self.inp.addbits(4);
                if zero_count == 0 {
                    bit_length[i] = 15;
                    i += 1;
                } else {
                    let mut zeros = zero_count as usize + 2;
                    while zeros > 0 && i < BC30 {
                        bit_length[i] = 0;
                        i += 1;
                        zeros -= 1;
                    }
                }
            } else {
                bit_length[i] = length;
                i += 1;
            }
        }
        self.tables.bd = make_decode_tables(&bit_length, BC30);

        let mut table = [0u8; HUFF_TABLE_SIZE30];
        let mut i = 0;
        while i < HUFF_TABLE_SIZE30 {
            self.inp.check_bounds()?;
            let number = decode_number(&mut self.inp, &self.tables.bd);
            if number < 16 {
                table[i] = ((number as u8) + self.rar3_old_table[i]) & 0xf;
                i += 1;
            } else if number < 18 {
                let (n, bits) = if number == 16 {
                    ((self.inp.getbits() >> 13) + 3, 3)
                } else {
                    ((self.inp.getbits() >> 9) + 11, 7)
                };
                self.inp.addbits(bits);
                if i == 0 {
                    return Ok(false);
                }
                let mut n = n;
                while n > 0 && i < HUFF_TABLE_SIZE30 {
                    table[i] = table[i - 1];
                    i += 1;
                    n -= 1;
                }
            } else {
                let (n, bits) = if number == 18 {
                    ((self.inp.getbits() >> 13) + 3, 3)
                } else {
                    ((self.inp.getbits() >> 9) + 11, 7)
                };
                self.inp.addbits(bits);
                let mut n = n;
                while n > 0 && i < HUFF_TABLE_SIZE30 {
                    table[i] = 0;
                    i += 1;
                    n -= 1;
                }
            }
        }

        self.rar3_tables_read = true;
        if self.inp.in_addr > self.inp.len() {
            return Ok(false);
        }
        self.tables.ld = make_decode_tables(&table[..NC30], NC30);
        self.tables.dd = make_decode_tables(&table[NC30..NC30 + DC30], DC30);
        self.tables.ldd = make_decode_tables(&table[NC30 + DC30..NC30 + DC30 + LDC30], LDC30);
        self.tables.rd = make_decode_tables(
            &table[NC30 + DC30 + LDC30..NC30 + DC30 + LDC30 + RC30],
            RC30,
        );
        self.rar3_old_table = table;
        Ok(true)
    }

    fn read_end_of_block(&mut self) -> Result<bool> {
        let bit_field = self.inp.getbits();
        let (new_table, new_file);
        if bit_field & 0x8000 != 0 {
            new_table = true;
            new_file = false;
            self.inp.addbits(1);
        } else {
            new_file = true;
            new_table = bit_field & 0x4000 != 0;
            self.inp.addbits(2);
        }
        self.rar3_tables_read = !new_table;
        if new_file {
            return Ok(false);
        }
        self.read_tables30()
    }

    fn unp_write_buf30(&mut self, write: &mut dyn FnMut(&[u8]) -> Result<()>) -> Result<()> {
        let mask = (self.max_win_size - 1) as u32;
        let mut written_border = self.wr_ptr as u32;
        let mut write_size = (self.unp_ptr as u32).wrapping_sub(written_border) & mask;

        let mut i = 0;
        while i < self.rar3_prg_stack.len() {
            let Some(flt) = self.rar3_prg_stack[i] else {
                i += 1;
                continue;
            };
            if flt.next_window {
                self.rar3_prg_stack[i].as_mut().unwrap().next_window = false;
                i += 1;
                continue;
            }

            let block_start = flt.block_start;
            let block_length = flt.block_length;
            if block_start.wrapping_sub(written_border) & mask < write_size {
                if written_border != block_start {
                    self.unp_write_area(written_border as usize, block_start as usize, write)?;
                    written_border = block_start;
                    write_size = (self.unp_ptr as u32).wrapping_sub(written_border) & mask;
                }
                if block_length <= write_size {
                    let block_end = block_start.wrapping_add(block_length) & mask;
                    let mut mem = std::mem::take(&mut self.vm_mem);
                    if mem.len() < VM_MEM_SIZE + 4 {
                        mem.resize(VM_MEM_SIZE + 4, 0);
                    }

                    let start = block_start as usize;
                    let len = block_length as usize;
                    if start < block_end as usize || block_end == 0 {
                        mem[..len].copy_from_slice(&self.window[start..start + len]);
                    } else {
                        let first = self.max_win_size - start;
                        mem[..first].copy_from_slice(&self.window[start..]);
                        mem[first..first + block_end as usize]
                            .copy_from_slice(&self.window[..block_end as usize]);
                    }

                    let mut init_r = flt.prg.init_r;
                    init_r[6] = self.written_file_size as u32;
                    let (mut filtered, mut filtered_size) =
                        execute_vm_filter(flt.prg.typ, &mut mem, &init_r, len);

                    self.rar3_prg_stack[i] = None;
                    while i + 1 < self.rar3_prg_stack.len() {
                        let next = match self.rar3_prg_stack[i + 1] {
                            Some(next) => next,
                            None => break,
                        };
                        if next.next_window
                            || next.block_start != block_start
                            || next.block_length != filtered_size as u32
                        {
                            break;
                        }
                        mem[..filtered_size].copy_from_slice(&filtered[..filtered_size]);
                        let mut init_r = next.prg.init_r;
                        init_r[6] = self.written_file_size as u32;
                        let (next_data, next_size) =
                            execute_vm_filter(next.prg.typ, &mut mem, &init_r, filtered_size);
                        filtered = next_data;
                        filtered_size = next_size;
                        self.rar3_prg_stack[i + 1] = None;
                        i += 1;
                    }
                    self.vm_mem = mem;

                    write(&filtered[..filtered_size])?;
                    self.written_file_size += filtered_size as u64;
                    written_border = block_end;
                    write_size = (self.unp_ptr as u32).wrapping_sub(written_border) & mask;
                } else {
                    for filter in self.rar3_prg_stack.iter_mut().skip(i).flatten() {
                        filter.next_window = false;
                    }
                    self.wr_ptr = written_border as usize;
                    return Ok(());
                }
            }
            i += 1;
        }

        self.unp_write_area(written_border as usize, self.unp_ptr, write)?;
        self.wr_ptr = self.unp_ptr;
        Ok(())
    }

    fn init_filters30(&mut self, solid: bool) {
        if !solid {
            self.rar3_old_filter_lengths.clear();
            self.rar3_last_filter = 0;
            self.rar3_filters.clear();
        }
        self.rar3_prg_stack.clear();
    }

    fn read_vm_code(&mut self) -> Result<bool> {
        let mut first_byte = self.inp.getbits() >> 8;
        self.inp.addbits(8);
        let mut length = (first_byte & 7) + 1;
        if length == 7 {
            length = (self.inp.getbits() >> 8) + 7;
            self.inp.addbits(8);
        } else if length == 8 {
            length = self.inp.getbits();
            self.inp.addbits(16);
        }
        if length == 0 {
            return Ok(false);
        }

        let mut code = vec![0u8; length as usize];
        for byte in &mut code {
            self.inp.check_bounds()?;
            *byte = (self.inp.getbits() >> 8) as u8;
            self.inp.addbits(8);
        }
        first_byte &= 0xff;
        self.add_vm_code(first_byte, &code)
    }

    fn add_vm_code(&mut self, first_byte: u32, code: &[u8]) -> Result<bool> {
        let mut vm_inp = BitInput::new();
        vm_inp.set_data(code);

        let filt_pos = if first_byte & 0x80 != 0 {
            let pos = read_vm_data(&mut vm_inp) as usize;
            if pos == 0 {
                self.init_filters30(false);
                0
            } else {
                pos - 1
            }
        } else {
            self.rar3_last_filter
        };

        if filt_pos > self.rar3_filters.len() {
            return Ok(false);
        }
        self.rar3_last_filter = filt_pos;
        let new_filter = filt_pos == self.rar3_filters.len();

        let mut filter;
        if new_filter {
            if filt_pos > MAX3_UNPACK_FILTERS {
                return Ok(false);
            }
            filter = VmFilter {
                block_start: 0,
                block_length: 0,
                next_window: false,
                parent: filt_pos,
                prg: VmPreparedProgram {
                    typ: VmFilterType::None,
                    init_r: [0; 7],
                },
            };
            self.rar3_filters.push(filter);
            self.rar3_old_filter_lengths.push(0);
        } else {
            filter = self.rar3_filters[filt_pos];
            filter.parent = filt_pos;
        }

        self.rar3_prg_stack.retain(|entry| entry.is_some());
        if self.rar3_prg_stack.len() > MAX3_UNPACK_FILTERS {
            return Ok(false);
        }
        self.rar3_prg_stack.push(None);
        let stack_pos = self.rar3_prg_stack.len() - 1;

        let raw_block_start = read_vm_data(&mut vm_inp);
        let raw_block_start = if first_byte & 0x40 != 0 {
            raw_block_start.wrapping_add(258)
        } else {
            raw_block_start
        };
        let adjusted_start =
            raw_block_start.wrapping_add(self.unp_ptr as u32) & (self.max_win_size as u32 - 1);

        let block_length = if first_byte & 0x20 != 0 {
            let length = read_vm_data(&mut vm_inp);
            if filt_pos < self.rar3_old_filter_lengths.len() {
                self.rar3_old_filter_lengths[filt_pos] = length;
            }
            length
        } else {
            self.rar3_old_filter_lengths
                .get(filt_pos)
                .copied()
                .unwrap_or(0)
        };

        filter.block_start = adjusted_start;
        filter.block_length = block_length;
        filter.next_window = self.wr_ptr != self.unp_ptr
            && ((self.wr_ptr.wrapping_sub(self.unp_ptr)) & (self.max_win_size - 1)) as u32
                <= raw_block_start;

        let mut init_r = [0u32; 7];
        init_r[4] = block_length;
        if first_byte & 0x10 != 0 {
            let init_mask = (vm_inp.getbits() >> 9) & 0x7f;
            vm_inp.addbits(7);
            for (i, reg) in init_r.iter_mut().enumerate() {
                if init_mask & (1 << i) != 0 {
                    *reg = read_vm_data(&mut vm_inp);
                }
            }
        }

        let filter_type = if new_filter {
            let vm_code_size = read_vm_data(&mut vm_inp) as usize;
            if vm_code_size >= 0x10000
                || vm_code_size == 0
                || vm_inp.in_addr + vm_code_size > code.len()
            {
                return Ok(false);
            }
            let mut vm_code = Vec::with_capacity(vm_code_size);
            for _ in 0..vm_code_size {
                if vm_inp.in_addr > code.len() {
                    return Ok(false);
                }
                vm_code.push((vm_inp.getbits() >> 8) as u8);
                vm_inp.addbits(8);
            }
            let typ = prepare_vm(&vm_code);
            self.rar3_filters[filt_pos].prg = VmPreparedProgram {
                typ,
                init_r: [0; 7],
            };
            typ
        } else {
            self.rar3_filters[filt_pos].prg.typ
        };

        filter.prg = VmPreparedProgram {
            typ: filter_type,
            init_r,
        };
        self.rar3_prg_stack[stack_pos] = Some(filter);
        Ok(true)
    }

    #[inline]
    fn insert_old_dist(&mut self, distance: usize) {
        self.old_dist[3] = self.old_dist[2];
        self.old_dist[2] = self.old_dist[1];
        self.old_dist[1] = self.old_dist[0];
        self.old_dist[0] = distance;
    }

    #[inline]
    fn wrap_down(&self, pos: usize) -> usize {
        if pos >= self.max_win_size {
            pos.wrapping_add(self.max_win_size)
        } else {
            pos
        }
    }

    #[inline]
    fn wrap_up(&self, pos: usize) -> usize {
        if pos >= self.max_win_size {
            pos - self.max_win_size
        } else {
            pos
        }
    }

    #[inline]
    fn slot_to_length(&mut self, slot: u32) -> usize {
        let (lbits, mut length): (u32, usize) = if slot < 8 {
            (0, 2 + slot as usize)
        } else {
            let lbits = slot / 4 - 1;
            (lbits, 2 + (((4 | (slot & 3)) as usize) << lbits))
        };
        if lbits > 0 {
            length += (self.inp.getbits() >> (16 - lbits)) as usize;
            self.inp.addbits(lbits);
        }
        length
    }

    fn copy_string(&mut self, mut length: usize, distance: usize) {
        let mut src_ptr = self.unp_ptr.wrapping_sub(distance);

        if distance > self.unp_ptr {
            src_ptr = src_ptr.wrapping_add(self.max_win_size);
            if distance > self.max_win_size || !self.first_win_done {
                while length > 0 {
                    self.window[self.unp_ptr] = 0;
                    self.unp_ptr = self.wrap_up(self.unp_ptr + 1);
                    length -= 1;
                }
                return;
            }
        }

        if src_ptr < self.max_win_size - MAX_INC_LZ_MATCH
            && self.unp_ptr < self.max_win_size - MAX_INC_LZ_MATCH
        {
            if distance < length {
                while length > 0 {
                    self.window[self.unp_ptr] = self.window[src_ptr];
                    self.unp_ptr += 1;
                    src_ptr += 1;
                    length -= 1;
                }
            } else {
                self.window
                    .copy_within(src_ptr..src_ptr + length, self.unp_ptr);
                self.unp_ptr += length;
            }
        } else {
            while length > 0 {
                let wrapped_src = self.wrap_up(src_ptr);
                self.window[self.unp_ptr] = self.window[wrapped_src];
                src_ptr = src_ptr.wrapping_add(1);
                self.unp_ptr = self.wrap_up(self.unp_ptr + 1);
                length -= 1;
            }
        }
    }

    fn unp_init_data(&mut self, solid: bool) {
        if !solid {
            self.old_dist = [usize::MAX; 4];
            self.last_length = 0;
            self.tables = BlockTables::default();
            self.unp_ptr = 0;
            self.wr_ptr = 0;
            self.prev_ptr = 0;
            self.first_win_done = false;
            self.write_border = self.max_win_size.min(UNPACK_MAX_WRITE);
            self.tables_read5 = false;
        }
        self.filters.clear();
        self.inp.init();
        self.written_file_size = 0;
        self.block_header = BlockHeader {
            block_size: -1,
            ..BlockHeader::default()
        };
    }

    fn read_block_header(&mut self) -> Result<bool> {
        self.block_header.header_size = 0;
        let align = (8 - self.inp.in_bit) & 7;
        self.inp.addbits(align);

        let block_flags = (self.inp.getbits() >> 8) as u8;
        self.inp.addbits(8);
        let byte_count = ((block_flags >> 3) & 3) as usize + 1;
        if byte_count == 4 {
            return Ok(false);
        }
        self.block_header.header_size = 2 + byte_count as u32;
        self.block_header.block_bit_size = (block_flags & 7) as u32 + 1;

        let saved_checksum = (self.inp.getbits() >> 8) as u8;
        self.inp.addbits(8);

        let mut block_size: u32 = 0;
        for i in 0..byte_count {
            block_size += (self.inp.getbits() >> 8) << (i * 8);
            self.inp.addbits(8);
        }
        self.block_header.block_size = block_size as i64;

        let checksum = 0x5a
            ^ block_flags
            ^ (block_size as u8)
            ^ (block_size >> 8) as u8
            ^ (block_size >> 16) as u8;
        if checksum != saved_checksum {
            return Ok(false);
        }

        self.block_header.block_start = self.inp.in_addr;
        self.block_header.last_block = (block_flags & 0x40) != 0;
        self.block_header.table_present = (block_flags & 0x80) != 0;
        Ok(true)
    }

    fn read_tables(&mut self) -> Result<bool> {
        if !self.block_header.table_present {
            return Ok(true);
        }

        let mut bit_length = [0u8; BC];
        let mut i = 0;
        while i < BC {
            let length = (self.inp.getbits() >> 12) as u8;
            self.inp.addbits(4);
            if length == 15 {
                let zero_count = (self.inp.getbits() >> 12) as u8;
                self.inp.addbits(4);
                if zero_count == 0 {
                    bit_length[i] = 15;
                    i += 1;
                } else {
                    let mut zeros = zero_count as usize + 2;
                    while zeros > 0 && i < BC {
                        bit_length[i] = 0;
                        i += 1;
                        zeros -= 1;
                    }
                }
            } else {
                bit_length[i] = length;
                i += 1;
            }
        }

        self.tables.bd = make_decode_tables(&bit_length, BC);

        let table_size = if self.extra_dist {
            NC + DCX + RC + LDC
        } else {
            NC + DCB + RC + LDC
        };
        let mut table = [0u8; NC + DCX + RC + LDC];
        let mut i = 0;
        while i < table_size {
            let number = decode_number(&mut self.inp, &self.tables.bd);
            if number < 16 {
                table[i] = number as u8;
                i += 1;
            } else if number < 18 {
                let (n, bits) = if number == 16 {
                    ((self.inp.getbits() >> 13) + 3, 3)
                } else {
                    ((self.inp.getbits() >> 9) + 11, 7)
                };
                self.inp.addbits(bits);
                if i == 0 {
                    return Ok(false);
                }
                let mut n = n;
                while n > 0 && i < table_size {
                    table[i] = table[i - 1];
                    i += 1;
                    n -= 1;
                }
            } else {
                let (n, bits) = if number == 18 {
                    ((self.inp.getbits() >> 13) + 3, 3)
                } else {
                    ((self.inp.getbits() >> 9) + 11, 7)
                };
                self.inp.addbits(bits);
                let mut n = n;
                while n > 0 && i < table_size {
                    table[i] = 0;
                    i += 1;
                    n -= 1;
                }
            }
        }

        self.tables_read5 = true;
        if self.inp.in_addr > self.inp.len() {
            return Ok(false);
        }

        let dcodes = if self.extra_dist { DCX } else { DCB };
        self.tables.ld = make_decode_tables(&table[..NC], NC);
        self.tables.dd = make_decode_tables(&table[NC..NC + dcodes], dcodes);
        self.tables.ldd = make_decode_tables(&table[NC + dcodes..NC + dcodes + LDC], LDC);
        self.tables.rd = make_decode_tables(&table[NC + dcodes + LDC..NC + dcodes + LDC + RC], RC);
        Ok(true)
    }

    fn read_filter_data(&mut self) -> u32 {
        let byte_count = (self.inp.getbits() >> 14) + 1;
        self.inp.addbits(2);

        let mut data = 0u32;
        for i in 0..byte_count {
            data += (self.inp.getbits() >> 8) << (i * 8);
            self.inp.addbits(8);
        }
        data
    }

    fn read_filter(&mut self) -> UnpackFilter {
        let block_start = self.read_filter_data() as usize;
        let mut block_length = self.read_filter_data();
        if block_length > MAX_FILTER_BLOCK_SIZE {
            block_length = 0;
        }
        let typ = (self.inp.getbits() >> 13) as u8;
        self.inp.addbits(3);
        let mut channels = 0;
        if typ == FILTER_DELTA {
            channels = ((self.inp.getbits() >> 11) + 1) as u8;
            self.inp.addbits(5);
        }
        UnpackFilter {
            typ,
            channels,
            next_window: false,
            block_start,
            block_length,
        }
    }

    fn add_filter(
        &mut self,
        mut filter: UnpackFilter,
        write: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<bool> {
        if self.filters.len() >= MAX_UNPACK_FILTERS {
            self.unp_write_buf(write)?;
            if self.filters.len() >= MAX_UNPACK_FILTERS {
                self.filters.clear();
            }
        }

        filter.next_window = self.wr_ptr != self.unp_ptr
            && self.wrap_down(self.wr_ptr.wrapping_sub(self.unp_ptr)) <= filter.block_start;
        filter.block_start = ((filter.block_start as u128 + self.unp_ptr as u128)
            % self.max_win_size as u128) as usize;
        self.filters.push(filter);
        Ok(true)
    }

    fn unp_write_buf(&mut self, write: &mut dyn FnMut(&[u8]) -> Result<()>) -> Result<()> {
        let mut written_border = self.wr_ptr;
        let full_write_size = self.wrap_down(self.unp_ptr.wrapping_sub(written_border));
        let mut write_size_left = full_write_size;
        let mut not_all_filters_processed = false;

        let mut i = 0;
        while i < self.filters.len() {
            let flt = self.filters[i];
            if flt.typ == FILTER_NONE {
                i += 1;
                continue;
            }
            if flt.next_window {
                if self.wrap_down(flt.block_start.wrapping_sub(self.wr_ptr)) <= full_write_size {
                    self.filters[i].next_window = false;
                }
                i += 1;
                continue;
            }

            let block_start = flt.block_start;
            let block_length = flt.block_length;
            if self.wrap_down(block_start.wrapping_sub(written_border)) < write_size_left {
                if written_border != block_start {
                    self.unp_write_area(written_border, block_start, write)?;
                    written_border = block_start;
                    write_size_left = self.wrap_down(self.unp_ptr.wrapping_sub(written_border));
                }
                if block_length as usize <= write_size_left {
                    if block_length > 0 {
                        let block_end = self.wrap_up(block_start + block_length as usize);

                        let mut mem = Vec::with_capacity(block_length as usize);
                        if block_start < block_end || block_end == 0 {
                            mem.extend_from_slice(
                                &self.window[block_start..block_start + block_length as usize],
                            );
                        } else {
                            mem.extend_from_slice(&self.window[block_start..]);
                            mem.extend_from_slice(&self.window[..block_end]);
                        }

                        let out = apply_filter(
                            flt.typ,
                            flt.channels,
                            &mem,
                            self.written_file_size as u32,
                        );
                        self.filters[i].typ = FILTER_NONE;
                        if let Some(out) = out {
                            write(&out)?;
                        }
                        self.written_file_size += block_length as u64;
                        written_border = block_end;
                        write_size_left = self.wrap_down(self.unp_ptr.wrapping_sub(written_border));
                    }
                } else {
                    self.wr_ptr = written_border;
                    for filter in self.filters.iter_mut().skip(i) {
                        if filter.typ != FILTER_NONE {
                            filter.next_window = false;
                        }
                    }
                    not_all_filters_processed = true;
                    break;
                }
            }
            i += 1;
        }

        self.filters.retain(|filter| filter.typ != FILTER_NONE);

        if !not_all_filters_processed {
            self.unp_write_area(written_border, self.unp_ptr, write)?;
            self.wr_ptr = self.unp_ptr;
        }

        self.write_border = self.wrap_up(self.unp_ptr + self.max_win_size.min(UNPACK_MAX_WRITE));
        if self.write_border == self.unp_ptr
            || (self.wr_ptr != self.unp_ptr
                && self.wrap_down(self.wr_ptr.wrapping_sub(self.unp_ptr))
                    < self.wrap_down(self.write_border.wrapping_sub(self.unp_ptr)))
        {
            self.write_border = self.wr_ptr;
        }
        Ok(())
    }

    fn unp_write_area(
        &mut self,
        start: usize,
        end: usize,
        write: &mut dyn FnMut(&[u8]) -> Result<()>,
    ) -> Result<()> {
        if end < start {
            write_clipped(
                &self.window[start..],
                &mut self.written_file_size,
                self.dest_unp_size,
                write,
            )?;
            write_clipped(
                &self.window[..end],
                &mut self.written_file_size,
                self.dest_unp_size,
                write,
            )?;
        } else {
            write_clipped(
                &self.window[start..end],
                &mut self.written_file_size,
                self.dest_unp_size,
                write,
            )?;
        }
        Ok(())
    }
}

fn read_vm_data(inp: &mut BitInput) -> u32 {
    let data = inp.getbits();
    match data & 0xc000 {
        0 => {
            inp.addbits(6);
            (data >> 10) & 0xf
        }
        0x4000 => {
            if data & 0x3c00 == 0 {
                let value = 0xffff_ff00 | ((data >> 2) & 0xff);
                inp.addbits(14);
                value
            } else {
                let value = (data >> 6) & 0xff;
                inp.addbits(10);
                value
            }
        }
        0x8000 => {
            inp.addbits(2);
            let value = inp.getbits();
            inp.addbits(16);
            value
        }
        _ => {
            inp.addbits(2);
            let mut value = inp.getbits() << 16;
            inp.addbits(16);
            value |= inp.getbits();
            inp.addbits(16);
            value
        }
    }
}

fn prepare_vm(code: &[u8]) -> VmFilterType {
    if code.is_empty() {
        return VmFilterType::None;
    }
    let mut xor_sum = 0u8;
    for &byte in &code[1..] {
        xor_sum ^= byte;
    }
    if xor_sum != code[0] {
        return VmFilterType::None;
    }
    let crc = crc32fast::hash(code);
    match (code.len(), crc) {
        (53, 0xad57_6887) => VmFilterType::E8,
        (57, 0x3cd7_e57e) => VmFilterType::E8E9,
        (120, 0x3769_893f) => VmFilterType::Itanium,
        (29, 0x0e06_077d) => VmFilterType::Delta,
        (149, 0x1c2c_5dc8) => VmFilterType::Rgb,
        (216, 0xbc85_e701) => VmFilterType::Audio,
        _ => VmFilterType::None,
    }
}

fn execute_vm_filter(
    typ: VmFilterType,
    mem: &mut [u8],
    r: &[u32; 7],
    data_size: usize,
) -> (Vec<u8>, usize) {
    let source = mem[..data_size.min(mem.len())].to_vec();
    match typ {
        VmFilterType::None => (Vec::new(), 0),
        VmFilterType::E8 | VmFilterType::E8E9 => {
            if !(4..=VM_MEM_SIZE).contains(&data_size) {
                return (source, data_size);
            }
            let mut out = source;
            let file_offset = r[6];
            let file_size: u32 = 0x100_0000;
            let cmp_byte2 = if typ == VmFilterType::E8E9 {
                0xe9
            } else {
                0xe8
            };
            let mut pos = 0usize;
            while pos < data_size - 4 {
                let cur = out[pos];
                pos += 1;
                if cur == 0xe8 || cur == cmp_byte2 {
                    let offset = (pos as u32).wrapping_add(file_offset);
                    let addr =
                        u32::from_le_bytes([out[pos], out[pos + 1], out[pos + 2], out[pos + 3]]);
                    if addr & 0x8000_0000 != 0 {
                        if addr.wrapping_add(offset) & 0x8000_0000 == 0 {
                            out[pos..pos + 4]
                                .copy_from_slice(&addr.wrapping_add(file_size).to_le_bytes());
                        }
                    } else if addr.wrapping_sub(file_size) & 0x8000_0000 != 0 {
                        out[pos..pos + 4].copy_from_slice(&addr.wrapping_sub(offset).to_le_bytes());
                    }
                    pos += 4;
                }
            }
            (out, data_size)
        }
        VmFilterType::Itanium => {
            if !(21..=VM_MEM_SIZE).contains(&data_size) {
                return (source, data_size);
            }
            let mut out = source;
            let mut file_offset = r[6] >> 4;
            let masks = [4u8, 4, 6, 6, 0, 0, 7, 7, 4, 4, 0, 0, 4, 4, 0, 0];
            let mut pos = 0usize;
            while pos < data_size - 21 {
                let byte = (out[pos] & 0x1f) as i32 - 0x10;
                if byte >= 0 {
                    let mask = masks[byte as usize];
                    if mask != 0 {
                        for i in 0..=2u32 {
                            if mask & (1 << i) != 0 {
                                let start = i as usize * 41 + 5;
                                let op_type = itanium_get_bits(&out, start + 37, 4);
                                if op_type == 5 {
                                    let offset = itanium_get_bits(&out, start + 13, 20);
                                    itanium_set_bits(
                                        &mut out,
                                        offset.wrapping_sub(file_offset) & 0xfffff,
                                        start + 13,
                                        20,
                                    );
                                }
                            }
                        }
                    }
                }
                pos += 16;
                file_offset = file_offset.wrapping_add(1);
            }
            (out, data_size)
        }
        VmFilterType::Delta => {
            let channels = r[0] as usize;
            if data_size > VM_MEM_SIZE / 2
                || channels > MAX3_UNPACK_CHANNELS as usize
                || channels == 0
            {
                return (source, data_size);
            }
            let border = data_size * 2;
            let mut src_pos = 0usize;
            for channel in 0..channels {
                let mut prev = 0u8;
                let mut dest = data_size + channel;
                while dest < border {
                    prev = prev.wrapping_sub(mem[src_pos]);
                    mem[dest] = prev;
                    src_pos += 1;
                    dest += channels;
                }
            }
            (mem[data_size..border].to_vec(), data_size)
        }
        VmFilterType::Rgb => {
            let width = r[0].wrapping_sub(3) as usize;
            let pos_r = r[1] as usize;
            if !(3..=VM_MEM_SIZE / 2).contains(&data_size) || width > data_size || pos_r > 2 {
                return (source, data_size);
            }
            let mut dest = source;
            let mut src_pos = 0usize;
            for channel in 0..3usize {
                let mut prev = 0u8;
                let mut i = channel;
                while i < data_size {
                    let predicted = if i >= width + 3 {
                        let upper = dest[i - width] as i32;
                        let upper_left = dest[i - width - 3] as i32;
                        let pred = prev as i32 + upper - upper_left;
                        let pa = (pred - prev as i32).abs();
                        let pb = (pred - upper).abs();
                        let pc = (pred - upper_left).abs();
                        if pa <= pb && pa <= pc {
                            prev as i32
                        } else if pb <= pc {
                            upper
                        } else {
                            upper_left
                        }
                    } else {
                        prev as i32
                    };
                    let cur = mem[src_pos];
                    src_pos += 1;
                    prev = (predicted as u8).wrapping_sub(cur);
                    dest[i] = prev;
                    i += 3;
                }
            }
            let mut i = pos_r;
            while i + 2 < data_size {
                let g = dest[i + 1];
                dest[i] = dest[i].wrapping_add(g);
                dest[i + 2] = dest[i + 2].wrapping_add(g);
                i += 3;
            }
            (dest, data_size)
        }
        VmFilterType::Audio => {
            let channels = r[0] as usize;
            if data_size > VM_MEM_SIZE / 2 || channels > 128 || channels == 0 {
                return (source, data_size);
            }
            let mut dest = vec![0u8; data_size];
            let mut src_pos = 0usize;
            for channel in 0..channels {
                let mut prev_byte = 0u32;
                let mut prev_delta = 0i32;
                let mut dif = [0u32; 7];
                let mut d1 = 0i32;
                let mut d2 = 0i32;
                let mut k1 = 0i32;
                let mut k2 = 0i32;
                let mut k3 = 0i32;
                let mut i = channel;
                let mut byte_count = 0u32;
                while i < data_size {
                    let d3 = d2;
                    d2 = prev_delta - d1;
                    d1 = prev_delta;

                    let predicted = 8u32
                        .wrapping_mul(prev_byte)
                        .wrapping_add((k1.wrapping_mul(d1)) as u32)
                        .wrapping_add((k2.wrapping_mul(d2)) as u32)
                        .wrapping_add((k3.wrapping_mul(d3)) as u32);
                    let predicted = (predicted >> 3) & 0xff;

                    let cur = mem[src_pos];
                    src_pos += 1;

                    let old_prev = prev_byte;
                    let predicted = predicted.wrapping_sub(cur as u32);
                    dest[i] = predicted as u8;
                    prev_delta = (predicted.wrapping_sub(old_prev) as u8) as i8 as i32;
                    prev_byte = predicted;

                    let d = ((cur as i8) as u32) << 3;
                    let d = d as i32;
                    dif[0] = dif[0].wrapping_add(d.unsigned_abs());
                    dif[1] = dif[1].wrapping_add((d.wrapping_sub(d1)).unsigned_abs());
                    dif[2] = dif[2].wrapping_add((d.wrapping_add(d1)).unsigned_abs());
                    dif[3] = dif[3].wrapping_add((d.wrapping_sub(d2)).unsigned_abs());
                    dif[4] = dif[4].wrapping_add((d.wrapping_add(d2)).unsigned_abs());
                    dif[5] = dif[5].wrapping_add((d.wrapping_sub(d3)).unsigned_abs());
                    dif[6] = dif[6].wrapping_add((d.wrapping_add(d3)).unsigned_abs());

                    if byte_count & 0x1f == 0 {
                        let mut min_dif = dif[0];
                        let mut num_min_dif = 0usize;
                        dif[0] = 0;
                        for (j, value) in dif.iter_mut().enumerate().skip(1) {
                            if *value < min_dif {
                                min_dif = *value;
                                num_min_dif = j;
                            }
                            *value = 0;
                        }
                        match num_min_dif {
                            1 => k1 = k1.saturating_sub(1).max(-16),
                            2 => k1 = (k1 + 1).min(16),
                            3 => k2 = k2.saturating_sub(1).max(-16),
                            4 => k2 = (k2 + 1).min(16),
                            5 => k3 = k3.saturating_sub(1).max(-16),
                            6 => k3 = (k3 + 1).min(16),
                            _ => {}
                        }
                    }

                    byte_count += 1;
                    i += channels;
                }
            }
            (dest, data_size)
        }
    }
}

fn itanium_get_bits(data: &[u8], bit_pos: usize, bit_count: usize) -> u32 {
    let mut addr = bit_pos / 8;
    let in_bit = (bit_pos & 7) as u32;
    let mut bit_field = data[addr] as u32;
    addr += 1;
    bit_field |= (data[addr] as u32) << 8;
    addr += 1;
    bit_field |= (data[addr] as u32) << 16;
    addr += 1;
    bit_field |= (data[addr] as u32) << 24;
    (bit_field >> in_bit) & (u32::MAX >> (32 - bit_count))
}

fn itanium_set_bits(data: &mut [u8], bit_field: u32, bit_pos: usize, bit_count: usize) {
    let addr = bit_pos / 8;
    let in_bit = (bit_pos & 7) as u32;
    let mut and_mask = u32::MAX >> (32 - bit_count);
    and_mask = !(and_mask << in_bit);
    let mut bit_field = bit_field << in_bit;

    for i in 0..4 {
        data[addr + i] &= and_mask as u8;
        data[addr + i] |= bit_field as u8;
        and_mask = (and_mask >> 8) | 0xff00_0000;
        bit_field >>= 8;
    }
}

fn build_rar3_distance_tables() -> ([i32; DC30], [u8; DC30]) {
    const COUNTS: [usize; 19] = [4, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 2, 14, 0, 12];
    let mut decode = [0i32; DC30];
    let mut bits = [0u8; DC30];
    let mut dist = 0i32;
    let mut slot = 0usize;
    for (bit_length, count) in COUNTS.into_iter().enumerate() {
        for _ in 0..count {
            if slot >= DC30 {
                break;
            }
            decode[slot] = dist;
            bits[slot] = bit_length as u8;
            slot += 1;
            dist += 1 << bit_length;
        }
    }
    (decode, bits)
}

fn write_clipped(
    data: &[u8],
    written: &mut u64,
    dest_size: i64,
    write: &mut dyn FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    if *written >= dest_size as u64 {
        return Ok(());
    }
    let left = dest_size - *written as i64;
    let n = (data.len() as i64).min(left).max(0) as usize;
    if n > 0 {
        write(&data[..n])?;
    }
    *written += data.len() as u64;
    Ok(())
}

fn apply_filter(typ: u8, channels: u8, data: &[u8], file_offset: u32) -> Option<Vec<u8>> {
    match typ {
        FILTER_E8 | FILTER_E8E9 => {
            let mut out = data.to_vec();
            let file_size: u32 = 0x100_0000;
            let cmp_byte2 = if typ == FILTER_E8E9 { 0xe9 } else { 0xe8 };
            let data_size = out.len();
            let mut pos = 0usize;
            while pos + 4 < data_size {
                let cur = out[pos];
                pos += 1;
                if cur == 0xe8 || cur == cmp_byte2 {
                    let offset = (pos as u32).wrapping_add(file_offset) % file_size;
                    let addr =
                        u32::from_le_bytes([out[pos], out[pos + 1], out[pos + 2], out[pos + 3]]);
                    if addr & 0x8000_0000 != 0 {
                        if addr.wrapping_add(offset) & 0x8000_0000 == 0 {
                            let new = addr.wrapping_add(file_size);
                            out[pos..pos + 4].copy_from_slice(&new.to_le_bytes());
                        }
                    } else if addr.wrapping_sub(file_size) & 0x8000_0000 != 0 {
                        let new = addr.wrapping_sub(offset);
                        out[pos..pos + 4].copy_from_slice(&new.to_le_bytes());
                    }
                    pos += 4;
                }
            }
            Some(out)
        }
        FILTER_ARM => {
            let mut out = data.to_vec();
            let mut pos = 0usize;
            while pos + 3 < out.len() {
                if out[pos + 3] == 0xeb {
                    let mut offset = out[pos] as u32
                        + (out[pos + 1] as u32) * 0x100
                        + (out[pos + 2] as u32) * 0x1_0000;
                    offset = offset.wrapping_sub((file_offset + pos as u32) / 4);
                    out[pos] = offset as u8;
                    out[pos + 1] = (offset >> 8) as u8;
                    out[pos + 2] = (offset >> 16) as u8;
                }
                pos += 4;
            }
            Some(out)
        }
        FILTER_DELTA => {
            let channels = channels as usize;
            if channels == 0 {
                return None;
            }
            let mut dst = vec![0u8; data.len()];
            let mut src_pos = 0usize;
            for channel in 0..channels {
                let mut prev = 0u8;
                let mut dest = channel;
                while dest < data.len() {
                    if src_pos >= data.len() {
                        return None;
                    }
                    prev = prev.wrapping_sub(data[src_pos]);
                    src_pos += 1;
                    dst[dest] = prev;
                    dest += channels;
                }
            }
            Some(dst)
        }
        _ => None,
    }
}
