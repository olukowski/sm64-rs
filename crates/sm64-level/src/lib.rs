//! Revision-pinned, bounded static course inspection. Not a game-script VM.
pub mod collision;
mod scripts;
pub mod segments;

use serde::Serialize;
use sm64_rom::Rom;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Data(String),
    #[error(transparent)]
    Mio0(#[from] sm64_mio0::Error),
}

#[derive(Debug, Clone, Serialize)]
pub struct Triangle {
    pub indices: [u16; 3],
    pub surface: u16,
    pub force: Option<i16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CollisionMesh {
    pub vertices: Vec<[i16; 3]>,
    pub triangles: Vec<Triangle>,
    /// Extent through COL_TRI_STOP; trailing water/special-object data is not parsed.
    pub consumed: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObjectSpawn {
    pub model: u8,
    pub acts: u8,
    pub position: [i16; 3],
    /// Signed rotation fields are degrees in level declarations, not angle units.
    pub angles: [i16; 3],
    pub behavior_parameter: u32,
    pub behavior_address: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct Area {
    pub index: u8,
    pub geometry_address: u32,
    pub collision_address: Option<u32>,
    pub collision: Option<CollisionMesh>,
    pub objects: Vec<ObjectSpawn>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SegmentInfo {
    pub segment: u8,
    pub rom_start: u32,
    pub rom_end: u32,
    pub decoded_size: usize,
    pub compressed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Level {
    pub name: String,
    pub areas: Vec<Area>,
    pub segments: Vec<SegmentInfo>,
    pub diagnostics: Vec<String>,
    pub commands_inspected: usize,
}

/// Inspect BOB through the US root script's current-level dispatch entry.
/// Layout references: n64decomp/sm64 sm64.ld and levels/scripts.c. These root
/// locations were checked against the canonical US revision 0 ROM; terrain and
/// object addresses are obtained from commands, never signature guesses.
pub fn load_bob(rom: &Rom) -> Result<Level, Error> {
    if rom.identity().known_revision != Some(sm64_rom::KnownRevision::UsRevision0) {
        return Err(Error::Data("course inspection requires the exact US revision 0 ROM; generic ROM inspection remains available".into()));
    }
    let mut segments = segments::SegmentMap::default();
    segments.load(rom.bytes(), 0x15, 0x2abca0, 0x2ac6b0, false)?;
    let entry = scripts::course_entry(rom.bytes(), &mut segments, 0x15000278, 9)?;
    scripts::inspect(rom.bytes(), segments, entry)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_rom_is_rejected_before_revision_specific_reads() {
        let mut bytes = vec![0; 64];
        bytes[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        let rom = Rom::from_bytes(bytes).unwrap();
        assert!(load_bob(&rom).unwrap_err().to_string().contains("exact US"));
    }

    #[test]
    #[ignore = "needs the US revision 0 ROM via SM64_ROM"]
    fn local_bob_static_mesh() {
        let Some(path) = std::env::var_os("SM64_ROM") else {
            eprintln!("skipped: set SM64_ROM to a user-supplied US revision 0 ROM");
            return;
        };
        let rom = Rom::open(path).unwrap();
        let level = load_bob(&rom).unwrap();
        assert_eq!(level.areas.len(), 1);
        let area = &level.areas[0];
        assert_eq!(area.index, 1);
        assert_eq!(area.geometry_address, 0x0e000488);
        assert_eq!(area.collision_address, Some(0x0700e958));
        let collision = area.collision.as_ref().unwrap();
        assert_eq!(collision.vertices.len(), 570);
        assert_eq!(collision.triangles.len(), 1060);
        assert_eq!(area.objects.len(), 30);
        assert_eq!(level.commands_inspected, 109);
        assert_eq!(level.segments.len(), 11);
    }
}
