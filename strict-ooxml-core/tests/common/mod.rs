//! Shared helpers for Stage-1 integration and property tests.
//!
//! Builds minimal ZIP archives in memory so tests can exercise the whole
//! `Package` pipeline without checking binary fixtures into the repository.

#![allow(clippy::cast_possible_truncation)]

use miniz_oxide::deflate::compress_to_vec;

/// Builds a ZIP archive from `(name, bytes, deflate)` entries.
///
/// `deflate == false` stores the entry verbatim; `true` uses raw DEFLATE.
pub(crate) fn build_zip(entries: &[(&str, &[u8], bool)]) -> Vec<u8> {
    let mut local = Vec::new();
    let mut central = Vec::new();
    let mut offsets = Vec::new();
    for (name, content, deflate) in entries {
        offsets.push(local.len() as u32);
        let stored = if *deflate {
            compress_to_vec(content, 6)
        } else {
            content.to_vec()
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let crc = crc32(content);
        push_local_header(&mut local, name, method, crc, content.len(), stored.len());
        local.extend_from_slice(&stored);
    }
    let cd_offset = local.len() as u32;
    for ((name, content, deflate), offset) in entries.iter().zip(offsets) {
        let stored_len = if *deflate {
            compress_to_vec(content, 6).len()
        } else {
            content.len()
        };
        let method: u16 = if *deflate { 8 } else { 0 };
        let crc = crc32(content);
        push_central_header(
            &mut central,
            name,
            method,
            crc,
            content.len(),
            stored_len,
            offset,
        );
    }
    let cd_size = central.len() as u32;
    let mut out = local;
    out.extend_from_slice(&central);
    out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
    out.extend_from_slice(&cd_size.to_le_bytes());
    out.extend_from_slice(&cd_offset.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out
}

fn push_local_header(
    out: &mut Vec<u8>,
    name: &str,
    method: u16,
    crc: u32,
    size: usize,
    stored: usize,
) {
    out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(stored as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}

#[allow(clippy::too_many_arguments)]
fn push_central_header(
    out: &mut Vec<u8>,
    name: &str,
    method: u16,
    crc: u32,
    size: usize,
    stored: usize,
    offset: u32,
) {
    out.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&20u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&method.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&crc.to_le_bytes());
    out.extend_from_slice(&(stored as u32).to_le_bytes());
    out.extend_from_slice(&(size as u32).to_le_bytes());
    out.extend_from_slice(&(name.len() as u16).to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());
    out.extend_from_slice(name.as_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}
