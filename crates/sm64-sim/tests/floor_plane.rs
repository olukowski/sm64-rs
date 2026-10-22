use sm64_sim::floor::{Error, FloorPlane, MAX_COORDINATE};

#[test]
fn authored_planes_truncate_fractional_coordinates_toward_zero() {
    // Invented coefficients and coordinates, not a recorded game surface.
    let plane = FloorPlane::new([0.25, 0.75, 0.5], -32.0).unwrap();
    assert_eq!(plane.height_at(-3.75, 1.9).unwrap(), 43.0);
    assert_eq!(plane.height_at(-3.75, 1.9), plane.height_at(-3.0, 1.0));
    assert_ne!(plane.height_at(-3.75, 1.9), plane.height_at(-4.0, 1.0));
    for value in [-MAX_COORDINATE, MAX_COORDINATE] {
        assert!(plane.height_at(value, value).is_ok());
    }
}

#[test]
fn flat_height_and_signed_zero_follow_the_supplied_coefficients() {
    let plane = FloorPlane::new([0.0, 1.0, 0.0], -24.0).unwrap();
    assert_eq!(plane.height_at(-123.5, 456.75).unwrap(), 24.0);
    let zero = FloorPlane::new([0.0, 1.0, 0.0], -0.0).unwrap();
    assert_eq!(
        zero.height_at(1.0, 1.0).unwrap().to_bits(),
        (-0.0_f32).to_bits()
    );
    // Scaling coefficients is not an invitation to normalize them silently.
    let scaled = FloorPlane::new([0.0, 0.5, 0.0], -12.0).unwrap();
    assert_eq!(scaled.height_at(1.0, 1.0).unwrap(), 24.0);
}

#[test]
fn invalid_planes_queries_and_overflow_fail_explicitly() {
    for normal in [
        [0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0],
        [f32::NAN, 1.0, 0.0],
        [0.0, 1.0, f32::INFINITY],
    ] {
        assert!(matches!(FloorPlane::new(normal, 0.0), Err(Error::Plane)));
    }
    assert!(matches!(
        FloorPlane::new([0.0, 1.0, 0.0], f32::NAN),
        Err(Error::Plane)
    ));
    let plane = FloorPlane::new([0.25, 1.0, 0.5], 0.0).unwrap();
    for invalid in [f32::NAN, f32::INFINITY, -f32::INFINITY, 8192.5, -8192.5] {
        assert_eq!(plane.height_at(invalid, 0.0), Err(Error::Query));
        assert_eq!(plane.height_at(0.0, invalid), Err(Error::Query));
    }
    let huge = FloorPlane::new([f32::MAX, 1.0, 0.0], 0.0).unwrap();
    assert_eq!(huge.height_at(2.0, 0.0), Err(Error::Overflow));
}

#[test]
fn separately_rounded_additions_are_not_reassociated() {
    // Authored cancellation witness, not an original surface. Both component
    // products survive together but disappear if the offset is added too early.
    let plane = FloorPlane::new([1.0, 1.0, 1.0], 16_777_216.0).unwrap();
    assert_eq!(plane.height_at(1.0, 1.0).unwrap(), -16_777_218.0);
    let reassociated = -(1.0_f32 + (1.0_f32 + 16_777_216.0_f32));
    assert_ne!(plane.height_at(1.0, 1.0).unwrap(), reassociated);
}

#[test]
#[ignore = "requires private SM64_FLOOR_QUERIES reference output and SM64_ROM; no fixtures bundled"]
fn observed_returned_planes_and_heights_match_exact_bits() {
    use serde_json::Value;
    use std::{fs::File, io::Read};
    if std::env::var_os("SM64_ROM").is_none() {
        eprintln!("SM64_ROM unset; skipping private plane-arithmetic comparison");
        return;
    }
    let Some(path) = std::env::var_os("SM64_FLOOR_QUERIES") else {
        eprintln!("SM64_FLOOR_QUERIES unset; skipping private plane-arithmetic comparison");
        return;
    };
    let mut bytes = Vec::new();
    File::open(path)
        .unwrap()
        .take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .unwrap();
    assert!(bytes.len() <= 64 * 1024 * 1024);
    let report: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(report["version"], 1);
    assert_eq!(report["status"], "exploratory_floor_calls");
    assert_eq!(report["entry"], 0x80381900_u32);
    assert_eq!(report["rom_sha256"], sm64_rom::US_SHA256);
    let calls = report["calls"].as_array().unwrap();
    assert!(!calls.is_empty() && calls.len() <= 100_000);
    let bits = |value: &Value| u32::try_from(value.as_u64().unwrap()).unwrap();
    for (index, call) in calls.iter().enumerate() {
        assert!(
            !call["surface"].is_null(),
            "call {index}: absent floor is outside this arithmetic test"
        );
        let surface = &call["surface"];
        let normal =
            std::array::from_fn(|axis| f32::from_bits(bits(&surface["normal_bits"][axis])));
        let offset = f32::from_bits(bits(&surface["origin_offset_bits"]));
        let plane = FloorPlane::new(normal, offset).unwrap();
        let vertices = std::array::from_fn(|vertex| {
            std::array::from_fn(|axis| {
                i16::try_from(surface["vertices"][vertex][axis].as_i64().unwrap()).unwrap()
            })
        });
        let constructed = FloorPlane::from_vertices(vertices).unwrap();
        let (constructed_normal, constructed_offset) = constructed.coefficients();
        assert_eq!(
            constructed_normal.map(f32::to_bits),
            normal.map(f32::to_bits),
            "call {index}: constructed normal"
        );
        assert_eq!(
            constructed_offset.to_bits(),
            offset.to_bits(),
            "call {index}: constructed origin offset"
        );
        let x = f32::from_bits(bits(&call["input_bits"][0]));
        let z = f32::from_bits(bits(&call["input_bits"][2]));
        for (label, candidate) in [("supplied", plane), ("constructed", constructed)] {
            assert_eq!(
                candidate.height_at(x, z).unwrap().to_bits(),
                bits(&call["height_bits"]),
                "call {index}: {label} plane height"
            );
        }
    }
    eprintln!(
        "{} returned-plane coefficients/heights matched exact f32 bits from supplied coefficients AND vertices; surfaces were oracle-selected, NOT an independent collision resolver",
        calls.len()
    );
}
