//! Bounded US0 flat-ground experiment, not a general Mario controller.
//!
//! Fresh implementation from action-boundary observations, not game code.
//! See docs/movement-slice.md for evidence, arithmetic, and the validation envelope.
//! The caller supplies an independently verified, empty, ordinary flat lane.
//! No diagnostic collision probes, camera approximation, or host physics are used.

use serde::{Deserialize, Serialize};

use crate::input::{ControllerInput, TickInput};
use crate::reference::{Frame, Manifest, Source, Trace};
use crate::{Error, MarioState, Runner, TickModel};

// US0 action identifiers: documented metadata, corroborated at 0x80254830.
pub const IDLE: u32 = 0x0c40_0201;
pub const WALKING: u32 = 0x0400_0440;
pub const BRAKING: u32 = 0x0400_0445;
pub const BRAKING_STOP: u32 = 0x0c00_023d;
pub const MAX_IDLE_TICKS: u8 = 5;
pub const MIN_WALK_TICKS: u8 = 10;
pub const MAX_WALK_TICKS: u8 = 30;

/// A caller-asserted safe region, NOT extracted terrain or a collision solver.
/// Bounds include Mario's entire unobstructed route; no slopes, special surfaces,
/// ceilings, walls, interactions, water or moving geometry may affect it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlatLane {
    pub min_x: f32,
    pub max_x: f32,
    pub min_z: f32,
    pub max_z: f32,
    pub height: f32,
}

fn unsupported(message: &str) -> Error {
    Error::Model(format!("flat-walk slice: {message}"))
}

impl FlatLane {
    pub fn validate(&self) -> Result<(), Error> {
        if [self.min_x, self.max_x, self.min_z, self.max_z, self.height]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
            || self.min_x >= self.max_x
            || self.min_z >= self.max_z
        {
            return Err(unsupported("invalid flat-lane bounds"));
        }
        Ok(())
    }

    fn contains(&self, position: [f32; 3]) -> bool {
        position[1] == self.height
            && (self.min_x..=self.max_x).contains(&position[0])
            && (self.min_z..=self.max_z).contains(&position[2])
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Idle,
    Walking,
    Braking,
    Complete,
}

/// One sequence: up to five idle ticks, 10..=30 full-forward ticks, then
/// neutral braking through the first BRAKING_STOP observation. Construct a new
/// model at idle/rest for each run; this is not a hidden-state restore API.
#[derive(Debug, Clone)]
pub struct FlatWalk {
    lane: FlatLane,
    phase: Phase,
    idle_ticks: u8,
    walk_ticks: u8,
}

impl FlatWalk {
    pub fn new(lane: FlatLane) -> Result<Self, Error> {
        lane.validate()?;
        Ok(Self {
            lane,
            phase: Phase::Idle,
            idle_ticks: 0,
            walk_ticks: 0,
        })
    }

    pub fn complete(&self) -> bool {
        self.phase == Phase::Complete
    }

    fn advance(&mut self, state: &mut MarioState, input: ControllerInput) -> Result<(), Error> {
        state.validate()?;
        let expected_action = match self.phase {
            Phase::Idle => IDLE,
            Phase::Walking => WALKING,
            Phase::Braking => BRAKING,
            Phase::Complete => {
                return Err(unsupported(
                    "sequence complete; stop animation is unsupported",
                ));
            }
        };
        if state.action != expected_action {
            return Err(unsupported(
                "action does not match model phase (start at idle)",
            ));
        }
        if !self.lane.contains(state.position)
            || state.face_yaw != i16::MIN
            || state.forward_velocity < 0.0
            || state.forward_velocity > 28.0
            || state.velocity != [0.0, 0.0, -state.forward_velocity]
            || (self.phase == Phase::Idle && state.forward_velocity != 0.0)
        {
            return Err(unsupported(
                "state is outside the flat, fixed -Z, grounded envelope",
            ));
        }
        if input.buttons != 0 || input.stick_x != 0 || ![0, 80].contains(&input.stick_y) {
            return Err(unsupported(
                "only neutral or raw (0,80), without buttons, is supported",
            ));
        }
        let forward = input.stick_y == 80;
        match self.phase {
            Phase::Idle if !forward => {
                if self.idle_ticks == MAX_IDLE_TICKS {
                    return Err(unsupported("idle observation budget exhausted"));
                }
                self.idle_ticks += 1;
                state.velocity = [0.0, 0.0, -0.0];
                return Ok(());
            }
            Phase::Idle => {
                // The first walking update starts from speed eight, and also
                // performs the same acceleration as subsequent walking ticks.
                state.forward_velocity = 8.0;
                self.phase = Phase::Walking;
            }
            Phase::Walking if !forward => {
                if self.walk_ticks < MIN_WALK_TICKS {
                    return Err(unsupported(
                        "low-speed release uses unsupported deceleration; hold at least ten ticks",
                    ));
                }
                self.phase = Phase::Braking;
            }
            Phase::Braking if forward => {
                return Err(unsupported("resuming during braking is unsupported"));
            }
            _ => {}
        }
        if self.phase == Phase::Walking {
            if self.walk_ticks == MAX_WALK_TICKS {
                return Err(unsupported("walking observation budget exhausted"));
            }
            self.walk_ticks += 1;
            let drag = state.forward_velocity / 43.0_f32;
            state.forward_velocity += 1.1_f32 - drag;
            state.action = WALKING;
        } else {
            state.forward_velocity = (state.forward_velocity - 4.0).max(0.0);
            state.action = BRAKING;
            if state.forward_velocity == 0.0 {
                state.action = BRAKING_STOP;
                self.phase = Phase::Complete;
            }
        }
        state.velocity = [0.0, 0.0, -state.forward_velocity];
        // Four separately rounded quarter steps, not one full displacement.
        // On an unobstructed horizontal floor only Z can change in this slice.
        let quarter = state.velocity[2] / 4.0;
        for _ in 0..4 {
            state.position[2] += quarter;
            if !self.lane.contains(state.position) {
                return Err(unsupported("quarter step leaves the verified flat lane"));
            }
        }
        state.validate()
    }
}

impl TickModel for FlatWalk {
    fn tick(
        &mut self,
        candidate: &mut MarioState,
        input: TickInput,
        _tick: u64,
    ) -> Result<(), Error> {
        // Keep both model phase and caller state unchanged on ANY failure.
        let mut next_model = self.clone();
        let mut next_state = *candidate;
        next_model.advance(&mut next_state, input.raw)?;
        *candidate = next_state;
        *self = next_model;
        Ok(())
    }
}

/// Explicit native initialization and authored inputs; never reference outputs.
/// Sampling/boundary strings identify the comparison protocol, not a save-state.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub version: u32,
    pub scenario: String,
    pub initialization: String,
    pub input_sampling: String,
    pub state_boundary: String,
    pub start_tick: u64,
    pub initial_state: MarioState,
    pub previous_input: ControllerInput,
    pub lane: FlatLane,
    pub segments: Vec<Segment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Segment {
    pub ticks: u8,
    pub input: ControllerInput,
}

impl Scenario {
    /// Runs the native model without reading a ROM, emulator or reference trace.
    /// Incomplete sequences and extra updates after the first stop are errors.
    pub fn run(&self) -> Result<Trace, Error> {
        if self.version != 1
            || self.segments.is_empty()
            || self.segments.len() > 43
            || self.segments.iter().any(|s| s.ticks == 0)
            || self
                .segments
                .iter()
                .map(|s| usize::from(s.ticks))
                .sum::<usize>()
                > 43
            || self.previous_input != ControllerInput::default()
        {
            return Err(unsupported(
                "invalid scenario version, segments or previous input",
            ));
        }
        let mut runner = Runner::with_snapshot(
            self.initial_state,
            self.start_tick,
            self.previous_input,
            FlatWalk::new(self.lane)?,
        )?;
        let mut frames = Vec::new();
        for segment in &self.segments {
            for _ in 0..segment.ticks {
                let tick = runner.tick();
                runner.step(segment.input)?;
                frames.push(Frame {
                    tick,
                    input: segment.input,
                    state: *runner.state(),
                });
            }
        }
        if !runner.into_model().complete() {
            return Err(unsupported(
                "scenario must finish at the first braking stop",
            ));
        }
        let trace = Trace {
            version: crate::reference::VERSION,
            manifest: Manifest {
                rom_sha256: sm64_rom::US_SHA256.to_owned(),
                source: Source::Native,
                scenario: self.scenario.clone(),
                producer: "sm64-rs bounded flat-walk model".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                settings: format!(
                    "Caller-asserted flat lane {:?}; fixed -Z; raw neutral/(0,80); no buttons; max 5 idle ticks, 10..=30 walking ticks; low-speed release and stop animation not simulated",
                    self.lane
                ),
                initialization: self.initialization.clone(),
                input_sampling: self.input_sampling.clone(),
                state_boundary: self.state_boundary.clone(),
            },
            start_tick: self.start_tick,
            initial_state: self.initial_state,
            previous_input: self.previous_input,
            frames,
        };
        trace.validate().map_err(|e| unsupported(&e.to_string()))?;
        Ok(trace)
    }
}
