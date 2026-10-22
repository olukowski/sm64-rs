//! Safe static Fast3D decoding, not an RSP/RDP emulator.
use sm64_level::segments::SegmentMap;
mod decoder;
mod texture;

pub use decoder::DisplayListStream;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct Error(pub String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WrapMode {
    Repeat,
    MirrorRepeat,
    Clamp,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AlphaMode {
    Opaque,
    Mask,
    Blend,
}
#[derive(Debug, Clone)]
pub struct Texture {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
    pub wrap_s: WrapMode,
    pub wrap_t: WrapMode,
}
#[derive(Debug, Clone)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
#[derive(Debug, Clone)]
pub struct Batch {
    /// Flattened triangles: every consecutive three vertices is one triangle.
    pub vertices: Vec<Vertex>,
    pub texture: Option<usize>,
    pub cull_back: bool,
    pub alpha: AlphaMode,
}
#[derive(Debug, Clone)]
pub struct DisplayList {
    pub batches: Vec<Batch>,
    pub textures: Vec<Texture>,
    pub diagnostics: Vec<String>,
    pub commands_inspected: usize,
}
pub fn decode_display_list(segments: &SegmentMap, entry: u32) -> Result<DisplayList, Error> {
    decoder::decode(segments, entry)
}

/// Decode with a caller's remaining scene-wide command budget. The per-list
/// limit still applies; zero allowance fails before reading any command.
pub fn decode_display_list_with_budget(
    segments: &SegmentMap,
    entry: u32,
    remaining_commands: usize,
) -> Result<DisplayList, Error> {
    decoder::decode_with_budget(segments, entry, remaining_commands)
}
