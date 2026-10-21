//! Bounds-checked segmented addresses and private in-memory ROM segment loads.

use crate::{Error, SegmentInfo};

const SLOT_COUNT: usize = 32;
const MAX_SEGMENT_SIZE: usize = 16 * 1024 * 1024;
const MAX_TOTAL_SIZE: usize = 64 * 1024 * 1024;
const MAX_LOADS: usize = 512;

struct Segment {
    info: SegmentInfo,
    bytes: Vec<u8>,
}

/// The top byte selects a slot; the low 24 bits are a byte offset in that slot.
/// This is address resolution only, not emulation of the game's heap/allocators.
pub struct SegmentMap {
    slots: Vec<Option<Segment>>,
    loaded_bytes: usize,
    history: Vec<SegmentInfo>,
}

impl Default for SegmentMap {
    fn default() -> Self {
        Self {
            slots: (0..SLOT_COUNT).map(|_| None).collect(),
            loaded_bytes: 0,
            history: Vec::new(),
        }
    }
}

impl SegmentMap {
    pub fn load(
        &mut self,
        rom: &[u8],
        slot: u8,
        start: u32,
        end: u32,
        compressed: bool,
    ) -> Result<(), Error> {
        let index = usize::from(slot);
        if index >= SLOT_COUNT {
            return Err(Error::Data(format!("invalid segment slot 0x{slot:02x}")));
        }
        if start >= end || end as usize > rom.len() {
            return Err(Error::Data(format!(
                "invalid ROM range 0x{start:x}..0x{end:x}"
            )));
        }
        if self.history.len() == MAX_LOADS {
            return Err(Error::Data("segment load-count budget exhausted".into()));
        }
        let source = &rom[start as usize..end as usize];
        let bytes = if compressed {
            sm64_mio0::decode(source, MAX_SEGMENT_SIZE)?.bytes
        } else {
            if source.len() > MAX_SEGMENT_SIZE {
                return Err(Error::Data("raw segment exceeds size budget".into()));
            }
            source.to_vec()
        };
        let old_size = self.slots[index]
            .as_ref()
            .map_or(0, |segment| segment.bytes.len());
        let new_total = self.loaded_bytes - old_size + bytes.len();
        if new_total > MAX_TOTAL_SIZE {
            return Err(Error::Data("loaded segment memory budget exhausted".into()));
        }
        let info = SegmentInfo {
            segment: slot,
            rom_start: start,
            rom_end: end,
            decoded_size: bytes.len(),
            compressed,
        };
        self.loaded_bytes = new_total;
        self.history.push(info.clone());
        self.slots[index] = Some(Segment { info, bytes });
        Ok(())
    }

    pub fn resolve(&self, address: u32) -> Result<&[u8], Error> {
        let slot = (address >> 24) as usize;
        let offset = (address & 0x00ff_ffff) as usize;
        let segment = self
            .slots
            .get(slot)
            .and_then(Option::as_ref)
            .ok_or_else(|| Error::Data(format!("unloaded segment in address 0x{address:08x}")))?;
        segment
            .bytes
            .get(offset..)
            .filter(|slice| !slice.is_empty())
            .ok_or_else(|| {
                Error::Data(format!(
                    "address 0x{address:08x} exceeds segment {} size {}",
                    segment.info.segment,
                    segment.bytes.len()
                ))
            })
    }

    pub fn history(&self) -> &[SegmentInfo] {
        &self.history
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_raw_segments_and_replacements() {
        let rom: Vec<u8> = (0..128).collect();
        let mut segments = SegmentMap::default();
        segments.load(&rom, 7, 16, 32, false).unwrap();
        assert_eq!(segments.resolve(0x07000003).unwrap(), &rom[19..32]);
        assert!(segments.resolve(0x07000010).is_err());
        assert!(segments.resolve(0x08000000).is_err());
        assert!(segments.resolve(0x80000000).is_err());
        segments.load(&rom, 7, 32, 40, false).unwrap();
        assert_eq!(segments.resolve(0x07000000).unwrap(), &rom[32..40]);
        assert_eq!(segments.history().len(), 2);
        assert_eq!(segments.loaded_bytes, 8);
    }

    #[test]
    fn validates_ranges_and_slot_numbers_without_changing_existing_data() {
        let mut segments = SegmentMap::default();
        for (slot, start, end) in [(32, 0, 4), (0, 4, 4), (0, 8, 4), (0, 0, 129)] {
            assert!(segments.load(&[0; 128], slot, start, end, false).is_err());
        }
        assert!(segments.history().is_empty());
    }

    #[test]
    fn maps_a_compressed_segment_with_exact_rom_boundary() {
        let mut rom = b"MIO0".to_vec();
        for word in [3u32, 17, 17] {
            rom.extend(word.to_be_bytes());
        }
        rom.extend([0xe0, b'a', b'b', b'c']);
        let mut segments = SegmentMap::default();
        segments.load(&rom, 7, 0, 20, true).unwrap();
        assert_eq!(segments.resolve(0x07000000).unwrap(), b"abc");
        assert!(segments.load(&rom, 8, 0, 19, true).is_err());
        assert_eq!(segments.history().len(), 1);
    }
}
