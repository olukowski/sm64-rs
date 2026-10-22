//! Diagnostic collision geometry, not original game render geometry.
use anyhow::{Result, bail};
use bevy::{asset::RenderAssetUsages, mesh::PrimitiveTopology, prelude::*};
use sm64_level::CollisionMesh;

/// Preserve SM64's X/Y/Z axes (Y up), expressing 100 game units as one viewer unit.
pub const SCALE: f32 = 0.01;

pub struct Converted {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub min: Vec3,
    pub max: Vec3,
    pub skipped: usize,
}

impl Converted {
    pub fn into_mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::default(),
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
    }
}

pub fn convert(source: &CollisionMesh) -> Result<Converted> {
    let mut result = Converted {
        positions: Vec::new(),
        normals: Vec::new(),
        colors: Vec::new(),
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
        skipped: 0,
    };
    for (number, triangle) in source.triangles.iter().enumerate() {
        let mut points = [Vec3::ZERO; 3];
        for (point, index) in points.iter_mut().zip(triangle.indices) {
            let Some(v) = source.vertices.get(usize::from(index)) else {
                bail!("triangle {number} references missing vertex {index}");
            };
            *point = Vec3::new(f32::from(v[0]), f32::from(v[1]), f32::from(v[2])) * SCALE;
        }
        let cross = (points[1] - points[0]).cross(points[2] - points[0]);
        if cross.length_squared() < 1e-12 {
            result.skipped += 1;
            continue;
        }
        let normal = cross.normalize();
        // Neutral surfaces encode orientation; special surface IDs get a stable tint.
        // These colors deliberately make no claim about original textures/materials.
        let base = if triangle.surface == 0 {
            Vec3::new(0.48, 0.68, 0.46)
        } else {
            let hash = u32::from(triangle.surface).wrapping_mul(2654435761);
            Vec3::new(
                0.35 + (hash & 255) as f32 / 510.,
                0.35 + ((hash >> 8) & 255) as f32 / 510.,
                0.35 + ((hash >> 16) & 255) as f32 / 510.,
            )
        };
        let shade = 0.45 + 0.55 * normal.dot(Vec3::new(0.3, 0.85, 0.43).normalize()).abs();
        let color = (base * shade).extend(1.0).to_array();
        for point in points {
            result.min = result.min.min(point);
            result.max = result.max.max(point);
            result.positions.push(point.to_array());
            result.normals.push(normal.to_array());
            result.colors.push(color);
        }
    }
    if result.positions.is_empty() {
        bail!("collision contains no nondegenerate triangles");
    }
    Ok(result)
}

/// Authored showcase location on BOB's upper flat field. This checks static
/// geometry for presentation only; it is NOT the original game's floor solver.
pub fn showcase_floor(source: &CollisionMesh) -> Result<Vec3> {
    flat_floor_at(source, -4500.0, 0.0)
}

fn flat_floor_at(source: &CollisionMesh, x: f32, z: f32) -> Result<Vec3> {
    let floor = |x: f32, z: f32| -> Option<f32> {
        source
            .triangles
            .iter()
            .filter_map(|triangle| {
                let [a, b, c] = triangle
                    .indices
                    .map(|i| source.vertices.get(usize::from(i)).copied())
                    .map(|v| v.map(|p| p.map(f32::from)));
                let (a, b, c) = (a?, b?, c?);
                if a[1] != b[1] || b[1] != c[1] {
                    return None;
                }
                let cross = |a: [f32; 3], b: [f32; 3], x: f32, z: f32| {
                    (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0])
                };
                // Upward winding in Y-up space has negative XZ signed area.
                if cross(a, b, c[0], c[2]) >= 0.0 {
                    return None;
                }
                let signs = [cross(a, b, x, z), cross(b, c, x, z), cross(c, a, x, z)];
                signs.iter().all(|v| *v <= 0.0).then_some(a[1])
            })
            .max_by(f32::total_cmp)
    };
    let y = floor(x, z)
        .ok_or_else(|| anyhow::anyhow!("showcase location has no horizontal terrain"))?;
    for (dx, dz) in [(-40., -40.), (-40., 40.), (40., -40.), (40., 40.)] {
        if floor(x + dx, z + dz) != Some(y) {
            bail!("Mario footprint is not on one flat plateau");
        }
    }
    Ok(Vec3::new(x, y, z))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm64_level::Triangle;
    fn fixture() -> CollisionMesh {
        CollisionMesh {
            vertices: vec![[0, 0, 0], [0, 0, 100], [100, 0, 0], [30000, 30000, 30000]],
            triangles: vec![Triangle {
                indices: [0, 1, 2],
                surface: 0,
                force: None,
            }],
            consumed: 0,
        }
    }
    #[test]
    fn preserves_axes_scale_bounds_and_flat_normals() {
        let data = convert(&fixture()).unwrap();
        assert_eq!(data.min, Vec3::ZERO);
        assert_eq!(data.max, Vec3::new(1., 0., 1.)); // unused vertex excluded
        assert_eq!(
            data.positions,
            vec![[0., 0., 0.], [0., 0., 1.], [1., 0., 0.]]
        );
        assert_eq!(data.normals, vec![[0., 1., 0.]; 3]);
        assert_eq!(data.colors.len(), 3);
    }
    #[test]
    fn showcase_grounding_requires_a_complete_horizontal_footprint() {
        let mut source = CollisionMesh {
            vertices: vec![
                [-6000, 25, 5000],
                [-6000, 25, 6000],
                [-5000, 25, 5000],
                [-5000, 25, 6000],
            ],
            triangles: vec![
                Triangle {
                    indices: [0, 1, 2],
                    surface: 0,
                    force: None,
                },
                Triangle {
                    indices: [2, 1, 3],
                    surface: 0,
                    force: None,
                },
            ],
            consumed: 0,
        };
        assert_eq!(
            flat_floor_at(&source, -5500., 5500.).unwrap(),
            Vec3::new(-5500., 25., 5500.)
        );
        source.triangles.pop();
        assert!(flat_floor_at(&source, -5500., 5500.).is_err());
        source.vertices[0][1] = 26;
        assert!(flat_floor_at(&source, -5500., 5500.).is_err());
    }

    #[test]
    fn invalid_index_is_an_error() {
        let mut data = fixture();
        data.triangles[0].indices[0] = 4;
        assert!(convert(&data).is_err());
    }
    #[test]
    fn skips_degenerates_but_rejects_empty_output() {
        let mut data = fixture();
        let mut degenerate = data.triangles[0].clone();
        degenerate.indices = [0, 0, 1];
        data.triangles.push(degenerate);
        assert_eq!(convert(&data).unwrap().skipped, 1);
        data.triangles.remove(0);
        assert!(convert(&data).is_err());
    }
}
