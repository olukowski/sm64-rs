//! Bounded reconstruction of an 8 × 10 skybox pointer grid, not skybox projection.
use crate::{Error, Texture, WrapMode};
use sm64_level::segments::SegmentMap;

const ROWS: usize = 8;
const COLUMNS: usize = 10;
const UNIQUE_COLUMNS: usize = 8;
const TILE_SIDE: usize = 32;
const TILE_BYTES: usize = TILE_SIDE * TILE_SIDE * 2;
const SIDE: usize = UNIQUE_COLUMNS * TILE_SIDE;

/// Read 80 big-endian segmented tile pointers from a four-byte-aligned table.
/// Each pointer must be eight-byte aligned and cover a resident 32 × 32 RGBA16
/// tile. Columns 8 and 9 must repeat the *pointers* in columns 0 and 1 of each
/// row. All 80 references are checked, including those wrap columns.
///
/// Output rows follow table order, top to bottom; no vertical flip, camera
/// projection, cubemap conversion, or original screen-space scrolling is applied.
/// The table and tiles may reside in different segments. Nothing is loaded from ROM.
pub fn decode_panorama(segments: &SegmentMap, table: u32) -> Result<Texture, Error> {
    if table & 3 != 0 {
        return Err(Error(format!("unaligned skybox table {table:#010x}")));
    }
    let bytes = segments
        .resolve_range(table, ROWS * COLUMNS * 4)
        .map_err(|e| Error(format!("skybox table {table:#010x}: {e}")))?;
    let mut pointers = [0u32; ROWS * COLUMNS];
    for (index, word) in bytes.as_chunks::<4>().0.iter().enumerate() {
        let pointer = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
        if pointer & 7 != 0 {
            return Err(Error(format!(
                "unaligned skybox tile [{}, {}] at {pointer:#010x}",
                index / COLUMNS,
                index % COLUMNS
            )));
        }
        segments.resolve_range(pointer, TILE_BYTES).map_err(|e| {
            Error(format!(
                "skybox tile [{}, {}] at {pointer:#010x}: {e}",
                index / COLUMNS,
                index % COLUMNS
            ))
        })?;
        pointers[index] = pointer;
    }
    for row in 0..ROWS {
        for col in 0..2 {
            if pointers[row * COLUMNS + UNIQUE_COLUMNS + col] != pointers[row * COLUMNS + col] {
                return Err(Error(format!(
                    "skybox row {row} wrap column {} does not repeat column {col}",
                    UNIQUE_COLUMNS + col
                )));
            }
        }
    }

    let mut rgba8 = vec![0; SIDE * SIDE * 4];
    for row in 0..ROWS {
        for col in 0..UNIQUE_COLUMNS {
            let tile = segments
                .resolve_range(pointers[row * COLUMNS + col], TILE_BYTES)
                .map_err(|e| Error(format!("skybox tile: {e}")))?;
            for y in 0..TILE_SIDE {
                for x in 0..TILE_SIDE {
                    let source = (y * TILE_SIDE + x) * 2;
                    let value = u16::from_be_bytes([tile[source], tile[source + 1]]);
                    let five = |shift: u32| {
                        let channel = ((value >> shift) & 31) as u8;
                        (channel << 3) | (channel >> 2)
                    };
                    let target = ((row * TILE_SIDE + y) * SIDE + col * TILE_SIDE + x) * 4;
                    rgba8[target..target + 4].copy_from_slice(&[
                        five(11),
                        five(6),
                        five(1),
                        if value & 1 == 0 { 0 } else { 255 },
                    ]);
                }
            }
        }
    }
    Ok(Texture {
        width: SIDE as u32,
        height: SIDE as u32,
        rgba8,
        wrap_s: WrapMode::Repeat,
        wrap_t: WrapMode::Clamp,
    })
}
