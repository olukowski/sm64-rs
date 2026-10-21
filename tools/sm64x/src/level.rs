//! Course metadata summaries deliberately exclude full mesh/asset dumps.
use std::collections::BTreeMap;
use std::io::Write;

use anyhow::Result;
use serde::Serialize;
use sm64_level::{Level, SegmentInfo};

#[derive(Serialize)]
struct AreaSummary {
    index: u8,
    geometry_address: u32,
    collision_address: Option<u32>,
    object_declarations: usize,
    vertices: usize,
    triangles: usize,
    collision_bytes_through_triangle_stop: usize,
    surfaces: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct Summary<'a> {
    name: &'a str,
    commands_inspected: usize,
    segments: &'a [SegmentInfo],
    areas: Vec<AreaSummary>,
    diagnostics: &'a [String],
}

pub fn report(level: &Level, json: bool) -> Result<()> {
    let areas: Vec<_> = level
        .areas
        .iter()
        .map(|area| {
            let mut surfaces = BTreeMap::new();
            if let Some(mesh) = &area.collision {
                for triangle in &mesh.triangles {
                    *surfaces
                        .entry(format!("0x{:04x}", triangle.surface))
                        .or_default() += 1;
                }
            }
            AreaSummary {
                index: area.index,
                geometry_address: area.geometry_address,
                collision_address: area.collision_address,
                object_declarations: area.objects.len(),
                vertices: area
                    .collision
                    .as_ref()
                    .map_or(0, |mesh| mesh.vertices.len()),
                triangles: area
                    .collision
                    .as_ref()
                    .map_or(0, |mesh| mesh.triangles.len()),
                collision_bytes_through_triangle_stop: area
                    .collision
                    .as_ref()
                    .map_or(0, |mesh| mesh.consumed),
                surfaces,
            }
        })
        .collect();
    let summary = Summary {
        name: &level.name,
        commands_inspected: level.commands_inspected,
        segments: &level.segments,
        areas,
        diagnostics: &level.diagnostics,
    };
    if json {
        return crate::print_json(&summary);
    }
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{} — static course inspection", summary.name)?;
    writeln!(
        stdout,
        "{} instructions inspected, {} segment loads",
        summary.commands_inspected,
        summary.segments.len()
    )?;
    for segment in summary.segments {
        writeln!(
            stdout,
            "  segment {:02x}: ROM 0x{:08x}..0x{:08x}, {} bytes ({})",
            segment.segment,
            segment.rom_start,
            segment.rom_end,
            segment.decoded_size,
            if segment.compressed {
                "MIO0 decoded"
            } else {
                "raw"
            }
        )?;
    }
    for area in summary.areas {
        writeln!(
            stdout,
            "Area {}: geometry 0x{:08x}, collision {:?}",
            area.index,
            area.geometry_address,
            area.collision_address
                .map(|address| format!("0x{address:08x}"))
        )?;
        writeln!(
            stdout,
            "  {} vertices, {} triangles, {} object declarations",
            area.vertices, area.triangles, area.object_declarations
        )?;
        for (surface, count) in area.surfaces {
            writeln!(stdout, "  surface {surface}: {count} triangles")?;
        }
    }
    for diagnostic in summary.diagnostics {
        writeln!(stdout, "Note: {diagnostic}")?;
    }
    Ok(())
}
