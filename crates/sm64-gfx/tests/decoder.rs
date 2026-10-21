use sm64_gfx::{AlphaMode, decode_display_list};
use sm64_level::segments::SegmentMap;
fn fixture(commands: &[(u32, u32)]) -> Vec<u8> {
    let mut data = vec![0; 1024];
    for (chunk, (a, b)) in data.as_chunks_mut::<8>().0.iter_mut().zip(commands) {
        chunk[..4].copy_from_slice(&a.to_be_bytes());
        chunk[4..].copy_from_slice(&b.to_be_bytes());
    }
    for i in 0..3 {
        let v = &mut data[512 + i * 16..528 + i * 16];
        v[..2].copy_from_slice(&(i as i16 * 10).to_be_bytes());
        v[8..10].copy_from_slice(&(-16i16).to_be_bytes());
        v[12..16].copy_from_slice(&[40, 80, 120, 255]);
    }
    data
}
fn map(data: &[u8]) -> SegmentMap {
    let mut s = SegmentMap::default();
    s.load(data, 7, 0, data.len() as u32, false).unwrap();
    s
}
#[test]
fn vertices_nested_state_and_branch() {
    let mut d = fixture(&[
        (0xb6000000, 0x20000),
        (0x04200030, 0x07000200),
        (0xbf000000, 0x00000a14),
        (0x06000000, 0x07000040),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    for (i, (a, b)) in [
        (0xb6000000u32, 0x2000u32),
        (0xbf000000, 0x00000a14),
        (0x06010000, 0x07000060),
        (0xbf000000, 0xffffffff),
        (0xb8000000, 0),
    ]
    .iter()
    .enumerate()
    {
        d[64 + i * 8..68 + i * 8].copy_from_slice(&a.to_be_bytes());
        d[68 + i * 8..72 + i * 8].copy_from_slice(&b.to_be_bytes());
    }
    let out = decode_display_list(&map(&d), 0x07000000).unwrap();
    assert_eq!(out.batches.len(), 2);
    assert_eq!(out.batches[0].vertices.len(), 3);
    assert!(out.batches[0].cull_back);
    assert_eq!(out.batches[1].vertices.len(), 6);
    assert!(!out.batches[1].cull_back);
    assert_eq!(out.batches[0].vertices[1].position, [10., 0., 0.]);
    assert_eq!(
        out.batches[0].vertices[0].color,
        [40. / 255., 80. / 255., 120. / 255., 1.]
    );
}
#[test]
fn texture_is_loaded_not_last_image_and_alpha_batches() {
    let mut d = fixture(&[
        (0xb6000000, 0x20000),
        (0x04200030, 0x07000200),
        (0xfd100000, 0x07000300),
        (0xf5100000, 0x07000000),
        (0xf3000000, 0x07000000),
        (0xf5100000, 0),
        (0xf2000000, 0),
        (0xbb000001, 0xffffffff),
        (0xfd100000, 0x07000310),
        (0xbf000000, 0x00000a14),
        (0xb9000002, 1),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    d[768..770].copy_from_slice(&[0xf8, 1]);
    let out = decode_display_list(&map(&d), 0x07000000).unwrap();
    assert_eq!(out.textures.len(), 1);
    assert_eq!(out.textures[0].rgba8, [255, 0, 0, 255]);
    assert_eq!(out.batches.len(), 2);
    assert_eq!(out.batches[1].alpha, AlphaMode::Mask);
    assert!(out.batches[0].vertices[0].uv[0].abs() < 0.0001);
}
#[test]
fn malformed_references_offsets_opcodes_cycles() {
    for commands in [
        vec![(0xbf000000, 0x00000a14)],
        vec![(0x04200030, 0x070003f0)],
        vec![(0x04200030, 0x08000000)],
        vec![(0x04f10000, 0x07000200)],
        vec![(0x01000000, 0)],
        vec![(0x06010000, 0x07000000)],
        vec![(0x06000000, 0x07000000)],
        vec![(0x04200030, 0x07000200), (0xbf000000, 0x00000b14)],
    ] {
        assert!(decode_display_list(&map(&fixture(&commands)), 0x07000000).is_err());
    }
    assert!(decode_display_list(&map(&fixture(&[])), 0x07000001).is_err());
}
#[test]
#[ignore = "requires local US0 ROM via SM64_ROM"]
fn local_bob_roots() {
    let Some(path) = std::env::var_os("SM64_ROM") else {
        eprintln!("skipped: set SM64_ROM");
        return;
    };
    let rom = sm64_rom::Rom::open(path).unwrap();
    let level = sm64_level::load_bob(&rom).unwrap();
    let mut roots = std::collections::BTreeSet::new();
    for area in &level.areas {
        let scene =
            sm64_level::geo::inspect_geo(&level.segment_data, area.geometry_address).unwrap();
        roots.extend(scene.draws.iter().map(|d| d.display_list));
    }
    for expected in [
        0x07004390, 0x07009d80, 0x0700a470, 0x0700a920, 0x0700dd18, 0x0700e338,
    ] {
        assert!(
            roots.contains(&expected),
            "missing BOB terrain root {expected:#010x}"
        );
    }
    // Includes statically reachable object models in addition to the area's terrain.
    for model in level.models.values() {
        if model.layer.is_some() {
            roots.insert(model.address);
        } else if let Ok(scene) = sm64_level::geo::inspect_geo(&level.segment_data, model.address) {
            roots.extend(scene.draws.iter().map(|d| d.display_list));
        }
    }
    let mut failures = vec![];
    for root in roots {
        match decode_display_list(&level.segment_data, root) {
            Ok(out) => eprintln!(
                "{root:#010x}: {} commands, {} triangles, {} textures, {} diagnostics",
                out.commands_inspected,
                out.batches
                    .iter()
                    .map(|b| b.vertices.len() / 3)
                    .sum::<usize>(),
                out.textures.len(),
                out.diagnostics.len()
            ),
            Err(e) => {
                eprintln!("{root:#010x}: ERROR {e}");
                failures.push((root, e));
            }
        }
    }
    assert!(failures.is_empty(), "{} failed roots", failures.len());
}

#[test]
fn lighting_is_baked_at_vertex_load() {
    let mut d = fixture(&[
        (0xbc000002, 0x80000040),
        (0x03860010, 0x07000300),
        (0x03880010, 0x07000310),
        (0xbb000000, 0x80008000),
        (0x04200030, 0x07000200),
        (0xb6000000, 0x20000),
        (0xbb000000, 0xffffffff),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    for i in 0..3 {
        d[524 + i * 16..528 + i * 16].copy_from_slice(&[0, 0, 127, 255]);
    }
    d[768..771].copy_from_slice(&[128; 3]);
    d[776..779].copy_from_slice(&[0, 0, 127]);
    d[784..787].copy_from_slice(&[32; 3]);
    let out = decode_display_list(&map(&d), 0x07000000).unwrap();
    let v = &out.batches[0].vertices[0];
    assert_eq!(v.normal, [0., 0., 1.]);
    assert!((v.color[0] - 160. / 255.).abs() < 0.0001);
    assert!(
        out.diagnostics
            .iter()
            .any(|d| d.contains("Lighting is baked"))
    );
}

#[test]
fn texture_scale_is_applied_at_vertex_load() {
    let mut d = fixture(&[
        (0xb6000000, 0x20000),
        (0xbb000000, 0x80008000),
        (0x04200030, 0x07000200),
        (0xfd100000, 0x07000300),
        (0xf5100000, 0x07000000),
        (0xf3000000, 0x07000000),
        (0xf5100000, 0),
        (0xf2000000, 0),
        (0xbb000001, 0xffffffff),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    d[768..770].copy_from_slice(&[0xff, 0xff]);
    let out = decode_display_list(&map(&d), 0x07000000).unwrap();
    assert_eq!(out.batches[0].vertices[0].uv[0], 0.25);
}
