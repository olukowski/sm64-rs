//! Bounded static traversal of level data declarations. Native calls, gameplay
//! conditions, the menu and heap semantics are NOT executed or approximated.
//! Format: https://github.com/n64decomp/sm64/blob/master/include/level_commands.h

use std::collections::BTreeMap;

use crate::collision::parse_static_collision;
use crate::segments::SegmentMap;
use crate::{Area, Error, Level, ObjectSpawn};

const MAX_COMMANDS: usize = 20_000;
const MAX_CALL_DEPTH: usize = 64;

pub(crate) struct Instruction {
    pub opcode: u8,
    pub bytes: Vec<u8>,
}

impl Instruction {
    fn expect_len(&self, length: usize) -> Result<(), Error> {
        if self.bytes.len() != length {
            return Err(Error::Data(format!(
                "opcode 0x{:02x} has length {}, expected {length}",
                self.opcode,
                self.bytes.len()
            )));
        }
        Ok(())
    }

    fn half(&self, offset: usize) -> u16 {
        u16::from_be_bytes(self.bytes[offset..offset + 2].try_into().unwrap())
    }

    fn word(&self, offset: usize) -> u32 {
        u32::from_be_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }
}

pub(crate) fn instruction(segments: &SegmentMap, address: u32) -> Result<Instruction, Error> {
    let data = segments.resolve(address)?;
    let length = data.get(1).copied().unwrap_or(0) as usize;
    if length < 4 || !length.is_multiple_of(4) || length > data.len() {
        return Err(Error::Data(format!(
            "invalid/truncated level instruction at 0x{address:08x}"
        )));
    }
    Ok(Instruction {
        opcode: data[0],
        bytes: data[..length].to_vec(),
    })
}

/// Read the root's current-level dispatch table, not its executable native menu.
/// The US revision gate is applied by the public loader before these offsets.
pub(crate) fn course_entry(
    rom: &[u8],
    segments: &mut SegmentMap,
    dispatch: u32,
    course_id: u32,
) -> Result<u32, Error> {
    let selector = instruction(segments, dispatch)?;
    selector.expect_len(4)?;
    if selector.opcode != 0x3c || selector.bytes[2..4] != [1, 3] {
        return Err(Error::Data(
            "root dispatch does not select the current level".into(),
        ));
    }
    let mut pc = dispatch + 4;
    for _ in 0..64 {
        let command = instruction(segments, pc)?;
        command.expect_len(12)?;
        if command.opcode != 0x0c || command.bytes[2] != 2 {
            return Err(Error::Data("unsupported root course dispatch entry".into()));
        }
        if command.word(4) == course_id {
            let wrapper = instruction(segments, command.word(8))?;
            wrapper.expect_len(16)?;
            if wrapper.opcode != 0 {
                return Err(Error::Data("course wrapper is not EXECUTE".into()));
            }
            let slot = u8::try_from(wrapper.half(2))
                .map_err(|_| Error::Data("invalid EXECUTE segment".into()))?;
            let entry = wrapper.word(12);
            if entry >> 24 != u32::from(slot) {
                return Err(Error::Data("EXECUTE entry uses a different segment".into()));
            }
            segments.load(rom, slot, wrapper.word(4), wrapper.word(8), false)?;
            return Ok(entry);
        }
        pc += 12;
    }
    Err(Error::Data(format!(
        "course {course_id} not found in root dispatch"
    )))
}

pub(crate) fn inspect(rom: &[u8], mut segments: SegmentMap, entry: u32) -> Result<Level, Error> {
    let mut pc = entry;
    let mut stack = Vec::new();
    let mut areas: Vec<Area> = Vec::new();
    let mut active = None;
    let mut ignored = BTreeMap::<u8, usize>::new();
    let mut diagnostics = vec![
        "Static declaration traversal only: native callbacks and gameplay conditions are not executed.".into(),
        "Geometry-layout pointers are recorded, not decoded. Object behaviors and macro objects are not instantiated.".into(),
        "Collision parsing stops at COL_TRI_STOP; special-object/environment-box tails are not interpreted.".into(),
    ];
    let mut inspected = 0;
    loop {
        if inspected == MAX_COMMANDS {
            return Err(Error::Data(
                "level instruction budget exhausted (possible loop)".into(),
            ));
        }
        inspected += 1;
        let command = instruction(&segments, pc)?;
        let mut next = pc
            .checked_add(command.bytes.len() as u32)
            .ok_or_else(|| Error::Data("instruction address overflow".into()))?;
        let opcode = command.opcode;
        match opcode {
            0x02 => {
                command.expect_len(4)?;
                break;
            }
            0x05 | 0x06 => {
                command.expect_len(8)?;
                if opcode == 6 {
                    if stack.len() == MAX_CALL_DEPTH {
                        return Err(Error::Data("level call-stack budget exhausted".into()));
                    }
                    stack.push(next);
                }
                next = command.word(4);
            }
            0x07 => {
                command.expect_len(4)?;
                if let Some(return_to) = stack.pop() {
                    next = return_to;
                } else {
                    break;
                }
            }
            0x11 | 0x12 => {
                command.expect_len(8)?;
                if active.is_some() || areas.is_empty() {
                    return Err(Error::Data(
                        "native callback before a complete area; cannot safely inspect".into(),
                    ));
                }
                diagnostics.push(format!("Stopped before native call 0x{opcode:02x} at 0x{pc:08x}; runtime initialization is not executed."));
                break;
            }
            0x17 | 0x18 | 0x1a => {
                command.expect_len(12)?;
                let slot = u8::try_from(command.half(2))
                    .map_err(|_| Error::Data("invalid load segment".into()))?;
                segments.load(rom, slot, command.word(4), command.word(8), opcode != 0x17)?;
            }
            0x1f => {
                command.expect_len(8)?;
                let index = command.bytes[2];
                if active.is_some()
                    || areas.len() >= 8
                    || areas.iter().any(|area| area.index == index)
                {
                    return Err(Error::Data(
                        "nested, duplicate or excessive area declaration".into(),
                    ));
                }
                areas.push(Area {
                    index,
                    geometry_address: command.word(4),
                    collision_address: None,
                    collision: None,
                    objects: Vec::new(),
                });
                active = Some(areas.len() - 1);
            }
            0x20 => {
                command.expect_len(4)?;
                if active.take().is_none() {
                    return Err(Error::Data("END_AREA without AREA".into()));
                }
            }
            0x24 => {
                command.expect_len(24)?;
                let area_index = active.ok_or_else(|| Error::Data("OBJECT outside AREA".into()))?;
                let area = &mut areas[area_index];
                if area.objects.len() >= 4096 {
                    return Err(Error::Data("area object budget exhausted".into()));
                }
                area.objects.push(ObjectSpawn {
                    acts: command.bytes[2],
                    model: command.bytes[3],
                    position: [
                        command.half(4) as i16,
                        command.half(6) as i16,
                        command.half(8) as i16,
                    ],
                    angles: [
                        command.half(10) as i16,
                        command.half(12) as i16,
                        command.half(14) as i16,
                    ],
                    behavior_parameter: command.word(16),
                    behavior_address: command.word(20),
                });
            }
            0x2e => {
                command.expect_len(8)?;
                let area_index =
                    active.ok_or_else(|| Error::Data("TERRAIN outside AREA".into()))?;
                let area = &mut areas[area_index];
                if area.collision.is_some() {
                    return Err(Error::Data("duplicate TERRAIN in area".into()));
                }
                let address = command.word(4);
                let mesh = parse_static_collision(segments.resolve(address)?)?;
                area.collision_address = Some(address);
                area.collision = Some(mesh);
            }
            // Known declarations with no control flow or segment changes. Their
            // encoded lengths are still checked; skipped opcode counts are public.
            0x1b | 0x1c | 0x1d | 0x1e | 0x30 | 0x31 | 0x32 | 0x34 | 0x35 | 0x37 | 0x38 => {
                command.expect_len(4)?;
                *ignored.entry(opcode).or_default() += 1;
            }
            0x21 | 0x22 | 0x26 | 0x27 | 0x2f | 0x33 | 0x36 | 0x39 => {
                command.expect_len(8)?;
                *ignored.entry(opcode).or_default() += 1;
            }
            0x25 | 0x28 | 0x2b | 0x3a => {
                command.expect_len(12)?;
                *ignored.entry(opcode).or_default() += 1;
            }
            _ => {
                return Err(Error::Data(format!(
                    "unsupported static opcode 0x{opcode:02x} at 0x{pc:08x}; refusing to guess control flow"
                )));
            }
        }
        pc = next;
    }
    if active.is_some() || areas.is_empty() {
        return Err(Error::Data("incomplete course area declarations".into()));
    }
    for (opcode, count) in ignored {
        diagnostics.push(format!(
            "Recorded/skipped {count} declaration(s) of opcode 0x{opcode:02x}; not simulated."
        ));
    }
    Ok(Level {
        name: "Bob-omb Battlefield".into(),
        areas,
        segments: segments.history().to_vec(),
        diagnostics,
        commands_inspected: inspected,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapped(script: &[u8]) -> SegmentMap {
        let mut map = SegmentMap::default();
        map.load(script, 14, 0, script.len() as u32, false).unwrap();
        map
    }

    #[test]
    fn reads_course_dispatch_and_execute_range_instead_of_guessing_asset_offsets() {
        let mut rom = vec![0; 256];
        rom[..4].copy_from_slice(&[0x3c, 4, 1, 3]);
        rom[4..8].copy_from_slice(&[0x0c, 12, 2, 0]);
        rom[8..12].copy_from_slice(&9u32.to_be_bytes());
        rom[12..16].copy_from_slice(&0x15000010u32.to_be_bytes());
        rom[16..20].copy_from_slice(&[0, 16, 0, 14]);
        rom[20..24].copy_from_slice(&128u32.to_be_bytes());
        rom[24..28].copy_from_slice(&160u32.to_be_bytes());
        rom[28..32].copy_from_slice(&0x0e000000u32.to_be_bytes());
        rom[128..132].copy_from_slice(&[7, 4, 0, 0]);
        let mut map = SegmentMap::default();
        map.load(&rom, 21, 0, 32, false).unwrap();
        assert_eq!(
            course_entry(&rom, &mut map, 0x15000000, 9).unwrap(),
            0x0e000000
        );
        assert_eq!(&map.resolve(0x0e000000).unwrap()[..4], &[7, 4, 0, 0]);
        assert_eq!(map.history()[1].rom_start, 128);
        // Re-mapping a root with a mismatched entry segment must fail.
        rom[28] = 0x0f;
        map.load(&rom, 21, 0, 32, false).unwrap();
        assert!(course_entry(&rom, &mut map, 0x15000000, 9).is_err());
    }

    #[test]
    fn validates_instruction_lengths_and_boundaries() {
        for script in [vec![7, 0, 0, 0], vec![7, 3, 0, 0], vec![7, 8, 0, 0]] {
            assert!(instruction(&mapped(&script), 0x0e000000).is_err());
        }
    }

    #[test]
    fn static_calls_follow_returns_and_record_objects() {
        let mut script = vec![0x1f, 8, 1, 0, 0x0e, 0, 0, 0]; // AREA
        script.extend([6, 8, 0, 0, 0x0e, 0, 0, 24]); // call object list
        script.extend([0x20, 4, 0, 0, 0x11, 8, 0, 0]); // END_AREA, native call
        script.extend([0x80, 0, 0, 0]);
        // Target must be after the entire native call, at byte 28.
        script[15] = 28;
        script.extend([0x24, 24, 0x1f, 7]);
        for value in [-100i16, 20, 30, 0, 90, -45] {
            script.extend(value.to_be_bytes());
        }
        script.extend(0x12345678u32.to_be_bytes());
        script.extend(0x13000000u32.to_be_bytes());
        script.extend([7, 4, 0, 0]);
        let level = inspect(&script, mapped(&script), 0x0e000000).unwrap();
        let object = &level.areas[0].objects[0];
        assert_eq!(object.position, [-100, 20, 30]);
        assert_eq!(object.angles, [0, 90, -45]);
        assert_eq!(object.behavior_parameter, 0x12345678);
        assert!(
            level
                .diagnostics
                .iter()
                .any(|text| text.contains("Stopped before native"))
        );
    }

    #[test]
    fn rejects_unknown_control_flow_and_recursive_calls() {
        let unknown = [0xff, 4, 0, 0];
        assert!(inspect(&unknown, mapped(&unknown), 0x0e000000).is_err());
        let recurse = [6, 8, 0, 0, 0x0e, 0, 0, 0];
        let error = inspect(&recurse, mapped(&recurse), 0x0e000000).unwrap_err();
        assert!(error.to_string().contains("stack"));
        let jump = [5, 8, 0, 0, 0x0e, 0, 0, 0];
        let error = inspect(&jump, mapped(&jump), 0x0e000000).unwrap_err();
        assert!(error.to_string().contains("instruction budget"));
    }
}
