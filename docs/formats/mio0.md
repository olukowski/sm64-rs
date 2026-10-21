# MIO0

`sm64-mio0` is an independent, safe Rust decoder. It has no engine or external
crate dependencies and uses only authored synthetic test data. Format reference:
[Hack64's MIO0 description](https://hack64.net/wiki/doku.php?id=super_mario_64:mio0).
The description and implementation here are written afresh; no game code or game
bytes are included.

## Layout

All offsets are relative to the start of the block. The header is 16 bytes:

| Offset | Size | Meaning |
| --- | --- | --- |
| 0 | 4 | ASCII `MIO0` |
| 4 | 4 | Decoded byte count, big-endian unsigned integer |
| 8 | 4 | Backreference stream offset, big-endian unsigned integer |
| 12 | 4 | Literal stream offset, big-endian unsigned integer |

Flags start at byte 16. Each flag byte is consumed most-significant bit first:

- `1`: copy one byte from the literal stream.
- `0`: read a big-endian two-byte reference. Its high four bits give the copy
  length minus 3; its low twelve bits give the backward distance minus 1.
  Thus lengths are 3–18 and distances are 1–4096. Copy one byte at a time so
  overlapping references can repeat bytes just produced.

Stop when the advertised output size is reached. Unused bits in the final flag
byte do not need to be zero.

## API and validation

`Header::parse(&[u8])` checks magic, header length, and
`16 <= compressed_offset <= literal_offset <= input.len()` without allocating.
`decode(input, max_output)` returns `Decoded { bytes, consumed }` or a typed
`Error` implementing `Display` and `std::error::Error`.

The decoder rejects an advertised size over `max_output` before allocating or
reading streams. Allocation is fallible; callers must choose a suitable output
budget (there is no implicit global cap). Decoding work is linear in output size;
the decoder does not allocate based on input stream offsets.

Each stream has a separate boundary: flags end at the backreference offset,
backreferences end at the literal offset, and literals end at input length.
Missing tokens, references before the beginning of output, and copies exceeding
the remaining advertised output are errors. Copies are never clipped.

`consumed` is one past the furthest byte actually read, with a minimum of 16 for
the header. It includes gaps before used data and the full final flag byte.
Unused streams contribute nothing, even if their declared offsets are farther
away. Empty output therefore consumes exactly 16 bytes, though all offsets must
still be valid. This value is not the sum of stream lengths and does not include
unread trailing bytes or imply any alignment for a following block. Trailing data
is accepted; literal data has no intrinsic end marker, so callers that know the
container boundary should pass only that block's slice.

Tests cover synthetic literals, overlapping references, flag-byte transitions,
empty output, offsets, stream truncation, invalid distances, output overshoot,
limits, suffixes, and deterministic small-input robustness. No ROM is required.
