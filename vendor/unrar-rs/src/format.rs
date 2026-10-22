//! RAR header constants and the public archive entry model.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::hash::{HashValue, BLAKE2_DIGEST_SIZE};

pub const RAR5_SIGNATURE: [u8; 8] = [0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x01, 0x00];
pub const RAR15_SIGNATURE: [u8; 7] = [0x52, 0x61, 0x72, 0x21, 0x1a, 0x07, 0x00];
pub const RAR14_SIGNATURE: [u8; 4] = [0x52, 0x45, 0x7e, 0x5e];

pub const MAX_HEADER_SIZE_RAR5: u64 = 2_000_000;
/// Default number of leading bytes scanned for an embedded SFX signature.
/// Matches UnRAR's `MAXSFXSIZE` (4 MiB).
pub const DEFAULT_MAX_SFX_SCAN: u64 = 0x40_0000;

pub const HFL_EXTRA: u64 = 0x0001;
pub const HFL_DATA: u64 = 0x0002;
pub const HFL_SKIPIFUNKNOWN: u64 = 0x0004;
pub const HFL_SPLITBEFORE: u64 = 0x0008;
pub const HFL_SPLITAFTER: u64 = 0x0010;
pub const HFL_CHILD: u64 = 0x0020;
pub const HFL_INHERITED: u64 = 0x0040;

pub const MHFL_VOLUME: u64 = 0x0001;
pub const MHFL_VOLNUMBER: u64 = 0x0002;
pub const MHFL_SOLID: u64 = 0x0004;
pub const MHFL_PROTECT: u64 = 0x0008;
pub const MHFL_LOCK: u64 = 0x0010;

pub const FHFL_DIRECTORY: u64 = 0x0001;
pub const FHFL_UTIME: u64 = 0x0002;
pub const FHFL_CRC32: u64 = 0x0004;
pub const FHFL_UNPUNKNOWN: u64 = 0x0008;

pub const EHFL_NEXTVOLUME: u64 = 0x0001;
pub const CHFL_CRYPT_PSWCHECK: u64 = 0x0001;

pub const FCI_SOLID: u64 = 0x0040;
pub const FCI_RAR5_COMPAT: u64 = 0x0010_0000;

pub const FHEXTRA_CRYPT: u64 = 0x01;
pub const FHEXTRA_HASH: u64 = 0x02;
pub const FHEXTRA_HTIME: u64 = 0x03;
pub const FHEXTRA_VERSION: u64 = 0x04;
pub const FHEXTRA_REDIR: u64 = 0x05;
pub const FHEXTRA_UOWNER: u64 = 0x06;
pub const FHEXTRA_SUBDATA: u64 = 0x07;

pub const FHEXTRA_CRYPT_PSWCHECK: u64 = 0x01;
pub const FHEXTRA_CRYPT_HASHMAC: u64 = 0x02;

pub const FHEXTRA_HTIME_UNIXTIME: u64 = 0x01;
pub const FHEXTRA_HTIME_MTIME: u64 = 0x02;
pub const FHEXTRA_HTIME_CTIME: u64 = 0x04;
pub const FHEXTRA_HTIME_ATIME: u64 = 0x08;
pub const FHEXTRA_HTIME_UNIX_NS: u64 = 0x10;

pub const FHEXTRA_REDIR_DIR: u64 = 0x01;

pub const FHEXTRA_UOWNER_UNAME: u64 = 0x01;
pub const FHEXTRA_UOWNER_GNAME: u64 = 0x02;
pub const FHEXTRA_UOWNER_NUMUID: u64 = 0x04;
pub const FHEXTRA_UOWNER_NUMGID: u64 = 0x08;

pub const MHEXTRA_LOCATOR: u64 = 0x01;
pub const MHEXTRA_METADATA: u64 = 0x02;

// RAR 1.5 - 4.x.
pub const LHD_SPLIT_BEFORE: u16 = 0x0001;
pub const LHD_SPLIT_AFTER: u16 = 0x0002;
pub const LHD_PASSWORD: u16 = 0x0004;
pub const LHD_COMMENT: u16 = 0x0008;
pub const LHD_SOLID: u16 = 0x0010;
pub const LHD_WINDOWMASK: u16 = 0x00e0;
pub const LHD_DIRECTORY: u16 = 0x00e0;
pub const LHD_LARGE: u16 = 0x0100;
pub const LHD_UNICODE: u16 = 0x0200;
pub const LHD_SALT: u16 = 0x0400;
pub const LHD_VERSION: u16 = 0x0800;
pub const LHD_EXTTIME: u16 = 0x1000;
pub const LONG_BLOCK: u16 = 0x8000;

pub const HEAD3_MAIN: u8 = 0x73;
pub const HEAD3_FILE: u8 = 0x74;
pub const HEAD3_CMT: u8 = 0x75;
pub const HEAD3_AV: u8 = 0x76;
pub const HEAD3_OLDSERVICE: u8 = 0x77;
pub const HEAD3_PROTECT: u8 = 0x78;
pub const HEAD3_SIGN: u8 = 0x79;
pub const HEAD3_SERVICE: u8 = 0x7a;
pub const HEAD3_ENDARC: u8 = 0x7b;

pub const HEAD_MARK: u8 = 0x00;
pub const HEAD_MAIN: u8 = 0x01;
pub const HEAD_FILE: u8 = 0x02;
pub const HEAD_SERVICE: u8 = 0x03;
pub const HEAD_CRYPT: u8 = 0x04;
pub const HEAD_ENDARC: u8 = 0x05;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rar14,
    Rar15,
    Rar50,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    Windows,
    Unix,
    Unknown(u8),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RedirectKind {
    UnixSymlink,
    WindowsSymlink,
    Junction,
    HardLink,
    FileCopy,
    Unknown(u64),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Redirect {
    pub kind: RedirectKind,
    pub name: String,
    pub is_dir: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileCrypt {
    pub salt: [u8; 16],
    pub init_v: [u8; 16],
    pub lg2_count: u8,
    pub psw_check: Option<[u8; 8]>,
    pub use_hash_key: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct UnixOwner {
    pub owner_numeric: bool,
    pub group_numeric: bool,
    pub owner_id: Option<u32>,
    pub group_id: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub is_dir: bool,
    pub unpacked_size: u64,
    pub packed_size: u64,
    pub method: u8,
    pub solid: bool,
    pub encrypted: bool,
    pub split_before: bool,
    pub split_after: bool,
    pub crc32: Option<u32>,
    pub blake2: Option<[u8; BLAKE2_DIGEST_SIZE]>,
    pub mtime: Option<SystemTime>,
    pub ctime: Option<SystemTime>,
    pub atime: Option<SystemTime>,
    pub host_os: HostOs,
    pub attributes: u32,
    pub redirect: Option<Redirect>,
    pub sub_data: Vec<u8>,
    pub version: Option<u32>,
    pub owner: Option<UnixOwner>,
    pub owner_name: Option<String>,
    pub group_name: Option<String>,
    pub service: bool,
    pub(crate) unp_ver: u32,
    pub(crate) win_size: u64,
    pub(crate) crypt: Option<FileCrypt>,
    pub(crate) rar3_salt: Option<[u8; 8]>,
    pub(crate) segments: Vec<DataSegment>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataSegment {
    pub volume: usize,
    pub offset: u64,
    pub len: u64,
}

impl Entry {
    pub(crate) fn expected_hash(&self) -> HashValue {
        if let Some(digest) = self.blake2 {
            HashValue::Blake2(digest)
        } else if let Some(crc) = self.crc32 {
            HashValue::Crc32(crc)
        } else {
            HashValue::None
        }
    }

    pub fn is_link(&self) -> bool {
        self.redirect.is_some()
    }

    /// Encryption parameters for this entry, if the data is encrypted.
    pub fn crypt(&self) -> Option<&FileCrypt> {
        self.crypt.as_ref()
    }

    /// Internal compression algorithm version (50/70 for RAR 5/7, 29 for RAR
    /// 3.x, 20/26 for RAR 2.x, 15/13 for RAR 1.x).
    pub fn compression_version(&self) -> u32 {
        self.unp_ver
    }

    /// Dictionary size for this entry, in bytes.
    pub fn dictionary_size(&self) -> u64 {
        self.win_size
    }
}

#[derive(Clone, Debug, Default)]
pub struct ArchiveInfo {
    pub format: Option<Format>,
    pub is_volume: bool,
    pub is_solid: bool,
    pub is_locked: bool,
    pub has_recovery_record: bool,
    pub is_encrypted: bool,
    pub is_first_volume: bool,
    pub comment: Option<String>,
    pub volume_number: u32,
}

pub(crate) fn unix_time(secs: u64) -> Option<SystemTime> {
    UNIX_EPOCH.checked_add(Duration::from_secs(secs))
}

pub(crate) fn dos_time(dos: u32) -> Option<SystemTime> {
    let sec = (dos & 0x1f) * 2;
    let min = (dos >> 5) & 0x3f;
    let hour = (dos >> 11) & 0x1f;
    let day = (dos >> 16) & 0x1f;
    let month = (dos >> 21) & 0x0f;
    let year = ((dos >> 25) & 0x7f) + 1980;

    // Days since the Unix epoch using Howard Hinnant's civil date algorithm.
    let y = year as i64 - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;

    let total = days * 86400 + hour as i64 * 3600 + min as i64 * 60 + sec as i64;
    if total < 0 {
        return None;
    }
    unix_time(total as u64)
}

pub(crate) fn win_time(filetime: u64) -> Option<SystemTime> {
    const EPOCH_DIFF: u64 = 11_644_473_600;
    let secs = filetime / 10_000_000;
    let nanos = ((filetime % 10_000_000) * 100) as u32;
    if secs < EPOCH_DIFF {
        return None;
    }
    UNIX_EPOCH.checked_add(Duration::new(secs - EPOCH_DIFF, nanos))
}
