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
    #[serde(skip_serializing_if = "Option::is_none")]
    visuals: Option<VisualSummary>,
}

#[derive(Serialize)]
struct DrawSummary {
    address: u32,
    layer: u8,
    batches: usize,
    triangles: usize,
    textures: Vec<[u32; 2]>,
}

#[derive(Serialize)]
struct VisualSummary {
    geo_commands: usize,
    display_list_commands: usize,
    draws: Vec<DrawSummary>,
    triangles: usize,
    textured_triangles: usize,
    texture_bindings: usize,
    diagnostics: Vec<String>,
}

fn visual_summary(level: &Level, address: u32) -> Result<VisualSummary> {
    let geo = sm64_level::geo::inspect_geo(&level.segment_data, address)?;
    let mut result = VisualSummary {
        geo_commands: geo.commands_inspected,
        display_list_commands: 0,
        draws: Vec::new(),
        triangles: 0,
        textured_triangles: 0,
        texture_bindings: 0,
        diagnostics: geo.diagnostics,
    };
    let mut diagnostics = std::collections::BTreeSet::new();
    for draw in geo.draws {
        let remaining = 1_000_000usize
            .checked_sub(result.geo_commands + result.display_list_commands)
            .ok_or_else(|| anyhow::anyhow!("visual report command budget exhausted"))?;
        let decoded = sm64_gfx::decode_display_list_with_budget(
            &level.segment_data,
            draw.display_list,
            remaining,
        )?;
        let triangles = decoded
            .batches
            .iter()
            .map(|batch| batch.vertices.len() / 3)
            .sum::<usize>();
        result.triangles += triangles;
        if result.triangles > 1_000_000 {
            anyhow::bail!("visual report triangle budget exhausted");
        }
        result.textured_triangles += decoded
            .batches
            .iter()
            .filter(|batch| batch.texture.is_some())
            .map(|batch| batch.vertices.len() / 3)
            .sum::<usize>();
        result.display_list_commands += decoded.commands_inspected;
        result.texture_bindings += decoded.textures.len();
        diagnostics.extend(decoded.diagnostics);
        result.draws.push(DrawSummary {
            address: draw.display_list,
            layer: draw.layer,
            batches: decoded.batches.len(),
            triangles,
            textures: decoded
                .textures
                .iter()
                .map(|texture| [texture.width, texture.height])
                .collect(),
        });
    }
    result.diagnostics.extend(diagnostics);
    Ok(result)
}

#[derive(Serialize)]
struct Summary<'a> {
    name: &'a str,
    commands_inspected: usize,
    model_bindings: usize,
    segments: &'a [SegmentInfo],
    areas: Vec<AreaSummary>,
    diagnostics: &'a [String],
}

pub fn report(level: &Level, json: bool, visuals: bool) -> Result<()> {
    let areas: Vec<_> = level
        .areas
        .iter()
        .map(|area| -> Result<AreaSummary> {
            let mut surfaces = BTreeMap::new();
            if let Some(mesh) = &area.collision {
                for triangle in &mesh.triangles {
                    *surfaces
                        .entry(format!("0x{:04x}", triangle.surface))
                        .or_default() += 1;
                }
            }
            Ok(AreaSummary {
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
                visuals: if visuals {
                    Some(visual_summary(level, area.geometry_address)?)
                } else {
                    None
                },
            })
        })
        .collect::<Result<_>>()?;
    let summary = Summary {
        name: &level.name,
        commands_inspected: level.commands_inspected,
        model_bindings: level.models.len(),
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
        if let Some(visuals) = area.visuals {
            writeln!(
                stdout,
                "  {} geo draws, {} render triangles ({} textured), {} texture bindings",
                visuals.draws.len(),
                visuals.triangles,
                visuals.textured_triangles,
                visuals.texture_bindings
            )?;
            for draw in visuals.draws {
                writeln!(
                    stdout,
                    "  draw 0x{:08x}, layer {}: {} batches, {} triangles, textures {:?}",
                    draw.address, draw.layer, draw.batches, draw.triangles, draw.textures
                )?;
            }
            for diagnostic in visuals.diagnostics {
                writeln!(stdout, "Visual note: {diagnostic}")?;
            }
        }
        for (surface, count) in area.surfaces {
            writeln!(stdout, "  surface {surface}: {count} triangles")?;
        }
    }
    for diagnostic in summary.diagnostics {
        writeln!(stdout, "Note: {diagnostic}")?;
    }
    Ok(())
}
