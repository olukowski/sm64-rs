//! Logical TMEM loads: source bytes are retained at LOAD_BLOCK time, not SET_IMAGE.
use crate::{Error, Texture, WrapMode};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct Tile {
    pub format: u8,
    pub size: u8,
    pub line: usize,
    pub tmem: usize,
    pub modes: [u8; 2],
    pub masks: [u8; 2],
    pub shifts: [u8; 2],
    pub ul: [u16; 2],
    pub lr: [u16; 2],
}
impl Tile {
    pub fn dimensions(self) -> Result<[usize; 2], Error> {
        let mut dims = [0; 2];
        for (axis, dim) in dims.iter_mut().enumerate() {
            if self.lr[axis] < self.ul[axis] {
                return Err(Error("inverted tile rectangle".into()));
            }
            *dim = usize::from((self.lr[axis] - self.ul[axis]) / 4 + 1);
        }
        if dims[0] * dims[1] > 1024 * 1024 {
            return Err(Error("texture pixel budget exhausted".into()));
        }
        Ok(dims)
    }
    pub fn wrap(self, axis: usize) -> WrapMode {
        if self.modes[axis] & 2 != 0 || self.masks[axis] == 0 {
            WrapMode::Clamp
        } else if self.modes[axis] & 1 != 0 {
            WrapMode::MirrorRepeat
        } else {
            WrapMode::Repeat
        }
    }
    pub fn uv(self, st: [f32; 2], scale: [f32; 2], dims: [usize; 2]) -> [f32; 2] {
        std::array::from_fn(|i| {
            let shift = self.shifts[i];
            let factor = if shift <= 10 {
                2f32.powi(-i32::from(shift))
            } else {
                2f32.powi(i32::from(16 - shift))
            };
            (st[i] * scale[i] * factor - f32::from(self.ul[i]) / 4.0 + 0.5) / dims[i] as f32
        })
    }
}

pub(crate) fn bits(size: u8) -> usize {
    4usize << size
}

/// Decode linear logical texels. RDP odd-row bank swaps are not applied to source bytes.
pub(crate) fn decode(
    tile: Tile,
    bytes: &[u8; 4096],
    valid: &[bool; 4096],
) -> Result<Texture, Error> {
    let [width, height] = tile.dimensions()?;
    let bpp = bits(tile.size);
    let row = if tile.line == 0 {
        (width * bpp).div_ceil(8)
    } else {
        tile.line * 8 * if tile.size == 3 { 2 } else { 1 }
    };
    if row * 8 < width * bpp {
        return Err(Error("tile line is shorter than its rectangle".into()));
    }
    let mut rgba8 = Vec::with_capacity(width * height * 4);
    for y in 0..height {
        for x in 0..width {
            let bit = x * bpp;
            let offset = tile.tmem * 8 + y * row + bit / 8;
            let count = bpp.div_ceil(8);
            if offset
                .checked_add(count)
                .is_none_or(|end| end > bytes.len())
                || !valid[offset..offset + count].iter().all(|v| *v)
            {
                return Err(Error(format!("texture reads unloaded TMEM at {offset:#x}")));
            }
            let src = &bytes[offset..offset + count];
            rgba8.extend(expand(tile.format, tile.size, src, bit.is_multiple_of(8))?);
        }
    }
    Ok(Texture {
        width: width as u32,
        height: height as u32,
        rgba8,
        wrap_s: tile.wrap(0),
        wrap_t: tile.wrap(1),
    })
}
fn expand(format: u8, size: u8, src: &[u8], high: bool) -> Result<[u8; 4], Error> {
    let n = if high { src[0] >> 4 } else { src[0] & 15 };
    Ok(match (format, size) {
        (0, 2) => {
            let v = u16::from_be_bytes([src[0], src[1]]);
            let five = |shift: u32| {
                let c = ((v >> shift) & 31u16) as u8;
                (c << 3) | (c >> 2)
            };
            [five(11), five(6), five(1), if v & 1 == 0 { 0 } else { 255 }]
        }
        (0, 3) => [src[0], src[1], src[2], src[3]],
        (3, 0) => {
            let i = (u16::from(n >> 1) * 255 / 7) as u8;
            [i, i, i, if n & 1 == 0 { 0 } else { 255 }]
        }
        (3, 1) => {
            let i = (src[0] >> 4) * 17;
            [i, i, i, (src[0] & 15) * 17]
        }
        (3, 2) => [src[0], src[0], src[0], src[1]],
        (4, 0) => [n * 17; 4],
        (4, 1) => [src[0]; 4],
        (2, _) => {
            return Err(Error(
                "CI texture requires unsupported palette/TLUT decoding".into(),
            ));
        }
        _ => {
            return Err(Error(format!(
                "unsupported texture format {format} size {size}"
            )));
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn channels() {
        assert_eq!(expand(0, 2, &[0xf8, 1], true).unwrap(), [255, 0, 0, 255]);
        assert_eq!(expand(0, 3, &[1, 2, 3, 4], true).unwrap(), [1, 2, 3, 4]);
        assert_eq!(expand(3, 0, &[0xf0], true).unwrap(), [255; 4]);
        assert_eq!(expand(3, 1, &[0x8f], true).unwrap(), [136, 136, 136, 255]);
        assert_eq!(expand(3, 2, &[42, 7], true).unwrap(), [42, 42, 42, 7]);
        assert_eq!(expand(4, 0, &[0x3a], false).unwrap(), [170; 4]);
        assert_eq!(expand(4, 1, &[42], true).unwrap(), [42; 4]);
        assert!(expand(2, 1, &[0], true).is_err());
    }
    #[test]
    fn dimensions_uv_wrap_bounds() {
        let mut t = Tile {
            lr: [124, 60],
            masks: [5, 4],
            modes: [1, 2],
            ..Default::default()
        };
        assert_eq!(t.dimensions().unwrap(), [32, 16]);
        assert_eq!(t.wrap(0), WrapMode::MirrorRepeat);
        assert_eq!(t.wrap(1), WrapMode::Clamp);
        assert_eq!(t.uv([-0.5, -0.5], [1., 1.], [32, 16]), [0., 0.]);
        t.shifts = [1, 15];
        t.ul = [4, 8];
        assert_eq!(t.uv([4., 2.], [1., 1.], [32, 16]), [1.5 / 32., 2.5 / 16.]);
        t.ul[0] = 128;
        assert!(t.dimensions().is_err());
        t.ul[0] = 0;
        assert!(decode(t, &[0; 4096], &[false; 4096]).is_err());
    }
}
