//! Safe, bounded MIO0 decompression with no engine or external dependencies.
//!
//! Stream offsets are relative to the beginning of the header. Trailing bytes
//! are permitted; see [`Decoded::consumed`] for the exact consumption convention.

use std::fmt;

/// The fields following the `MIO0` magic, stored big-endian on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub decoded_size: u32,
    pub compressed_offset: u32,
    pub literal_offset: u32,
}

impl Header {
    /// Read the header and validate `16 <= compressed <= literal <= input.len()`.
    ///
    /// This does not read stream contents or allocate output.
    pub fn parse(input: &[u8]) -> Result<Self, Error> {
        if input.len() < 16 {
            return Err(Error::TruncatedHeader {
                available: input.len(),
            });
        }
        if &input[..4] != b"MIO0" {
            return Err(Error::InvalidMagic);
        }
        let word = |start| u32::from_be_bytes(input[start..start + 4].try_into().unwrap());
        let header = Self {
            decoded_size: word(4),
            compressed_offset: word(8),
            literal_offset: word(12),
        };
        if header.compressed_offset < 16
            || header.compressed_offset > header.literal_offset
            || usize::try_from(header.literal_offset).map_or(true, |end| end > input.len())
        {
            return Err(Error::InvalidOffsets {
                compressed: header.compressed_offset,
                literal: header.literal_offset,
                input_len: input.len(),
            });
        }
        Ok(header)
    }
}

/// Decoded data and the extent of input actually read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub bytes: Vec<u8>,
    /// One past the furthest byte read, relative to the input's beginning.
    ///
    /// Includes the 16-byte header and any intervening padding. An unused
    /// stream contributes no extent, even if its offset is beyond this value.
    /// In particular, an empty output consumes exactly 16 bytes. The final
    /// flag byte counts in full, including unused bits. This is not a sum of
    /// stream lengths, nor necessarily the next block's start (padding may follow).
    pub consumed: usize,
}

/// An independently bounded MIO0 input stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Flags,
    Backreferences,
    Literals,
}

/// Header, stream, or resource-limit failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    TruncatedHeader {
        available: usize,
    },
    InvalidMagic,
    InvalidOffsets {
        compressed: u32,
        literal: u32,
        input_len: usize,
    },
    OutputLimitExceeded {
        decoded_size: u32,
        max_output: usize,
    },
    AllocationFailed,
    TruncatedStream {
        stream: Stream,
        offset: usize,
    },
    InvalidBackreference {
        distance: usize,
        produced: usize,
    },
    OutputOvershoot {
        length: usize,
        remaining: usize,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TruncatedHeader { available } => {
                write!(f, "MIO0 header needs 16 bytes, got {available}")
            }
            Self::InvalidMagic => write!(f, "invalid MIO0 magic"),
            Self::InvalidOffsets {
                compressed,
                literal,
                input_len,
            } => write!(
                f,
                "invalid MIO0 stream offsets: compressed={compressed}, literal={literal}, input length={input_len}"
            ),
            Self::OutputLimitExceeded {
                decoded_size,
                max_output,
            } => write!(
                f,
                "MIO0 output size {decoded_size} exceeds limit {max_output}"
            ),
            Self::AllocationFailed => write!(f, "could not allocate MIO0 output"),
            Self::TruncatedStream { stream, offset } => {
                write!(f, "truncated MIO0 {stream:?} stream at offset {offset}")
            }
            Self::InvalidBackreference { distance, produced } => write!(
                f,
                "MIO0 backreference distance {distance} exceeds produced size {produced}"
            ),
            Self::OutputOvershoot { length, remaining } => write!(
                f,
                "MIO0 backreference length {length} exceeds remaining output {remaining}"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// Decode one MIO0 block, allowing trailing input.
///
/// Checks the advertised output against `max_output` before any allocation and
/// reserves output fallibly. Callers must choose an appropriate memory budget;
/// there is no implicit global limit. Flags are bounded by the compressed-stream
/// offset, references by the literal-stream offset, and literals by input length.
/// A reference must fit the remaining output exactly or leave room; it is never
/// silently clipped. Overlapping backreferences are supported.
pub fn decode(input: &[u8], max_output: usize) -> Result<Decoded, Error> {
    let header = Header::parse(input)?;
    let size = usize::try_from(header.decoded_size)
        .ok()
        .filter(|&size| size <= max_output)
        .ok_or(Error::OutputLimitExceeded {
            decoded_size: header.decoded_size,
            max_output,
        })?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(size)
        .map_err(|_| Error::AllocationFailed)?;

    // Header validation guarantees these offsets fit usize and lie in input.
    let mut reference_pos = header.compressed_offset as usize;
    let mut literal_pos = header.literal_offset as usize;
    let mut flag_pos = 16;
    let mut consumed = 16;
    let mut flags = 0u8;
    let mut bits_left = 0;
    while bytes.len() < size {
        if bits_left == 0 {
            if flag_pos == header.compressed_offset as usize {
                return Err(Error::TruncatedStream {
                    stream: Stream::Flags,
                    offset: flag_pos,
                });
            }
            flags = input[flag_pos];
            flag_pos += 1;
            consumed = consumed.max(flag_pos);
            bits_left = 8;
        }
        let literal = flags & 0x80 != 0;
        flags <<= 1;
        bits_left -= 1;
        if literal {
            let byte = *input.get(literal_pos).ok_or(Error::TruncatedStream {
                stream: Stream::Literals,
                offset: literal_pos,
            })?;
            literal_pos += 1;
            consumed = consumed.max(literal_pos);
            bytes.push(byte);
        } else {
            if header.literal_offset as usize - reference_pos < 2 {
                return Err(Error::TruncatedStream {
                    stream: Stream::Backreferences,
                    offset: reference_pos,
                });
            }
            let pair = u16::from_be_bytes([input[reference_pos], input[reference_pos + 1]]);
            reference_pos += 2;
            consumed = consumed.max(reference_pos);
            let length = usize::from(pair >> 12) + 3;
            let distance = usize::from(pair & 0x0fff) + 1;
            if distance > bytes.len() {
                return Err(Error::InvalidBackreference {
                    distance,
                    produced: bytes.len(),
                });
            }
            if length > size - bytes.len() {
                return Err(Error::OutputOvershoot {
                    length,
                    remaining: size - bytes.len(),
                });
            }
            for _ in 0..length {
                let byte = bytes[bytes.len() - distance];
                bytes.push(byte);
            }
        }
    }
    Ok(Decoded { bytes, consumed })
}

#[cfg(test)]
mod tests {
    use super::*;

    // All fixtures are authored here, not extracted from a game.
    fn header(size: u32, compressed: u32, literal: u32) -> Vec<u8> {
        let mut data = b"MIO0".to_vec();
        for word in [size, compressed, literal] {
            data.extend_from_slice(&word.to_be_bytes());
        }
        data
    }

    fn block(size: u32, flags: &[u8], references: &[u8], literals: &[u8]) -> Vec<u8> {
        let compressed = 16 + flags.len() as u32;
        let mut data = header(size, compressed, compressed + references.len() as u32);
        data.extend_from_slice(flags);
        data.extend_from_slice(references);
        data.extend_from_slice(literals);
        data
    }

    #[test]
    fn parses_big_endian_header() {
        let data = header(0x12345678, 16, 16);
        assert_eq!(
            Header::parse(&data),
            Ok(Header {
                decoded_size: 0x12345678,
                compressed_offset: 16,
                literal_offset: 16,
            })
        );
    }

    #[test]
    fn literal_and_suffix() {
        let mut data = block(3, &[0xe0], &[], b"abc");
        let consumed = data.len();
        data.extend_from_slice(b"unrelated suffix");
        assert_eq!(
            decode(&data, 3),
            Ok(Decoded {
                bytes: b"abc".to_vec(),
                consumed
            })
        );
    }

    #[test]
    fn overlapping_reference() {
        let data = block(10, &[0x80], &[0x60, 0x00], b"a");
        assert_eq!(decode(&data, 10).unwrap().bytes, b"aaaaaaaaaa");
    }

    #[test]
    fn nontrivial_distance_and_max_length() {
        let data = block(21, &[0xe0], &[0xf0, 0x02], b"abc");
        assert_eq!(decode(&data, 21).unwrap().bytes, b"abcabcabcabcabcabcabc");
    }

    #[test]
    fn flags_cross_byte_boundary() {
        let data = block(12, &[0xff, 0x80], &[0x00, 0x02], b"abcdefghi");
        assert_eq!(decode(&data, 12).unwrap().bytes, b"abcdefghighi");
    }

    #[test]
    fn empty_output_uses_only_header() {
        for data in [header(0, 16, 16), block(0, &[0xff], &[0, 0], b"unused")] {
            assert_eq!(
                decode(&data, 0),
                Ok(Decoded {
                    bytes: vec![],
                    consumed: 16
                })
            );
        }
    }

    #[test]
    fn first_reference_requires_existing_output() {
        // A reference cannot seed output.
        let data = block(3, &[0], &[0, 0], b"padding");
        assert_eq!(
            decode(&data, 3),
            Err(Error::InvalidBackreference {
                distance: 1,
                produced: 0
            })
        );
    }

    #[test]
    fn truncated_headers_and_magic() {
        let data = header(0, 16, 16);
        for end in 0..16 {
            assert_eq!(
                Header::parse(&data[..end]),
                Err(Error::TruncatedHeader { available: end })
            );
        }
        let mut bad = data;
        bad[0] = b'x';
        assert_eq!(Header::parse(&bad), Err(Error::InvalidMagic));
    }

    #[test]
    fn rejects_invalid_offsets_even_for_empty_output() {
        for (compressed, literal) in [(0, 16), (15, 16), (17, 16), (16, 17), (u32::MAX, u32::MAX)] {
            assert!(matches!(
                decode(&header(0, compressed, literal), 0),
                Err(Error::InvalidOffsets { .. })
            ));
        }
    }

    #[test]
    fn missing_flags_cannot_read_reference_stream() {
        let mut data = header(1, 16, 18);
        data.extend_from_slice(&[0xff, 0xff, b'a']);
        assert_eq!(
            decode(&data, 1),
            Err(Error::TruncatedStream {
                stream: Stream::Flags,
                offset: 16
            })
        );
    }

    #[test]
    fn flags_exhausted_after_eight_tokens() {
        let data = block(9, &[0xff], &[], b"abcdefghi");
        assert_eq!(
            decode(&data, 9),
            Err(Error::TruncatedStream {
                stream: Stream::Flags,
                offset: 17
            })
        );
    }

    #[test]
    fn truncated_reference_cannot_read_literals() {
        for references in [&[][..], &[0][..]] {
            let data = block(4, &[0x80], references, b"a");
            assert_eq!(
                decode(&data, 4),
                Err(Error::TruncatedStream {
                    stream: Stream::Backreferences,
                    offset: 17
                })
            );
        }
    }

    #[test]
    fn truncated_literals() {
        let data = block(2, &[0xc0], &[], b"a");
        assert_eq!(
            decode(&data, 2),
            Err(Error::TruncatedStream {
                stream: Stream::Literals,
                offset: data.len()
            })
        );
    }

    #[test]
    fn reference_before_start_and_max_distance() {
        for (references, distance) in [([0, 1], 2), ([0x0f, 0xff], 4096)] {
            let data = block(4, &[0x80], &references, b"a");
            assert_eq!(
                decode(&data, 4),
                Err(Error::InvalidBackreference {
                    distance,
                    produced: 1
                })
            );
        }
    }

    #[test]
    fn rejects_output_overshoot() {
        let data = block(3, &[0x80], &[0, 0], b"a");
        assert_eq!(
            decode(&data, 3),
            Err(Error::OutputOvershoot {
                length: 3,
                remaining: 2
            })
        );
    }

    #[test]
    fn output_limit_checked_before_streams_or_allocation() {
        let data = header(u32::MAX, 16, 16);
        assert_eq!(
            decode(&data, 1024),
            Err(Error::OutputLimitExceeded {
                decoded_size: u32::MAX,
                max_output: 1024
            })
        );
        assert!(matches!(
            decode(&block(1, &[0x80], &[], b"a"), 0),
            Err(Error::OutputLimitExceeded { .. })
        ));
    }

    #[test]
    fn arbitrary_small_inputs_do_not_panic_or_exceed_limit() {
        let mut state = 0x12345678u32;
        for len in 0..96 {
            for _ in 0..128 {
                let mut data = vec![0; len];
                for byte in &mut data {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    *byte = state as u8;
                }
                if let Ok(decoded) = decode(&data, 64) {
                    assert!(decoded.bytes.len() <= 64);
                    assert!((16..=data.len()).contains(&decoded.consumed));
                }
                // Also exercise structurally valid headers with random streams.
                if len >= 16 {
                    let compressed = 16 + (data[8] as usize % (len - 15));
                    let literal = compressed + (data[12] as usize % (len - compressed + 1));
                    let size = u32::from(data[4] % 65);
                    data[..16].copy_from_slice(&header(size, compressed as u32, literal as u32));
                }
                if let Ok(decoded) = decode(&data, 64) {
                    assert!(decoded.bytes.len() <= 64);
                    assert!((16..=data.len()).contains(&decoded.consumed));
                }
            }
        }
    }
}
