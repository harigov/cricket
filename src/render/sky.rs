//! Procedural day/night sky textures (generated once at startup).

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

const SKY_W: u32 = 2048;
const SKY_H: u32 = 1024;

/// Per-texel hash threshold for discrete stars (`h > STAR_THRESHOLD`).
const STAR_THRESHOLD: f32 = 0.9991;
/// Stars only appear above this normalised altitude (`v` = 0 horizon, 1 zenith).
const STAR_MIN_V: f32 = 0.28;

/// Deterministic 2D hash in `[0, 1)`.
pub fn sky_hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut n = x
        .wrapping_mul(374_761_393)
        .wrapping_add(y.wrapping_mul(668_265_263))
        .wrapping_add(seed.wrapping_mul(982_451_653));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    (n & 0x00FF_FFFF) as f32 / 16_777_215.0
}

/// Smooth value noise with bilinear interpolation.
pub fn sky_value_noise(u: f32, v: f32, scale: f32, seed: u32) -> f32 {
    let x = u * scale;
    let y = v * scale;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let smooth = |t: f32| t * t * (3.0 - 2.0 * t);
    let sfx = smooth(fx);
    let sfy = smooth(fy);

    let sample = |xi: i32, yi: i32| -> f32 { sky_hash(xi as u32, yi as u32, seed) };

    let a = sample(ix, iy);
    let b = sample(ix + 1, iy);
    let c = sample(ix, iy + 1);
    let d = sample(ix + 1, iy + 1);
    let ab = a + (b - a) * sfx;
    let cd = c + (d - c) * sfx;
    ab + (cd - ab) * sfy
}

/// Multi-octave fractal noise for cloud/haze layers.
pub fn sky_fbm(u: f32, v: f32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.55;
    let mut freq = 2.4;
    for octave in 0..4 {
        sum += sky_value_noise(u, v, freq, seed.wrapping_add(octave * 97)) * amp;
        amp *= 0.5;
        freq *= 2.05;
    }
    sum
}

/// Sample procedural sky colour at normalised UV `(u, v)` where `v=0` is horizon
/// and `v=1` is zenith.
pub fn sample_sky_color(u: f32, v: f32, night: bool) -> [f32; 3] {
    if night {
        sample_night_sky(u, v)
    } else {
        sample_day_sky(u, v)
    }
}

fn sample_day_sky(u: f32, v: f32) -> [f32; 3] {
    let t = v.clamp(0.0, 1.0).powf(1.12);
    // Horizon haze warms toward pale gold; zenith deep saturated blue.
    let horizon = [0.78_f32, 0.86, 0.96];
    let zenith = [0.22, 0.46, 0.86];
    let mut rgb = [
        horizon[0] + (zenith[0] - horizon[0]) * t,
        horizon[1] + (zenith[1] - horizon[1]) * t,
        horizon[2] + (zenith[2] - horizon[2]) * t,
    ];

    // Restrained cloud wisps — stronger near mid-altitude, fade at zenith/horizon.
    let cloud_mask = (1.0 - (v - 0.42).abs() * 2.2).clamp(0.0, 1.0);
    let n1 = sky_fbm(u + 0.17, v * 0.9 + 0.04, 11);
    let n2 = sky_fbm(u * 1.3 + 0.5, v * 1.1, 29) * 0.45;
    let clouds = ((n1 + n2) * 0.5).powf(1.6) * cloud_mask * 0.20;
    rgb[0] += clouds * 0.32;
    rgb[1] += clouds * 0.26;
    rgb[2] += clouds * 0.14;

    // Subtle horizontal haze variation (not stripes).
    let haze = sky_value_noise(u, v * 0.35 + 0.1, 1.8, 53) * 0.028;
    rgb[0] += haze;
    rgb[1] += haze * 0.9;
    rgb[2] += haze * 0.65;

    rgb.map(|c| c.clamp(0.0, 1.0))
}

fn sample_night_sky(u: f32, v: f32) -> [f32; 3] {
    let t = v.clamp(0.0, 1.0).powf(1.08);
    let horizon = [0.04_f32, 0.06, 0.12];
    let zenith = [0.01, 0.02, 0.06];
    let mut rgb = [
        horizon[0] + (zenith[0] - horizon[0]) * t,
        horizon[1] + (zenith[1] - horizon[1]) * t,
        horizon[2] + (zenith[2] - horizon[2]) * t,
    ];

    if v > STAR_MIN_V {
        let star_u = (u * SKY_W as f32).floor() as u32;
        let star_v = (v * SKY_H as f32).floor() as u32;
        let h = sky_hash(star_u, star_v, 9001);
        // ~0.09% of cells become stars — discrete 2D distribution.
        if h > STAR_THRESHOLD {
            let intensity = 0.45 + sky_hash(star_v, star_u, 4243) * 0.55;
            let twinkle = sky_hash(star_u ^ 17, star_v ^ 31, 771) * 0.15;
            let star = (intensity + twinkle).min(1.0);
            rgb[0] += star * 0.95;
            rgb[1] += star * 0.97;
            rgb[2] += star;
        }
    }

    rgb.map(|c| c.clamp(0.0, 1.0))
}

/// Build a complete sky texture image for day (`night = false`) or night.
pub fn create_sky_texture(night: bool) -> Image {
    let mut data = Vec::with_capacity((SKY_W * SKY_H * 4) as usize);
    for y in 0..SKY_H {
        let v = (y as f32 + 0.5) / SKY_H as f32;
        for x in 0..SKY_W {
            let u = (x as f32 + 0.5) / SKY_W as f32;
            let rgb = sample_sky_color(u, v, night);
            data.extend_from_slice(&[
                (rgb[0] * 255.0) as u8,
                (rgb[1] * 255.0) as u8,
                (rgb[2] * 255.0) as u8,
                255,
            ]);
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: SKY_W,
            height: SKY_H,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.usage =
        TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::RENDER_ATTACHMENT;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..Default::default()
    });
    img
}

/// Select cached day/night sky texture handle for the current stadium time.
pub fn sky_texture_for_time<'a>(
    night: bool,
    day: &'a Handle<Image>,
    night_tex: &'a Handle<Image>,
) -> &'a Handle<Image> {
    if night { night_tex } else { day }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sky_generation_is_deterministic() {
        let a = create_sky_texture(false);
        let b = create_sky_texture(false);
        assert_eq!(a.data.as_ref().unwrap(), b.data.as_ref().unwrap());
    }

    #[test]
    fn day_and_night_differ() {
        let day = create_sky_texture(false);
        let night = create_sky_texture(true);
        assert_ne!(day.data.as_ref().unwrap(), night.data.as_ref().unwrap());
    }

    #[test]
    fn night_stars_are_not_full_horizontal_rows() {
        let img = create_sky_texture(true);
        let data = img.data.as_ref().unwrap();
        let w = SKY_W as usize;
        let mut rows_all_bright = 0usize;
        for y in (SKY_H as usize / 3)..(SKY_H as usize * 2 / 3) {
            let bright = (0..w)
                .filter(|&x| {
                    let i = (y * w + x) * 4;
                    data[i] > 240 && data[i + 1] > 240 && data[i + 2] > 240
                })
                .count();
            if bright > w / 2 {
                rows_all_bright += 1;
            }
        }
        assert_eq!(rows_all_bright, 0, "found horizontal star bands");
    }

    fn star_region_cell_count() -> usize {
        let rows = (0..SKY_H)
            .filter(|&y| {
                let v = (y as f32 + 0.5) / SKY_H as f32;
                v > STAR_MIN_V
            })
            .count();
        SKY_W as usize * rows
    }

    fn count_hash_stars() -> usize {
        (0..SKY_H)
            .map(|y| {
                let v = (y as f32 + 0.5) / SKY_H as f32;
                if v <= STAR_MIN_V {
                    return 0;
                }
                (0..SKY_W)
                    .filter(|&x| sky_hash(x, y, 9001) > STAR_THRESHOLD)
                    .count()
            })
            .sum()
    }

    #[test]
    fn night_has_discrete_star_count_in_range() {
        let hash_stars = count_hash_stars();
        let region_cells = star_region_cell_count();
        let expected = region_cells as f32 * (1.0 - STAR_THRESHOLD);
        let min_expected = (expected * 0.65).floor() as usize;
        let max_expected = (expected * 1.35).ceil() as usize;
        assert!(
            hash_stars >= min_expected,
            "too few stars: {hash_stars} (expected ~{expected:.0}, min {min_expected})"
        );
        assert!(
            hash_stars <= max_expected,
            "too many stars: {hash_stars} (expected ~{expected:.0}, max {max_expected})"
        );
        assert!(hash_stars >= 1000, "too few stars: {hash_stars}");
        assert!(hash_stars <= 2200, "too many stars: {hash_stars}");

        // Generated texture should contain one bright texel per hash star (dim stars ~120+).
        let img = create_sky_texture(true);
        let data = img.data.as_ref().unwrap();
        let w = SKY_W as usize;
        let mut tex_stars = 0usize;
        for y in 0..SKY_H as usize {
            for x in 0..w {
                let i = (y * w + x) * 4;
                if data[i] > 120 && data[i + 1] > 120 && data[i + 2] > 120 {
                    tex_stars += 1;
                }
            }
        }
        assert!(
            tex_stars >= hash_stars.saturating_sub(16),
            "texture star texels ({tex_stars}) diverged from hash count ({hash_stars})"
        );
    }

    #[test]
    fn day_sky_has_vertical_gradient() {
        let bottom = sample_sky_color(0.5, 0.05, false);
        let top = sample_sky_color(0.5, 0.95, false);
        // Zenith is deeper/darker than the pale horizon.
        let bottom_luma = bottom[0] * 0.299 + bottom[1] * 0.587 + bottom[2] * 0.114;
        let top_luma = top[0] * 0.299 + top[1] * 0.587 + top[2] * 0.114;
        assert!(top_luma < bottom_luma - 0.08);
    }

    #[test]
    fn texture_selector_picks_correct_handle() {
        let day = Handle::<Image>::default();
        let night = Handle::<Image>::default();
        assert!(std::ptr::eq(
            sky_texture_for_time(false, &day, &night),
            &day
        ));
        assert!(std::ptr::eq(
            sky_texture_for_time(true, &day, &night),
            &night
        ));
    }
}
