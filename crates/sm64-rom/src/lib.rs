//! Read-only Nintendo 64 ROM inspection.
//!
//! All offsets and identity hashes use canonical big-endian (.z64) bytes, regardless
//! of the source filename or byte order. Recognition is deliberately separate from
//! parsing: an unknown ROM can have a valid N64 header.

use std::fmt;
use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use serde::Serialize;
use sha1::{Digest, Sha1};
use sha2::Sha256;
use thiserror::Error;

pub const HEADER_SIZE: usize = 0x40;
/// Largest original N64 cartridge size; expanded ROM hacks are not yet supported.
pub const MAX_ROM_SIZE: usize = 64 * 1024 * 1024;
/// Canonical US revision 0 identity, independently checked against the local ROM.
/// Community reference: https://github.com/n64decomp/sm64/blob/master/sm64.us.sha1
pub const US_SHA1: &str = "9bef1128717f958171a4afac3ed78ee2bb4e86ce";
pub const US_SHA256: &str = "17ce077343c6133f8c9f2d6d6d9a4ab62c8cd2aa57c40aea1f490b4c8bb21d91";

#[derive(Debug, Error)]
pub enum Error {
    #[error("could not read ROM: {0}")]
    Io(#[from] io::Error),
    #[error("ROM is too short: {actual} bytes (need at least {HEADER_SIZE})")]
    TooShort { actual: usize },
    #[error("ROM exceeds the {MAX_ROM_SIZE}-byte safety limit")]
    TooLarge,
    #[error("unrecognized N64 byte-order magic: {magic:02x?}")]
    InvalidMagic { magic: [u8; 4] },
    #[error("ROM length {length} is not a multiple of four bytes")]
    Misaligned { length: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ByteOrder {
    BigEndian,
    ByteSwapped,
    LittleEndian,
}

impl ByteOrder {
    pub fn detect(bytes: &[u8]) -> Result<Self, Error> {
        let magic: [u8; 4] = bytes
            .get(..4)
            .ok_or(Error::TooShort {
                actual: bytes.len(),
            })?
            .try_into()
            .expect("four-byte slice");
        match magic {
            [0x80, 0x37, 0x12, 0x40] => Ok(Self::BigEndian),
            [0x37, 0x80, 0x40, 0x12] => Ok(Self::ByteSwapped),
            [0x40, 0x12, 0x37, 0x80] => Ok(Self::LittleEndian),
            _ => Err(Error::InvalidMagic { magic }),
        }
    }
}

impl fmt::Display for ByteOrder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BigEndian => "big-endian (.z64)",
            Self::ByteSwapped => "byte-swapped (.v64)",
            Self::LittleEndian => "little-endian (.n64)",
        })
    }
}

/// N64 cartridge header. CRC fields are stored values, NOT verified checksums.
/// Format: https://n64brew.dev/wiki/ROM_Header
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Header {
    pub clock_rate: u32,
    pub entry_point: u32,
    pub release: u32,
    pub crc1: u32,
    pub crc2: u32,
    /// Terminal-safe representation; trailing spaces/NULs removed.
    pub title: String,
    /// Four bytes at 0x3b..0x3f (manufacturer, cartridge ID, country).
    pub game_code: String,
    pub country_code: u8,
    pub revision: u8,
}

impl Header {
    /// Parse canonical big-endian header bytes, without requiring a complete ROM.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::TooShort {
                actual: bytes.len(),
            });
        }
        if ByteOrder::detect(bytes)? != ByteOrder::BigEndian {
            return Err(Error::InvalidMagic {
                magic: bytes[..4].try_into().expect("checked header size"),
            });
        }
        let word = |offset| u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let title = &bytes[0x20..0x34];
        let title_end = title
            .iter()
            .rposition(|&byte| byte != b' ' && byte != 0)
            .map_or(0, |index| index + 1);
        Ok(Self {
            clock_rate: word(0x04),
            entry_point: word(0x08),
            release: word(0x0c),
            crc1: word(0x10),
            crc2: word(0x14),
            title: printable(&title[..title_end]),
            game_code: printable(&bytes[0x3b..0x3f]),
            country_code: bytes[0x3e],
            revision: bytes[0x3f],
        })
    }

    pub fn country_name(&self) -> &'static str {
        match self.country_code {
            b'E' => "North America",
            b'J' => "Japan",
            b'P' => "Europe",
            b'D' => "Germany",
            b'F' => "France",
            b'I' => "Italy",
            b'S' => "Spain",
            b'U' => "Australia",
            _ => "unknown",
        }
    }
}

fn printable(bytes: &[u8]) -> String {
    let mut result = String::new();
    for &byte in bytes {
        if (0x20..=0x7e).contains(&byte) && byte != b'\\' {
            result.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(result, "\\x{byte:02x}").expect("writing to String");
        }
    }
    result
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum KnownRevision {
    UsRevision0,
}

impl fmt::Display for KnownRevision {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UsRevision0 => f.write_str("Super Mario 64 — US revision 0"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Identity {
    pub sha1: String,
    pub sha256: String,
    pub known_revision: Option<KnownRevision>,
}

/// An owned, normalized ROM image. No API writes back to the source file.
#[derive(Debug)]
pub struct Rom {
    bytes: Vec<u8>,
    source_order: ByteOrder,
    header: Header,
    identity: Identity,
}

impl Rom {
    /// Read at most the safety limit plus one byte, including for nonregular files.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let file = File::open(path)?;
        if file.metadata()?.len() > MAX_ROM_SIZE as u64 {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        file.take(MAX_ROM_SIZE as u64 + 1).read_to_end(&mut bytes)?;
        Self::from_bytes(bytes)
    }

    pub fn from_bytes(mut bytes: Vec<u8>) -> Result<Self, Error> {
        if bytes.len() > MAX_ROM_SIZE {
            return Err(Error::TooLarge);
        }
        if bytes.len() < HEADER_SIZE {
            return Err(Error::TooShort {
                actual: bytes.len(),
            });
        }
        let source_order = ByteOrder::detect(&bytes)?;
        if !bytes.len().is_multiple_of(4) {
            return Err(Error::Misaligned {
                length: bytes.len(),
            });
        }
        match source_order {
            ByteOrder::BigEndian => {}
            ByteOrder::ByteSwapped => {
                for pair in bytes.as_chunks_mut::<2>().0 {
                    pair.swap(0, 1);
                }
            }
            ByteOrder::LittleEndian => {
                for word in bytes.as_chunks_mut::<4>().0 {
                    word.reverse();
                }
            }
        }
        let header = Header::parse(&bytes)?;
        let sha1 = format!("{:x}", Sha1::digest(&bytes));
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        let known_revision =
            (bytes.len() == 8 * 1024 * 1024 && sha1 == US_SHA1 && sha256 == US_SHA256)
                .then_some(KnownRevision::UsRevision0);
        Ok(Self {
            bytes,
            source_order,
            header,
            identity: Identity {
                sha1,
                sha256,
                known_revision,
            },
        })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn source_order(&self) -> ByteOrder {
        self.source_order
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = vec![0; 128];
        bytes[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        bytes[8..12].copy_from_slice(&0x8000_1234u32.to_be_bytes());
        bytes[0x10..0x14].copy_from_slice(&0x1234_abcdu32.to_be_bytes());
        bytes[0x20..0x34].copy_from_slice(b"SYNTHETIC FIXTURE   ");
        bytes[0x3b..0x3f].copy_from_slice(b"NXTE");
        bytes[0x3f] = 7;
        bytes[64..68].copy_from_slice(&[1, 2, 3, 4]);
        bytes
    }

    #[test]
    fn parses_authored_header() {
        let rom = Rom::from_bytes(fixture()).unwrap();
        assert_eq!(rom.header().title, "SYNTHETIC FIXTURE");
        assert_eq!(rom.header().entry_point, 0x8000_1234);
        assert_eq!(rom.header().crc1, 0x1234_abcd);
        assert_eq!(rom.header().game_code, "NXTE");
        assert_eq!(rom.header().country_name(), "North America");
        assert_eq!(rom.header().revision, 7);
        assert_eq!(rom.identity().known_revision, None);
    }

    #[test]
    fn all_three_orders_have_identical_canonical_identity() {
        let original = fixture();
        let canonical = Rom::from_bytes(original.clone()).unwrap();
        for (order, width) in [(ByteOrder::ByteSwapped, 2), (ByteOrder::LittleEndian, 4)] {
            let mut swapped = original.clone();
            for chunk in swapped.chunks_exact_mut(width) {
                chunk.reverse();
            }
            let rom = Rom::from_bytes(swapped).unwrap();
            assert_eq!(rom.source_order(), order);
            assert_eq!(rom.bytes(), original);
            assert_eq!(rom.header(), canonical.header());
            assert_eq!(rom.identity(), canonical.identity());
        }
    }

    #[test]
    fn changing_payload_changes_identity() {
        let mut bytes = fixture();
        let before = Rom::from_bytes(bytes.clone()).unwrap();
        bytes[70] ^= 1;
        let after = Rom::from_bytes(bytes).unwrap();
        assert_ne!(before.identity().sha1, after.identity().sha1);
        assert_ne!(before.identity().sha256, after.identity().sha256);
    }

    #[test]
    fn rejects_short_unknown_and_unaligned_inputs() {
        for length in 0..HEADER_SIZE {
            assert!(matches!(
                Rom::from_bytes(vec![0; length]),
                Err(Error::TooShort { .. })
            ));
            assert!(Header::parse(&vec![0; length]).is_err());
        }
        assert!(matches!(
            Rom::from_bytes(vec![0; 128]),
            Err(Error::InvalidMagic { .. })
        ));
        for width in [1, 2, 3] {
            let mut bytes = fixture();
            bytes.extend(vec![0; width]);
            assert!(matches!(
                Rom::from_bytes(bytes),
                Err(Error::Misaligned { .. })
            ));
        }
    }

    #[test]
    fn terminal_control_characters_are_escaped() {
        let mut bytes = fixture();
        bytes[0x20] = 0x1b;
        bytes[0x21] = 0xff;
        bytes[0x22] = b'\\';
        let rom = Rom::from_bytes(bytes).unwrap();
        assert!(rom.header().title.starts_with("\\x1b\\xff\\x5c"));
    }

    #[test]
    fn canonical_header_parser_rejects_swapped_input() {
        let mut bytes = fixture();
        for pair in bytes.as_chunks_mut::<2>().0 {
            pair.reverse();
        }
        assert!(Header::parse(&bytes).is_err());
    }
}
