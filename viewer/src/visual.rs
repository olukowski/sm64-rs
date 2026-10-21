//! Converts decoded original render geometry, never collision triangles, to Bevy.
use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    mesh::PrimitiveTopology,
    prelude::{AlphaMode, Image, Mat4, Mesh, Vec3, default},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};
use sm64_gfx::{AlphaMode as N64Alpha, Batch, Texture, WrapMode};
use sm64_level::{Area, Level};

use crate::mesh::SCALE;

pub struct VisualBatch {
    pub mesh: Mesh,
    pub texture: Option<usize>,
    pub alpha: AlphaMode,
    pub cull_back: bool,
    pub layer: u8,
}

pub struct VisualScene {
    pub batches: Vec<VisualBatch>,
    pub textures: Vec<Texture>,
    pub min: Vec3,
    pub max: Vec3,
    pub triangles: usize,
    pub textured_triangles: usize,
    pub draws: usize,
    pub commands: usize,
    pub diagnostics: Vec<String>,
}

fn same_texture(a: &Texture, b: &Texture) -> bool {
    a.width == b.width
        && a.height == b.height
        && a.wrap_s == b.wrap_s
        && a.wrap_t == b.wrap_t
        && a.rgba8 == b.rgba8
}

pub fn load(level: &Level, area: &Area) -> Result<VisualScene> {
    let geo = sm64_level::geo::inspect_geo(&level.segment_data, area.geometry_address)?;
    let mut result = VisualScene {
        batches: Vec::new(),
        textures: Vec::new(),
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
        triangles: 0,
        textured_triangles: 0,
        draws: geo.draws.len(),
        commands: geo.commands_inspected,
        diagnostics: geo.diagnostics,
    };
    let mut texture_bytes = 0;
    let mut diagnostics = BTreeSet::new();
    for draw in geo.draws {
        let decoded = sm64_gfx::decode_display_list_with_budget(
            &level.segment_data,
            draw.display_list,
            1_000_000 - result.commands,
        )
        .with_context(|| {
            format!(
                "display list 0x{:08x}, layer {}",
                draw.display_list, draw.layer
            )
        })?;
        result.commands += decoded.commands_inspected;
        diagnostics.extend(decoded.diagnostics);
        let mut texture_map = Vec::new();
        for texture in decoded.textures {
            let index = if let Some(index) = result
                .textures
                .iter()
                .position(|old| same_texture(old, &texture))
            {
                index
            } else {
                texture_bytes += texture.rgba8.len();
                if result.textures.len() >= 512 || texture_bytes > 128 * 1024 * 1024 {
                    bail!("visual texture budget exhausted");
                }
                let index = result.textures.len();
                result.textures.push(texture);
                index
            };
            texture_map.push(index);
        }
        let transform = Mat4::from_cols_array_2d(&draw.transform);
        if !transform.is_finite() {
            bail!("nonfinite geometry transform");
        }
        for batch in decoded.batches {
            if batch.vertices.is_empty() {
                continue;
            }
            if !batch.vertices.len().is_multiple_of(3) {
                bail!("decoded batch is not a triangle list");
            }
            result.triangles += batch.vertices.len() / 3;
            if result.triangles > 1_000_000 {
                bail!("visual triangle budget exhausted");
            }
            if batch.texture.is_some() {
                result.textured_triangles += batch.vertices.len() / 3;
            }
            let texture = batch
                .texture
                .map(|index| {
                    texture_map
                        .get(index)
                        .copied()
                        .context("decoded batch references missing texture")
                })
                .transpose()?;
            let (mesh, min, max) = convert_batch(&batch, transform)?;
            result.min = result.min.min(min);
            result.max = result.max.max(max);
            let alpha = match draw.layer {
                4 => AlphaMode::Mask(0.5),
                5..=7 => AlphaMode::Blend,
                _ => match batch.alpha {
                    N64Alpha::Opaque => AlphaMode::Opaque,
                    N64Alpha::Mask => AlphaMode::Mask(0.5),
                    N64Alpha::Blend => AlphaMode::Blend,
                },
            };
            result.batches.push(VisualBatch {
                mesh,
                texture,
                alpha,
                cull_back: batch.cull_back,
                layer: draw.layer,
            });
        }
    }
    result.diagnostics.extend(diagnostics);
    if result.triangles == 0 {
        bail!("geometry layout produced no static render triangles");
    }
    Ok(result)
}

fn convert_batch(batch: &Batch, transform: Mat4) -> Result<(Mesh, Vec3, Vec3)> {
    let normal_transform = transform.inverse().transpose();
    if !normal_transform.is_finite() {
        bail!("singular geometry transform");
    }
    let mut positions = Vec::with_capacity(batch.vertices.len());
    let mut normals = Vec::with_capacity(batch.vertices.len());
    let mut uvs = Vec::with_capacity(batch.vertices.len());
    let mut colors = Vec::with_capacity(batch.vertices.len());
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for vertex in &batch.vertices {
        let point = transform.transform_point3(Vec3::from(vertex.position)) * SCALE;
        if !point.is_finite()
            || !vertex
                .uv
                .iter()
                .chain(vertex.color.iter())
                .all(|n| n.is_finite())
        {
            bail!("nonfinite decoded vertex");
        }
        min = min.min(point);
        max = max.max(point);
        positions.push(point.to_array());
        normals.push(
            normal_transform
                .transform_vector3(Vec3::from(vertex.normal))
                .normalize_or_zero()
                .to_array(),
        );
        uvs.push(vertex.uv);
        colors.push(vertex.color);
    }
    let mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
    .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
    .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, colors);
    Ok((mesh, min, max))
}

pub fn image(texture: Texture) -> Image {
    let mut image = Image::new(
        Extent3d {
            width: texture.width,
            height: texture.height,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        texture.rgba8,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    let wrap = |mode| match mode {
        WrapMode::Repeat => ImageAddressMode::Repeat,
        WrapMode::MirrorRepeat => ImageAddressMode::MirrorRepeat,
        WrapMode::Clamp => ImageAddressMode::ClampToEdge,
    };
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: wrap(texture.wrap_s),
        address_mode_v: wrap(texture.wrap_t),
        // Bilinear host sampling approximates the original filtered appearance;
        // it is not the RDP's three-point filter. No generated mipmaps yet.
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Nearest,
        ..default()
    });
    image
}

/// Explicit diagnostic edge overlay. Native collision remains untouched.
pub fn collision_lines(source: &sm64_level::CollisionMesh) -> Result<Mesh> {
    let converted = crate::mesh::convert(source)?;
    let mut positions = Vec::new();
    let mut normals = Vec::new();
    for (points, faces) in converted
        .positions
        .as_chunks::<3>()
        .0
        .iter()
        .zip(converted.normals.as_chunks::<3>().0)
    {
        for [a, b] in [[0, 1], [1, 2], [2, 0]] {
            for i in [a, b] {
                positions.push((Vec3::from(points[i]) + Vec3::from(faces[i]) * 0.015).to_array());
                normals.push(faces[i]);
            }
        }
    }
    Ok(
        Mesh::new(PrimitiveTopology::LineList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm64_gfx::Vertex;

    #[test]
    fn original_vertices_use_geo_transform_then_shared_game_scale() {
        let vertex = Vertex {
            position: [100., 0., 0.],
            normal: [0., 1., 0.],
            uv: [0.25, 0.5],
            color: [1.; 4],
        };
        let batch = Batch {
            vertices: vec![vertex; 3],
            texture: None,
            cull_back: true,
            alpha: N64Alpha::Opaque,
        };
        let transform = Mat4::from_translation(Vec3::new(100., 200., 300.));
        let (mesh, min, max) = convert_batch(&batch, transform).unwrap();
        assert_eq!(min, Vec3::new(2., 2., 3.));
        assert_eq!(min, max);
        assert!(mesh.attribute(Mesh::ATTRIBUTE_UV_0).is_some());
        assert!(convert_batch(&batch, Mat4::from_scale(Vec3::ZERO)).is_err());
    }

    #[test]
    fn collision_overlay_uses_lines_and_never_replaces_visual_geometry() {
        let source = sm64_level::CollisionMesh {
            vertices: vec![[0, 0, 0], [0, 0, 100], [100, 0, 0]],
            triangles: vec![sm64_level::Triangle {
                indices: [0, 1, 2],
                surface: 0,
                force: None,
            }],
            consumed: 0,
        };
        let mesh = collision_lines(&source).unwrap();
        assert_eq!(mesh.primitive_topology(), PrimitiveTopology::LineList);
        assert_eq!(mesh.count_vertices(), 6);
        assert_eq!(source.vertices[0], [0, 0, 0]);
    }

    #[test]
    fn texture_sampler_preserves_mirroring_and_clamping() {
        let image = image(Texture {
            width: 1,
            height: 1,
            rgba8: vec![255; 4],
            wrap_s: WrapMode::MirrorRepeat,
            wrap_t: WrapMode::Clamp,
        });
        let ImageSampler::Descriptor(sampler) = image.sampler else {
            panic!("custom sampler");
        };
        assert_eq!(sampler.address_mode_u, ImageAddressMode::MirrorRepeat);
        assert_eq!(sampler.address_mode_v, ImageAddressMode::ClampToEdge);
    }
}
