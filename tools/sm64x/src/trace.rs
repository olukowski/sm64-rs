//! Read-only trace tools; no ROM, emulator, or gameplay model is needed.
use std::{
    fs::File,
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use serde::Serialize;
use sm64_sim::reference::{self, Difference, FloatPolicy, Source, Trace};

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Validate a versioned observation trace (does not authenticate the capture).
    Validate { path: PathBuf },
    /// Report first differing input/state field; nonzero exit on divergence.
    Compare {
        expected: PathBuf,
        actual: PathBuf,
        /// Compare floats within this finite nonnegative absolute error instead of exact f32 bits.
        #[arg(long, value_parser = tolerance)]
        absolute_tolerance: Option<f64>,
    },
}

fn tolerance(value: &str) -> std::result::Result<f64, String> {
    let value = value
        .parse::<f64>()
        .map_err(|_| "expected a numeric tolerance".to_string())?;
    if !value.is_finite() || value < 0.0 {
        return Err("tolerance must be finite and nonnegative".into());
    }
    Ok(value)
}

fn read(path: &Path) -> Result<Trace> {
    let file = File::open(path).with_context(|| format!("opening trace {}", path.display()))?;
    Trace::read(file).with_context(|| format!("reading trace {}", path.display()))
}

#[derive(Serialize)]
struct Validated<'a> {
    status: &'static str,
    manifest: &'a reference::Manifest,
    start_tick: u64,
    frames: usize,
    limitations: &'static str,
}

#[derive(Serialize)]
struct Comparison {
    status: &'static str,
    float_policy: &'static str,
    absolute_tolerance: Option<f64>,
    expected_source: Source,
    actual_source: Source,
    expected_frames: usize,
    actual_frames: usize,
    difference: Option<Difference>,
    limitations: &'static str,
}

pub fn run(command: Command, json: bool) -> Result<()> {
    match command {
        Command::Validate { path } => {
            let trace = read(&path)?;
            let report = Validated {
                status: "valid_observation_trace",
                manifest: &trace.manifest,
                start_tick: trace.start_tick,
                frames: trace.frames.len(),
                limitations: "Schema/identity assertions validated; capture provenance, initialization and timing are not authenticated. This is not a save-state.",
            };
            if json {
                return crate::print_json(&report);
            }
            let mut stdout = std::io::stdout().lock();
            writeln!(
                stdout,
                "Valid observation trace: {} frames from tick {}",
                report.frames, report.start_tick
            )?;
            writeln!(
                stdout,
                "Source: {:?}; scenario: {:?}",
                trace.manifest.source, trace.manifest.scenario
            )?;
            writeln!(stdout, "Note: {}", report.limitations)?;
        }
        Command::Compare {
            expected,
            actual,
            absolute_tolerance,
        } => {
            let expected = read(&expected)?;
            let actual = read(&actual)?;
            let policy = absolute_tolerance.map_or(FloatPolicy::Exact, FloatPolicy::Absolute);
            let difference = reference::compare(&expected, &actual, policy)?;
            let diverged = difference.is_some();
            let report = Comparison {
                status: if diverged {
                    "diverged"
                } else {
                    "equal_observations"
                },
                float_policy: if absolute_tolerance.is_some() {
                    "absolute_tolerance"
                } else {
                    "exact_f32_bits"
                },
                absolute_tolerance,
                expected_source: expected.manifest.source,
                actual_source: actual.manifest.source,
                expected_frames: expected.frames.len(),
                actual_frames: actual.frames.len(),
                difference,
                limitations: "Agreement covers recorded observations only, not hidden state, capture correctness or authentic gameplay; review both manifests/settings separately.",
            };
            if json {
                crate::print_json(&report)?;
            } else {
                let mut stdout = std::io::stdout().lock();
                writeln!(
                    stdout,
                    "{} ({}, tolerance {:?})",
                    report.status, report.float_policy, report.absolute_tolerance
                )?;
                if let Some(diff) = &report.difference {
                    writeln!(stdout, "Tick {}: {}", diff.tick, diff.field)?;
                    writeln!(
                        stdout,
                        "  expected: {}\n  actual:   {}",
                        diff.expected, diff.actual
                    )?;
                }
                writeln!(stdout, "Note: {}", report.limitations)?;
            }
            if diverged {
                bail!("trace observations diverged");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tolerances_are_explicit_finite_nonnegative_numbers() {
        assert_eq!(tolerance("0").unwrap(), 0.0);
        assert_eq!(tolerance("0.01").unwrap(), 0.01);
        for bad in ["-0.1", "NaN", "inf", "-inf", "garbage"] {
            assert!(tolerance(bad).is_err());
        }
    }
}
