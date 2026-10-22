//! Authored arithmetic examples, not captured geometry or original classification rules.
use sm64_sim::floor::{Error, FloorPlane};

type Triangle = [[i16; 3]; 3];

fn bits((normal, offset): ([f32; 3], f32)) -> ([u32; 3], u32) {
    (normal.map(f32::to_bits), offset.to_bits())
}

// Independent integer determinant: admitted products/differences are exact f32
// integers, including the positive zero produced by a zero determinant.
fn cross(vertices: Triangle) -> [f32; 3] {
    let a: [i64; 3] =
        std::array::from_fn(|i| i64::from(vertices[1][i]) - i64::from(vertices[0][i]));
    let b: [i64; 3] =
        std::array::from_fn(|i| i64::from(vertices[2][i]) - i64::from(vertices[0][i]));
    std::array::from_fn(|i| {
        (a[(i + 1) % 3] * b[(i + 2) % 3] - a[(i + 2) % 3] * b[(i + 1) % 3]) as f32
    })
}

fn offset(normal: [f32; 3], vertex: [i16; 3]) -> f32 {
    let [x, y, z] = std::array::from_fn(|i| normal[i] * f32::from(vertex[i]));
    -((x + y) + z)
}

fn reference(vertices: Triangle) -> ([f32; 3], f32) {
    let [x, y, z] = cross(vertices);
    let length = ((x * x + y * y) + z * z).sqrt();
    let inverse = 1.0 / length;
    let normal = [x * inverse, y * inverse, z * inverse];
    (normal, offset(normal, vertices[0]))
}

fn reject(vertices: Triangle) {
    assert!(
        matches!(FloorPlane::from_vertices(vertices), Err(Error::Vertices)),
        "expected authored-domain rejection: {vertices:?}"
    );
}

#[test]
fn flat_and_analytical_three_four_five_slope() {
    let flat = FloorPlane::from_vertices([[2, 24, 3], [2, 24, 8], [7, 24, 3]]).unwrap();
    assert_eq!(flat.coefficients(), ([0.0, 1.0, 0.0], -24.0));
    assert_eq!(flat.height_at(7.0, -9.0).unwrap(), 24.0);

    // y = 10 - 3x/4; determinant (12,16,0), length 20.
    let slope = FloorPlane::from_vertices([[0, 10, 0], [0, 10, 4], [4, 7, 0]]).unwrap();
    assert_eq!(bits(slope.coefficients()), bits(([0.6, 0.8, 0.0], -8.0)));
    assert_eq!(slope.height_at(4.0, 2.0).unwrap(), 7.0);
}

#[test]
fn horizontal_normals_are_not_simplified_to_unit_y() {
    // Invented determinant 41: reciprocal multiplication rounds below one.
    let plane = FloorPlane::from_vertices([[0, 17, 0], [0, 17, 41], [1, 17, 0]]).unwrap();
    let (normal, _) = plane.coefficients();
    assert_eq!(normal.map(f32::to_bits), [0, 0x3f7f_ffff, 0]);
}

#[test]
fn zero_determinants_are_positive_but_the_zero_offset_is_negative() {
    let flat = FloorPlane::from_vertices([[0, 0, 0], [0, 0, 1], [1, 0, 0]]).unwrap();
    assert_eq!(bits(flat.coefficients()), bits(([0.0, 1.0, 0.0], -0.0)));
    // Floating edge products would produce nx = (+0 * -1) - (1 * +0) = -0.
    // Exact integer determinants convert to +0 instead; this is not a license
    // to canonicalize zeros in caller-supplied coefficients or height results.
    let x_zero = FloorPlane::from_vertices([[0, 0, 0], [0, 0, 1], [1, 0, -1]]).unwrap();
    assert_eq!(bits(x_zero.coefficients()), bits(([0.0, 1.0, 0.0], -0.0)));
    let [zero, one, minus_one] = [0.0_f32, 1.0, -1.0];
    let floating_x = (zero * minus_one) - (one * zero);
    assert_eq!(floating_x.to_bits(), (-0.0_f32).to_bits());
    let z_zero = FloorPlane::from_vertices([[0, 0, 0], [-1, 0, 1], [1, 0, 0]]).unwrap();
    assert_eq!(bits(z_zero.coefficients()), bits(([0.0, 1.0, 0.0], -0.0)));
    let floating_z = (minus_one * zero) - (zero * one);
    assert_eq!(floating_z.to_bits(), (-0.0_f32).to_bits());
}

#[test]
fn downward_degenerate_and_vertical_are_not_supported() {
    for vertices in [
        [[0, 0, 0], [1, 0, 0], [0, 0, 1]], // reversed winding
        [[1, 2, 3]; 3],                    // coincident
        [[0, 0, 0], [1, 2, 3], [2, 4, 6]], // collinear
        [[0, 0, 0], [0, 1, 0], [0, 0, 1]], // vertical, nonzero cross
    ] {
        reject(vertices);
    }
    // No original floor-classification threshold is inferred: tiny positive ny
    // is still supported in this deliberately conservative arithmetic domain.
    assert!(FloorPlane::from_vertices([[0, 0, 0], [0, 8192, 1], [1, 8192, 0]]).is_ok());
}

#[test]
fn coordinate_limits_are_inclusive_and_checked_in_every_slot() {
    for bound in [-8192_i16, 8192] {
        let inward = if bound < 0 { 1 } else { -1 };
        let triangle = [
            [bound, bound, bound],
            [bound, bound, bound + inward],
            [bound + inward, bound, bound],
        ];
        for rotation in 0..3 {
            let vertices = std::array::from_fn(|i| triangle[(i + rotation) % 3]);
            assert!(FloorPlane::from_vertices(vertices).is_ok(), "{vertices:?}");
        }
    }
    for vertex in 0..3 {
        for axis in 0..3 {
            for invalid in [-8193, 8193, i16::MIN, i16::MAX] {
                let mut vertices = [[0, 0, 0], [0, 0, 1], [1, 0, 0]];
                vertices[vertex][axis] = invalid;
                reject(vertices);
            }
        }
    }
}

// Inspect the authored exactness envelope independently, including cyclic bases.
fn products(vertices: Triangle, origin: usize) -> Vec<[i64; 3]> {
    let a: [i64; 3] = std::array::from_fn(|i| {
        i64::from(vertices[(origin + 1) % 3][i]) - i64::from(vertices[origin][i])
    });
    let b: [i64; 3] = std::array::from_fn(|i| {
        i64::from(vertices[(origin + 2) % 3][i]) - i64::from(vertices[origin][i])
    });
    (0..3)
        .map(|i| {
            let left = a[(i + 1) % 3] * b[(i + 2) % 3];
            let right = a[(i + 2) % 3] * b[(i + 1) % 3];
            [left, right, left - right]
        })
        .collect()
}

#[test]
fn exact_cross_envelope_checks_products_differences_and_every_cyclic_base() {
    const LIMIT: i64 = 1 << 24;
    let boundary = [[0, 0, 0], [0, 0, 4096], [4096, 0, 0]];
    assert!(FloorPlane::from_vertices(boundary).is_ok());
    assert_eq!(cross(boundary)[1], LIMIT as f32);

    // Small positive cross after cancellation does not excuse oversized factors.
    let cancellation = [[0, 0, 0], [4096, 0, 4096], [4097, 0, 4096]];
    assert_eq!(cross(cancellation)[1], 4096.0);
    assert!(
        products(cancellation, 0)
            .iter()
            .any(|p| p[0].abs() > LIMIT || p[1].abs() > LIMIT)
    );
    reject(cancellation);

    // Both individual products at origin zero fit, but their difference does not.
    let difference = [[0, 0, 0], [4096, 0, 4096], [4096, 0, -4096]];
    assert!(
        products(difference, 0)
            .iter()
            .all(|p| p[0].abs() <= LIMIT && p[1].abs() <= LIMIT)
    );
    assert!(products(difference, 0).iter().any(|p| p[2].abs() > LIMIT));
    reject(difference);

    // Origin zero passes everything; changing the edge base exposes large
    // cancelling products. All three cyclic choices must be checked.
    let other_base = [[0, 0, 0], [4096, 0, 4095], [-4096, 0, -4096]];
    assert!(
        products(other_base, 0)
            .iter()
            .flatten()
            .all(|p| p.abs() <= LIMIT)
    );
    assert!((1..3).all(|i| {
        products(other_base, i)
            .iter()
            .flatten()
            .any(|p| p.abs() > LIMIT)
    }));
    for rotation in 0..3 {
        reject(std::array::from_fn(|i| other_base[(i + rotation) % 3]));
    }
}

#[test]
fn authored_rounding_witnesses_pin_reciprocal_norm_and_offset_order() {
    // Invented by a deterministic small-integer search, never game geometry.
    let reciprocal_and_base = [[-49, 684, 743], [881, 717, 261], [809, -996, -766]];
    let norm_and_offset = [[-780, 553, -733], [-880, -450, 667], [241, 25, 536]];
    for vertices in [reciprocal_and_base, norm_and_offset] {
        assert_eq!(
            bits(FloorPlane::from_vertices(vertices).unwrap().coefficients()),
            bits(reference(vertices)),
            "{vertices:?}"
        );
    }

    let vertices = reciprocal_and_base;
    let (normal, origin) = reference(vertices);
    let [x, y, z] = cross(vertices);
    let length = ((x * x + y * y) + z * z).sqrt();
    let divided = [x / length, y / length, z / length];
    assert_ne!(normal.map(f32::to_bits), divided.map(f32::to_bits));
    assert_ne!(origin.to_bits(), offset(normal, vertices[1]).to_bits());
    let rotated = [vertices[1], vertices[2], vertices[0]];
    assert_eq!(
        bits(FloorPlane::from_vertices(rotated).unwrap().coefficients()),
        bits(reference(rotated))
    );
    assert_ne!(
        origin.to_bits(),
        FloorPlane::from_vertices(rotated)
            .unwrap()
            .coefficients()
            .1
            .to_bits()
    );

    let vertices = norm_and_offset;
    let (normal, origin) = reference(vertices);
    let [x, y, z] = cross(vertices);
    let other_inverse = 1.0 / (x * x + (y * y + z * z)).sqrt();
    let reassociated = [x * other_inverse, y * other_inverse, z * other_inverse];
    assert_ne!(normal.map(f32::to_bits), reassociated.map(f32::to_bits));
    let [tx, ty, tz] = std::array::from_fn(|i| normal[i] * f32::from(vertices[0][i]));
    assert_ne!(origin.to_bits(), (-(tx + (ty + tz))).to_bits());
}

#[test]
#[ignore = "requires private SM64_FLOOR_LISTS snapshot report and SM64_ROM; no fixtures bundled"]
fn private_snapshot_vertices_reconstruct_oracle_coefficients() {
    use serde_json::Value;
    use std::{fs::File, io::Read};
    if std::env::var_os("SM64_ROM").is_none() {
        eprintln!("SM64_ROM unset; skipping private floor-construction comparison");
        return;
    }
    let Some(path) = std::env::var_os("SM64_FLOOR_LISTS") else {
        eprintln!("SM64_FLOOR_LISTS unset; skipping private floor-construction comparison");
        return;
    };
    const MAX_BYTES: u64 = 256 * 1024 * 1024;
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(
        bytes.len() as u64 <= MAX_BYTES,
        "floor-list report exceeds 256 MiB"
    );
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(report["version"], 1);
    assert_eq!(report["status"], "exploratory_floor_list_memory");
    assert_eq!(report["rom_sha256"], sm64_rom::US_SHA256);
    let snapshots = report["snapshots"].as_array().unwrap();
    assert!(!snapshots.is_empty());
    let bit_value = |value: &Value| u32::try_from(value.as_u64().unwrap()).unwrap();
    let mut total = 0;
    let mut rejected = 0;
    let mut mismatched = 0;
    for (index, snapshot) in snapshots.iter().enumerate() {
        let surfaces = snapshot["surfaces"].as_object().unwrap();
        for (address, surface) in surfaces {
            total += 1;
            let rows = surface["vertices"].as_array().unwrap();
            assert_eq!(rows.len(), 3);
            let vertices: Triangle = std::array::from_fn(|vertex| {
                let coordinates = rows[vertex].as_array().unwrap();
                assert_eq!(coordinates.len(), 3);
                std::array::from_fn(|axis| {
                    i16::try_from(coordinates[axis].as_i64().unwrap()).unwrap()
                })
            });
            // The constructor receives ONLY vertices. Oracle coefficient bits
            // are metadata used after construction, never constructor inputs.
            let plane = match FloorPlane::from_vertices(vertices) {
                Ok(plane) => plane,
                Err(error) => {
                    rejected += 1;
                    eprintln!("snapshot {index}, surface {address}, {vertices:?}: {error}");
                    continue;
                }
            };
            let normal_bits = surface["normal_bits"].as_array().unwrap();
            assert_eq!(normal_bits.len(), 3);
            let expected = (
                std::array::from_fn(|axis| bit_value(&normal_bits[axis])),
                bit_value(&surface["origin_offset_bits"]),
            );
            let actual = bits(plane.coefficients());
            if actual != expected {
                mismatched += 1;
                eprintln!(
                    "snapshot {index}, surface {address}, {vertices:?}: actual {actual:?}, oracle {expected:?}"
                );
            }
        }
    }
    eprintln!(
        "{} stored snapshots; {total} surfaces; {} exact matches; {rejected} outside authored domain; {mismatched} mismatches; no floor classification/selection tested",
        snapshots.len(),
        total - rejected - mismatched
    );
    assert!(total > 0, "no snapshot surfaces compared");
    assert_eq!(
        rejected, 0,
        "private surfaces outside authored construction domain"
    );
    assert_eq!(mismatched, 0, "private construction bit mismatches");
}
