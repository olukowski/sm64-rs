use crate::{
    AlphaMode, Batch, DisplayList, Error, Vertex,
    texture::{self, Tile},
};
use sm64_level::segments::SegmentMap;
use std::collections::{HashMap, HashSet};

const MAX_COMMANDS: usize = 200_000;
const MAX_TRIANGLES: usize = 100_000;
const MAX_DEPTH: usize = 64;
const MAX_TEXTURE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone, Copy, Default)]
struct CachedVertex {
    position: [f32; 3],
    normal: [f32; 3],
    st: [f32; 2],
    color: [f32; 4],
}
#[derive(Clone, Copy, Default)]
struct Light {
    color: [f32; 3],
    direction: [f32; 3],
}
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct TextureKey {
    tile: Tile,
    generation: u64,
}

/// Sequential Fast3D roots sharing lighting and material state.
///
/// Vertex caches are deliberately cleared between roots: independently transformed
/// joint draws must load their own vertices. Cross-root vertex reuse is unsupported
/// and fails rather than applying a new joint transform to old positions.
/// Output, texture indices, diagnostics and budgets are local to each root.
/// Any decoding error permanently poisons the stream; partial output is never returned.
pub struct DisplayListStream<'a> {
    decoder: Decoder<'a>,
    poisoned: bool,
}

impl<'a> DisplayListStream<'a> {
    pub fn new(segments: &'a SegmentMap) -> Self {
        Self {
            decoder: Decoder::new(segments),
            poisoned: false,
        }
    }

    /// Decode one root within both the per-root and caller's remaining command limits.
    pub fn decode_next(
        &mut self,
        entry: u32,
        remaining_commands: usize,
    ) -> Result<DisplayList, Error> {
        self.decode_next_with_normal_transform(
            entry,
            remaining_commands,
            [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        )
    }

    /// Approximate world-space directional shading for posed joints. Matrix is
    /// column-major inverse-transpose of the draw transform; positions/normals
    /// returned to the caller remain local. Not original RSP quantization.
    pub fn decode_next_with_normal_transform(
        &mut self,
        entry: u32,
        remaining_commands: usize,
        normal_transform: [[f32; 3]; 3],
    ) -> Result<DisplayList, Error> {
        if normal_transform.iter().flatten().any(|v| !v.is_finite()) {
            self.poisoned = true;
            return Err(Error("nonfinite lighting normal transform".into()));
        }
        self.decoder.normal_transform = normal_transform;
        if self.poisoned {
            return Err(Error(
                "display-list stream poisoned by an earlier error".into(),
            ));
        }
        let d = &mut self.decoder;
        d.vertices.fill(None);
        d.textures.clear();
        d.texture_has_alpha.clear();
        d.texture_bytes = 0;
        d.triangles = 0;
        d.notices.clear();
        d.command_limit = remaining_commands.min(MAX_COMMANDS);
        match d.run(entry) {
            Ok(()) => Ok(std::mem::replace(&mut d.out, empty_output())),
            Err(error) => {
                self.poisoned = true;
                d.out = empty_output();
                Err(error)
            }
        }
    }
}

fn empty_output() -> DisplayList {
    DisplayList {
        batches: vec![],
        textures: vec![],
        diagnostics: vec![],
        commands_inspected: 0,
    }
}

struct Decoder<'a> {
    segments: &'a SegmentMap,
    out: DisplayList,
    vertices: [Option<CachedVertex>; 16],
    geometry: u32,
    tiles: [Tile; 8],
    image: Option<(u32, u8)>,
    tmem: [u8; 4096],
    valid: [bool; 4096],
    generation: u64,
    textures: HashMap<TextureKey, usize>,
    texture_has_alpha: Vec<bool>,
    texture_bytes: usize,
    texture_on: bool,
    tile: usize,
    scale: [f32; 2],
    lights: [Option<Light>; 8],
    light_count: usize,
    other_l: u32,
    env: [f32; 4],
    prim: [f32; 4],
    alpha_source: u8,
    triangles: usize,
    command_limit: usize,
    notices: HashSet<String>,
    normal_transform: [[f32; 3]; 3],
}
impl<'a> Decoder<'a> {
    fn new(segments: &'a SegmentMap) -> Self {
        Self {
            segments,
            out: empty_output(),
            vertices: [None; 16],
            geometry: 0x22004,
            tiles: [Tile::default(); 8],
            image: None,
            tmem: [0; 4096],
            valid: [false; 4096],
            generation: 0,
            textures: HashMap::new(),
            texture_has_alpha: vec![],
            texture_bytes: 0,
            texture_on: false,
            tile: 0,
            scale: [1.; 2],
            lights: [None; 8],
            light_count: 1,
            other_l: 0,
            env: [1.; 4],
            prim: [1.; 4],
            alpha_source: 4,
            triangles: 0,
            command_limit: MAX_COMMANDS,
            notices: HashSet::new(),
            normal_transform: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        }
    }
    fn note(&mut self, message: &str) {
        if self.notices.insert(message.into()) {
            self.out.diagnostics.push(message.into());
        }
    }
    fn bytes(&self, address: u32, len: usize) -> Result<&[u8], Error> {
        self.segments
            .resolve_range(address, len)
            .map_err(|e| Error(e.to_string()))
    }
    fn load_vertices(&mut self, w0: u32, w1: u32) -> Result<(), Error> {
        let n = ((w0 >> 20) & 15) as usize + 1;
        let start = ((w0 >> 16) & 15) as usize;
        if start + n > 16 {
            return Err(Error("vertex cache load exceeds 16 entries".into()));
        }
        if w0 & 0xffff != (n * 16) as u32 {
            return Err(Error("invalid old Fast3D VTX byte length".into()));
        }
        if self.geometry & 0xc0000 != 0 {
            return Err(Error("unsupported generated texture coordinates".into()));
        }
        let data = self.bytes(w1, n * 16)?.to_vec();
        for (i, v) in data.as_chunks::<16>().0.iter().enumerate() {
            let signed = |at| f32::from(i16::from_be_bytes([v[at], v[at + 1]]));
            let normal = if self.geometry & 0x20000 != 0 {
                normalize([
                    f32::from(v[12] as i8),
                    f32::from(v[13] as i8),
                    f32::from(v[14] as i8),
                ])
            } else {
                [0.; 3]
            };
            let mut color = [
                f32::from(v[12]) / 255.,
                f32::from(v[13]) / 255.,
                f32::from(v[14]) / 255.,
                f32::from(v[15]) / 255.,
            ];
            if self.geometry & 0x20000 != 0 {
                self.note("Lighting is baked at VTX load using normalized normals and directional lights; optional caller-supplied joint normal transform, no RSP quantization or camera transform.");
                let lit_normal = normalize(std::array::from_fn(|row| {
                    (0..3)
                        .map(|col| self.normal_transform[col][row] * normal[col])
                        .sum()
                }));
                if self.lights[self.light_count].is_none() {
                    self.note("Missing inherited light state: neutral ambient approximation used.");
                }
                let ambient = self.lights[self.light_count]
                    .map(|l| l.color)
                    .unwrap_or([0.5; 3]);
                let mut rgb = ambient;
                for j in 0..self.light_count {
                    if let Some(light) = self.lights[j] {
                        let d = dot(lit_normal, normalize(light.direction)).max(0.);
                        for (c, component) in rgb.iter_mut().enumerate() {
                            *component += light.color[c] * d;
                        }
                    } else {
                        self.note(
                            "Missing inherited light state: neutral ambient approximation used.",
                        );
                        for c in &mut rgb {
                            *c += 0.5;
                        }
                    }
                }
                for c in 0..3 {
                    color[c] = rgb[c].clamp(0., 1.);
                }
            }
            self.vertices[start + i] = Some(CachedVertex {
                position: [signed(0), signed(2), signed(4)],
                normal,
                st: [
                    signed(8) / 32. * self.scale[0],
                    signed(10) / 32. * self.scale[1],
                ],
                color,
            });
        }
        Ok(())
    }
    fn texture(&mut self) -> Result<Option<usize>, Error> {
        if !self.texture_on {
            return Ok(None);
        }
        let tile = self.tiles[self.tile];
        let key = TextureKey {
            tile,
            generation: self.generation,
        };
        if let Some(&index) = self.textures.get(&key) {
            return Ok(Some(index));
        }
        for axis in 0..2 {
            let dims = tile.dimensions()?;
            if tile.masks[axis] != 0
                && tile.wrap(axis) != crate::WrapMode::Clamp
                && 1usize << tile.masks[axis] != dims[axis]
            {
                return Err(Error(
                    "texture wrap mask differs from tile rectangle; unsupported period".into(),
                ));
            }
        }
        self.note("Textures use logical LOAD_BLOCK TMEM, half-texel-center UVs, and host filtering; no RDP bank swizzle or three-point filtering.");
        let tex = texture::decode(tile, &self.tmem, &self.valid)?;
        if self.texture_bytes + tex.rgba8.len() > MAX_TEXTURE_BYTES || self.textures.len() >= 4096 {
            return Err(Error("texture cache budget exhausted".into()));
        }
        self.texture_bytes += tex.rgba8.len();
        let index = self.out.textures.len();
        self.texture_has_alpha
            .push(tex.rgba8.as_chunks::<4>().0.iter().any(|c| c[3] < 255));
        self.out.textures.push(tex);
        self.textures.insert(key, index);
        Ok(Some(index))
    }
    fn triangle(&mut self, word: u32) -> Result<(), Error> {
        if self.triangles >= MAX_TRIANGLES {
            return Err(Error("triangle output budget exhausted".into()));
        }
        let mut input = [CachedVertex::default(); 3];
        for (i, v) in input.iter_mut().enumerate() {
            let raw = ((word >> (16 - i * 8)) & 255) as usize;
            if !raw.is_multiple_of(10) || raw / 10 >= 16 {
                return Err(Error(format!("malformed TRI1 vertex index {raw}")));
            }
            *v = self.vertices[raw / 10]
                .ok_or_else(|| Error(format!("TRI1 references unloaded vertex {}", raw / 10)))?;
        }
        if self.geometry & 0x1000 != 0 {
            return Err(Error(
                "front-face culling is not representable by Batch".into(),
            ));
        }
        let texture = self.texture()?;
        // Selected primitive/environment alpha replaces SHADE alpha; it is
        // not multiplied by the raw vertex byte (often zero on Mario limbs).
        let vertex_alpha = |v: &CachedVertex| match self.alpha_source {
            3 => self.prim[3],
            5 => self.env[3],
            _ => v.color[3],
        };
        let mut alpha = if self.other_l & 0x4000 != 0 || input.iter().any(|v| vertex_alpha(v) < 1.)
        {
            AlphaMode::Blend
        } else {
            AlphaMode::Opaque
        };
        if alpha == AlphaMode::Opaque
            && (self.other_l & 3 != 0 || texture.is_some_and(|i| self.texture_has_alpha[i]))
        {
            alpha = AlphaMode::Mask;
        }
        let cull_back = self.geometry & 0x2000 != 0;
        if self
            .out
            .batches
            .last()
            .is_none_or(|b| b.texture != texture || b.cull_back != cull_back || b.alpha != alpha)
        {
            self.out.batches.push(Batch {
                vertices: vec![],
                texture,
                cull_back,
                alpha,
            });
        }
        let dims = if texture.is_some() {
            self.tiles[self.tile].dimensions()?
        } else {
            [1; 2]
        };
        let vertices = input.map(|v| Vertex {
            position: v.position,
            normal: v.normal,
            uv: if texture.is_some() {
                self.tiles[self.tile].uv(v.st, [1.; 2], dims)
            } else {
                [0.; 2]
            },
            color: [v.color[0], v.color[1], v.color[2], vertex_alpha(&v)],
        });
        self.out
            .batches
            .last_mut()
            .unwrap()
            .vertices
            .extend(vertices);
        self.triangles += 1;
        Ok(())
    }
    fn run(&mut self, entry: u32) -> Result<(), Error> {
        let mut pc = entry;
        let mut stack = Vec::new();
        // An address can recur in sequential calls with different state, but not on an active path.
        let mut active = HashSet::new();
        let mut path = Vec::new();
        loop {
            if self.out.commands_inspected >= self.command_limit {
                return Err(Error(
                    "display-list command budget exhausted (possible cycle)".into(),
                ));
            }
            if pc & 7 != 0 {
                return Err(Error(format!("unaligned display-list address {pc:#010x}")));
            }
            if !active.insert(pc) {
                return Err(Error(format!("display-list cycle at {pc:#010x}")));
            }
            path.push(pc);
            let cmd = self.bytes(pc, 8)?;
            let w0 = u32::from_be_bytes(cmd[..4].try_into().unwrap());
            let w1 = u32::from_be_bytes(cmd[4..].try_into().unwrap());
            self.out.commands_inspected += 1;
            let op = (w0 >> 24) as u8;
            let next = pc
                .checked_add(8)
                .filter(|n| n >> 24 == pc >> 24)
                .ok_or_else(|| Error("display-list segment overflow".into()))?;
            let result: Result<(), Error> = (|| {
                match op {
                    0x00 => {}
                    0x04 => self.load_vertices(w0, w1)?,
                    0x06 => {
                        let branch = (w0 >> 16) & 255;
                        if branch > 1 {
                            return Err(Error("invalid DL push flag".into()));
                        }
                        if branch == 0 {
                            if stack.len() >= MAX_DEPTH {
                                return Err(Error("display-list stack budget exhausted".into()));
                            }
                            stack.push((next, path.len()));
                        }
                        if w1 == 0 {
                            return Err(Error("null display-list target".into()));
                        }
                        pc = w1;
                        return Ok(());
                    }
                    0xb8 => {
                        if let Some((ret, len)) = stack.pop() {
                            for address in path.drain(len..) {
                                active.remove(&address);
                            }
                            pc = ret;
                        } else {
                            pc = 0;
                        }
                        return Ok(());
                    }
                    0xbf => self.triangle(w1)?,
                    0xb6 => self.geometry &= !w1,
                    0xb7 => self.geometry |= w1,
                    0xbb => {
                        if w0 & 0x3800 != 0 {
                            return Err(Error("texture mip levels unsupported".into()));
                        }
                        self.texture_on = w0 & 255 != 0;
                        self.tile = ((w0 >> 8) & 7) as usize;
                        self.scale = [(w1 >> 16) as f32 / 65536., (w1 & 65535) as f32 / 65536.];
                    }
                    0xfd => {
                        self.image = Some((w1, ((w0 >> 19) & 3) as u8));
                    }
                    0xf5 => {
                        let t = &mut self.tiles[((w1 >> 24) & 7) as usize];
                        t.format = ((w0 >> 21) & 7) as u8;
                        t.size = ((w0 >> 19) & 3) as u8;
                        t.line = ((w0 >> 9) & 511) as usize;
                        t.tmem = (w0 & 511) as usize;
                        t.modes = [((w1 >> 8) & 3) as u8, ((w1 >> 18) & 3) as u8];
                        t.masks = [((w1 >> 4) & 15) as u8, ((w1 >> 14) & 15) as u8];
                        t.shifts = [(w1 & 15) as u8, ((w1 >> 10) & 15) as u8];
                    }
                    0xf2 => {
                        let t = &mut self.tiles[((w1 >> 24) & 7) as usize];
                        t.ul = [((w0 >> 12) & 4095) as u16, (w0 & 4095) as u16];
                        t.lr = [((w1 >> 12) & 4095) as u16, (w1 & 4095) as u16];
                    }
                    0xf3 => {
                        if w0 & 0xffffff != 0 {
                            return Err(Error(
                                "nonzero LOAD_BLOCK source origin unsupported".into(),
                            ));
                        }
                        let (address, size) = self
                            .image
                            .ok_or_else(|| Error("LOAD_BLOCK without SET_IMAGE".into()))?;
                        let tile = self.tiles[((w1 >> 24) & 7) as usize];
                        if tile.size != size {
                            return Err(Error("LOAD_BLOCK image/tile size mismatch".into()));
                        }
                        let n =
                            ((((w1 >> 12) & 4095) + 1) as usize * texture::bits(size)).div_ceil(8);
                        let offset = tile.tmem * 8;
                        if offset + n > 4096 {
                            return Err(Error("LOAD_BLOCK exceeds TMEM budget".into()));
                        }
                        let source = self.bytes(address, n)?.to_vec();
                        self.tmem[offset..offset + n].copy_from_slice(&source);
                        self.valid[offset..offset + n].fill(true);
                        self.generation += 1;
                    }
                    0x03 => {
                        let index = ((w0 >> 16) & 255) as usize;
                        if (0x86..=0x94).contains(&index) && index.is_multiple_of(2) {
                            let data = self.bytes(w1, 12)?;
                            self.lights[(index - 0x86) / 2] = Some(Light {
                                color: std::array::from_fn(|i| f32::from(data[i]) / 255.),
                                direction: std::array::from_fn(|i| f32::from(data[8 + i] as i8)),
                            });
                        } else {
                            return Err(Error(format!("unsupported MOVEMEM index {index:#x}")));
                        }
                    }
                    0xbc => match w0 & 255 {
                        2 => {
                            let value = w1.wrapping_sub(0x80000000);
                            if !(64..=256).contains(&value) || !value.is_multiple_of(32) {
                                return Err(Error("invalid light count".into()));
                            }
                            self.light_count = (value / 32 - 1) as usize;
                        }
                        8 => self.note("Fog parameters ignored in static materials."),
                        _ => {
                            return Err(Error(format!(
                                "unsupported MOVEWORD index {:#x}",
                                w0 & 255
                            )));
                        }
                    },
                    0xb9 | 0xba => {
                        let shift = (w0 >> 8) & 255;
                        let len = w0 & 255;
                        if len == 0 || shift + len > 32 {
                            return Err(Error("invalid other-mode bit range".into()));
                        }
                        if op == 0xb9 {
                            let mask = if len == 32 {
                                u32::MAX
                            } else {
                                ((1u32 << len) - 1) << shift
                            };
                            self.other_l = (self.other_l & !mask) | (w1 & mask);
                        } else {
                            self.note("RDP texture filtering, perspective, dithering and cycle settings approximated by host materials.");
                        }
                    }
                    0xfa => self.prim = rgba(w1),
                    0xfb => self.env = rgba(w1),
                    0xfc => {
                        self.note("Combiner approximated as texture × baked vertex shade, with primitive/environment alpha when selected; no RDP combiner emulation.");
                        let d = (w1 & 7) as u8;
                        let c = ((w1 >> 18) & 7) as u8;
                        self.alpha_source = if d == 3 || d == 5 {
                            d
                        } else if c == 3 || c == 5 {
                            c
                        } else {
                            4
                        };
                    }
                    0xbe => self.note("CULLDL visibility hint ignored: static geometry retained."),
                    0xe6..=0xe8 => {}
                    0xf8 => self.note("Fog color ignored in static materials."),
                    _ => return Err(Error(format!("unsupported opcode {op:#04x}"))),
                }
                pc = next;
                Ok(())
            })();
            result.map_err(|e| Error(format!("at {:#010x}: {}", *path.last().unwrap(), e)))?;
            if pc == 0 {
                return Ok(());
            }
        }
    }
}
fn rgba(word: u32) -> [f32; 4] {
    word.to_be_bytes().map(|b| f32::from(b) / 255.)
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
fn normalize(v: [f32; 3]) -> [f32; 3] {
    let len = dot(v, v).sqrt();
    if len > 0. {
        v.map(|c| c / len)
    } else {
        [0.; 3]
    }
}
pub(crate) fn decode(segments: &SegmentMap, entry: u32) -> Result<DisplayList, Error> {
    decode_with_budget(segments, entry, MAX_COMMANDS)
}

pub(crate) fn decode_with_budget(
    segments: &SegmentMap,
    entry: u32,
    remaining: usize,
) -> Result<DisplayList, Error> {
    let mut decoder = Decoder::new(segments);
    decoder.command_limit = remaining.min(MAX_COMMANDS);
    decoder.run(entry)?;
    Ok(decoder.out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn segments(data: &[u8]) -> SegmentMap {
        let mut map = SegmentMap::default();
        map.load(data, 7, 0, data.len() as u32, false).unwrap();
        map
    }
    #[test]
    fn scene_budget_is_enforced_before_reading_and_null_calls_do_not_terminate_successfully() {
        let data = [0, 0, 0, 0, 0, 0, 0, 0, 0xb8, 0, 0, 0, 0, 0, 0, 0];
        let map = segments(&data);
        assert!(decode_with_budget(&map, 0x07000000, 0).is_err());
        assert!(decode_with_budget(&map, 0x07000000, 1).is_err());
        assert_eq!(
            decode_with_budget(&map, 0x07000000, 2)
                .unwrap()
                .commands_inspected,
            2
        );
        for flag in [0, 1] {
            let null = [6, flag, 0, 0, 0, 0, 0, 0];
            assert!(
                decode(&segments(&null), 0x07000000)
                    .unwrap_err()
                    .0
                    .contains("null")
            );
        }
    }

    #[test]
    fn bounded_commands_stack_triangles_and_texture_cache() {
        let data = vec![0; (MAX_COMMANDS + 1) * 8];
        let map = segments(&data);
        assert!(
            decode(&map, 0x07000000)
                .unwrap_err()
                .0
                .contains("command budget")
        );
        let mut calls = vec![0; (MAX_DEPTH + 2) * 8];
        for (i, command) in calls.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            command[0] = 6;
            command[4..].copy_from_slice(&(0x07000000 + ((i + 1) * 8) as u32).to_be_bytes());
        }
        assert!(
            decode(&segments(&calls), 0x07000000)
                .unwrap_err()
                .0
                .contains("stack budget")
        );
        let mut decoder = Decoder::new(&map);
        decoder.triangles = MAX_TRIANGLES;
        assert!(
            decoder
                .triangle(0)
                .unwrap_err()
                .0
                .contains("triangle output budget")
        );
        decoder.texture_on = true;
        decoder.tiles[0].format = 4;
        decoder.valid.fill(true);
        decoder.texture_bytes = MAX_TEXTURE_BYTES;
        assert!(
            decoder
                .texture()
                .unwrap_err()
                .0
                .contains("texture cache budget")
        );
    }
    #[test]
    fn source_and_tmem_bounds() {
        for words in [
            [
                (0xfd100000u32, 0x07000038u32),
                (0xf5100000, 0x07000000),
                (0xf3000000, 0x07007000),
            ],
            [
                (0xfd100000, 0x07000000),
                (0xf51001ff, 0x07000000),
                (0xf3000000, 0x07007000),
            ],
        ] {
            let mut data = vec![0; 64];
            for (command, (a, b)) in data.as_chunks_mut::<8>().0.iter_mut().zip(words) {
                command[..4].copy_from_slice(&a.to_be_bytes());
                command[4..].copy_from_slice(&b.to_be_bytes());
            }
            assert!(decode(&segments(&data), 0x07000000).is_err());
        }
    }
}
