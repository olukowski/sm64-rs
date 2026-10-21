//! Authored synthetic observations only; no ROM, gameplay model, or captured traces.
use sm64_sim::input::{ControllerInput, TickInput};
use sm64_sim::reference::{
    self, Error, FloatPolicy, Frame, MAX_BYTES, MAX_FRAMES, Manifest, Source, Trace, VERSION,
};
use sm64_sim::{MarioState, TickModel};

fn authored() -> Trace {
    Trace {
        version: VERSION,
        manifest: Manifest {
            rom_sha256: sm64_rom::US_SHA256.into(),
            source: Source::Authored,
            scenario: "synthetic button-edge counter".into(),
            producer: "authored integration test".into(),
            version: "1".into(),
            settings: "toy, no gameplay".into(),
            initialization: "counter zero, previous button 1 held".into(),
            input_sampling: "one raw sample per tick".into(),
            state_boundary: "after transition".into(),
        },
        start_tick: 7,
        initial_state: MarioState::default(),
        previous_input: ControllerInput {
            buttons: 1,
            ..ControllerInput::default()
        },
        frames: vec![
            Frame {
                tick: 7,
                input: ControllerInput {
                    buttons: 1,
                    ..ControllerInput::default()
                },
                state: MarioState::default(),
            },
            Frame {
                tick: 8,
                input: ControllerInput::default(),
                state: MarioState {
                    face_yaw: 1,
                    ..MarioState::default()
                },
            },
            Frame {
                tick: 9,
                input: ControllerInput {
                    buttons: 1,
                    stick_x: 2,
                    stick_y: -3,
                },
                state: MarioState {
                    position: [2.0, -3.0, 0.0],
                    action: 1,
                    face_yaw: 1,
                    ..MarioState::default()
                },
            },
        ],
    }
}

fn diff(actual: &Trace) -> reference::Difference {
    reference::compare(&authored(), actual, FloatPolicy::Exact)
        .unwrap()
        .unwrap()
}

#[test]
fn strict_roundtrip_and_provenance() {
    let trace = authored();
    let mut bytes = Vec::new();
    trace.write(&mut bytes).unwrap();
    assert_eq!(Trace::read(bytes.as_slice()).unwrap(), trace);
    assert!(
        String::from_utf8(bytes)
            .unwrap()
            .contains("\"source\":\"authored\"")
    );
    assert_eq!(
        reference::compare(&trace, &trace, FloatPolicy::default()).unwrap(),
        None
    );
    let mut other = trace.clone();
    other.manifest.source = Source::Native;
    other.manifest.producer = "different producer".into();
    other.manifest.version = "2".into();
    other.manifest.settings = "different settings".into();
    other.manifest.initialization = "equivalent observations".into();
    assert_eq!(
        reference::compare(&trace, &other, FloatPolicy::Exact).unwrap(),
        None
    );
}

#[test]
fn injected_state_and_raw_input_differences() {
    for (field, mutate) in [
        (
            "state.position[0]",
            (|f: &mut Frame| f.state.position[0] += 1.0) as fn(&mut Frame),
        ),
        ("state.velocity[2]", |f: &mut Frame| {
            f.state.velocity[2] = 1.0
        }),
        ("state.forward_velocity", |f: &mut Frame| {
            f.state.forward_velocity = 1.0
        }),
        ("state.action", |f: &mut Frame| f.state.action = u32::MAX),
        ("state.face_yaw", |f: &mut Frame| {
            f.state.face_yaw = i16::MIN
        }),
        ("input.buttons", |f: &mut Frame| f.input.buttons = u16::MAX),
        ("input.stick_x", |f: &mut Frame| f.input.stick_x = i8::MIN),
        ("input.stick_y", |f: &mut Frame| f.input.stick_y = i8::MAX),
    ] {
        let mut trace = authored();
        mutate(&mut trace.frames[1]);
        let difference = diff(&trace);
        assert_eq!(difference.tick, 8);
        assert_eq!(difference.field, field);
        assert_ne!(difference.expected, difference.actual);
    }
    let mut trace = authored();
    trace.initial_state.action = 2;
    assert_eq!(diff(&trace).field, "initial_state.action");
    trace.previous_input.buttons = 0;
    assert_eq!(diff(&trace).field, "previous_input.buttons");
}

#[test]
fn float_bits_tolerance_and_wide_subtraction() {
    let expected = authored();
    let mut actual = expected.clone();
    actual.frames[0].state.position[0] = -0.0;
    let d = diff(&actual);
    assert!(d.expected.contains("0x00000000"));
    assert!(d.actual.contains("0x80000000"));
    assert_eq!(
        reference::compare(&expected, &actual, FloatPolicy::Absolute(0.0)).unwrap(),
        None
    );
    actual.frames[0].state.position[0] = 0.125;
    assert_eq!(
        reference::compare(&expected, &actual, FloatPolicy::Absolute(0.125)).unwrap(),
        None
    );
    assert!(
        reference::compare(&expected, &actual, FloatPolicy::Absolute(0.124))
            .unwrap()
            .is_some()
    );
    let mut expected = expected;
    expected.frames[0].state.position[0] = f32::MAX;
    actual.frames[0].state.position[0] = -f32::MAX;
    assert_eq!(
        reference::compare(
            &expected,
            &actual,
            FloatPolicy::Absolute(f64::from(f32::MAX) * 2.0)
        )
        .unwrap(),
        None
    );
    for tolerance in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(matches!(
            reference::compare(&expected, &actual, FloatPolicy::Absolute(tolerance)),
            Err(Error::InvalidTolerance)
        ));
        assert!(matches!(
            reference::replay(&authored(), Toy, FloatPolicy::Absolute(tolerance)),
            Err(Error::InvalidTolerance)
        ));
    }
}

#[test]
fn tick_and_length_validation() {
    let mut trace = authored();
    trace.frames.remove(1);
    assert!(trace.validate().is_err());
    let mut trace = authored();
    trace.frames.pop();
    let d = diff(&trace);
    assert_eq!(
        (
            d.tick,
            d.field.as_str(),
            d.expected.as_str(),
            d.actual.as_str()
        ),
        (9, "frames.length", "3", "2")
    );
    let mut trace = authored();
    let mut extra = trace.frames[2].clone();
    extra.tick = 10;
    trace.frames.push(extra);
    assert_eq!(diff(&trace).field, "frames.length");
    trace.frames[3].tick = 9;
    assert!(trace.validate().is_err());
    trace.frames.clear();
    assert!(trace.validate().is_err());
    let mut trace = authored();
    trace.frames.truncate(1);
    trace.start_tick = u64::MAX;
    trace.frames[0].tick = u64::MAX;
    assert!(trace.validate().is_err());
    trace.start_tick -= 1;
    trace.frames[0].tick -= 1;
    trace.validate().unwrap();
}

#[test]
fn protocol_and_full_validation_precede_differences() {
    for mutate in [
        (|t: &mut Trace| t.manifest.scenario = "another scenario".into()) as fn(&mut Trace),
        |t: &mut Trace| t.manifest.input_sampling = "another sampling".into(),
        |t: &mut Trace| t.manifest.state_boundary = "before transition".into(),
        |t: &mut Trace| {
            t.start_tick += 1;
            for f in &mut t.frames {
                f.tick += 1;
            }
        },
    ] {
        let mut trace = authored();
        trace.frames[0].state.action = 999;
        mutate(&mut trace);
        assert!(matches!(
            reference::compare(&authored(), &trace, FloatPolicy::Exact),
            Err(Error::Incompatible { .. })
        ));
    }
    let mut trace = authored();
    trace.frames[0].state.action = 999;
    trace.frames[2].state.velocity[2] = f32::NAN;
    assert!(reference::compare(&authored(), &trace, FloatPolicy::Exact).is_err());
    assert!(reference::compare(&trace, &authored(), FloatPolicy::Exact).is_err());
    assert!(reference::replay(&trace, Toy, FloatPolicy::Exact).is_err());
}

#[test]
fn invalid_metadata_revision_and_nonfinite_components() {
    for mutate in [
        (|t: &mut Trace| t.version = 2) as fn(&mut Trace),
        |t: &mut Trace| t.manifest.rom_sha256 = "wrong revision".into(),
        |t: &mut Trace| t.manifest.rom_sha256.make_ascii_uppercase(),
        |t: &mut Trace| t.manifest.scenario = " ".into(),
        |t: &mut Trace| t.manifest.producer = "\n".into(),
        |t: &mut Trace| t.manifest.version.clear(),
        |t: &mut Trace| t.manifest.settings.clear(),
        |t: &mut Trace| t.manifest.initialization.clear(),
        |t: &mut Trace| t.manifest.input_sampling.clear(),
        |t: &mut Trace| t.manifest.state_boundary.clear(),
    ] {
        let mut trace = authored();
        mutate(&mut trace);
        assert!(trace.validate().is_err());
        let mut output = Vec::new();
        assert!(trace.write(&mut output).is_err());
        assert!(output.is_empty());
        assert!(Trace::read(serde_json::to_vec(&trace).unwrap().as_slice()).is_err());
    }
    for component in 0..7 {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let mut trace = authored();
            let s = &mut trace.initial_state;
            match component {
                0..=2 => s.position[component] = value,
                3..=5 => s.velocity[component - 3] = value,
                _ => s.forward_velocity = value,
            }
            assert!(trace.validate().is_err());
            trace.frames[2].state = trace.initial_state;
            trace.initial_state = MarioState::default();
            assert!(trace.validate().is_err());
        }
    }
}

#[test]
fn strict_json_and_limits() {
    let value = serde_json::to_value(authored()).unwrap();
    for pointer in [
        "",
        "/manifest",
        "/initial_state",
        "/previous_input",
        "/frames/0",
        "/frames/0/input",
        "/frames/0/state",
    ] {
        let mut bad = value.clone();
        bad.pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unknown".into(), true.into());
        assert!(Trace::read(serde_json::to_vec(&bad).unwrap().as_slice()).is_err());
    }
    for pointer in [
        "/version",
        "/manifest",
        "/frames",
        "/initial_state",
        "/previous_input",
    ] {
        let mut bad = value.clone();
        bad.as_object_mut().unwrap().remove(&pointer[1..]);
        assert!(Trace::read(serde_json::to_vec(&bad).unwrap().as_slice()).is_err());
    }
    for bytes in [b"{".as_slice(), b"{}", b"null", b"[]"] {
        assert!(Trace::read(bytes).is_err());
    }
    let mut bytes = serde_json::to_vec(&authored()).unwrap();
    bytes.pop();
    assert!(Trace::read(bytes.as_slice()).is_err());
    let mut bytes = serde_json::to_vec(&authored()).unwrap();
    bytes.extend_from_slice(b" true");
    assert!(Trace::read(bytes.as_slice()).is_err());
    // Infinite whitespace would hang without the bounded read.
    assert!(matches!(
        Trace::read(std::io::repeat(b' ')),
        Err(Error::TooLarge)
    ));
    let mut trace = authored();
    trace.manifest.settings = "x".repeat(MAX_BYTES);
    let mut output = Vec::new();
    assert!(matches!(trace.write(&mut output), Err(Error::TooLarge)));
    assert!(output.is_empty());
    let mut trace = authored();
    trace.frames = (0..MAX_FRAMES)
        .map(|i| Frame {
            tick: trace.start_tick + i as u64,
            ..trace.frames[0].clone()
        })
        .collect();
    trace.validate().unwrap();
    trace.frames.push(Frame {
        tick: trace.start_tick + MAX_FRAMES as u64,
        ..trace.frames[0].clone()
    });
    assert!(trace.validate().is_err());
    assert!(Trace::read(serde_json::to_vec(&trace).unwrap().as_slice()).is_err());
}

struct Toy;
impl TickModel for Toy {
    fn tick(
        &mut self,
        state: &mut MarioState,
        input: TickInput,
        _: u64,
    ) -> Result<(), sm64_sim::Error> {
        state.position[0] += f32::from(input.raw.stick_x);
        state.position[1] += f32::from(input.raw.stick_y);
        state.action += input.pressed.count_ones();
        state.face_yaw += input.released.count_ones() as i16;
        Ok(())
    }
}

#[test]
fn toy_replay_restores_button_edges_and_tick() {
    assert_eq!(
        reference::replay(&authored(), Toy, FloatPolicy::Exact).unwrap(),
        None
    );
    let mut trace = authored();
    trace.previous_input.buttons = 0;
    let d = reference::replay(&trace, Toy, FloatPolicy::Exact)
        .unwrap()
        .unwrap();
    assert_eq!((d.tick, d.field.as_str()), (7, "state.action"));
    struct Fails;
    impl TickModel for Fails {
        fn tick(
            &mut self,
            _: &mut MarioState,
            _: TickInput,
            tick: u64,
        ) -> Result<(), sm64_sim::Error> {
            assert_eq!(tick, 7);
            Err(sm64_sim::Error::Model("authored failure".into()))
        }
    }
    assert!(matches!(
        reference::replay(&authored(), Fails, FloatPolicy::Exact),
        Err(Error::Model { tick: 7, .. })
    ));
    struct Nonfinite;
    impl TickModel for Nonfinite {
        fn tick(
            &mut self,
            state: &mut MarioState,
            _: TickInput,
            _: u64,
        ) -> Result<(), sm64_sim::Error> {
            state.velocity[0] = f32::NAN;
            Ok(())
        }
    }
    assert!(matches!(
        reference::replay(&authored(), Nonfinite, FloatPolicy::Exact),
        Err(Error::Model { tick: 7, .. })
    ));
}
