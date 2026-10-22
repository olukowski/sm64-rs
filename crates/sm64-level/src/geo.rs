//! Bounded static geometry-layout traversal, not a native callback interpreter.
use crate::{Error, segments::SegmentMap};
use std::collections::{BTreeMap, BTreeSet};

/// Animation channels in skeleton traversal order (including null-DL parts).
#[derive(Debug, Clone, Default)]
pub struct JointPose {
    pub root_translation: [f32; 3],
    /// One rotation per active animated part, including the root and null-DL parts.
    /// Binary angles: one full turn is 65536, not 360.
    pub rotations: Vec<[i16; 3]>,
}

/// Explicit policy for inspecting dynamic layouts without executing native code.
#[derive(Debug, Clone, Default)]
pub struct PoseOptions {
    pub pose: JointPose,
    /// Callback address -> zero-based child index; no default branch is guessed.
    pub switch_selections: BTreeMap<u32, usize>,
    pub render_distance: f32,
    pub allowed_omitted_callbacks: BTreeSet<u32>,
    pub allow_empty_held: bool,
}

type Matrix = [[f32; 4]; 4];
const MAX_COMMANDS: usize = 100_000;
const MAX_DEPTH: usize = 64;
const MAX_DRAWS: usize = 16_384;
const IDENTITY: Matrix = [
    [1., 0., 0., 0.],
    [0., 1., 0., 0.],
    [0., 0., 1., 0.],
    [0., 0., 0., 1.],
];

#[derive(Debug, Clone)]
pub struct GeoDraw {
    pub display_list: u32,
    pub layer: u8,
    /// Column-major homogeneous transform, in original SM64 units.
    pub transform: Matrix,
}
#[derive(Debug, Clone)]
pub struct GeoScene {
    pub draws: Vec<GeoDraw>,
    pub diagnostics: Vec<String>,
    pub commands_inspected: usize,
}

#[derive(Clone, Copy)]
struct NodeContext {
    transform: Matrix,
    active: bool,
    selector: Option<usize>,
}
#[derive(Clone, Copy)]
struct Scope {
    parent: Matrix,
    active: bool,
    children_seen: usize,
    selector: Option<usize>,
    last_node: Option<NodeContext>,
}
struct Call {
    next: u32,
    terminator: u8,
    depth: usize,
}

fn fail(pc: u32, message: impl std::fmt::Display) -> Error {
    Error::Data(format!("geo at 0x{pc:08x}: {message}"))
}
fn half(bytes: &[u8], at: usize) -> i16 {
    i16::from_be_bytes([bytes[at], bytes[at + 1]])
}
fn word(bytes: &[u8], at: usize) -> u32 {
    u32::from_be_bytes(
        bytes[at..at + 4]
            .try_into()
            .expect("validated command size"),
    )
}
fn triple(bytes: &[u8], at: usize) -> [f32; 3] {
    [
        half(bytes, at) as f32,
        half(bytes, at + 2) as f32,
        half(bytes, at + 4) as f32,
    ]
}
fn multiply(a: Matrix, b: Matrix) -> Matrix {
    let mut result = [[0.; 4]; 4];
    for (col, values) in result.iter_mut().enumerate() {
        for (row, value) in values.iter_mut().enumerate() {
            *value = (0..4).map(|k| a[k][row] * b[col][k]).sum();
        }
    }
    result
}
fn translation(v: [f32; 3]) -> Matrix {
    let mut m = IDENTITY;
    m[3][..3].copy_from_slice(&v);
    m
}
fn rotation(degrees: [f32; 3]) -> Matrix {
    // Geometry rotations are declared in degrees; Z then X then Y on vectors.
    let [x, y, z] = degrees.map(|v| v.to_radians());
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let rx = [
        [1., 0., 0., 0.],
        [0., cx, sx, 0.],
        [0., -sx, cx, 0.],
        [0., 0., 0., 1.],
    ];
    let ry = [
        [cy, 0., -sy, 0.],
        [0., 1., 0., 0.],
        [sy, 0., cy, 0.],
        [0., 0., 0., 1.],
    ];
    let rz = [
        [cz, sz, 0., 0.],
        [-sz, cz, 0., 0.],
        [0., 0., 1., 0.],
        [0., 0., 0., 1.],
    ];
    multiply(ry, multiply(rx, rz))
}
// Joint channels use XYZ Euler rotation, unlike static geo's ZXY nodes.
// A column vector therefore sees Rx first: Rz * Ry * Rx.
fn animation_rotation(angles: [i16; 3]) -> Matrix {
    let [x, y, z] = angles.map(|v| v as f32 * (360. / 65536.));
    multiply(
        rotation([0., 0., z]),
        multiply(rotation([0., y, 0.]), rotation([x, 0., 0.])),
    )
}

fn checked_pointer(segments: &SegmentMap, address: u32, length: usize) -> Result<(), Error> {
    if address & 3 != 0 {
        return Err(fail(address, "unaligned pointer"));
    }
    segments.resolve_range(address, length).map(|_| ())
}

/// Decode static draw references reachable from a segmented geometry-layout root.
/// Dynamic geometry is diagnosed, never invoked. Unknown opcodes and ambiguous
/// switch/LOD or dynamic-transform subtrees fail rather than inventing a branch.
pub fn inspect_geo(segments: &SegmentMap, entry: u32) -> Result<GeoScene, Error> {
    inspect_core(segments, entry, None)
}

/// Traverse explicitly selected branches and apply a complete joint pose.
/// Native callbacks are never executed; only allowlisted ASM callbacks may be omitted.
/// Every active animated part consumes one rotation, including the first part,
/// which also adds root translation. Exactly `rotations.len()` parts must be encountered.
pub fn inspect_posed_geo(
    segments: &SegmentMap,
    entry: u32,
    options: &PoseOptions,
) -> Result<GeoScene, Error> {
    if !options.render_distance.is_finite()
        || options.pose.root_translation.iter().any(|v| !v.is_finite())
    {
        return Err(fail(
            entry,
            "non-finite pose root translation or render distance",
        ));
    }
    inspect_core(segments, entry, Some(options))
}

fn inspect_core(
    segments: &SegmentMap,
    entry: u32,
    options: Option<&PoseOptions>,
) -> Result<GeoScene, Error> {
    checked_pointer(segments, entry, 4)?;
    let mut joints_consumed = 0usize;
    let mut scene = GeoScene {
        draws: vec![],
        diagnostics: vec![],
        commands_inspected: 0,
    };
    let mut pc = entry;
    let mut scope = Scope {
        parent: IDENTITY,
        active: true,
        children_seen: 0,
        selector: None,
        last_node: None,
    };
    let mut nodes: Vec<Scope> = vec![];
    let mut calls: Vec<Call> = vec![];
    loop {
        if scene.commands_inspected == MAX_COMMANDS {
            return Err(fail(pc, "command budget exhausted (possible loop)"));
        }
        checked_pointer(segments, pc, 4)?;
        let header = segments.resolve_range(pc, 4)?;
        let op = header[0];
        let param = header[1];
        let with_dl = param & 0x80 != 0;
        let base_len = match op {
            0x00 | 0x02 | 0x0d | 0x0e | 0x15 | 0x16 | 0x18 | 0x19 | 0x1a | 0x1e => 8,
            0x01 | 0x03..=0x07 | 0x09 | 0x0b | 0x0c | 0x17 | 0x1b | 0x20 => 4,
            0x08 | 0x13 | 0x1c => 12,
            0x0a => {
                if param == 0 {
                    8
                } else {
                    12
                }
            }
            0x0f => 20,
            0x10 => match (param >> 4) & 7 {
                0 => 16,
                1 | 2 => 8,
                3 => 4,
                _ => return Err(fail(pc, "unknown translate/rotate field layout")),
            },
            0x11 | 0x12 | 0x14 | 0x1d => 8,
            0x1f => 16,
            _ => {
                return Err(fail(
                    pc,
                    format!("unknown opcode 0x{op:02x}; length is unknown"),
                ));
            }
        };
        let optional_dl = matches!(op, 0x10..=0x12 | 0x14 | 0x1d);
        if optional_dl && param & 0x70 != 0 && op != 0x10 {
            return Err(fail(pc, "reserved transform parameter bits"));
        }
        let len = base_len + usize::from(optional_dl && with_dl) * 4;
        let bytes = segments.resolve_range(pc, len).map_err(|e| fail(pc, e))?;
        let next = pc
            .checked_add(len as u32)
            .filter(|next| next >> 24 == pc >> 24)
            .ok_or_else(|| fail(pc, "command crosses segmented address boundary"))?;
        scene.commands_inspected += 1;
        let mut local = IDENTITY;
        let mut draw = None;
        let mut node = true;
        let mut active = scope.active
            && scope
                .selector
                .is_none_or(|selected| selected == scope.children_seen);
        let mut selector = None;
        let omission = match op {
            0x00 | 0x02 => {
                if op == 0x02 && param > 1 {
                    return Err(fail(pc, "unknown branch mode"));
                }
                let target = word(bytes, 4);
                checked_pointer(segments, target, 4)?;
                if op == 0x00 || param == 1 {
                    if calls.len() == MAX_DEPTH {
                        return Err(fail(pc, "call-depth budget exhausted"));
                    }
                    calls.push(Call {
                        next,
                        terminator: if op == 0 { 1 } else { 3 },
                        depth: nodes.len(),
                    });
                }
                pc = target;
                continue;
            }
            0x01 | 0x03 => {
                if let Some(call) = calls.pop() {
                    if call.terminator != op || nodes.len() != call.depth {
                        return Err(fail(
                            pc,
                            "mismatched call terminator or unbalanced called nodes",
                        ));
                    }
                    pc = call.next;
                    continue;
                }
                if op != 1 || !nodes.is_empty() {
                    return Err(fail(pc, "unmatched return or unclosed nodes at root end"));
                }
                validate_selection(&scope, pc)?;
                if let Some(options) = options {
                    let expected = options.pose.rotations.len();
                    if joints_consumed != expected {
                        return Err(fail(
                            pc,
                            format!(
                                "pose joint count mismatch: consumed {joints_consumed}, expected {expected}"
                            ),
                        ));
                    }
                }
                return Ok(scene);
            }
            0x04 => {
                let parent = scope
                    .last_node
                    .ok_or_else(|| fail(pc, "OPEN_NODE without a preceding node"))?;
                if nodes.len() == MAX_DEPTH {
                    return Err(fail(pc, "node-depth budget exhausted"));
                }
                // The switch's selection belongs to the opened child scope,
                // not to subsequent siblings after CLOSE_NODE.
                if let Some(last) = scope.last_node.as_mut() {
                    last.selector = None;
                }
                nodes.push(scope);
                scope = Scope {
                    parent: parent.transform,
                    active: parent.active,
                    children_seen: 0,
                    selector: parent.selector,
                    last_node: None,
                };
                node = false;
                None
            }
            0x05 => {
                if calls.last().is_some_and(|call| nodes.len() <= call.depth) {
                    return Err(fail(pc, "called layout closes caller's node"));
                }
                validate_selection(&scope, pc)?;
                scope = nodes
                    .pop()
                    .ok_or_else(|| fail(pc, "CLOSE_NODE without OPEN_NODE"))?;
                node = false;
                None
            }
            0x0d if options.is_some() => {
                let distance = options.unwrap().render_distance;
                active &= half(bytes, 4) as f32 <= distance && distance < half(bytes, 6) as f32;
                None
            }
            0x0e if options.is_some() => {
                if active {
                    let callback = word(bytes, 4);
                    let selected = *options
                        .unwrap()
                        .switch_selections
                        .get(&callback)
                        .ok_or_else(|| {
                            fail(
                                pc,
                                format!("missing switch selection for callback 0x{callback:08x}"),
                            )
                        })?;
                    let declared = half(bytes, 2);
                    if declared < 0 || (declared > 0 && selected >= declared as usize) {
                        return Err(fail(
                            pc,
                            format!(
                                "switch selection {selected} out of range for callback 0x{callback:08x}"
                            ),
                        ));
                    }
                    selector = Some(selected);
                }
                None
            }
            0x13 if options.is_some() => {
                if active {
                    let pose = &options.unwrap().pose;
                    let mut offset = triple(bytes, 2);
                    if joints_consumed == 0 {
                        for (value, root) in offset.iter_mut().zip(pose.root_translation) {
                            *value += root;
                        }
                    }
                    let angles = pose.rotations.get(joints_consumed).ok_or_else(|| {
                        fail(pc, "pose has too few rotations for active animated parts")
                    })?;
                    local = multiply(translation(offset), animation_rotation(*angles));
                    joints_consumed += 1;
                }
                draw = Some(word(bytes, 8));
                None
            }
            0x1c if options.is_some() => {
                if active && !options.unwrap().allow_empty_held {
                    return Err(fail(pc, "held object requires allow_empty_held"));
                }
                if active {
                    scene.diagnostics.push(format!(
                        "geo 0x{pc:08x}: held object callback 0x{:08x} omitted as empty node",
                        word(bytes, 8)
                    ));
                }
                active = false;
                None
            }
            0x06 | 0x07 | 0x0d | 0x0e | 0x13 | 0x14 | 0x1b | 0x1c => {
                return Err(fail(
                    pc,
                    format!(
                        "unsupported subtree opcode 0x{op:02x} (view/flags, LOD/switch, animation, billboard, or held object); no branch guessed"
                    ),
                ));
            }
            0x08 => Some("screen-area viewport omitted; static world descendants retained"),
            0x09 => Some("orthographic projection omitted; static world descendants retained"),
            0x0a => Some("camera frustum/projection and optional native callback omitted"),
            0x0b => None,
            0x0c => Some("zbuffer scene state omitted; draw layers retained"),
            0x0f => {
                Some("camera pose and native callback omitted; world-space descendants retained")
            }
            0x10 => {
                local = match (param >> 4) & 7 {
                    0 => multiply(translation(triple(bytes, 4)), rotation(triple(bytes, 10))),
                    1 => translation(triple(bytes, 2)),
                    2 => rotation(triple(bytes, 2)),
                    3 => rotation([0., half(bytes, 2) as f32, 0.]),
                    _ => unreachable!("field layout validated"),
                };
                None
            }
            0x11 => {
                local = translation(triple(bytes, 2));
                None
            }
            0x12 => {
                local = rotation(triple(bytes, 2));
                None
            }
            0x15 => {
                draw = Some(word(bytes, 4));
                None
            }
            0x16 => Some("dynamic shadow omitted"),
            0x17 => Some("render objects omitted (no object graph instantiated)"),
            0x18 if options.is_some() => {
                if active {
                    let callback = word(bytes, 4);
                    if !options
                        .unwrap()
                        .allowed_omitted_callbacks
                        .contains(&callback)
                    {
                        return Err(fail(
                            pc,
                            format!("native ASM callback 0x{callback:08x} is not allowlisted"),
                        ));
                    }
                    scene.diagnostics.push(format!("geo 0x{pc:08x}: native ASM callback 0x{callback:08x} and generated display lists omitted; identity group retained"));
                }
                None
            }
            0x18 => Some("native ASM callback and generated display lists omitted"),
            0x19 => Some("background/skybox and native background callback omitted"),
            0x1a | 0x1e | 0x1f => {
                node = false;
                None
            }
            0x1d => {
                let scale = word(bytes, 4) as f32 / 65536.;
                local[0][0] = scale;
                local[1][1] = scale;
                local[2][2] = scale;
                None
            }
            0x20 => {
                Some("culling radius ignored; static descendants retained without frustum culling")
            }
            _ => unreachable!("opcode validated"),
        };
        if let Some(message) = omission
            && (options.is_none() || active)
        {
            scene.diagnostics.push(format!("geo 0x{pc:08x}: {message}"));
        }
        if optional_dl && with_dl {
            draw = Some(word(bytes, base_len));
        }
        if node {
            let transform = multiply(scope.parent, local);
            if transform.iter().flatten().any(|v| !v.is_finite()) {
                return Err(fail(pc, "non-finite transform"));
            }
            // A selected switch child must actually open its child list, even
            // when that list is empty. Branch commands never count as children.
            if let Some(previous) = scope.last_node
                && previous.selector.is_some()
            {
                return Err(fail(pc, "switch node has no child scope"));
            }
            scope.children_seen += 1;
            scope.last_node = Some(NodeContext {
                transform,
                active,
                selector,
            });
            if let Some(display_list) = draw.filter(|address| *address != 0 && active) {
                let layer = if optional_dl || op == 0x13 {
                    param & 0x0f
                } else {
                    param
                };
                if layer > 7 {
                    return Err(fail(pc, "invalid render layer (expected 0..7)"));
                }
                checked_pointer(segments, display_list, 8)?;
                if scene.draws.len() == MAX_DRAWS {
                    return Err(fail(pc, "draw budget exhausted"));
                }
                scene.draws.push(GeoDraw {
                    display_list,
                    layer,
                    transform,
                });
            }
        }
        pc = next;
    }
}

fn validate_selection(scope: &Scope, pc: u32) -> Result<(), Error> {
    if let Some(selected) = scope.selector
        && selected >= scope.children_seen
    {
        return Err(fail(
            pc,
            format!(
                "switch selection {selected} out of range: only {} children",
                scope.children_seen
            ),
        ));
    }
    if scope.last_node.is_some_and(|node| node.selector.is_some()) {
        return Err(fail(pc, "switch node has no child scope"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn animated_xyz_rotation_differs_from_static_geo_zxy() {
        let joint = animation_rotation([16384, 0, 16384]);
        // Unit Y: X quarter-turn maps it to Z; later Z rotation leaves it there.
        assert!(joint[1][0].abs() < 1e-5);
        assert!(joint[1][1].abs() < 1e-5);
        assert!((joint[1][2] - 1.0).abs() < 1e-5);
        let static_node = rotation([90.0, 0.0, 90.0]);
        assert!((static_node[1][0] + 1.0).abs() < 1e-5);
    }

    fn inspect(bytes: &[u8]) -> Result<GeoScene, Error> {
        let mut segments = SegmentMap::default();
        segments
            .load(bytes, 14, 0, bytes.len() as u32, false)
            .unwrap();
        segments.load(&[0; 8], 7, 0, 8, false).unwrap();
        inspect_geo(&segments, 0x0e000000)
    }
    fn near(actual: [f32; 4], expected: [f32; 4]) {
        for (a, b) in actual.into_iter().zip(expected) {
            assert!((a - b).abs() < 0.0001, "{actual:?} != {expected:?}");
        }
    }

    fn posed(bytes: &[u8], options: &PoseOptions) -> Result<GeoScene, Error> {
        let mut segments = SegmentMap::default();
        segments
            .load(bytes, 14, 0, bytes.len() as u32, false)
            .unwrap();
        segments.load(&[0; 8], 7, 0, 8, false).unwrap();
        inspect_posed_geo(&segments, 0x0e000000, options)
    }
    fn part(x: i16, dl: bool) -> Vec<u8> {
        let [hi, lo] = x.to_be_bytes();
        vec![0x13, 1, hi, lo, 0, 0, 0, 0, if dl { 7 } else { 0 }, 0, 0, 0]
    }
    const OPEN: [u8; 4] = [4, 0, 0, 0];
    const CLOSE: [u8; 4] = [5, 0, 0, 0];
    const END: [u8; 4] = [1, 0, 0, 0];

    #[test]
    fn posed_null_root_binary_angles_and_exact_channels() {
        let bytes = [
            part(10, false),
            OPEN.to_vec(),
            part(2, true),
            CLOSE.to_vec(),
            vec![0x15, 1, 0, 0, 7, 0, 0, 0],
            END.to_vec(),
        ]
        .concat();
        let mut options = PoseOptions::default();
        options.pose.root_translation = [3., 4., 5.];
        options.pose.rotations = vec![[0; 3], [0, 16384, 0]];
        let scene = posed(&bytes, &options).unwrap();
        near(scene.draws[0].transform[3], [15., 4., 5., 1.]);
        near(scene.draws[0].transform[0], [0., 0., -1., 0.]);
        assert_eq!(scene.draws[1].transform, IDENTITY);
        options.pose.rotations = vec![[0; 3], [0, 0, i16::MIN]];
        near(
            posed(&bytes, &options).unwrap().draws[0].transform[0],
            [-1., 0., 0., 0.],
        );
        options.pose.rotations.clear();
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("too few")
        );
        options.pose.rotations = vec![[0; 3]; 3];
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("count mismatch")
        );
        options.pose.root_translation[0] = f32::NAN;
        assert!(posed(&bytes, &options).is_err());
        options.pose.root_translation[0] = f32::INFINITY;
        assert!(posed(&bytes, &options).is_err());
        assert!(posed(&END, &PoseOptions::default()).is_ok());
        let one_part = [part(10, true), END.to_vec()].concat();
        let mut root_options = PoseOptions::default();
        assert!(
            posed(&one_part, &root_options)
                .unwrap_err()
                .to_string()
                .contains("too few")
        );
        root_options.pose.root_translation = [3., 4., 5.];
        root_options.pose.rotations = vec![[0, 16384, 0]];
        let root_scene = posed(&one_part, &root_options).unwrap();
        near(root_scene.draws[0].transform[3], [13., 4., 5., 1.]);
        near(root_scene.draws[0].transform[0], [0., 0., -1., 0.]);
        root_options.pose.rotations.push([0; 3]);
        assert!(
            posed(&one_part, &root_options)
                .unwrap_err()
                .to_string()
                .contains("count mismatch")
        );
        assert!(inspect(&bytes).is_err());
    }

    #[test]
    fn posed_switch_zero_cases_calls_and_inactive_joints() {
        // Two callees are direct switch children. Their branch commands are not nodes.
        let mut bytes = vec![0x0e, 0, 0, 0, 0x80, 0, 0, 1];
        bytes.extend(OPEN);
        bytes.extend([2, 1, 0, 0, 14, 0, 0, 36]);
        bytes.extend([2, 1, 0, 0, 14, 0, 0, 52]);
        bytes.extend(CLOSE);
        bytes.extend(END);
        bytes.extend(part(10, true));
        bytes.extend([3, 0, 0, 0]);
        bytes.extend(part(20, true));
        bytes.extend([3, 0, 0, 0]);
        let mut options = PoseOptions::default();
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("80000001")
        );
        options.switch_selections.insert(0x80000001, 1);
        options.pose.rotations = vec![[0; 3]];
        let scene = posed(&bytes, &options).unwrap();
        assert_eq!(scene.draws.len(), 1);
        near(scene.draws[0].transform[3], [20., 0., 0., 1.]);
        options.switch_selections.insert(0x80000001, 2);
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("out of range")
        );
        bytes[3] = 1;
        options.switch_selections.insert(0x80000001, 1);
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("out of range")
        );
        bytes[3] = 0;
        bytes[36] = 0xff; // Even unselected command lengths must be known.
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("unknown opcode")
        );
    }

    #[test]
    fn posed_lod_signed_half_open_and_siblings() {
        let bytes = [
            vec![0x0d, 0, 0, 0, 0xff, 0xf6, 0, 10],
            OPEN.to_vec(),
            part(5, true),
            CLOSE.to_vec(),
            part(9, true),
            END.to_vec(),
        ]
        .concat();
        let mut options = PoseOptions {
            render_distance: -10.,
            ..Default::default()
        };
        options.pose.rotations = vec![[0; 3]; 2];
        assert_eq!(posed(&bytes, &options).unwrap().draws.len(), 2);
        options.render_distance = 10.;
        options.pose.rotations = vec![[0; 3]];
        let scene = posed(&bytes, &options).unwrap();
        assert_eq!(scene.draws.len(), 1);
        near(scene.draws[0].transform[3], [9., 0., 0., 1.]);
        options.render_distance = f32::NAN;
        assert!(posed(&bytes, &options).is_err());
    }

    #[test]
    fn posed_callback_allowlist_held_and_dynamic_rejection() {
        let bytes = [
            part(0, false),
            vec![0x18, 0, 0, 0, 0x80, 0, 0, 2],
            OPEN.to_vec(),
            vec![0x15, 1, 0, 0, 7, 0, 0, 0],
            CLOSE.to_vec(),
            vec![0x1c, 0, 0, 0, 0, 0, 0, 0, 0x80, 0, 0, 3],
            OPEN.to_vec(),
            part(1, true),
            CLOSE.to_vec(),
            END.to_vec(),
        ]
        .concat();
        let mut options = PoseOptions::default();
        options.pose.rotations = vec![[0; 3]];
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("80000002")
        );
        options.allowed_omitted_callbacks.insert(0x80000002);
        assert!(
            posed(&bytes, &options)
                .unwrap_err()
                .to_string()
                .contains("allow_empty_held")
        );
        options.allow_empty_held = true;
        let scene = posed(&bytes, &options).unwrap();
        assert_eq!(scene.draws.len(), 1);
        assert!(scene.diagnostics.iter().any(|s| s.contains("80000002")));
        assert!(scene.diagnostics.iter().any(|s| s.contains("80000003")));
        for op in [0x06, 0x07, 0x14, 0x1b, 0xff] {
            assert!(posed(&[op, 0, 0, 0, 0, 0, 0, 0], &options).is_err());
        }
    }

    #[test]
    fn posed_budgets_and_missing_switch_children() {
        let options = PoseOptions::default();
        assert!(
            posed(&[0, 0, 0, 0, 14, 0, 0, 0], &options)
                .unwrap_err()
                .to_string()
                .contains("call-depth")
        );
        assert!(
            posed(&[2, 0, 0, 0, 14, 0, 0, 0], &options)
                .unwrap_err()
                .to_string()
                .contains("command budget")
        );
        let mut options = options;
        options.switch_selections.insert(0x80000001, 0);
        for tail in [END.to_vec(), [OPEN, CLOSE, END].concat()] {
            let bytes = [vec![0x0e, 0, 0, 0, 0x80, 0, 0, 1], tail].concat();
            assert!(posed(&bytes, &options).is_err());
        }
    }

    #[test]
    fn authored_nested_transforms_and_sibling_scope() {
        let bytes = [
            0x11, 0, 0, 10, 0, 20, 0, 30, // translate parent
            4, 0, 0, 0, 0x10, 0x30, 0, 90, // rotate Y
            4, 0, 0, 0, 0x1d, 0x85, 0, 0, 0, 2, 0, 0, 7, 0, 0, 0, // scale with DL
            4, 0, 0, 0, 0x11, 0x81, 0, 1, 0, 0, 0, 0, 7, 0, 0, 0, // translated child with DL
            5, 0, 0, 0, 5, 0, 0, 0, 0x15, 4, 0, 0, 7, 0, 0, 0, // sibling of rotation
            5, 0, 0, 0, 0x15, 1, 0, 0, 7, 0, 0, 0, // root sibling
            1, 0, 0, 0,
        ];
        let scene = inspect(&bytes).unwrap();
        assert_eq!(scene.draws.len(), 4);
        assert_eq!(
            scene.draws.iter().map(|d| d.layer).collect::<Vec<_>>(),
            [5, 1, 4, 1]
        );
        near(scene.draws[0].transform[0], [0., 0., -2., 0.]);
        near(scene.draws[1].transform[3], [10., 20., 28., 1.]);
        near(scene.draws[2].transform[0], [1., 0., 0., 0.]);
        near(scene.draws[2].transform[3], [10., 20., 30., 1.]);
        assert_eq!(scene.draws[3].transform, IDENTITY);
    }

    #[test]
    fn authored_calls_tail_branches_and_terminators() {
        let bytes = [
            0, 0, 0, 0, 14, 0, 0, 12, // link to END-terminated layout
            1, 0, 0, 0, 2, 1, 0, 0, 14, 0, 0, 24, // call RETURN-terminated layout
            1, 0, 0, 0, 2, 0, 0, 0, 14, 0, 0, 32, // tail branch
            0x15, 6, 0, 0, 7, 0, 0, 0, 3, 0, 0, 0,
        ];
        let scene = inspect(&bytes).unwrap();
        assert_eq!(scene.draws.len(), 1);
        assert_eq!(scene.draws[0].layer, 6);
        assert_eq!(scene.commands_inspected, 7);
        assert!(inspect(&[3, 0, 0, 0]).is_err());
        let mut wrong = bytes;
        wrong[40] = 1;
        assert!(
            inspect(&wrong)
                .unwrap_err()
                .to_string()
                .contains("terminator")
        );
    }

    #[test]
    fn rejects_unknown_truncated_unbalanced_and_bad_pointers() {
        for bytes in [
            vec![0xff, 0, 0, 0],
            vec![0x15, 1, 0, 0],
            vec![0x10, 0x40, 0, 0],
            vec![4, 0, 0, 0],
            vec![5, 0, 0, 0],
            vec![0x0b, 0, 0, 0, 4, 0, 0, 0, 1, 0, 0, 0],
            vec![2, 0, 0, 0, 15, 0, 0, 0],
            vec![2, 0, 0, 0, 14, 0, 0, 1],
            vec![0x15, 8, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0],
            vec![0x15, 1, 0, 0, 7, 0, 0, 4, 1, 0, 0, 0],
        ] {
            assert!(inspect(&bytes).is_err(), "accepted {bytes:?}");
        }
        for op in [0x0d, 0x0e, 0x13, 0x14, 0x1c] {
            let mut bytes = vec![0; 16];
            bytes[0] = op;
            assert!(
                inspect(&bytes)
                    .unwrap_err()
                    .to_string()
                    .contains("unsupported subtree")
            );
        }
    }

    #[test]
    fn loops_and_recursion_hit_budgets() {
        assert!(
            inspect(&[2, 0, 0, 0, 14, 0, 0, 0])
                .unwrap_err()
                .to_string()
                .contains("command budget")
        );
        assert!(
            inspect(&[0, 0, 0, 0, 14, 0, 0, 0])
                .unwrap_err()
                .to_string()
                .contains("call-depth")
        );
        let mut bytes = vec![];
        for _ in 0..=MAX_DEPTH {
            bytes.extend([0x0b, 0, 0, 0, 4, 0, 0, 0]);
        }
        assert!(
            inspect(&bytes)
                .unwrap_err()
                .to_string()
                .contains("node-depth")
        );
        let mut bytes = vec![];
        for _ in 0..=MAX_DRAWS {
            bytes.extend([0x15, 1, 0, 0, 7, 0, 0, 0]);
        }
        assert!(
            inspect(&bytes)
                .unwrap_err()
                .to_string()
                .contains("draw budget")
        );
    }

    #[test]
    fn authored_transform_encodings_and_truncated_prefixes() {
        // Each transform is a sibling, so none accidentally accumulates state.
        let commands: &[&[u8]] = &[
            &[
                0x10, 0x81, 0, 0, 0, 3, 0, 4, 0, 5, 0, 0, 0, 0, 0, 90, 7, 0, 0, 0,
            ],
            &[0x10, 0x91, 0, 3, 0, 4, 0, 5, 7, 0, 0, 0],
            &[0x10, 0xa1, 0, 0, 0, 0, 0, 90, 7, 0, 0, 0],
            &[0x10, 0xb1, 0, 90, 7, 0, 0, 0],
            &[0x12, 0x81, 0, 0, 0, 0, 0, 90, 7, 0, 0, 0],
        ];
        let mut fixture = commands.concat();
        fixture.extend([1, 0, 0, 0]);
        let scene = inspect(&fixture).unwrap();
        assert_eq!(scene.draws.len(), 5);
        near(scene.draws[0].transform[3], [3., 4., 5., 1.]);
        near(scene.draws[0].transform[0], [0., 1., 0., 0.]);
        near(scene.draws[1].transform[3], [3., 4., 5., 1.]);
        near(scene.draws[1].transform[0], [1., 0., 0., 0.]);
        near(scene.draws[2].transform[0], [0., 1., 0., 0.]);
        near(scene.draws[3].transform[0], [0., 0., -1., 0.]);
        near(scene.draws[4].transform[0], [0., 1., 0., 0.]);
        for end in 1..fixture.len() {
            assert!(inspect(&fixture[..end]).is_err(), "accepted prefix {end}");
        }
    }

    #[test]
    fn calls_inherit_transform_and_leave_parent_scope_intact() {
        let bytes = [
            0x11, 0, 0, 10, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 14, 0, 0, 36, 5, 0, 0, 0, 0x15, 1,
            0, 0, 7, 0, 0, 0, 1, 0, 0, 0, 0x11, 0x81, 0, 2, 0, 0, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0,
        ];
        let scene = inspect(&bytes).unwrap();
        assert_eq!(scene.draws.len(), 2);
        near(scene.draws[0].transform[3], [12., 0., 0., 1.]);
        assert_eq!(scene.draws[1].transform, IDENTITY);
    }

    #[test]
    fn native_callbacks_are_diagnosed_not_dereferenced() {
        let scene = inspect(&[
            0x18, 0, 0, 0, 0x80, 0xff, 0xff, 0xff, 0x15, 1, 0, 0, 7, 0, 0, 0, 1, 0, 0, 0,
        ])
        .unwrap();
        assert_eq!(scene.draws.len(), 1);
        assert!(scene.diagnostics[0].contains("native ASM"));
    }

    #[test]
    #[ignore = "needs the US revision 0 ROM via SM64_ROM; send output to ignored .cache logs"]
    fn local_bob_geo() {
        let Some(path) = std::env::var_os("SM64_ROM") else {
            eprintln!("skipped: set SM64_ROM to a user-supplied US revision 0 ROM");
            return;
        };
        let rom = sm64_rom::Rom::open(path).unwrap();
        let level = crate::load_bob(&rom).unwrap();
        let scene = inspect_geo(&level.segment_data, level.areas[0].geometry_address).unwrap();
        assert!(!scene.draws.is_empty());
        eprintln!(
            "BOB: {} draws, {} commands",
            scene.draws.len(),
            scene.commands_inspected
        );
        for draw in &scene.draws {
            eprintln!(
                "DL 0x{:08x} layer {} transform {:?}",
                draw.display_list, draw.layer, draw.transform
            );
        }
        for diagnostic in &scene.diagnostics {
            eprintln!("{diagnostic}");
        }
    }
}
