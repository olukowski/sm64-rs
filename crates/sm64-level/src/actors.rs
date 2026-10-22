//! Revision-pinned global actor declarations. No game behavior is executed.
//!
//! US0 root level-script metadata: segment loads at 0x15000000/0x15000018,
//! model 1 geometry binding at 0x15000040. See docs/formats/mario.md.
use sm64_rom::{KnownRevision, Rom};

use crate::{Error, segments::SegmentMap};

#[derive(Debug)]
pub struct MarioAssets {
    /// Private resident ROM bytes; deliberately not serializable.
    pub segments: SegmentMap,
    pub geometry_address: u32,
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_be_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("checked descriptor length"),
    )
}

fn load_declaration(
    rom: &[u8],
    segments: &mut SegmentMap,
    address: u32,
    slot: u8,
    compressed: bool,
) -> Result<(), Error> {
    let bytes = segments.resolve_range(address, 12)?;
    let expected = [if compressed { 0x18 } else { 0x17 }, 12, 0, slot];
    if bytes[..4] != expected {
        return Err(Error::Data(format!(
            "unexpected actor load declaration at 0x{address:08x}"
        )));
    }
    let (start, end) = (word(bytes, 4), word(bytes, 8));
    segments.load(rom, slot, start, end, compressed)
}

fn mario_binding(segments: &SegmentMap, address: u32) -> Result<u32, Error> {
    let bytes = segments.resolve_range(address, 8)?;
    if bytes[..4] != [0x22, 8, 0, 1] {
        return Err(Error::Data(
            "root declaration is not Mario's geometry binding".into(),
        ));
    }
    let geometry = word(bytes, 4);
    if geometry >> 24 != 0x17 || geometry & 3 != 0 {
        return Err(Error::Data(
            "Mario geometry binding is outside aligned segment 0x17".into(),
        ));
    }
    segments.resolve_range(geometry, 4)?;
    Ok(geometry)
}

/// Load only the global Mario graphics/layout declarations from the canonical
/// US revision. ROM boundaries and geometry entry are read from those commands,
/// not signature-scanned or inferred from a filename. No scripts are executed.
pub fn load_mario(rom: &Rom) -> Result<MarioAssets, Error> {
    if rom.identity().known_revision != Some(KnownRevision::UsRevision0) {
        return Err(Error::Data(
            "Mario assets require the exact US revision 0 ROM".into(),
        ));
    }
    let mut segments = SegmentMap::default();
    segments.load(rom.bytes(), 0x15, 0x2abca0, 0x2ac6b0, false)?;
    load_declaration(rom.bytes(), &mut segments, 0x15000000, 4, true)?;
    load_declaration(rom.bytes(), &mut segments, 0x15000018, 0x17, false)?;
    let geometry_address = mario_binding(&segments, 0x15000040)?;
    Ok(MarioAssets {
        segments,
        geometry_address,
    })
}

/// ROM idle-animation snapshot with explicit normal/cap-on/open-eye/fist choices.
/// Omitted native torso/head/hand/material callbacks are not emulated.
#[derive(Debug)]
pub struct MarioSnapshot {
    pub segments: SegmentMap,
    pub geometry: crate::geo::GeoScene,
    pub animation_id: usize,
    pub frame: u16,
}

pub fn load_mario_idle(rom: &Rom) -> Result<MarioSnapshot, Error> {
    let assets = load_mario(rom)?;
    let animation = crate::animation::load_idle_center(rom)?;
    let options = crate::geo::PoseOptions {
        pose: crate::geo::JointPose {
            root_translation: animation.root_translation,
            rotations: animation.rotations,
        },
        switch_selections: [
            (0x80277150, 0), // normal near-detail root, not distance switching
            (0x802776d8, 0), // opaque normal form
            (0x80277740, 0), // cap on
            (0x802771bc, 0), // open eyes
            (0x802774f4, 0), // normal hands
        ]
        .into(),
        render_distance: 0.0,
        allowed_omitted_callbacks: [
            0x80277d6c, 0x802770a4, 0x802b1bb0, 0x80277294, 0x802773a4, 0x80277824, 0x802775cc,
        ]
        .into(),
        allow_empty_held: true,
    };
    let mut geometry =
        crate::geo::inspect_posed_geo(&assets.segments, assets.geometry_address, &options)?;
    // The static normal-cap profile excludes the two wing draw nodes. Their
    // runtime visibility callbacks are not executed; this choice is explicit.
    geometry
        .draws
        .retain(|draw| draw.display_list != 0x0401ca00);
    geometry.diagnostics.push("Normal-cap presentation profile explicitly suppresses wing display-list nodes 0x170003cc/0x17000404 (DL 0x0401ca00); native visibility callbacks not emulated.".into());
    Ok(MarioSnapshot {
        segments: assets.segments,
        geometry,
        animation_id: crate::animation::IDLE_HEAD_CENTER,
        frame: animation.frame,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binding_requires_declared_model_segment_alignment_and_bounds() {
        let mut root = vec![0; 16];
        root[..8].copy_from_slice(&[0x22, 8, 0, 1, 0x17, 0, 0, 0]);
        let mut segments = SegmentMap::default();
        segments.load(&root, 0x15, 0, 16, false).unwrap();
        segments.load(&[0; 4], 0x17, 0, 4, false).unwrap();
        assert_eq!(mario_binding(&segments, 0x15000000).unwrap(), 0x17000000);
        for (offset, value) in [(0, 0x21), (1, 4), (3, 2), (4, 4), (7, 1), (7, 16)] {
            let mut invalid = root.clone();
            invalid[offset] = value;
            segments.load(&invalid, 0x15, 0, 16, false).unwrap();
            assert!(mario_binding(&segments, 0x15000000).is_err());
        }
    }

    #[test]
    fn load_declarations_are_checked_before_residency_changes() {
        let mut source = vec![0; 32];
        source[..12].copy_from_slice(&[0x17, 12, 0, 4, 0, 0, 0, 16, 0, 0, 0, 32]);
        let mut segments = SegmentMap::default();
        segments.load(&source, 0x15, 0, 32, false).unwrap();
        load_declaration(&source, &mut segments, 0x15000000, 4, false).unwrap();
        assert_eq!(segments.data(4).unwrap().len(), 16);
        assert!(load_declaration(&source, &mut segments, 0x15000000, 4, true).is_err());
        assert!(load_declaration(&source, &mut segments, 0x15000000, 5, false).is_err());
        assert!(load_declaration(&source, &mut segments, 0x15000018, 4, false).is_err());
    }

    #[test]
    fn unknown_revision_is_rejected_before_actor_reads() {
        let mut bytes = vec![0; 64];
        bytes[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        let rom = Rom::from_bytes(bytes).unwrap();
        assert!(
            load_mario(&rom)
                .unwrap_err()
                .to_string()
                .contains("exact US")
        );
    }

    #[test]
    #[ignore = "needs the US revision 0 ROM via SM64_ROM"]
    fn local_mario_declarations() {
        let Some(path) = std::env::var_os("SM64_ROM") else {
            eprintln!("skipped: set SM64_ROM to a user-supplied US revision 0 ROM");
            return;
        };
        let rom = Rom::open(path).unwrap();
        let actor = load_mario(&rom).unwrap();
        assert_eq!(actor.geometry_address, 0x17002dd4);
        assert!(actor.segments.data(4).unwrap().len() > 100_000);
        assert!(actor.segments.data(0x17).unwrap().len() > 10_000);
    }
}
