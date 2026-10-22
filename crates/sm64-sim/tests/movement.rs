//! Authored synthetic states/geometry only; not original-game trace fixtures.
use sm64_sim::{
    MarioState, Runner,
    input::ControllerInput,
    movement::{
        BRAKING, BRAKING_STOP, DECELERATING, FlatLane, FlatWalk, IDLE, MAX_IDLE_TICKS,
        MAX_WALK_TICKS, MIN_BRAKING_WALK_TICKS, MIN_WALK_TICKS, Scenario, Segment, WALKING,
    },
};

fn lane() -> FlatLane {
    FlatLane {
        min_x: -100.0,
        max_x: 100.0,
        min_z: -2000.0,
        max_z: 2000.0,
        height: 12.0,
    }
}
fn initial() -> MarioState {
    MarioState {
        position: [17.0, 12.0, 1024.0],
        velocity: [0.0, 0.0, -0.0],
        face_yaw: i16::MIN,
        action: IDLE,
        ..Default::default()
    }
}
fn forward() -> ControllerInput {
    ControllerInput {
        stick_y: 80,
        ..Default::default()
    }
}
fn runner() -> Runner<FlatWalk> {
    Runner::new(initial(), FlatWalk::new(lane()).unwrap()).unwrap()
}

#[test]
fn idle_walk_release_stops_for_each_supported_hold_length() {
    assert_eq!(MIN_WALK_TICKS, 1);
    assert_eq!(MAX_WALK_TICKS, 30);
    let mut short_durations = std::collections::BTreeSet::new();
    for held in MIN_WALK_TICKS..=MAX_WALK_TICKS {
        let mut r = runner();
        for _ in 0..MAX_IDLE_TICKS {
            r.step(ControllerInput::default()).unwrap();
            assert_eq!(r.state(), &initial());
        }
        let mut speed = 8.0_f32;
        let mut z = initial().position[2];
        for _ in 0..held {
            speed += 1.1_f32 - speed / 43.0;
            for _ in 0..4 {
                z -= speed / 4.0;
            }
            r.step(forward()).unwrap();
            assert_eq!(r.state().action, WALKING);
            assert_eq!(r.state().forward_velocity.to_bits(), speed.to_bits());
            assert_eq!(r.state().position, [17.0, 12.0, z]);
        }
        let short = held < MIN_BRAKING_WALK_TICKS;
        let decrement = if short { 1.0_f32 } else { 4.0_f32 };
        let duration = (speed / decrement).ceil() as u8;
        if short {
            short_durations.insert(duration);
        }
        let stop_action = if short { IDLE } else { BRAKING_STOP };
        for release_tick in 1..=duration {
            speed = (speed - decrement).max(0.0);
            for _ in 0..4 {
                z += -speed / 4.0;
            }
            r.step(ControllerInput::default()).unwrap();
            assert_eq!(r.state().forward_velocity.to_bits(), speed.to_bits());
            assert_eq!(r.state().velocity[2].to_bits(), (-speed).to_bits());
            assert_eq!(r.state().position[2].to_bits(), z.to_bits());
            // The release action starts on the first neutral tick; the first
            // zero-speed tick changes directly to the terminal action.
            let expected = if release_tick == duration {
                stop_action
            } else if short {
                DECELERATING
            } else {
                BRAKING
            };
            assert_eq!(
                r.state().action,
                expected,
                "hold {held}, release {release_tick}"
            );
        }
        assert_eq!(r.state().forward_velocity.to_bits(), 0.0_f32.to_bits());
        assert_eq!(r.state().velocity[2].to_bits(), (-0.0_f32).to_bits());
        let last = *r.state();
        let tick = r.tick();
        for input in [ControllerInput::default(), forward()] {
            assert!(r.step(input).is_err());
            assert_eq!(r.state(), &last);
            assert_eq!(r.state().velocity[2].to_bits(), (-0.0_f32).to_bits());
            assert_eq!(r.tick(), tick);
            assert_eq!(r.previous_input(), ControllerInput::default());
        }
        assert!(r.into_model().complete());
    }
    assert_eq!(short_durations, (9..=16).collect());
}

#[test]
fn displacement_retains_four_separately_rounded_quarters() {
    let mut r = runner();
    let mut found_rounding_difference = false;
    for _ in 0..MAX_WALK_TICKS {
        let before = r.state().position[2];
        r.step(forward()).unwrap();
        let v = r.state().velocity[2];
        let mut quarter_result = before;
        for _ in 0..4 {
            quarter_result += v / 4.0;
        }
        assert_eq!(r.state().position[2].to_bits(), quarter_result.to_bits());
        found_rounding_difference |= (before + v).to_bits() != quarter_result.to_bits();
    }
    assert!(found_rounding_difference);
}

#[test]
fn unsupported_inputs_leave_state_input_tick_and_model_unchanged() {
    let invalid = [
        ControllerInput {
            buttons: 0x8000,
            ..Default::default()
        },
        ControllerInput {
            stick_x: 1,
            stick_y: 80,
            ..Default::default()
        },
        ControllerInput {
            stick_y: 79,
            ..Default::default()
        },
        ControllerInput {
            stick_y: -80,
            ..Default::default()
        },
    ];
    for input in invalid {
        let mut r = runner();
        assert!(r.step(input).is_err());
        assert_eq!(r.state(), &initial());
        assert_eq!(r.tick(), 0);
        assert_eq!(r.previous_input(), ControllerInput::default());
        r.step(forward()).unwrap();
        let mut control = runner();
        control.step(forward()).unwrap();
        assert_eq!(r.state(), control.state());
    }
}

#[test]
fn lane_escape_is_atomic_and_can_retry_without_advancing_phase() {
    let mut restricted = lane();
    restricted.min_z = 1023.0;
    let mut r = Runner::new(initial(), FlatWalk::new(restricted).unwrap()).unwrap();
    assert!(r.step(forward()).is_err());
    assert_eq!(r.tick(), 0);
    assert_eq!(r.state(), &initial());
    r.step(ControllerInput::default()).unwrap();
    assert_eq!(r.tick(), 1);
    assert_eq!(r.state(), &initial());
}

#[test]
fn limits_stop_animation_and_braking_resume_fail_explicitly() {
    let mut r = runner();
    for _ in 0..MAX_IDLE_TICKS {
        r.step(ControllerInput::default()).unwrap();
    }
    assert!(r.step(ControllerInput::default()).is_err());
    for _ in 0..MAX_WALK_TICKS {
        r.step(forward()).unwrap();
    }
    assert!(r.step(forward()).is_err());
    r.step(ControllerInput::default()).unwrap();
    assert!(r.step(forward()).is_err());
    while r.state().action != BRAKING_STOP {
        r.step(ControllerInput::default()).unwrap();
    }
    let last = *r.state();
    assert!(r.step(ControllerInput::default()).is_err());
    assert_eq!(r.state(), &last);
}

#[test]
fn resuming_short_deceleration_is_rejected_atomically() {
    for held in MIN_WALK_TICKS..MIN_BRAKING_WALK_TICKS {
        let mut r = runner();
        let mut control = runner();
        for _ in 0..held {
            r.step(forward()).unwrap();
            control.step(forward()).unwrap();
        }
        r.step(ControllerInput::default()).unwrap();
        control.step(ControllerInput::default()).unwrap();
        while r.state().action == DECELERATING {
            let state = *r.state();
            let tick = r.tick();
            assert!(
                r.step(forward())
                    .unwrap_err()
                    .to_string()
                    .contains("resuming")
            );
            assert_eq!(r.state(), &state);
            assert_eq!(r.tick(), tick);
            assert_eq!(r.previous_input(), ControllerInput::default());
            // Retrying neutral must preserve the hidden phase as well.
            r.step(ControllerInput::default()).unwrap();
            control.step(ControllerInput::default()).unwrap();
            assert_eq!(r.state(), control.state());
            assert_eq!(r.tick(), control.tick());
        }
        assert_eq!(r.state().action, IDLE);
        assert!(r.into_model().complete());
    }
}

#[test]
fn invalid_state_and_lane_are_rejected() {
    for invalid in [f32::NAN, f32::INFINITY, 2_000_000.0] {
        let mut l = lane();
        l.height = invalid;
        assert!(FlatWalk::new(l).is_err());
    }
    let mut l = lane();
    l.min_x = l.max_x;
    assert!(FlatWalk::new(l).is_err());
    for mutation in 0..6 {
        let mut s = initial();
        match mutation {
            0 => s.position[1] += 1.0,
            1 => s.position[0] = 200.0,
            2 => s.face_yaw = 0,
            3 => s.action = WALKING,
            4 => s.forward_velocity = 2.0,
            _ => s.velocity[0] = 1.0,
        }
        let mut r = Runner::new(s, FlatWalk::new(lane()).unwrap()).unwrap();
        assert!(r.step(forward()).is_err());
        assert_eq!(r.state(), &s);
    }
}

fn scenario() -> Scenario {
    Scenario {
        version: 1,
        scenario: "authored-test".into(),
        initialization: "Invented grounded state and empty flat lane; not a reference capture."
            .into(),
        input_sampling: "Raw input before each tick".into(),
        state_boundary: "Immediately before/after each model tick".into(),
        start_tick: 50,
        initial_state: initial(),
        previous_input: ControllerInput::default(),
        lane: lane(),
        segments: vec![
            Segment {
                ticks: 3,
                input: ControllerInput::default(),
            },
            Segment {
                ticks: 12,
                input: forward(),
            },
            Segment {
                ticks: 5,
                input: ControllerInput::default(),
            },
        ],
    }
}

#[test]
fn native_scenario_has_explicit_provenance_and_rejects_incomplete_or_extra_ticks() {
    let s = scenario();
    let t = s.run().unwrap();
    assert_eq!(t.manifest.source, sm64_sim::reference::Source::Native);
    assert_eq!(t.start_tick, 50);
    assert_eq!(t.frames.len(), 20);
    assert_eq!(t.frames.last().unwrap().state.action, BRAKING_STOP);
    for ticks in [0, 4, 6] {
        let mut bad = s.clone();
        bad.segments[2].ticks = ticks;
        assert!(bad.run().is_err());
    }
    let mut bad = s.clone();
    bad.version = 2;
    assert!(bad.run().is_err());
    let mut bad = s.clone();
    bad.start_tick = u64::MAX;
    assert!(bad.run().is_err());
    let mut bad = s.clone();
    bad.scenario = " ".into();
    assert!(bad.run().is_err());
    let mut bad = s;
    bad.previous_input.buttons = 1;
    assert!(bad.run().is_err());
}

#[test]
fn short_scenarios_end_exactly_on_first_idle_and_reject_extra_updates() {
    for held in MIN_WALK_TICKS..MIN_BRAKING_WALK_TICKS {
        let mut speed = 8.0_f32;
        for _ in 0..held {
            speed += 1.1_f32 - speed / 43.0;
        }
        let duration = speed.ceil() as u8;
        let mut s = scenario();
        s.segments[0].ticks = MAX_IDLE_TICKS;
        s.segments[1].ticks = held;
        s.segments[2].ticks = duration;
        let trace = s.run().unwrap();
        assert_eq!(
            trace.frames.len(),
            usize::from(MAX_IDLE_TICKS + held + duration)
        );
        assert!(trace.frames.len() <= 30);
        let last = trace.frames.last().unwrap();
        assert_eq!(last.state.action, IDLE);
        assert_eq!(last.state.forward_velocity.to_bits(), 0.0_f32.to_bits());
        assert_eq!(last.state.velocity[2].to_bits(), (-0.0_f32).to_bits());
        assert!(trace.manifest.settings.contains("1..=30 walking ticks"));
        for ticks in [duration - 1, duration + 1] {
            let mut bad = s.clone();
            bad.segments[2].ticks = ticks;
            assert!(bad.run().is_err());
        }
        // IDLE at the boundary is terminal, not permission to start a new walk.
        s.segments.push(Segment {
            ticks: 1,
            input: forward(),
        });
        assert!(s.run().is_err());
    }
}

#[test]
fn negative_z_keeps_the_nonmoving_x_signed_zero() {
    let mut state = initial();
    state.position[0] = -0.0;
    let mut r = Runner::new(state, FlatWalk::new(lane()).unwrap()).unwrap();
    r.step(forward()).unwrap();
    r.step(ControllerInput::default()).unwrap();
    assert_eq!(r.state().position[0].to_bits(), (-0.0_f32).to_bits());
}

fn diagonal_lane() -> FlatLane {
    FlatLane {
        max_x: 1000.0,
        ..lane()
    }
}

fn diagonal_initial() -> MarioState {
    MarioState {
        face_yaw: 24576,
        ..initial()
    }
}

#[test]
fn diagonal_holds_have_exact_velocity_quarters_signed_zeros_and_short_release() {
    use sm64_sim::movement::MAX_DIAGONAL_WALK_TICKS;
    assert_eq!(MAX_DIAGONAL_WALK_TICKS, 5);
    let mut rounding_differs = [false; 2];
    for held in 1..=MAX_DIAGONAL_WALK_TICKS {
        let mut r =
            Runner::new(diagonal_initial(), FlatWalk::new(diagonal_lane()).unwrap()).unwrap();
        for _ in 0..MAX_IDLE_TICKS {
            r.step(ControllerInput::default()).unwrap();
            assert_eq!(r.state().velocity.map(f32::to_bits), [0, 0, 0x8000_0000]);
            assert_eq!(r.state().action, IDLE);
        }
        let mut speed = 8.0_f32;
        let mut position = diagonal_initial().position;
        let mut tick = 0;
        loop {
            let walking = tick < held;
            if walking {
                speed += 1.1_f32 - speed / 43.0_f32;
            } else {
                speed = (speed - 1.0_f32).max(0.0);
            }
            let component = speed * std::f32::consts::FRAC_1_SQRT_2;
            let velocity = [component, 0.0, -component];
            let before = position;
            for _ in 0..4 {
                position[0] += component / 4.0;
                position[2] += -component / 4.0;
            }
            for (i, axis) in [0, 2].into_iter().enumerate() {
                rounding_differs[i] |=
                    (before[axis] + velocity[axis]).to_bits() != position[axis].to_bits();
            }
            r.step(if walking {
                forward()
            } else {
                ControllerInput::default()
            })
            .unwrap();
            assert_eq!(r.state().forward_velocity.to_bits(), speed.to_bits());
            assert_eq!(
                r.state().velocity.map(f32::to_bits),
                velocity.map(f32::to_bits)
            );
            assert_eq!(
                r.state().position.map(f32::to_bits),
                position.map(f32::to_bits)
            );
            assert_eq!(r.state().face_yaw, 24576);
            assert_eq!(
                r.state().action,
                if walking {
                    WALKING
                } else if speed == 0.0 {
                    IDLE
                } else {
                    DECELERATING
                }
            );
            if speed == 0.0 {
                break;
            }
            tick += 1;
        }
        assert_eq!(r.state().velocity.map(f32::to_bits), [0, 0, 0x8000_0000]);
        let last = *r.state();
        let tick = r.tick();
        assert!(r.step(ControllerInput::default()).is_err());
        assert!(r.step(forward()).is_err());
        assert_eq!(r.state(), &last);
        assert_eq!(r.tick(), tick);
        assert!(r.into_model().complete());
    }
    assert_eq!(rounding_differs, [true, true]);
}

#[test]
fn sixth_diagonal_forward_is_atomic_and_neutral_retry_matches_control() {
    let make = || Runner::new(diagonal_initial(), FlatWalk::new(diagonal_lane()).unwrap()).unwrap();
    let mut r = make();
    let mut control = make();
    for _ in 0..5 {
        r.step(forward()).unwrap();
        control.step(forward()).unwrap();
    }
    let last = *r.state();
    assert!(
        r.step(forward())
            .unwrap_err()
            .to_string()
            .contains("budget")
    );
    assert_eq!(r.state(), &last);
    assert_eq!(r.tick(), 5);
    assert_eq!(r.previous_input(), forward());
    loop {
        r.step(ControllerInput::default()).unwrap();
        control.step(ControllerInput::default()).unwrap();
        assert_eq!(r.state(), control.state());
        if r.state().action == IDLE {
            break;
        }
    }
    assert!(r.into_model().complete());
}

#[test]
fn headings_lock_only_on_success_and_reject_idle_and_walking_turns() {
    use sm64_sim::{TickModel, input::TickInput};
    let input = |raw| TickInput {
        raw,
        ..Default::default()
    };
    for yaw in [0, 24575, 24577, -32767] {
        for raw in [ControllerInput::default(), forward()] {
            let mut model = FlatWalk::new(lane()).unwrap();
            let mut state = MarioState {
                face_yaw: yaw,
                ..initial()
            };
            let before = state;
            assert!(model.tick(&mut state, input(raw), 0).is_err());
            assert_eq!(state, before);
            // The failed first tick must not lock any heading or phase.
            state = diagonal_initial();
            model.tick(&mut state, input(raw), 0).unwrap();
        }
    }
    for initial_yaw in [i16::MIN, 24576] {
        for idle_count in [0, 1, MAX_IDLE_TICKS] {
            let mut model = FlatWalk::new(lane()).unwrap();
            let mut state = MarioState {
                face_yaw: initial_yaw,
                ..initial()
            };
            for tick in 0..idle_count {
                model
                    .tick(&mut state, input(ControllerInput::default()), tick.into())
                    .unwrap();
            }
            if idle_count != 0 {
                let mut tampered = state;
                tampered.face_yaw = if initial_yaw == i16::MIN {
                    24576
                } else {
                    i16::MIN
                };
                let before = tampered;
                for raw in [ControllerInput::default(), forward()] {
                    assert!(model.tick(&mut tampered, input(raw), 5).is_err());
                    assert_eq!(tampered, before);
                }
            }
            model.tick(&mut state, input(forward()), 5).unwrap();
            let mut control = model.clone();
            let mut expected = state;
            let mut tampered = state;
            tampered.face_yaw = if initial_yaw == i16::MIN {
                24576
            } else {
                i16::MIN
            };
            // Supply the other heading's correct velocity so this specifically
            // tests persisted heading, not merely a mismatched velocity.
            let v = state.forward_velocity;
            let c = v * std::f32::consts::FRAC_1_SQRT_2;
            tampered.velocity = if initial_yaw == i16::MIN {
                [c, 0.0, -c]
            } else {
                [0.0, 0.0, -v]
            };
            let before = tampered;
            assert!(model.tick(&mut tampered, input(forward()), 6).is_err());
            assert_eq!(tampered, before);
            model
                .tick(&mut state, input(ControllerInput::default()), 6)
                .unwrap();
            control
                .tick(&mut expected, input(ControllerInput::default()), 6)
                .unwrap();
            assert_eq!(state, expected);
        }
    }
}

#[test]
fn diagonal_x_lane_escape_rolls_back_heading_phase_and_state() {
    use sm64_sim::{TickModel, input::TickInput};
    let mut restricted = lane();
    restricted.max_x = initial().position[0] + 3.0; // Second diagonal quarter exits.
    let mut model = FlatWalk::new(restricted).unwrap();
    let mut state = diagonal_initial();
    let before = state;
    assert!(
        model
            .tick(
                &mut state,
                TickInput {
                    raw: forward(),
                    ..Default::default()
                },
                0
            )
            .unwrap_err()
            .to_string()
            .contains("quarter step")
    );
    assert_eq!(state, before);
    assert_eq!(state.velocity.map(f32::to_bits), [0, 0, 0x8000_0000]);
    // A failed diagonal tick must not lock heading: -Z fits the same lane.
    state.face_yaw = i16::MIN;
    let mut control = FlatWalk::new(restricted).unwrap();
    let mut expected = state;
    let input = TickInput {
        raw: forward(),
        ..Default::default()
    };
    model.tick(&mut state, input, 0).unwrap();
    control.tick(&mut expected, input, 0).unwrap();
    assert_eq!(state, expected);
}

#[test]
fn diagonal_rejects_wrong_direction_velocity_atomically() {
    use sm64_sim::{TickModel, input::TickInput};
    for axis in 0..3 {
        let mut model = FlatWalk::new(lane()).unwrap();
        let mut state = diagonal_initial();
        let input = TickInput {
            raw: forward(),
            ..Default::default()
        };
        model.tick(&mut state, input, 0).unwrap();
        let valid = state;
        state.velocity[axis] += 0.25;
        let before = state;
        assert!(model.tick(&mut state, input, 1).is_err());
        assert_eq!(state, before);
        state = valid;
        model.tick(&mut state, input, 1).unwrap();
    }
}

#[test]
fn diagonal_scenarios_declare_the_bounded_heading_and_stop_at_first_idle() {
    for held in 1..=5 {
        let mut speed = 8.0_f32;
        for _ in 0..held {
            speed += 1.1_f32 - speed / 43.0;
        }
        let duration = speed.ceil() as u8;
        let mut s = scenario();
        s.initial_state = diagonal_initial();
        s.lane = diagonal_lane();
        s.segments[1].ticks = held;
        s.segments[2].ticks = duration;
        let trace = s.run().unwrap();
        assert_eq!(trace.frames.last().unwrap().state.action, IDLE);
        assert!(trace.manifest.settings.contains("fixed initial yaw 24576"));
        assert!(
            trace
                .manifest
                .settings
                .contains("yaw 24576: 1..=5 walking ticks")
        );
        assert!(trace.manifest.settings.contains("FRAC_1_SQRT_2"));
        for ticks in [duration - 1, duration + 1] {
            s.segments[2].ticks = ticks;
            assert!(s.run().is_err());
        }
        s.segments[1].ticks = 6;
        assert!(s.run().is_err());
    }
}
