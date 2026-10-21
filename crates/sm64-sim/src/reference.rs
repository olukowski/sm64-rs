//! Versioned observations, not save-states or an implementation of gameplay.
//! All examples/tests must declare their provenance; authored data is synthetic.

use std::io::{Read, Write};

use serde::{Deserialize, Serialize};

use crate::input::ControllerInput;
use crate::{MarioState, Runner, TickModel};

pub const VERSION: u32 = 1;
pub const MAX_FRAMES: usize = 100_000;
pub const MAX_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum Source {
    Reference,
    Native,
    Authored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub rom_sha256: String,
    pub source: Source,
    pub scenario: String,
    pub producer: String,
    pub version: String,
    pub settings: String,
    pub initialization: String,
    pub input_sampling: String,
    pub state_boundary: String,
}

/// Input at tick N drives the pre-state to this frame's post-tick state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Frame {
    pub tick: u64,
    pub input: ControllerInput,
    pub state: MarioState,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trace {
    pub version: u32,
    pub manifest: Manifest,
    /// The next transition, not an already completed transition.
    pub start_tick: u64,
    /// An observation only: does not contain all state needed by a future model.
    pub initial_state: MarioState,
    pub previous_input: ControllerInput,
    pub frames: Vec<Frame>,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("trace exceeds {MAX_BYTES} bytes")]
    TooLarge,
    #[error("invalid trace: {0}")]
    Invalid(String),
    #[error("absolute tolerance must be finite and nonnegative")]
    InvalidTolerance,
    #[error("incompatible protocol field {field}: expected {expected}, actual {actual}")]
    Incompatible {
        field: &'static str,
        expected: String,
        actual: String,
    },
    #[error("model failed at tick {tick}: {source}")]
    Model {
        tick: u64,
        #[source]
        source: crate::Error,
    },
}

impl Trace {
    pub fn validate(&self) -> Result<(), Error> {
        let invalid = |message: &str| Error::Invalid(message.to_owned());
        if self.version != VERSION {
            return Err(invalid("unsupported format version"));
        }
        if self.manifest.rom_sha256 != sm64_rom::US_SHA256 {
            return Err(invalid(
                "ROM SHA-256 must be the canonical US revision 0 identity",
            ));
        }
        for (field, value) in [
            ("scenario", &self.manifest.scenario),
            ("producer", &self.manifest.producer),
            ("version", &self.manifest.version),
            ("settings", &self.manifest.settings),
            ("initialization", &self.manifest.initialization),
            ("input_sampling", &self.manifest.input_sampling),
            ("state_boundary", &self.manifest.state_boundary),
        ] {
            if value.trim().is_empty() {
                return Err(Error::Invalid(format!("manifest.{field} is blank")));
            }
        }
        if self.frames.is_empty() || self.frames.len() > MAX_FRAMES {
            return Err(invalid("frame count must be 1..=100000"));
        }
        // Reserve the next tick too: Runner must be able to commit the last step.
        self.start_tick
            .checked_add(self.frames.len() as u64)
            .ok_or_else(|| invalid("tick counter overflow"))?;
        self.initial_state
            .validate()
            .map_err(|e| Error::Invalid(format!("initial_state: {e}")))?;
        for (index, frame) in self.frames.iter().enumerate() {
            if frame.tick != self.start_tick + index as u64 {
                return Err(Error::Invalid(format!(
                    "nonconsecutive tick at frame {index}"
                )));
            }
            frame
                .state
                .validate()
                .map_err(|e| Error::Invalid(format!("state at tick {}: {e}", frame.tick)))?;
        }
        Ok(())
    }

    /// Reads no more than the byte cap plus one sentinel byte before decoding.
    pub fn read(reader: impl Read) -> Result<Self, Error> {
        let mut bytes = Vec::new();
        reader.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes)?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        let trace: Self = serde_json::from_slice(&bytes)?;
        trace.validate()?;
        Ok(trace)
    }

    /// Validation (including encoded size) precedes any output. I/O failures may
    /// still leave partial output; path ownership/atomic replacement is the caller's job.
    pub fn write(&self, mut writer: impl Write) -> Result<(), Error> {
        self.validate()?;
        let bytes = serde_json::to_vec(self)?;
        if bytes.len() > MAX_BYTES {
            return Err(Error::TooLarge);
        }
        writer.write_all(&bytes)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum FloatPolicy {
    #[default]
    Exact,
    Absolute(f64),
}

impl FloatPolicy {
    fn validate(self) -> Result<(), Error> {
        match self {
            Self::Absolute(value) if !value.is_finite() || value < 0.0 => {
                Err(Error::InvalidTolerance)
            }
            _ => Ok(()),
        }
    }

    fn equal(self, expected: f32, actual: f32) -> bool {
        match self {
            Self::Exact => expected.to_bits() == actual.to_bits(),
            Self::Absolute(tolerance) => {
                (f64::from(expected) - f64::from(actual)).abs() <= tolerance
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Difference {
    pub tick: u64,
    pub field: String,
    pub expected: String,
    pub actual: String,
}

fn difference(
    tick: u64,
    field: impl Into<String>,
    expected: impl ToString,
    actual: impl ToString,
) -> Difference {
    Difference {
        tick,
        field: field.into(),
        expected: expected.to_string(),
        actual: actual.to_string(),
    }
}

fn input_difference(
    tick: u64,
    prefix: &str,
    expected: ControllerInput,
    actual: ControllerInput,
) -> Option<Difference> {
    for (name, e, a) in [
        (
            "buttons",
            i32::from(expected.buttons),
            i32::from(actual.buttons),
        ),
        (
            "stick_x",
            i32::from(expected.stick_x),
            i32::from(actual.stick_x),
        ),
        (
            "stick_y",
            i32::from(expected.stick_y),
            i32::from(actual.stick_y),
        ),
    ] {
        if e != a {
            return Some(difference(tick, format!("{prefix}.{name}"), e, a));
        }
    }
    None
}

fn state_difference(
    tick: u64,
    prefix: &str,
    expected: &MarioState,
    actual: &MarioState,
    policy: FloatPolicy,
) -> Option<Difference> {
    let floats = expected
        .position
        .iter()
        .zip(&actual.position)
        .enumerate()
        .map(|(i, (e, a))| (format!("position[{i}]"), *e, *a))
        .chain(
            expected
                .velocity
                .iter()
                .zip(&actual.velocity)
                .enumerate()
                .map(|(i, (e, a))| (format!("velocity[{i}]"), *e, *a)),
        )
        .chain(std::iter::once((
            "forward_velocity".into(),
            expected.forward_velocity,
            actual.forward_velocity,
        )));
    for (name, e, a) in floats {
        if !policy.equal(e, a) {
            // Include bits so exact differences (especially signed zero) are unambiguous.
            return Some(difference(
                tick,
                format!("{prefix}.{name}"),
                format!("{e} (0x{:08x})", e.to_bits()),
                format!("{a} (0x{:08x})", a.to_bits()),
            ));
        }
    }
    if expected.face_yaw != actual.face_yaw {
        return Some(difference(
            tick,
            format!("{prefix}.face_yaw"),
            expected.face_yaw,
            actual.face_yaw,
        ));
    }
    if expected.action != actual.action {
        return Some(difference(
            tick,
            format!("{prefix}.action"),
            expected.action,
            actual.action,
        ));
    }
    None
}

/// Validates both complete traces before reporting the first observation/input
/// difference. Provenance and implementation settings remain available for review,
/// but do not have to match across implementations.
pub fn compare(
    expected: &Trace,
    actual: &Trace,
    policy: FloatPolicy,
) -> Result<Option<Difference>, Error> {
    expected.validate()?;
    actual.validate()?;
    policy.validate()?;
    for (field, e, a) in [
        (
            "rom_sha256",
            &expected.manifest.rom_sha256,
            &actual.manifest.rom_sha256,
        ),
        (
            "scenario",
            &expected.manifest.scenario,
            &actual.manifest.scenario,
        ),
        (
            "input_sampling",
            &expected.manifest.input_sampling,
            &actual.manifest.input_sampling,
        ),
        (
            "state_boundary",
            &expected.manifest.state_boundary,
            &actual.manifest.state_boundary,
        ),
    ] {
        if e != a {
            return Err(Error::Incompatible {
                field,
                expected: e.clone(),
                actual: a.clone(),
            });
        }
    }
    if expected.start_tick != actual.start_tick {
        return Err(Error::Incompatible {
            field: "start_tick",
            expected: expected.start_tick.to_string(),
            actual: actual.start_tick.to_string(),
        });
    }
    let tick = expected.start_tick;
    if let Some(diff) = input_difference(
        tick,
        "previous_input",
        expected.previous_input,
        actual.previous_input,
    )
    .or_else(|| {
        state_difference(
            tick,
            "initial_state",
            &expected.initial_state,
            &actual.initial_state,
            policy,
        )
    }) {
        return Ok(Some(diff));
    }
    for (e, a) in expected.frames.iter().zip(&actual.frames) {
        if let Some(diff) = input_difference(e.tick, "input", e.input, a.input)
            .or_else(|| state_difference(e.tick, "state", &e.state, &a.state, policy))
        {
            return Ok(Some(diff));
        }
    }
    if expected.frames.len() != actual.frames.len() {
        return Ok(Some(difference(
            tick + expected.frames.len().min(actual.frames.len()) as u64,
            "frames.length",
            expected.frames.len(),
            actual.frames.len(),
        )));
    }
    Ok(None)
}

/// Replays caller-supplied transitions, not built-in gameplay. `MarioState` is
/// only an observation: the caller MUST initialize the model's hidden state,
/// world, and settings according to the manifest before calling this helper.
/// A successful replay proves agreement only for the supplied observations.
pub fn replay<M: TickModel>(
    reference: &Trace,
    model: M,
    policy: FloatPolicy,
) -> Result<Option<Difference>, Error> {
    reference.validate()?;
    policy.validate()?;
    let mut runner = Runner::with_snapshot(
        reference.initial_state,
        reference.start_tick,
        reference.previous_input,
        model,
    )
    .map_err(|source| Error::Model {
        tick: reference.start_tick,
        source,
    })?;
    for frame in &reference.frames {
        runner.step(frame.input).map_err(|source| Error::Model {
            tick: frame.tick,
            source,
        })?;
        if let Some(diff) =
            state_difference(frame.tick, "state", &frame.state, runner.state(), policy)
        {
            return Ok(Some(diff));
        }
    }
    Ok(None)
}
