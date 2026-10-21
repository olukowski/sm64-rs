//! Static collision triangle sections, not the complete collision command stream.
//! Format references: n64decomp/sm64 include/surface_terrains.h (format macros),
//! https://hack64.net/wiki/doku.php?id=super_mario_64:collision_data

use crate::{CollisionMesh, Error, Triangle};

const MAX_TRIANGLES: usize = 200_000;

fn word(bytes: &[u8], position: &mut usize) -> Result<u16, Error> {
    let data = bytes
        .get(*position..*position + 2)
        .ok_or_else(|| Error::Data(format!("truncated collision data at byte 0x{position:x}")))?;
    *position += 2;
    Ok(u16::from_be_bytes([data[0], data[1]]))
}

pub fn surface_has_force(surface: u16) -> bool {
    matches!(
        surface,
        0x0004 | 0x000e | 0x0024 | 0x0025 | 0x0027 | 0x002c | 0x002d
    )
}

/// Stop immediately after COL_TRI_STOP (0x41). Special objects and environment
/// boxes after that marker are intentionally not decoded by this first version.
pub fn parse_static_collision(bytes: &[u8]) -> Result<CollisionMesh, Error> {
    let mut position = 0;
    if word(bytes, &mut position)? != 0x40 {
        return Err(Error::Data(
            "collision section does not begin with COL_INIT (0x40)".into(),
        ));
    }
    let count = usize::from(word(bytes, &mut position)?);
    if bytes.len().saturating_sub(position) < count * 6 {
        return Err(Error::Data("truncated collision vertex array".into()));
    }
    let mut vertices = Vec::with_capacity(count);
    for _ in 0..count {
        vertices.push([
            word(bytes, &mut position)? as i16,
            word(bytes, &mut position)? as i16,
            word(bytes, &mut position)? as i16,
        ]);
    }
    let mut triangles = Vec::new();
    loop {
        let surface = word(bytes, &mut position)?;
        if surface == 0x41 {
            return Ok(CollisionMesh {
                vertices,
                triangles,
                consumed: position,
            });
        }
        if (0x40..=0x64).contains(&surface) {
            return Err(Error::Data(format!(
                "unexpected collision control word 0x{surface:04x} before COL_TRI_STOP"
            )));
        }
        let count = usize::from(word(bytes, &mut position)?);
        if triangles.len() + count > MAX_TRIANGLES {
            return Err(Error::Data("collision triangle budget exhausted".into()));
        }
        let has_force = surface_has_force(surface);
        let stride = if has_force { 8 } else { 6 };
        if bytes.len().saturating_sub(position) < count * stride {
            return Err(Error::Data("truncated collision triangle batch".into()));
        }
        for _ in 0..count {
            let indices = [
                word(bytes, &mut position)?,
                word(bytes, &mut position)?,
                word(bytes, &mut position)?,
            ];
            if indices
                .iter()
                .any(|&index| usize::from(index) >= vertices.len())
            {
                return Err(Error::Data(format!(
                    "collision vertex index outside {}-vertex array",
                    vertices.len()
                )));
            }
            let force = if has_force {
                Some(word(bytes, &mut position)? as i16)
            } else {
                None
            };
            triangles.push(Triangle {
                indices,
                surface,
                force,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(words: &[u16]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_be_bytes()).collect()
    }

    fn mesh(surface: u16) -> Vec<u16> {
        let mut words = vec![
            0x40,
            3,
            0,
            0,
            0,
            100,
            0,
            0,
            0,
            0,
            (-100i16) as u16,
            surface,
            1,
            0,
            1,
            2,
        ];
        if surface_has_force(surface) {
            words.push(0xff82);
        }
        words.push(0x41);
        words
    }

    #[test]
    fn parses_signed_vertices_and_preserves_tail_boundary() {
        let mut words = mesh(0);
        let expected = words.len() * 2;
        words.extend([0x43, 17, 0xffff]); // Tail is intentionally uninterpreted.
        let parsed = parse_static_collision(&bytes(&words)).unwrap();
        assert_eq!(parsed.vertices[2], [0, 0, -100]);
        assert_eq!(parsed.triangles[0].indices, [0, 1, 2]);
        assert_eq!(parsed.consumed, expected);
    }

    #[test]
    fn parameter_surfaces_have_four_words_per_triangle() {
        for surface in [4, 0xe, 0x24, 0x25, 0x27, 0x2c, 0x2d] {
            let parsed = parse_static_collision(&bytes(&mesh(surface))).unwrap();
            assert_eq!(parsed.triangles[0].force, Some(-126));
            assert_eq!(parsed.triangles[0].surface, surface);
        }
        assert!(!surface_has_force(0));
    }

    #[test]
    fn every_truncated_prefix_is_rejected() {
        let data = bytes(&mesh(0x65)); // IDs above control-word range are surfaces.
        for end in 0..data.len() {
            assert!(
                parse_static_collision(&data[..end]).is_err(),
                "prefix {end}"
            );
        }
        assert!(parse_static_collision(&data).is_ok());
    }

    #[test]
    fn rejects_invalid_indices_and_unexpected_control_words() {
        let mut words = mesh(0);
        words[15] = 3;
        assert!(parse_static_collision(&bytes(&words)).is_err());
        words[15] = 1;
        words[11] = 0x44;
        assert!(parse_static_collision(&bytes(&words)).is_err());
        assert!(parse_static_collision(&bytes(&[0x40, 0, 0x42])).is_err());
    }
}
