//! Authored toy transitions only; these tests do not model Mario gameplay.

use std::cell::RefCell;
use std::rc::Rc;

use sm64_sim::input::{ButtonTracker, ControllerInput, TickInput};
use sm64_sim::{Error, MarioState, Runner, TickModel};

#[derive(Debug, Clone, Copy, Default)]
enum Outcome {
    #[default]
    Valid,
    Error,
    Nonfinite,
}

#[derive(Debug, Default)]
struct Toy {
    calls: Rc<RefCell<Vec<(u64, TickInput)>>>,
    outcome: Outcome,
}

impl TickModel for Toy {
    fn tick(&mut self, state: &mut MarioState, input: TickInput, tick: u64) -> Result<(), Error> {
        self.calls.borrow_mut().push((tick, input));
        // Arbitrary test arithmetic, not movement rules.
        state.position[0] += 1.0;
        state.action = u32::from(input.pressed);
        match self.outcome {
            Outcome::Valid => Ok(()),
            Outcome::Error => Err(Error::Model("test failure".into())),
            Outcome::Nonfinite => {
                state.velocity[2] = f32::NAN;
                Ok(())
            }
        }
    }
}

fn raw(buttons: u16) -> ControllerInput {
    ControllerInput {
        buttons,
        stick_x: i8::MIN,
        stick_y: i8::MAX,
    }
}

#[test]
fn edges_preserve_raw_bits_and_sticks() {
    let mut tracker = ButtonTracker::default();
    let first = tracker.update(raw(0x8001));
    assert_eq!(first.raw, raw(0x8001));
    assert_eq!(
        (first.held, first.pressed, first.released),
        (0x8001, 0x8001, 0)
    );
    let held = tracker.update(raw(0x8001));
    assert_eq!((held.held, held.pressed, held.released), (0x8001, 0, 0));
    let changed = tracker.update(raw(0x4001));
    assert_eq!(
        (changed.held, changed.pressed, changed.released),
        (0x4001, 0x4000, 0x8000)
    );
    let released = tracker.update(raw(0));
    assert_eq!(
        (released.held, released.pressed, released.released),
        (0, 0, 0x4001)
    );
    assert_eq!(tracker.previous(), raw(0));
}

#[test]
fn initial_snapshot_held_buttons_are_not_new_presses() {
    let model = Toy::default();
    let calls = Rc::clone(&model.calls);
    let mut runner = Runner::with_snapshot(MarioState::default(), 37, raw(0xffff), model).unwrap();
    runner.step(raw(0xffff)).unwrap();
    let calls = calls.borrow();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, 37);
    assert_eq!(
        (calls[0].1.held, calls[0].1.pressed, calls[0].1.released),
        (0xffff, 0, 0)
    );
    assert_eq!(runner.tick(), 38);
}

#[test]
fn each_step_is_exactly_one_transition() {
    let mut runner = Runner::new(MarioState::default(), Toy::default()).unwrap();
    assert_eq!(runner.tick(), 0);
    assert_eq!(runner.previous_input(), ControllerInput::default());
    runner.step(raw(1)).unwrap();
    assert_eq!(runner.state().position, [1.0, 0.0, 0.0]);
    assert_eq!(runner.tick(), 1);
    assert_eq!(runner.previous_input(), raw(1));
    runner.step(raw(2)).unwrap();
    assert_eq!(runner.state().position, [2.0, 0.0, 0.0]);
    assert_eq!(runner.tick(), 2);
    let model = runner.into_model();
    let calls = model.calls.borrow();
    assert_eq!(calls.iter().map(|call| call.0).collect::<Vec<_>>(), [0, 1]);
}

#[test]
fn deterministic_rerun() {
    let initial = MarioState {
        position: [12.5, -3.0, 7.0],
        action: 0xfeed_beef,
        ..MarioState::default()
    };
    let inputs = [raw(0x8001), raw(0x8001), raw(0x4000), raw(0)];
    let run = || {
        let mut runner = Runner::with_snapshot(initial, 19, raw(1), Toy::default()).unwrap();
        let mut snapshots = Vec::new();
        for input in inputs {
            runner.step(input).unwrap();
            snapshots.push((*runner.state(), runner.tick(), runner.previous_input()));
        }
        snapshots
    };
    assert_eq!(run(), run());
}

#[test]
fn model_error_and_invalid_output_are_transactional() {
    for outcome in [Outcome::Error, Outcome::Nonfinite] {
        let initial = MarioState {
            action: 0x1234_5678,
            ..MarioState::default()
        };
        let model = Toy {
            outcome,
            ..Toy::default()
        };
        let calls = Rc::clone(&model.calls);
        let mut runner = Runner::with_snapshot(initial, 42, raw(8), model).unwrap();
        let error = runner.step(raw(16)).unwrap_err();
        match outcome {
            Outcome::Error => assert_eq!(error, Error::Model("test failure".into())),
            Outcome::Nonfinite => assert_eq!(error, Error::NonFiniteState { field: "velocity" }),
            Outcome::Valid => unreachable!(),
        }
        assert_eq!(runner.state(), &initial);
        assert_eq!(runner.tick(), 42);
        assert_eq!(runner.previous_input(), raw(8));
        // Retrying sees the same tick and edges, not a committed input snapshot.
        assert!(runner.step(raw(16)).is_err());
        let calls = calls.borrow();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0], calls[1]);
    }
}

#[test]
fn overflow_is_transactional_and_does_not_invoke_model() {
    let model = Toy::default();
    let calls = Rc::clone(&model.calls);
    let initial = MarioState::default();
    let mut runner = Runner::with_snapshot(initial, u64::MAX - 1, raw(4), model).unwrap();
    runner.step(raw(8)).unwrap();
    let last_state = *runner.state();
    assert_eq!(runner.step(raw(16)), Err(Error::TickOverflow));
    assert_eq!(runner.state(), &last_state);
    assert_eq!(runner.tick(), u64::MAX);
    assert_eq!(runner.previous_input(), raw(8));
    assert_eq!(calls.borrow().len(), 1);
    assert_eq!(calls.borrow()[0].0, u64::MAX - 1);
}

#[test]
fn all_nonfinite_state_components_are_rejected_before_model_execution() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for component in 0..7 {
            let mut state = MarioState::default();
            let field = match component {
                0..=2 => {
                    state.position[component] = value;
                    "position"
                }
                3..=5 => {
                    state.velocity[component - 3] = value;
                    "velocity"
                }
                _ => {
                    state.forward_velocity = value;
                    "forward_velocity"
                }
            };
            assert_eq!(state.validate(), Err(Error::NonFiniteState { field }));
            let model = Toy::default();
            let calls = Rc::clone(&model.calls);
            assert!(matches!(
                Runner::new(state, model),
                Err(Error::NonFiniteState { .. })
            ));
            assert!(calls.borrow().is_empty());
        }
    }
}

#[test]
fn finite_state_has_no_gameplay_range_restrictions() {
    MarioState {
        position: [f32::MAX, f32::MIN, -0.0],
        velocity: [f32::MIN_POSITIVE, -f32::MAX, 0.0],
        forward_velocity: f32::MAX,
        face_yaw: i16::MIN,
        action: u32::MAX,
    }
    .validate()
    .unwrap();
}

#[test]
fn serde_round_trips_raw_state_and_inputs_and_rejects_unknown_fields() {
    let state = MarioState {
        position: [1.5, -2.0, 3.0],
        velocity: [-4.0, 5.0, 6.0],
        forward_velocity: -7.0,
        face_yaw: i16::MIN,
        action: u32::MAX,
    };
    let json = serde_json::to_string(&state).unwrap();
    assert_eq!(serde_json::from_str::<MarioState>(&json).unwrap(), state);
    let mut value = serde_json::to_value(state).unwrap();
    value["unknown"] = true.into();
    assert!(serde_json::from_value::<MarioState>(value).is_err());

    let tracker = ButtonTracker::new(raw(0xffff));
    let input = tracker.derive(raw(0x8000));
    let json = serde_json::to_string(&input).unwrap();
    assert_eq!(serde_json::from_str::<TickInput>(&json).unwrap(), input);
    let json = serde_json::to_string(&tracker).unwrap();
    assert_eq!(
        serde_json::from_str::<ButtonTracker>(&json).unwrap(),
        tracker
    );
    assert!(
        serde_json::from_str::<ControllerInput>(
            r#"{"buttons":65535,"stick_x":-128,"stick_y":127,"unknown":0}"#
        )
        .is_err()
    );
    let mut value = serde_json::to_value(input).unwrap();
    value["unknown"] = true.into();
    assert!(serde_json::from_value::<TickInput>(value).is_err());
    let mut value = serde_json::to_value(tracker).unwrap();
    value["unknown"] = true.into();
    assert!(serde_json::from_value::<ButtonTracker>(value).is_err());
}
