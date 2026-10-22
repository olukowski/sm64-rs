//! Bounded height arithmetic for an ALREADY SELECTED floor plane, not floor search.
//!
//! Fresh inference from paired US0 `find_floor` observations at 0x80381900;
//! see docs/research/floor-queries.md. The caller supplies plane coefficients.
//! Selection, eligibility, geometry-to-plane construction, partitions, ties,
//! special surfaces and original coordinate-overflow behavior remain unimplemented.

use thiserror::Error;

/// Authored support bound for this arithmetic experiment, not a game-world rule
/// or a claim about the width of the original coordinate conversion.
pub const MAX_COORDINATE: f32 = 8192.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum Error {
    #[error("floor plane requires finite coefficients and a positive Y coefficient")]
    Plane,
    #[error("plane-height query must be finite and within the +/-8192 experiment envelope")]
    Query,
    #[error("plane-height arithmetic produced a nonfinite result")]
    Overflow,
}

/// Coefficients supplied by the caller; this type neither selects a surface nor
/// normalizes/reconstructs a plane. Positive Y is an API constraint, not the
/// original game's floor-classification threshold.
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
