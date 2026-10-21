//! Caller-supplied transitions, one per successful `step`.

use serde::{Deserialize, Serialize};

use crate::Error;
use crate::input::{ButtonTracker, ControllerInput, TickInput};

/// Raw state storage, not a description of implemented gameplay.
/// The default action is zero, with no assigned gameplay meaning.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MarioState {
    pub position: [f32; 3],
    pub velocity: [f32; 3],
    pub forward_velocity: f32,
    /// Signed binary angle: one full turn spans 65536 units.
    pub face_yaw: i16,
    /// Original action bits, uninterpreted by this crate.
    pub action: u32,
}

impl MarioState {
    /// Checks only finiteness; imposes no gameplay ranges or action semantics.
    pub fn validate(&self) -> Result<(), Error> {
        for (field, finite) in [
            ("position", self.position.iter().all(|v| v.is_finite())),
            ("velocity", self.velocity.iter().all(|v| v.is_finite())),
            ("forward_velocity", self.forward_velocity.is_finite()),
        ] {
            if !finite {
                return Err(Error::NonFiniteState { field });
            }
        }
        Ok(())
    }
}

pub trait TickModel {
    /// Applies exactly one transition. `tick` identifies the transition being run.
    fn tick(
        &mut self,
        candidate: &mut MarioState,
        input: TickInput,
        tick: u64,
    ) -> Result<(), Error>;
}

/// Commits state, input history, and tick together only after a valid transition.
/// Model-internal side effects are not rolled back on failure; models should avoid
/// such effects if retries must reproduce the same result.
#[derive(Debug)]
pub struct Runner<M: TickModel> {
    state: MarioState,
    tick: u64,
    input: ButtonTracker,
    model: M,
}

impl<M: TickModel> Runner<M> {
    /// Starts with transition zero next and an all-zero previous raw input.
    pub fn new(state: MarioState, model: M) -> Result<Self, Error> {
        Self::with_snapshot(state, 0, ControllerInput::default(), model)
    }

    /// Restores caller-supplied state and input history. `tick` is the next
    /// transition's index, not the number of an already-completed transition.
    pub fn with_snapshot(
        state: MarioState,
        tick: u64,
        previous_input: ControllerInput,
        model: M,
    ) -> Result<Self, Error> {
        state.validate()?;
        Ok(Self {
            state,
            tick,
            input: ButtonTracker::new(previous_input),
            model,
        })
    }

    pub fn state(&self) -> &MarioState {
        &self.state
    }

    pub fn tick(&self) -> u64 {
        self.tick
    }

    pub fn previous_input(&self) -> ControllerInput {
        self.input.previous()
    }

    pub fn into_model(self) -> M {
        self.model
    }

    pub fn step(&mut self, input: ControllerInput) -> Result<(), Error> {
        self.state.validate()?;
        let next_tick = self.tick.checked_add(1).ok_or(Error::TickOverflow)?;
        let mut candidate = self.state;
        self.model
            .tick(&mut candidate, self.input.derive(input), self.tick)?;
        candidate.validate()?;
        self.state = candidate;
        self.input.update(input);
        self.tick = next_tick;
        Ok(())
    }
}
