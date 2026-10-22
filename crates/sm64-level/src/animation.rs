//! Bounded sampling of Mario DMA animation data, not animation/gameplay logic.
//! Declarations and US0 metadata are cited in docs/formats/mario.md.
use sm64_rom::{KnownRevision, Rom};

use crate::Error;

pub const IDLE_HEAD_CENTER: usize = 0xc5;
const TABLE_START: usize = 0x4ec000;
const TABLE_END: usize = 0x579c20;
const MAX_BONES: usize = 256;

#[derive(Debug, Clone)]
pub struct PoseSample {
    /// Source-unit translation, not world placement or accumulated movement.
    /// Runtime object-height/divisor scaling is deliberately not reconstructed.
    pub root_translation: [f32; 3],
    /// XYZ signed binary angles, one triplet per rotational joint.
    pub rotations: Vec<[i16; 3]>,
    pub frame: u16,
    pub frames: u16,
    pub translation_divisor: i16,
}

fn invalid(message: &str) -> Error {
    Error::Data(format!("Mario animation: {message}"))
}
fn u16_at(data: &[u8], at: usize) -> Result<u16, Error> {
    let bytes = data
        .get(at..at + 2)
        .ok_or_else(|| invalid("truncated halfword"))?;
    Ok(u16::from_be_bytes(
        bytes.try_into().expect("checked halfword"),
    ))
}
fn u32_at(data: &[u8], at: usize) -> Result<u32, Error> {
    let bytes = data
        .get(at..at + 4)
        .ok_or_else(|| invalid("truncated word"))?;
    Ok(u32::from_be_bytes(bytes.try_into().expect("checked word")))
}

/// Sample one nonnegative frame, clamping each short channel to its last value.
/// Only flags 0/NOLOOP are accepted; generic horizontal/vertical/suppressed root
/// translation modes are NOT guessed. All channels are validated, even at frame0.
pub fn sample(data: &[u8], frame: u16) -> Result<PoseSample, Error> {
    if data.len() < 24 || data.len() > 1024 * 1024 {
        return Err(invalid("invalid payload length"));
    }
    let flags = u16_at(data, 0)?;
    let divisor = u16_at(data, 2)? as i16;
    let start = u16_at(data, 4)? as i16;
    let loop_start = u16_at(data, 6)? as i16;
    let frames = u16_at(data, 8)?;
    let bones = usize::from(u16_at(data, 10)?);
    if flags & !1 != 0
        || divisor <= 0
        || start != 0
        || loop_start != 0
        || frames == 0
        || frames > i16::MAX as u16
        || frame >= frames
        || bones == 0
        || bones > MAX_BONES
        || u32_at(data, 20)? as usize != data.len()
    {
        return Err(invalid(
            "unsupported flags/frame range, skeleton size or root scaling metadata",
        ));
    }
    let values = u32_at(data, 12)? as usize;
    let indices = u32_at(data, 16)? as usize;
    let channels = (bones + 1) * 3;
    let index_end = indices
        .checked_add(channels * 4)
        .ok_or_else(|| invalid("index overflow"))?;
    if values < 24
        || indices < 24
        || !values.is_multiple_of(2)
        || !indices.is_multiple_of(2)
        || values >= data.len()
        || index_end > data.len()
        || (values >= indices && values < index_end)
    {
        return Err(invalid("invalid/overlapping relative channel offsets"));
    }
    let mut samples = Vec::with_capacity(channels);
    for channel in 0..channels {
        let count = usize::from(u16_at(data, indices + channel * 4)?);
        let index = usize::from(u16_at(data, indices + channel * 4 + 2)?);
        if count == 0 {
            return Err(invalid("zero-length channel"));
        }
        let end = values
            .checked_add((index + count) * 2)
            .ok_or_else(|| invalid("value overflow"))?;
        let begin = values + index * 2;
        if end > data.len() || (begin < index_end && end > indices) {
            return Err(invalid("channel exceeds values or overlaps index"));
        }
        let offset = values + (index + usize::from(frame).min(count - 1)) * 2;
        samples.push(u16_at(data, offset)? as i16);
    }
    Ok(PoseSample {
        root_translation: [
            f32::from(samples[0]),
            f32::from(samples[1]),
            f32::from(samples[2]),
        ],
        rotations: samples[3..].as_chunks::<3>().0.to_vec(),
        frame,
        frames,
        translation_divisor: divisor,
    })
}

fn descriptor(table: &[u8], id: usize) -> Result<&[u8], Error> {
    let count = u32_at(table, 0)? as usize;
    if count == 0 || count > 4096 || id >= count || u32_at(table, 4)? != 0 {
        return Err(invalid(
            "invalid DMA table count, ID or stored source address",
        ));
    }
    let table_end = 8 + count * 8;
    let offset = u32_at(table, 8 + id * 8)? as usize;
    let length = u32_at(table, 12 + id * 8)? as usize;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| invalid("descriptor overflow"))?;
    if table_end > table.len() || offset < table_end || !offset.is_multiple_of(2) || length < 24 {
        return Err(invalid("invalid DMA payload range"));
    }
    table
        .get(offset..end)
        .ok_or_else(|| invalid("DMA payload exceeds table region"))
}

/// Sample the explicitly chosen center-idle animation's first frame, not the
/// gameplay-selected idle variant or an evolving animation state machine.
/// Exact US0 identity is checked before reading revision-specific ROM addresses.
pub fn load_idle_center(rom: &Rom) -> Result<PoseSample, Error> {
    if rom.identity().known_revision != Some(KnownRevision::UsRevision0) {
        return Err(invalid("requires the exact US revision 0 ROM"));
    }
    let table = rom
        .bytes()
        .get(TABLE_START..TABLE_END)
        .ok_or_else(|| invalid("missing DMA table"))?;
    if u32_at(table, 0)? != 209 {
        return Err(invalid("unexpected US0 animation count"));
    }
    let pose = sample(descriptor(table, IDLE_HEAD_CENTER)?, 0)?;
    if pose.rotations.len() != 20 || pose.frames != 30 || pose.translation_divisor != 189 {
        return Err(invalid("unexpected center-idle metadata"));
    }
    Ok(pose)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn authored() -> Vec<u8> {
        // One root translation plus one rotational joint; six channels. All
        // values are independently invented, not copied from any animation.
        let mut data = vec![0; 24 + 6 * 4 + 8 * 2];
        for (at, value) in [(0, 1u16), (2, 100), (8, 3), (10, 1)] {
            data[at..at + 2].copy_from_slice(&value.to_be_bytes());
        }
        data[12..16].copy_from_slice(&48u32.to_be_bytes());
        data[16..20].copy_from_slice(&24u32.to_be_bytes());
        let length = data.len() as u32;
        data[20..24].copy_from_slice(&length.to_be_bytes());
        for (channel, (count, index)) in [(3u16, 0u16), (1, 3), (1, 4), (1, 5), (1, 6), (1, 7)]
            .into_iter()
            .enumerate()
        {
            let at = 24 + channel * 4;
            data[at..at + 2].copy_from_slice(&count.to_be_bytes());
            data[at + 2..at + 4].copy_from_slice(&index.to_be_bytes());
        }
        for (i, value) in [11i16, 22, 33, 44, -55, 8192, -16384, 2048]
            .into_iter()
            .enumerate()
        {
            data[48 + i * 2..50 + i * 2].copy_from_slice(&value.to_be_bytes());
        }
        data
    }

    #[test]
    fn samples_relative_indices_and_clamps_short_channels_without_interpolation() {
        let data = authored();
        let pose = sample(&data, 0).unwrap();
        assert_eq!(pose.root_translation, [11.0, 44.0, -55.0]);
        assert_eq!(pose.rotations, [[8192, -16384, 2048]]);
        assert_eq!(
            sample(&data, 2).unwrap().root_translation,
            [33.0, 44.0, -55.0]
        );
        assert_eq!(pose.translation_divisor, 100);
        assert_eq!(pose.frames, 3);
        assert!(sample(&data, 3).is_err());
    }

    #[test]
    fn malformed_or_unsupported_headers_and_all_channels_are_rejected() {
        for length in 0..24 {
            assert!(sample(&authored()[..length], 0).is_err());
        }
        for (at, value) in [
            (0, 8u16),
            (2, 0),
            (4, 1),
            (6, 1),
            (8, 0),
            (10, 0),
            (10, 257),
            (24, 0),
            (46, 65535),
        ] {
            let mut data = authored();
            data[at..at + 2].copy_from_slice(&value.to_be_bytes());
            assert!(sample(&data, 0).is_err(), "offset {at}");
        }
        for (at, value) in [(12, 25u32), (12, 40), (16, 60), (16, u32::MAX), (20, 24)] {
            let mut data = authored();
            data[at..at + 4].copy_from_slice(&value.to_be_bytes());
            assert!(sample(&data, 0).is_err(), "offset {at}");
        }
    }

    #[test]
    fn dma_ranges_do_not_require_nonoverlapping_payloads() {
        let payload = authored();
        let mut table = vec![0; 24];
        table[..4].copy_from_slice(&2u32.to_be_bytes());
        for at in [8, 16] {
            table[at..at + 4].copy_from_slice(&24u32.to_be_bytes());
            table[at + 4..at + 8].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        }
        table.extend(&payload);
        assert_eq!(descriptor(&table, 0).unwrap(), payload);
        assert_eq!(descriptor(&table, 1).unwrap(), payload);
        assert!(descriptor(&table, 2).is_err());
        table[4] = 1;
        assert!(descriptor(&table, 0).is_err());
    }

    #[test]
    fn unknown_revision_is_rejected_before_animation_reads() {
        let mut bytes = vec![0; 64];
        bytes[..4].copy_from_slice(&[0x80, 0x37, 0x12, 0x40]);
        let rom = Rom::from_bytes(bytes).unwrap();
        assert!(
            load_idle_center(&rom)
                .unwrap_err()
                .to_string()
                .contains("exact US")
        );
    }

    #[test]
    #[ignore = "needs the US revision 0 ROM via SM64_ROM"]
    fn local_idle_snapshot() {
        let Some(path) = std::env::var_os("SM64_ROM") else {
            eprintln!("skipped: set SM64_ROM to a user-supplied US revision 0 ROM");
            return;
        };
        let pose = load_idle_center(&Rom::open(path).unwrap()).unwrap();
        assert_eq!(pose.rotations.len(), 20);
        assert_eq!(pose.frames, 30);
        assert!(pose.root_translation.iter().all(|v| v.is_finite()));
    }
}
