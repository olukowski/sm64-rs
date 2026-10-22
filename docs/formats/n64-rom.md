# N64 ROM container and identity

The inspector accepts unpacked cartridge images with a 64-byte header and a
word-aligned length. Input is limited to 64 MiB. It opens the source read-only.

## Byte order

The first word distinguishes three encodings:

| Initial bytes (hex) | Encoding | Normalize to big-endian |
| --- | --- | --- |
| `80 37 12 40` | big-endian, commonly .z64 | no change |
| `37 80 40 12` | byte-swapped, commonly .v64 | swap each adjacent pair |
| `40 12 37 80` | little-endian, commonly .n64 | reverse each four-byte word |

Extensions are only conventions. Normalization happens in an owned memory buffer;
it does not create or alter a ROM file. Every public ROM byte slice, header field,
offset and identity hash uses the normalized bytes.

## Header subset

Integers are big-endian after normalization.

| Offset | Size | Meaning |
| --- | --- | --- |
| 0x04 | 4 | Clock-rate field |
| 0x08 | 4 | Entry point |
| 0x0c | 4 | Release field |
| 0x10 | 4 | Stored CRC1 |
| 0x14 | 4 | Stored CRC2 |
| 0x20 | 20 | Internal title |
| 0x3b | 4 | Manufacturer, two-byte cartridge ID, country |
| 0x3e | 1 | Country byte (also part of the four-byte code above) |
| 0x3f | 1 | Revision byte |

Text output removes trailing title spaces/NULs and escapes nonprintable/non-ASCII
bytes and backslashes; untrusted header bytes cannot inject terminal controls.

CRC1 and CRC2 are reported, not verified. The entry point is metadata, not something
the inspector executes. Parsing a header establishes neither bootability nor game
identity.

## Revision recognition

SHA-1 and SHA-256 cover the entire canonical image, including padding. Recognition
requires the expected length and both hashes. Currently only the original US
revision 0 is recognized. Unknown images still support generic header inspection
and MIO0 scanning; no US-specific addresses are applied to them.

## References

- [N64brew ROM header documentation](https://n64brew.dev/wiki/ROM_Header)
- [n64decomp/sm64 canonical US SHA-1](https://github.com/n64decomp/sm64/blob/9921382a68bb0c865e5e45eb594d9c64db59b1af/sm64.us.sha1)

These notes and the Rust reader are written independently. No game code or
extracted ROM fixtures are included.
