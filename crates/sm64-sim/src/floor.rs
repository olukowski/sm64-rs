//! Bounded plane construction and returned-height arithmetic, not floor search.
//!
//! Fresh inference from US0 floor observations at 0x80381900 and partition memory;
//! see docs/research/floor-queries.md. Planes may be supplied or built from vertices.
//! Selection, eligibility, partitions, ties, special surfaces and original
//! coordinate-overflow behavior remain unimplemented.

use thiserror::Error;

/// Authored support bound for this arithmetic experiment, not a game-world rule
/// or a claim about the width of the original coordinate conversion.
pub const MAX_COORDINATE: f32 = 8192.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Error {
    #[error("floor plane requires finite coefficients and a positive Y coefficient")]
    Plane,
    #[error("triangle is outside the bounded exact-cross upward-plane experiment")]
    Vertices,
    #[error("plane-height query must be finite and within the +/-8192 experiment envelope")]
    Query,
    #[error("plane-height arithmetic produced a nonfinite result")]
    Overflow,
}

/// Plane coefficients without surface selection or eligibility. Positive Y is an
/// API constraint, not the original game's floor-classification threshold.
#[derive(Debug, Clone, Copy)]
pub struct FloorPlane {
    normal: [f32; 3],
    origin_offset: f32,
}

impl FloorPlane {
    pub fn new(normal: [f32; 3], origin_offset: f32) -> Result<Self, Error> {
        if normal.iter().any(|v| !v.is_finite()) || !origin_offset.is_finite() || normal[1] <= 0.0 {
            return Err(Error::Plane);
        }
        Ok(Self {
            normal,
            origin_offset,
        })
    }

    /// Construct from ordered signed-integer vertices within an authored domain.
    /// All coordinates must be within +/-8192 and every cyclic cross-product
    /// product/difference must be exactly representable within +/-2^24. This
    /// avoids inferring original integer width or inexact cross-product rounding.
    /// Degenerate, vertical and downward-facing triangles are unsupported.
    /// This is not containment, classification, partitioning or floor selection.
    pub fn from_vertices(vertices: [[i16; 3]; 3]) -> Result<Self, Error> {
        if vertices
            .iter()
            .flatten()
            .any(|&value| f32::from(value).abs() > MAX_COORDINATE)
        {
            return Err(Error::Vertices);
        }
        const MAX_EXACT: i64 = 1 << 24;
        // Check all cyclic choices, rather than inferring which equivalent
        // integer edge construction the original loader used from this corpus.
        let mut cross = [0.0_f32; 3];
        for origin in 0..3 {
            let a: [i64; 3] = std::array::from_fn(|axis| {
                i64::from(vertices[(origin + 1) % 3][axis]) - i64::from(vertices[origin][axis])
            });
            let b: [i64; 3] = std::array::from_fn(|axis| {
                i64::from(vertices[(origin + 2) % 3][axis]) - i64::from(vertices[origin][axis])
            });
            for axis in 0..3 {
                let left = a[(axis + 1) % 3] * b[(axis + 2) % 3];
                let right = a[(axis + 2) % 3] * b[(axis + 1) % 3];
                if [left, right, left - right]
                    .iter()
                    .any(|value| value.abs() > MAX_EXACT)
                {
                    return Err(Error::Vertices);
                }
                if origin == 0 {
                    // An exact zero determinant becomes +0. Floating edge
                    // products instead introduce negative zeros absent from
                    // the observed plane normals. i64 is only a safe host
                    // convenience inside this explicitly exact domain.
                    cross[axis] = (left - right) as f32;
                }
            }
        }
        if cross[1] <= 0.0 {
            return Err(Error::Vertices);
        }
        let x_squared = cross[0] * cross[0];
        let y_squared = cross[1] * cross[1];
        let z_squared = cross[2] * cross[2];
        let xy_squared = x_squared + y_squared;
        let length_squared = xy_squared + z_squared;
        let inverse_length = 1.0 / length_squared.sqrt();
        let normal = cross.map(|value| value * inverse_length);
        let x_term = normal[0] * f32::from(vertices[0][0]);
        let y_term = normal[1] * f32::from(vertices[0][1]);
        let z_term = normal[2] * f32::from(vertices[0][2]);
        let xy = x_term + y_term;
        let origin_offset = -(xy + z_term);
        Self::new(normal, origin_offset)
    }

    pub fn coefficients(&self) -> ([f32; 3], f32) {
        (self.normal, self.origin_offset)
    }

    /// Truncate X/Z toward zero within the explicitly bounded domain, then
    /// evaluate the supplied plane with separately rounded f32 operations.
    /// No query Y, height acceptance, edge containment or floor sentinel is
    /// represented here: this is ONLY the observed returned-plane arithmetic.
    pub fn height_at(&self, x: f32, z: f32) -> Result<f32, Error> {
        if [x, z]
            .iter()
            .any(|v| !v.is_finite() || v.abs() > MAX_COORDINATE)
        {
            return Err(Error::Query);
        }
        // Every admitted value fits in both signed 16- and 32-bit integers.
        // i32 is an implementation convenience, not inferred original width.
        let x = (x as i32) as f32;
        let z = (z as i32) as f32;
        let x_term = self.normal[0] * x;
        let z_term = self.normal[2] * z;
        let horizontal = x_term + z_term;
        let numerator = horizontal + self.origin_offset;
        let height = -numerator / self.normal[1];
        if !height.is_finite() {
            return Err(Error::Overflow);
        }
        Ok(height)
    }
}
