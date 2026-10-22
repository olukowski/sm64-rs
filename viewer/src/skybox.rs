//! Host cubemap projection of ROM skybox tiles, not the N64 scrolling algorithm.
use anyhow::{Result, bail};
use bevy::{
    asset::RenderAssetUsages,
    image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    prelude::{Image, Vec3, default},
    render::render_resource::{
        Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
    },
};
use sm64_gfx::Texture;

const CUBE_SIDE: u32 = 256;

fn face_direction(face: usize, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1., -v, -u),
        1 => Vec3::new(-1., -v, u),
        2 => Vec3::new(u, 1., v),
        3 => Vec3::new(u, -1., -v),
        4 => Vec3::new(u, -v, 1.),
        5 => Vec3::new(-u, -v, -1.),
        _ => unreachable!("six cubemap faces"),
    }
    .normalize()
}

fn panorama_uv(direction: Vec3) -> (f32, f32) {
    let u = 0.5 + direction.z.atan2(direction.x) / std::f32::consts::TAU;
    let v = 0.5 - direction.y.clamp(-1., 1.).asin() / std::f32::consts::PI;
    (u, v)
}

fn sample(panorama: &Texture, u: f32, v: f32) -> [u8; 4] {
    let w = panorama.width as isize;
    let h = panorama.height as isize;
    let fx = u * w as f32 - 0.5;
    let fy = (v * h as f32 - 0.5).clamp(0., (h - 1) as f32);
    let (x, y) = (fx.floor() as isize, fy.floor() as isize);
    let (tx, ty) = (fx - fx.floor(), fy - fy.floor());
    let pixel = |x: isize, y: isize, c: usize| {
        f32::from(
            panorama.rgba8
                [(y.clamp(0, h - 1) as usize * w as usize + x.rem_euclid(w) as usize) * 4 + c],
        )
    };
    let mut rgba = [255; 4];
    for (c, value) in rgba[..3].iter_mut().enumerate() {
        let a = pixel(x, y, c) * (1. - tx) + pixel(x + 1, y, c) * tx;
        let b = pixel(x, y + 1, c) * (1. - tx) + pixel(x + 1, y + 1, c) * tx;
        *value = (a * (1. - ty) + b * ty).round().clamp(0., 255.) as u8;
    }
    rgba
}

/// Six D2 layers in +X,-X,+Y,-Y,+Z,-Z order, sampled from a logical
/// equirectangular panorama. Projection/orientation/filtering are presentation
/// choices, not the original screen-aligned skybox's camera-angle rules.
pub fn cube_image(panorama: &Texture) -> Result<Image> {
    if panorama.width != 256 || panorama.height != 256 || panorama.rgba8.len() != 256 * 256 * 4 {
        bail!("skybox panorama must be a complete 256x256 RGBA8 image");
    }
    let mut data = Vec::with_capacity((CUBE_SIDE * CUBE_SIDE * 6 * 4) as usize);
    for face in 0..6 {
        for y in 0..CUBE_SIDE {
            for x in 0..CUBE_SIDE {
                let u = 2. * (x as f32 + 0.5) / CUBE_SIDE as f32 - 1.;
                let v = 2. * (y as f32 + 0.5) / CUBE_SIDE as f32 - 1.;
                let (s, t) = panorama_uv(face_direction(face, u, v));
                data.extend(sample(panorama, s, t));
            }
        }
    }
    let mut image = Image::new(
        Extent3d {
            width: CUBE_SIDE,
            height: CUBE_SIDE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::default(),
    );
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..default()
    });
    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        ..default()
    });
    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sm64_gfx::WrapMode;
    #[test]
    fn projection_axes_and_seams_are_explicit() {
        let (_, north) = panorama_uv(Vec3::Y);
        let (_, south) = panorama_uv(Vec3::NEG_Y);
        assert_eq!(north, 0.);
        assert_eq!(south, 1.);
        assert_eq!(panorama_uv(Vec3::X), (0.5, 0.5));
        assert!(face_direction(2, 0., 0.).abs_diff_eq(Vec3::Y, 1e-6));
        let panorama = Texture {
            width: 256,
            height: 256,
            rgba8: vec![99; 256 * 256 * 4],
            wrap_s: WrapMode::Repeat,
            wrap_t: WrapMode::Clamp,
        };
        assert_eq!(sample(&panorama, 0., 0.5), sample(&panorama, 1., 0.5));
        let image = cube_image(&panorama).unwrap();
        assert_eq!(image.texture_descriptor.size.depth_or_array_layers, 6);
        assert_eq!(image.data.as_ref().unwrap().len(), 256 * 256 * 6 * 4);
        assert_eq!(
            image.texture_view_descriptor.unwrap().dimension,
            Some(TextureViewDimension::Cube)
        );
        let mut invalid = panorama;
        invalid.rgba8.pop();
        assert!(cube_image(&invalid).is_err());
    }
}
