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
