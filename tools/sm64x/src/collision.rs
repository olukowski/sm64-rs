//! Static geometric probes, explicitly separate from original-game collision.
use std::io::Write;

use anyhow::{Context, Result};
use clap::Args;
use serde::Serialize;
use sm64_sim::collision::{Kind, VerticalHits, WallHit, World};

#[derive(Debug, Args)]
pub struct Options {
    #[arg(value_parser = ["bob"])]
    course: String,
    #[arg(long, default_value_t = 1)]
    area: u8,
    /// X,Y,Z in original units; Y is the wall probe's horizontal slice.
    #[arg(long, value_parser = position, allow_hyphen_values = true)]
    at: [f64; 3],
    /// Inclusive lower height bound for both floor and ceiling searches.
    #[arg(long, default_value_t = -32768.0, allow_hyphen_values = true)]
    min_y: f64,
    /// Inclusive upper height bound; returned floor need not be below --at Y.
    #[arg(long, default_value_t = 32767.0, allow_hyphen_values = true)]
    max_y: f64,
    /// Two-sided XZ distance at --at Y; no wall push or actor collision.
    #[arg(long, default_value_t = 50.0)]
    radius: f64,
    /// Diagnostic classification policy, not verified original-game behavior.
    #[arg(long, default_value_t = 0.01)]
    vertical_threshold: f64,
}

fn position(value: &str) -> std::result::Result<[f64; 3], String> {
    let values = value
        .split(',')
        .map(str::parse::<f64>)
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|_| "expected finite x,y,z coordinates".to_string())?;
    let result: [f64; 3] = values
        .try_into()
        .map_err(|_| "expected exactly three coordinates".to_string())?;
    if result
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.0)
    {
        return Err("coordinates must be finite and within +/-1,000,000".into());
    }
    Ok(result)
}

#[derive(Serialize)]
struct Report {
    status: &'static str,
    canonical_rom_sha256: String,
    course: String,
    area: u8,
    at: [f64; 3],
    height_interval: [f64; 2],
    wall_radius: f64,
    vertical_threshold: f64,
    /// Floor, ceiling, wall counts before querying.
    surface_counts: [usize; 3],
    skipped_degenerate: usize,
    vertical: VerticalHits,
    walls: Vec<WallHit>,
    limitations: &'static str,
}

pub fn run(rom: &sm64_rom::Rom, options: Options, json: bool) -> Result<()> {
    let level = sm64_level::load_bob(rom)?;
    let area = level
        .areas
        .iter()
        .find(|area| area.index == options.area)
        .context("requested area does not exist")?;
    let mesh = area
        .collision
        .as_ref()
        .context("requested area has no static collision")?;
    let world = World::from_mesh(mesh, options.vertical_threshold)?;
    let mut surface_counts = [0; 3];
    for surface in world.surfaces() {
        surface_counts[match surface.kind {
            Kind::Floor => 0,
            Kind::Ceiling => 1,
            Kind::Wall => 2,
        }] += 1;
    }
    let report = Report {
        status: "diagnostic_geometry_not_original_collision",
        canonical_rom_sha256: rom.identity().sha256.clone(),
        course: level.name,
        area: options.area,
        at: options.at,
        height_interval: [options.min_y, options.max_y],
        wall_radius: options.radius,
        vertical_threshold: options.vertical_threshold,
        surface_counts,
        skipped_degenerate: world.skipped_degenerate(),
        vertical: world.vertical(options.at[0], options.at[2], options.min_y, options.max_y)?,
        walls: world.walls(options.at, options.radius)?,
        limitations: "All surface IDs; inclusive edges; source-order ties; f64 geometry; no original spatial ordering, quantization, exclusions, tolerances, or wall resolution.",
    };
    if json {
        return crate::print_json(&report);
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(
        stdout,
        "{} area {} — DIAGNOSTIC GEOMETRY, not original collision",
        report.course, report.area
    )?;
    writeln!(
        stdout,
        "Surfaces [floor, ceiling, wall]: {:?}; {} degenerate skipped",
        report.surface_counts, report.skipped_degenerate
    )?;
    writeln!(
        stdout,
        "Height interval: {:?}; X/Z: {}, {}",
        report.height_interval, report.at[0], report.at[2]
    )?;
    for (label, hit) in [
        ("Highest floor", &report.vertical.floor),
        ("Lowest ceiling", &report.vertical.ceiling),
    ] {
        match hit {
            Some(hit) => writeln!(
                stdout,
                "{label}: y={} triangle={} surface=0x{:04x} normal={:?}",
                hit.height, hit.triangle, hit.surface, hit.normal
            )?,
            None => writeln!(stdout, "{label}: none in interval")?,
        }
    }
    writeln!(
        stdout,
        "Wall candidates at Y={}, radius={}: {}",
        report.at[1],
        report.wall_radius,
        report.walls.len()
    )?;
    for hit in report.walls {
        writeln!(
            stdout,
            "  triangle={} surface=0x{:04x} distance={} point={:?}",
            hit.triangle, hit.surface, hit.distance, hit.point
        )?;
    }
    writeln!(stdout, "Note: {}", report.limitations)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positions_require_bounded_finite_triples() {
        assert_eq!(position("-1,2,3").unwrap(), [-1.0, 2.0, 3.0]);
        for value in ["", "0,1", "0,1,2,3", "NaN,0,0", "inf,0,0", "0,0,1000001"] {
            assert!(position(value).is_err());
        }
    }
}
