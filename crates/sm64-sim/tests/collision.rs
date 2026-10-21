//! Authored geometry only. These verify probes, not original-game behavior.
use sm64_level::{CollisionMesh, Triangle};
use sm64_sim::collision::{Error, Kind, World};

fn mesh(vertices: &[[i16; 3]], indices: &[[u16; 3]]) -> CollisionMesh {
    CollisionMesh {
        vertices: vertices.to_vec(),
        triangles: indices
            .iter()
            .enumerate()
            .map(|(i, &indices)| Triangle {
                indices,
                surface: i as u16,
                force: Some(-(i as i16)),
            })
            .collect(),
        consumed: 0,
    }
}
fn world(vertices: &[[i16; 3]], indices: &[[u16; 3]]) -> World {
    World::from_mesh(&mesh(vertices, indices), 0.01).unwrap()
}

#[test]
fn winding_classifies_and_preserves_metadata() {
    let w = world(
        &[[0, 0, 0], [0, 0, 100], [100, 0, 0]],
        &[[0, 1, 2], [2, 1, 0], [0, 0, 1]],
    );
    assert_eq!(w.surfaces()[0].kind, Kind::Floor);
    assert_eq!(w.surfaces()[1].kind, Kind::Ceiling);
    assert_eq!(w.skipped_degenerate(), 1);
    let hit = w.vertical(20.0, 20.0, -1.0, 1.0).unwrap();
    assert_eq!(hit.floor.unwrap().normal, [0.0, 1.0, 0.0]);
    let ceiling = hit.ceiling.unwrap();
    assert_eq!(ceiling.surface, 1);
    assert_eq!(ceiling.force, Some(-1));
    assert_eq!(ceiling.triangle, 1);
}

#[test]
fn overlapping_floors_ceilings_and_ties_use_explicit_interval() {
    let w = world(
        &[
            [0, 0, 0],
            [0, 0, 100],
            [100, 0, 0],
            [0, 40, 0],
            [0, 40, 100],
            [100, 40, 0],
            [0, 80, 0],
            [0, 80, 100],
            [100, 80, 0],
        ],
        &[[0, 1, 2], [3, 4, 5], [3, 4, 5], [5, 4, 3], [8, 7, 6]],
    );
    let hit = w.vertical(10.0, 10.0, -100.0, 100.0).unwrap();
    assert_eq!(hit.floor.unwrap().triangle, 1);
    assert_eq!(hit.ceiling.unwrap().height, 40.0);
    let hit = w.vertical(10.0, 10.0, 41.0, 100.0).unwrap();
    assert!(hit.floor.is_none());
    assert_eq!(hit.ceiling.unwrap().height, 80.0);
    assert_eq!(
        w.vertical(10.0, 10.0, 0.0, 0.0)
            .unwrap()
            .floor
            .unwrap()
            .height,
        0.0
    );
}

#[test]
fn slopes_shared_edges_vertices_and_ledges() {
    let w = world(
        &[[0, 0, 0], [0, 0, 100], [100, 100, 0], [100, 100, 100]],
        &[[0, 1, 2], [2, 1, 3]],
    );
    assert!(
        (w.vertical(25.0, 25.0, -100.0, 200.0)
            .unwrap()
            .floor
            .unwrap()
            .height
            - 25.0)
            .abs()
            < 1e-12
    );
    assert_eq!(
        w.vertical(50.0, 50.0, -100.0, 200.0)
            .unwrap()
            .floor
            .unwrap()
            .triangle,
        0
    );
    for (x, z) in [(0.0, 0.0), (100.0, 100.0), (100.0, 0.0)] {
        assert!(w.vertical(x, z, -100.0, 200.0).unwrap().floor.is_some());
    }
    for (x, z) in [(-0.01, 0.0), (100.01, 50.0), (50.0, 100.01)] {
        assert!(w.vertical(x, z, -100.0, 200.0).unwrap().floor.is_none());
    }
}

#[test]
fn walls_are_two_sided_and_do_not_mutate_position() {
    let w = world(&[[0, 0, 0], [0, 100, 0], [0, 0, 100]], &[[0, 1, 2]]);
    assert_eq!(w.surfaces()[0].kind, Kind::Wall);
    assert!(
        w.vertical(0.0, 10.0, -100.0, 200.0)
            .unwrap()
            .floor
            .is_none()
    );
    for x in [-5.0, 5.0] {
        let p = [x, 20.0, 20.0];
        let hits = w.walls(p, 5.0).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].point, [0.0, 20.0, 20.0]);
        assert_eq!(hits[0].distance, 5.0);
        assert_eq!(p, [x, 20.0, 20.0]);
        assert!(w.walls(p, 4.99).unwrap().is_empty());
    }
    assert!(w.walls([0.0, 101.0, 0.0], 100.0).unwrap().is_empty());
    // A slice at the triangle apex is a point rather than a segment.
    assert_eq!(w.walls([0.0, 100.0, 0.0], 0.0).unwrap().len(), 1);
    assert!(w.walls([0.0, 90.0, 90.0], 1.0).unwrap().is_empty());
}

#[test]
fn wall_endpoints_corners_and_slanted_planes() {
    let w = world(
        &[[0, 0, 0], [0, 100, 0], [0, 0, 100], [100, 0, 0]],
        &[[0, 1, 2], [0, 3, 1]],
    );
    let hits = w.walls([3.0, 20.0, 4.0], 5.0).unwrap();
    assert_eq!(hits.len(), 2);
    assert_eq!(hits[0].triangle, 0);
    assert_eq!(hits[1].triangle, 1);
    assert_eq!(w.walls([0.0, 0.0, 105.0], 5.0).unwrap().len(), 1);
    let w = world(&[[0, 0, 0], [1, 100, 0], [0, 0, 100]], &[[0, 1, 2]]);
    let hit = &w.walls([5.5, 50.0, 20.0], 5.0).unwrap()[0];
    assert_eq!(hit.point, [0.5, 50.0, 20.0]);
}

#[test]
fn empty_degenerate_and_extreme_i16_geometry() {
    let w = world(&[], &[]);
    assert!(w.vertical(0.0, 0.0, -100.0, 100.0).unwrap().floor.is_none());
    assert!(w.walls([0.0; 3], 1.0).unwrap().is_empty());
    let w = world(
        &[
            [i16::MIN, 0, i16::MIN],
            [i16::MIN, 0, i16::MAX],
            [i16::MAX, 0, i16::MIN],
        ],
        &[[0, 1, 2]],
    );
    assert_eq!(
        w.vertical(-10.0, -10.0, 0.0, 0.0)
            .unwrap()
            .floor
            .unwrap()
            .height,
        0.0
    );
    let w = world(&[[0, 0, 0], [1, 1, 1], [2, 2, 2]], &[[0, 1, 2]]);
    assert!(w.surfaces().is_empty());
    assert_eq!(w.skipped_degenerate(), 1);
}

#[test]
fn sloped_source_vertices_stay_inside_inclusive_height_bounds() {
    let vertices = [[84, 15, 46], [-26, -44, -10], [47, 88, -36]];
    for indices in [[0, 1, 2], [2, 1, 0]] {
        let w = world(&vertices, &[indices]);
        for [x, y, z] in vertices {
            let y = f64::from(y);
            let hit = w.vertical(f64::from(x), f64::from(z), y, y).unwrap();
            let hit = hit
                .floor
                .or(hit.ceiling)
                .expect("source vertex must be in its triangle");
            assert_eq!(hit.height, y);
        }
    }
}

#[test]
fn invalid_indices_thresholds_and_queries_fail_explicitly() {
    let mesh = mesh(&[[0, 0, 0]], &[[0, 0, 1]]);
    assert!(matches!(
        World::from_mesh(&mesh, 0.01),
        Err(Error::Index {
            triangle: 0,
            vertex: 1
        })
    ));
    for threshold in [0.0, -0.01, 1.0, f64::NAN, f64::INFINITY] {
        assert!(matches!(
            World::from_mesh(&mesh, threshold),
            Err(Error::Threshold)
        ));
    }
    let w = world(&[], &[]);
    for bad in [f64::NAN, f64::INFINITY, -f64::INFINITY, 1_000_001.0] {
        for values in [
            [bad, 0.0, 0.0, 1.0],
            [0.0, bad, 0.0, 1.0],
            [0.0, 0.0, bad, 1.0],
            [0.0, 0.0, 0.0, bad],
        ] {
            assert!(
                w.vertical(values[0], values[1], values[2], values[3])
                    .is_err()
            );
        }
        assert!(w.walls([bad, 0.0, 0.0], 1.0).is_err());
        assert!(w.walls([0.0, bad, 0.0], 1.0).is_err());
        assert!(w.walls([0.0, 0.0, bad], 1.0).is_err());
        assert!(w.walls([0.0; 3], bad).is_err());
    }
    assert!(matches!(
        w.vertical(0.0, 0.0, 1.0, 0.0),
        Err(Error::Interval)
    ));
    assert!(w.walls([0.0; 3], -1.0).is_err());
}

#[test]
#[ignore = "needs the US revision 0 ROM via SM64_ROM"]
fn local_bob_queries_are_finite_and_preserve_source_identity() {
    let Some(path) = std::env::var_os("SM64_ROM") else {
        eprintln!("skipped: set SM64_ROM to a user-supplied US revision 0 ROM");
        return;
    };
    let rom = sm64_rom::Rom::open(path).unwrap();
    let level = sm64_level::load_bob(&rom).unwrap();
    let mesh = level.areas[0].collision.as_ref().unwrap();
    let w = World::from_mesh(mesh, 0.01).unwrap();
    assert_eq!(
        w.surfaces().len() + w.skipped_degenerate(),
        mesh.triangles.len()
    );
    let mut floors = 0;
    for x in (-8000..=8000).step_by(500) {
        for z in (-8000..=8000).step_by(500) {
            let hit = w
                .vertical(f64::from(x), f64::from(z), -32768.0, 32767.0)
                .unwrap();
            if let Some(floor) = hit.floor {
                floors += 1;
                assert!(floor.height.is_finite());
                assert_eq!(floor.surface, mesh.triangles[floor.triangle].surface);
            }
        }
    }
    assert!(floors > 0);
}
