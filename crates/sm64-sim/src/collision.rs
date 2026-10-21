//! Static geometric probes in original units, NOT the original collision solver.
//!
//! Uses independently written plane/intersection mathematics. Classification,
//! inclusive edges and source-order ties are explicit diagnostic policies; original
//! quantization, spatial ordering, surface exclusions and wall pushes remain unverified.
use serde::Serialize;
use sm64_level::CollisionMesh;
use thiserror::Error;

type Point = [f64; 3];
const MAX_TRIANGLES: usize = 200_000;
const QUERY_LIMIT: f64 = 1_000_000.0;

#[derive(Debug, Error, PartialEq)]
pub enum Error {
    #[error("collision triangle budget exceeds {MAX_TRIANGLES}")]
    Budget,
    #[error("triangle {triangle} references missing vertex {vertex}")]
    Index { triangle: usize, vertex: u16 },
    #[error("vertical normal threshold must be finite and strictly between zero and one")]
    Threshold,
    #[error(
        "query coordinates/radius must be finite and within +/-{QUERY_LIMIT}; radius must be nonnegative"
    )]
    Query,
    #[error("vertical query minimum exceeds maximum")]
    Interval,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Floor,
    Ceiling,
    Wall,
}

#[derive(Debug, Clone, Serialize)]
pub struct Surface {
    /// Index in the source CollisionMesh, including any skipped degenerates.
    pub triangle: usize,
    pub surface: u16,
    pub force: Option<i16>,
    pub kind: Kind,
    pub normal: Point,
    #[serde(skip)]
    vertices: [Point; 3],
    /// Integer-derived plane coefficients avoid normalizing before height queries.
    #[serde(skip)]
    plane_normal: Point,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerticalHit {
    pub triangle: usize,
    pub surface: u16,
    pub force: Option<i16>,
    pub normal: Point,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct VerticalHits {
    pub floor: Option<VerticalHit>,
    pub ceiling: Option<VerticalHit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WallHit {
    pub triangle: usize,
    pub surface: u16,
    pub force: Option<i16>,
    pub normal: Point,
    /// Closest point on the triangle's horizontal slice, not a resolved position.
    pub point: Point,
    pub distance: f64,
}

#[derive(Debug)]
pub struct World {
    surfaces: Vec<Surface>,
    skipped_degenerate: usize,
}

impl World {
    /// Caller chooses classification threshold; there is deliberately no implicit
    /// original-game compatibility policy. Triangles with |normal.y| <= threshold
    /// are walls; the others are floors/ceilings according to winding.
    pub fn from_mesh(mesh: &CollisionMesh, vertical_threshold: f64) -> Result<Self, Error> {
        if !vertical_threshold.is_finite()
            || !(0.0..1.0).contains(&vertical_threshold)
            || vertical_threshold == 0.0
        {
            return Err(Error::Threshold);
        }
        if mesh.triangles.len() > MAX_TRIANGLES {
            return Err(Error::Budget);
        }
        let mut surfaces = Vec::with_capacity(mesh.triangles.len());
        let mut skipped_degenerate = 0;
        for (triangle, source) in mesh.triangles.iter().enumerate() {
            let mut vertices = [[0.0; 3]; 3];
            for (destination, &index) in vertices.iter_mut().zip(&source.indices) {
                *destination = mesh
                    .vertices
                    .get(usize::from(index))
                    .ok_or(Error::Index {
                        triangle,
                        vertex: index,
                    })?
                    .map(f64::from);
            }
            let cross = cross(sub(vertices[1], vertices[0]), sub(vertices[2], vertices[0]));
            let length = dot(cross, cross).sqrt();
            if length == 0.0 {
                skipped_degenerate += 1;
                continue;
            }
            let normal = cross.map(|value| value / length);
            let kind = if normal[1] > vertical_threshold {
                Kind::Floor
            } else if normal[1] < -vertical_threshold {
                Kind::Ceiling
            } else {
                Kind::Wall
            };
            surfaces.push(Surface {
                triangle,
                surface: source.surface,
                force: source.force,
                kind,
                normal,
                vertices,
                plane_normal: cross,
            });
        }
        Ok(Self {
            surfaces,
            skipped_degenerate,
        })
    }

    pub fn surfaces(&self) -> &[Surface] {
        &self.surfaces
    }
    pub fn skipped_degenerate(&self) -> usize {
        self.skipped_degenerate
    }

    /// Highest upward-facing triangle and lowest downward-facing triangle at X/Z
    /// within inclusive height bounds. No implicit step tolerance or floor sentinel.
    /// Equal heights retain the first source triangle. All surface IDs participate.
    pub fn vertical(&self, x: f64, z: f64, min_y: f64, max_y: f64) -> Result<VerticalHits, Error> {
        validate(&[x, z, min_y, max_y])?;
        if min_y > max_y {
            return Err(Error::Interval);
        }
        let mut result = VerticalHits {
            floor: None,
            ceiling: None,
        };
        for surface in &self.surfaces {
            if surface.kind == Kind::Wall || !contains_xz(surface.vertices, x, z) {
                continue;
            }
            let [nx, ny, nz] = surface.plane_normal;
            let origin = surface.vertices[0];
            // Keep integer-derived coefficients unnormalized. At source vertices
            // this numerator is exact in f64, including on sloped triangles; a
            // normalized plane can round a vertex outside an inclusive Y bound.
            let height = (ny * origin[1] - nx * (x - origin[0]) - nz * (z - origin[2])) / ny;
            if height < min_y || height > max_y {
                continue;
            }
            let slot = match surface.kind {
                Kind::Floor => &mut result.floor,
                Kind::Ceiling => &mut result.ceiling,
                Kind::Wall => unreachable!(),
            };
            if slot.as_ref().is_none_or(|old| match surface.kind {
                Kind::Floor => height > old.height,
                Kind::Ceiling => height < old.height,
                Kind::Wall => false,
            }) {
                *slot = Some(VerticalHit {
                    triangle: surface.triangle,
                    surface: surface.surface,
                    force: surface.force,
                    normal: surface.normal,
                    height,
                });
            }
        }
        Ok(result)
    }

    /// Two-sided wall candidates within an XZ radius at one explicit Y sample.
    /// Includes endpoints and shared edges, preserves source order, and does not
    /// push the caller, deduplicate coplanar walls, or implement Mario's cylinder.
    pub fn walls(&self, position: Point, radius: f64) -> Result<Vec<WallHit>, Error> {
        validate(&position)?;
        validate(&[radius])?;
        if radius < 0.0 {
            return Err(Error::Query);
        }
        let mut result = Vec::new();
        for surface in &self.surfaces {
            if surface.kind != Kind::Wall {
                continue;
            }
            let Some([a, b]) = horizontal_slice(surface.vertices, position[1]) else {
                continue;
            };
            let delta = sub(b, a);
            let length_squared = delta[0] * delta[0] + delta[2] * delta[2];
            let fraction = if length_squared == 0.0 {
                0.0
            } else {
                (((position[0] - a[0]) * delta[0] + (position[2] - a[2]) * delta[2])
                    / length_squared)
                    .clamp(0.0, 1.0)
            };
            let point = [
                a[0] + fraction * delta[0],
                position[1],
                a[2] + fraction * delta[2],
            ];
            let distance =
                ((point[0] - position[0]).powi(2) + (point[2] - position[2]).powi(2)).sqrt();
            if distance <= radius {
                result.push(WallHit {
                    triangle: surface.triangle,
                    surface: surface.surface,
                    force: surface.force,
                    normal: surface.normal,
                    point,
                    distance,
                });
            }
        }
        Ok(result)
    }
}

fn validate(values: &[f64]) -> Result<(), Error> {
    if values
        .iter()
        .all(|v| v.is_finite() && v.abs() <= QUERY_LIMIT)
    {
        Ok(())
    } else {
        Err(Error::Query)
    }
}
fn sub(a: Point, b: Point) -> Point {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: Point, b: Point) -> f64 {
    a.into_iter().zip(b).map(|(a, b)| a * b).sum()
}
fn cross(a: Point, b: Point) -> Point {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn contains_xz(vertices: [Point; 3], x: f64, z: f64) -> bool {
    let sides = std::array::from_fn::<_, 3, _>(|i| {
        let a = vertices[i];
        let b = vertices[(i + 1) % 3];
        (b[0] - a[0]) * (z - a[2]) - (b[2] - a[2]) * (x - a[0])
    });
    sides.iter().all(|&v| v >= 0.0) || sides.iter().all(|&v| v <= 0.0)
}
fn horizontal_slice(vertices: [Point; 3], y: f64) -> Option<[Point; 2]> {
    let mut points = Vec::with_capacity(3);
    for i in 0..3 {
        let a = vertices[i];
        let b = vertices[(i + 1) % 3];
        if a[1] == y && !points.contains(&a) {
            points.push(a);
        }
        if (a[1] < y && b[1] > y) || (a[1] > y && b[1] < y) {
            let fraction = (y - a[1]) / (b[1] - a[1]);
            points.push([
                a[0] + fraction * (b[0] - a[0]),
                y,
                a[2] + fraction * (b[2] - a[2]),
            ]);
        }
    }
    match points.as_slice() {
        [] => None,
        [a] => Some([*a, *a]),
        [a, b, ..] => Some([*a, *b]),
    }
}
