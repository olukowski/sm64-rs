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
fn selected_environment_alpha_replaces_zero_vertex_alpha() {
    let mut d = fixture(&[
        (0xb6000000, 0x20000),
        (0xfb000000, 0xffffffff),
        (0xfc000000, 5),
        (0x04200030, 0x07000200),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    for i in 0..3 {
        d[512 + i * 16 + 15] = 0;
    }
    let out = decode_display_list(&map(&d), 0x07000000).unwrap();
    assert_eq!(out.batches[0].alpha, AlphaMode::Opaque);
    assert!(out.batches[0].vertices.iter().all(|v| v.color[3] == 1.0));
}

#[test]
fn posed_light_transform_changes_shading_not_returned_local_geometry() {
    let mut d = fixture(&[
        (0x03860010, 0x07000300),
        (0x03880010, 0x07000310),
        (0x04200030, 0x07000200),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    d[768..771].copy_from_slice(&[255, 255, 255]);
    d[776] = 127;
    for i in 0..3 {
        d[512 + i * 16 + 12..512 + i * 16 + 16].copy_from_slice(&[127, 0, 0, 255]);
    }
    let m = map(&d);
    let lit = sm64_gfx::DisplayListStream::new(&m)
        .decode_next(0x07000000, 100)
        .unwrap();
    let dark = sm64_gfx::DisplayListStream::new(&m)
        .decode_next_with_normal_transform(
            0x07000000,
            100,
            [[0., 1., 0.], [-1., 0., 0.], [0., 0., 1.]],
        )
        .unwrap();
    assert_eq!(lit.batches[0].vertices[0].color, [1., 1., 1., 1.]);
    assert_eq!(dark.batches[0].vertices[0].color, [0., 0., 0., 1.]);
    assert_eq!(
        lit.batches[0].vertices[0].position,
        dark.batches[0].vertices[0].position
    );
    assert_eq!(dark.batches[0].vertices[0].normal, [1., 0., 0.]);
    let mut invalid = sm64_gfx::DisplayListStream::new(&m);
    assert!(
        invalid
            .decode_next_with_normal_transform(0x07000000, 100, [[f32::NAN; 3]; 3])
            .is_err()
    );
    assert!(invalid.decode_next(0x07000000, 100).is_err());
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

fn write_commands(data: &mut [u8], offset: usize, commands: &[(u32, u32)]) {
    for (chunk, (a, b)) in data[offset..]
        .as_chunks_mut::<8>()
        .0
        .iter_mut()
        .zip(commands)
    {
        chunk[..4].copy_from_slice(&a.to_be_bytes());
        chunk[4..].copy_from_slice(&b.to_be_bytes());
    }
}

#[test]
fn stream_inherits_lights_tmem_tiles_and_geometry_but_not_output_indices() {
    let mut d = fixture(&[
        (0xbc000002, 0x80000040),
        (0x03860010, 0x07000320),
        (0x03880010, 0x07000330),
        (0xb6000000, 0x2000), // no back culling, lighting remains enabled
        (0xbb000001, 0x80008000),
        (0x04200030, 0x07000200),
        (0xfd100000, 0x07000300),
        (0xf5100000, 0x07000000),
        (0xf3000000, 0x07000000),
        (0xf5100000, 0),
        (0xf2000000, 0),
        (0xbf000000, 0x00000a14),
        (0xfd100000, 0x07000310),
        (0xf3000000, 0x07000000),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    write_commands(
        &mut d,
        256,
        &[
            (0x04200030, 0x07000200),
            (0xbf000000, 0x00000a14),
            (0xb8000000, 0),
        ],
    );
    for i in 0..3 {
        d[524 + i * 16..528 + i * 16].copy_from_slice(&[0, 0, 127, 255]);
    }
    d[768..770].copy_from_slice(&[0xf8, 1]); // red opaque
    d[784..786].copy_from_slice(&[0x07, 0xc0]); // green transparent
    d[800..803].copy_from_slice(&[128, 0, 0]);
    d[808..811].copy_from_slice(&[0, 0, 127]);
    d[816..819].copy_from_slice(&[32, 16, 8]);
    let segments = map(&d);
    let mut stream = sm64_gfx::DisplayListStream::new(&segments);
    let first = stream.decode_next(0x07000000, 100).unwrap();
    assert_eq!(first.textures.len(), 2);
    assert_eq!(first.batches[1].texture, Some(1));
    for _ in 0..2 {
        let next = stream.decode_next(0x07000100, 3).unwrap();
        assert_eq!(next.commands_inspected, 3);
        assert_eq!(next.batches.len(), 1);
        assert_eq!(next.textures.len(), 1);
        assert_eq!(next.textures[0].rgba8, [0, 255, 0, 0]);
        let b = &next.batches[0];
        assert_eq!(b.texture, Some(0));
        assert_eq!(b.alpha, AlphaMode::Mask);
        assert!(!b.cull_back);
        assert_eq!(b.vertices[0].uv[0], 0.25);
        assert_eq!(
            b.vertices[0].color,
            [160. / 255., 16. / 255., 8. / 255., 1.]
        );
        assert!(
            next.diagnostics
                .iter()
                .any(|s| s.contains("Lighting is baked"))
        );
        assert!(
            !next
                .diagnostics
                .iter()
                .any(|s| s.contains("Missing inherited"))
        );
    }
    let fresh = decode_display_list(&segments, 0x07000100).unwrap();
    assert_ne!(
        fresh.batches[0].vertices[0].color,
        first.batches[0].vertices[0].color
    );
    assert_eq!(fresh.batches[0].texture, None);
    assert!(
        fresh
            .diagnostics
            .iter()
            .any(|s| s.contains("Missing inherited"))
    );
}

#[test]
fn stream_clears_vertices_and_poison_prevents_partial_output_or_retry() {
    let mut d = fixture(&[
        (0x04200030, 0x07000200),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    write_commands(&mut d, 128, &[(0xbf000000, 0x00000a14), (0xb8000000, 0)]);
    write_commands(
        &mut d,
        256,
        &[
            (0x04200030, 0x07000200),
            (0xbf000000, 0x00000a14),
            (0x01000000, 0), // error after emitting a triangle internally
        ],
    );
    let segments = map(&d);
    let mut stream = sm64_gfx::DisplayListStream::new(&segments);
    stream.decode_next(0x07000000, 3).unwrap();
    assert!(
        stream
            .decode_next(0x07000080, 2)
            .unwrap_err()
            .0
            .contains("unloaded vertex")
    );
    assert!(
        stream
            .decode_next(0x07000000, 3)
            .unwrap_err()
            .0
            .contains("poisoned")
    );
    let mut stream = sm64_gfx::DisplayListStream::new(&segments);
    assert!(
        stream
            .decode_next(0x07000100, 3)
            .unwrap_err()
            .0
            .contains("unsupported opcode")
    );
    assert!(
        stream
            .decode_next(0x07000000, 3)
            .unwrap_err()
            .0
            .contains("poisoned")
    );
    for budget in [0, 2] {
        let mut stream = sm64_gfx::DisplayListStream::new(&segments);
        assert!(
            stream
                .decode_next(0x07000000, budget)
                .unwrap_err()
                .0
                .contains("command budget")
        );
    }
}

#[test]
fn stream_inherits_othermode_and_primitive_environment_alpha() {
    for (color_op, combiner) in [(0xfa000000, 3), (0xfb000000, 5)] {
        let mut d = fixture(&[
            (0xb6000000, 0x20000),
            (color_op, 0xffffff80),
            (0xfc000000, combiner),
            (0xb9000002, 1),
            (0xb8000000, 0),
        ]);
        write_commands(
            &mut d,
            128,
            &[
                (0x04200030, 0x07000200),
                (0xbf000000, 0x00000a14),
                (0xfc000000, 4),
                (0xbf000000, 0x00000a14),
                (0xb8000000, 0),
            ],
        );
        let segments = map(&d);
        let mut stream = sm64_gfx::DisplayListStream::new(&segments);
        assert!(
            stream
                .decode_next(0x07000000, 5)
                .unwrap()
                .batches
                .is_empty()
        );
        let next = stream.decode_next(0x07000080, 5).unwrap();
        assert_eq!(next.batches[0].alpha, AlphaMode::Blend);
        assert_eq!(next.batches[0].vertices[0].color[3], 128. / 255.);
        assert_eq!(next.batches[1].alpha, AlphaMode::Mask);
    }
}

// Author-built commands from GBI gsDPSetCombineLERP/GCCc*w* field layout.
// RGB inputs are a,b,c,d; alpha is independently set to SHADE in both cycles.
fn combine_rgb(c0: [u32; 4], c1: [u32; 4]) -> (u32, u32) {
    let [a0, b0, c0, d0] = c0;
    let [a1, b1, c1, d1] = c1;
    (
        0xfc000000 | (a0 << 20) | (c0 << 15) | (7 << 12) | (7 << 9) | (a1 << 5) | c1,
        (b0 << 28)
            | (b1 << 24)
            | (7 << 21)
            | (7 << 18)
            | (d0 << 15)
            | (7 << 12)
            | (4 << 9)
            | (d1 << 6)
            | (7 << 3)
            | 4,
    )
}

#[test]
fn rgb_combiner_exact_fields_split_and_unknown_fallback() {
    use sm64_gfx::ColorCombiner::{Decal, Modulate};
    let decal = [1, 4, 8, 4];
    let modulate = [1, 15, 4, 7];
    // Changing either cycle or any RGB input must not falsely claim Decal.
    let mut cases = vec![(decal, decal, Decal), (modulate, modulate, Modulate)];
    for cycle in 0..2 {
        for input in 0..4 {
            let mut changed = decal;
            changed[input] ^= 1;
            cases.push(if cycle == 0 {
                (changed, decal, Modulate)
            } else {
                (decal, changed, Modulate)
            });
        }
    }
    for (c0, c1, expected) in cases {
        let data = fixture(&[
            (0xb6000000, 0x20000),
            (0x04200030, 0x07000200),
            (0xbf000000, 0x00000a14), // default is Modulate
            combine_rgb(c0, c1),
            (0xbf000000, 0x00000a14),
            combine_rgb(modulate, modulate),
            (0xbf000000, 0x00000a14),
            (0xb8000000, 0),
        ]);
        let out = decode_display_list(&map(&data), 0x07000000).unwrap();
        assert_eq!(out.batches[0].color_combiner, Modulate);
        if expected == Decal {
            assert_eq!(out.batches.len(), 3);
            assert_eq!(out.batches[1].color_combiner, Decal);
            assert_eq!(out.batches[2].color_combiner, Modulate);
        } else {
            assert_eq!(out.batches.len(), 1);
            assert_eq!(out.batches[0].vertices.len(), 9);
            if c0 != modulate || c1 != modulate {
                assert!(
                    out.diagnostics
                        .iter()
                        .any(|s| s.contains("Unknown RGB combiner"))
                );
            }
        }
    }
}

#[test]
fn rgb_combiner_inherits_across_stream_roots() {
    use sm64_gfx::ColorCombiner::{Decal, Modulate};
    let mut data = fixture(&[combine_rgb([1, 4, 8, 4], [1, 4, 8, 4]), (0xb8000000, 0)]);
    let root = fixture(&[
        (0xb6000000, 0x20000),
        (0x04200030, 0x07000200),
        (0xbf000000, 0x00000a14),
        combine_rgb([1, 15, 4, 7], [1, 15, 4, 7]),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    data[64..112].copy_from_slice(&root[..48]);
    let segments = map(&data);
    let mut stream = sm64_gfx::DisplayListStream::new(&segments);
    assert!(
        stream
            .decode_next(0x07000000, 100)
            .unwrap()
            .batches
            .is_empty()
    );
    let out = stream.decode_next(0x07000040, 100).unwrap();
    assert_eq!(out.batches.len(), 2);
    assert_eq!(out.batches[0].color_combiner, Decal);
    assert_eq!(out.batches[1].color_combiner, Modulate);
}

#[test]
fn decal_texture_alpha_interpolates_rgb_without_masking_shade_base() {
    use sm64_gfx::ColorCombiner::{Decal, Modulate};
    let mut data = fixture(&[
        (0xb6000000, 0x20000),
        (0x04200030, 0x07000200),
        (0xfd100000, 0x07000300),
        (0xf5100000, 0x07000000),
        (0xf3000000, 0x07000000),
        (0xf5100000, 0),
        (0xf2000000, 0),
        (0xbb000001, 0xffffffff),
        combine_rgb([1, 15, 4, 7], [1, 15, 4, 7]),
        (0xbf000000, 0x00000a14),
        combine_rgb([1, 4, 8, 4], [1, 4, 8, 4]),
        (0xbf000000, 0x00000a14),
        (0xb8000000, 0),
    ]);
    // Transparent black texel: Decal must still display the entire shade base.
    data[768..770].copy_from_slice(&[0, 0]);
    let out = decode_display_list(&map(&data), 0x07000000).unwrap();
    assert_eq!(out.batches.len(), 2);
    assert_eq!(out.batches[0].color_combiner, Modulate);
    assert_eq!(out.batches[0].alpha, AlphaMode::Mask);
    assert_eq!(out.batches[1].color_combiner, Decal);
    assert_eq!(out.batches[1].alpha, AlphaMode::Opaque);
    assert_eq!(out.batches[0].texture, out.batches[1].texture);
    assert_eq!(
        out.batches[0].vertices[0].color,
        out.batches[1].vertices[0].color
    );
}
