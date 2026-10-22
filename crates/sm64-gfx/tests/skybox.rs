use sm64_gfx::{WrapMode, skybox::decode_panorama};
use sm64_level::segments::SegmentMap;

const TABLE: u32 = 0x0b000004;
const TILE_BYTES: usize = 32 * 32 * 2;

fn pointer(table: &mut [u8], row: usize, col: usize, address: u32) {
    let start = 4 + (row * 10 + col) * 4;
    table[start..start + 4].copy_from_slice(&address.to_be_bytes());
}

// Authored data: deliberately shuffled tile storage, separate table segment,
// nonzero tile base, and a pattern varying by tile, local x/y, and alpha.
fn fixture() -> (Vec<u8>, Vec<u8>) {
    let mut tiles = vec![0; 8 + 64 * TILE_BYTES];
    let mut table = vec![0; 4 + 80 * 4];
    for row in 0..8 {
        for col in 0..10 {
            let tile = row * 8 + col % 8;
            let offset = 8 + (63 - tile) * TILE_BYTES;
            pointer(&mut table, row, col, 0x0a000000 | offset as u32);
            for y in 0..32 {
                for x in 0..32 {
                    let r = (tile + x) % 32;
                    let g = (row * 3 + y) % 32;
                    let b = (col % 8 * 4 + x + y) % 32;
                    let alpha = (x + y + tile) % 2;
                    let word = ((r << 11) | (g << 6) | (b << 1) | alpha) as u16;
                    let start = offset + (y * 32 + x) * 2;
                    tiles[start..start + 2].copy_from_slice(&word.to_be_bytes());
                }
            }
        }
    }
    (tiles, table)
}

fn map(tiles: &[u8], table: &[u8]) -> SegmentMap {
    let mut segments = SegmentMap::default();
    segments
        .load(tiles, 10, 0, tiles.len() as u32, false)
        .unwrap();
    segments
        .load(table, 11, 0, table.len() as u32, false)
        .unwrap();
    segments
}

#[test]
fn patterned_tiles_follow_pointer_order_and_preserve_pixel_orientation() {
    let (tiles, table) = fixture();
    let texture = decode_panorama(&map(&tiles, &table), TABLE).unwrap();
    assert_eq!((texture.width, texture.height), (256, 256));
    assert_eq!(texture.wrap_s, WrapMode::Repeat);
    assert_eq!(texture.wrap_t, WrapMode::Clamp);
    assert_eq!(texture.rgba8.len(), 256 * 256 * 4);
    for y in 0..256 {
        for x in 0..256 {
            let row = y / 32;
            let col = x / 32;
            let tile = row * 8 + col;
            // Independent channel expansion (multiplication/division, not shifts).
            let expand = |v: usize| (v * 8 + v / 4) as u8;
            let expected = [
                expand((tile + x % 32) % 32),
                expand((row * 3 + y % 32) % 32),
                expand((col * 4 + x % 32 + y % 32) % 32),
                if (x + y + tile) % 2 == 0 { 0 } else { 255 },
            ];
            let start = (y * 256 + x) * 4;
            assert_eq!(&texture.rgba8[start..start + 4], &expected, "pixel {x},{y}");
        }
    }
}

#[test]
fn every_pointer_including_wrap_columns_is_bounds_and_alignment_checked() {
    let (tiles, table) = fixture();
    for index in 0..80 {
        for bad in [
            0x0c000000,                          // unloaded slot
            0xff000000,                          // invalid slot
            0x0a800000,                          // offset outside resident bytes
            0x0afffff8,                          // cannot spill across a segment boundary
            0x0a000010 + 63 * TILE_BYTES as u32, // eight bytes short of a tile
            0x0a000009,                          // odd texel alignment
            0x0a00000a,                          // texel-aligned but not DMA-aligned
            0,
        ] {
            let mut damaged = table.clone();
            pointer(&mut damaged, index / 10, index % 10, bad);
            assert!(
                decode_panorama(&map(&tiles, &damaged), TABLE).is_err(),
                "index {index}, pointer {bad:#x}"
            );
        }
    }
}

#[test]
fn rejects_truncated_tables_unloaded_tables_and_misaligned_tables() {
    let (tiles, table) = fixture();
    for length in 1..table.len() {
        assert!(decode_panorama(&map(&tiles, &table[..length]), TABLE).is_err());
    }
    let segments = map(&tiles, &table);
    for address in [
        TABLE + 1,
        TABLE + 2,
        TABLE + 3,
        TABLE + 4,
        0x0c000000,
        0xff000000,
        0x0bfffffc,
    ] {
        assert!(decode_panorama(&segments, address).is_err());
    }
    assert!(decode_panorama(&SegmentMap::default(), TABLE).is_err());
}

#[test]
fn rejects_each_invalid_repeat_even_when_tile_bytes_are_identical() {
    let (mut tiles, table) = fixture();
    tiles.fill(0);
    for row in 0..8 {
        for col in 8..10 {
            let mut damaged = table.clone();
            // Valid resident tile, but not the matching column's pointer.
            let different = 0x0a000008 + ((63 - (row * 8 + 2)) * TILE_BYTES) as u32;
            pointer(&mut damaged, row, col, different);
            let error = decode_panorama(&map(&tiles, &damaged), TABLE).unwrap_err();
            assert!(error.0.contains("wrap column"));
        }
    }
}

#[test]
fn repeated_interior_tiles_are_legal() {
    let (tiles, mut table) = fixture();
    for row in 0..8 {
        for col in 0..10 {
            pointer(&mut table, row, col, 0x0a000008);
        }
    }
    assert!(decode_panorama(&map(&tiles, &table), TABLE).is_ok());
}

#[test]
#[ignore = "requires local US0 ROM via SM64_ROM"]
fn local_bob_panorama() {
    let Some(path) = std::env::var_os("SM64_ROM") else {
        eprintln!("skipped: set SM64_ROM");
        return;
    };
    let rom = sm64_rom::Rom::open(path).unwrap();
    let level = sm64_level::load_bob(&rom).unwrap();
    let texture = decode_panorama(&level.segment_data, 0x0a020000).unwrap();
    assert_eq!((texture.width, texture.height), (256, 256));
}
