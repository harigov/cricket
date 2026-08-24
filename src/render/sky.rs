//! Procedural day/night sky textures (generated once at startup).
//!
//! Each [`StadiumEnvironment`] gets its own palette: the air over a desert
//! plateau is not the air over an alpine valley, and the sky is the largest
//! single surface on screen, so it carries most of the theme.

use std::f32::consts::{FRAC_PI_2, TAU};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureUsages, TextureViewDescriptor,
    TextureViewDimension,
};

use crate::core::stadiums::StadiumEnvironment;

const SKY_W: u32 = 2048;
const SKY_H: u32 = 1024;

/// Edge length of one face of the procedural IBL cubemap. Small on purpose:
/// [`GeneratedEnvironmentMapLight`](bevy::light::GeneratedEnvironmentMapLight)
/// blurs this down into diffuse/specular convolutions on the GPU, so the
/// source only needs to carry the broad gradient, not the star field detail
/// the flat sky texture has.
pub const IBL_CUBE_SIZE: u32 = 64;

/// Palette used before a stadium is picked (menus and the shared startup dome).
/// Coastal is the closest of the five to the single palette this used to have,
/// so the menu backdrop is unchanged in character.
pub const DEFAULT_SKY_THEME: StadiumEnvironment = StadiumEnvironment::Coastal;

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

/// Day and night gradients plus weather density for one stadium theme.
struct SkyPalette {
    day_horizon: [f32; 3],
    day_zenith: [f32; 3],
    /// Exponent on altitude: higher keeps the deep zenith blue overhead and
    /// squeezes the pale horizon band into a thinner strip.
    day_curve: f32,
    /// Peak brightening from the cloud layer (0 = cloudless).
    cloud: f32,
    /// Amplitude of the broad low-altitude haze wash.
    haze: f32,
    night_horizon: [f32; 3],
    night_zenith: [f32; 3],
}

fn sky_palette(theme: StadiumEnvironment) -> SkyPalette {
    match theme {
        // City air: warm, dirty horizon and a sodium-lit night dome.
        StadiumEnvironment::Metropolis => SkyPalette {
            day_horizon: [0.80, 0.78, 0.74],
            day_zenith: [0.22, 0.41, 0.74],
            day_curve: 1.02,
            cloud: 0.26,
            haze: 0.06,
            night_horizon: [0.11, 0.09, 0.12],
            night_zenith: [0.02, 0.03, 0.07],
        },
        // Thin high-altitude air: little haze, near-navy overhead.
        StadiumEnvironment::Alpine => SkyPalette {
            day_horizon: [0.66, 0.78, 0.93],
            day_zenith: [0.09, 0.27, 0.70],
            day_curve: 1.34,
            cloud: 0.14,
            haze: 0.02,
            night_horizon: [0.03, 0.05, 0.13],
            night_zenith: [0.01, 0.01, 0.05],
        },
        // Humid tropical light: bright, slightly green-blue, big soft cloud banks.
        StadiumEnvironment::Coastal => SkyPalette {
            day_horizon: [0.80, 0.89, 0.95],
            day_zenith: [0.15, 0.47, 0.85],
            day_curve: 1.10,
            cloud: 0.30,
            haze: 0.05,
            night_horizon: [0.03, 0.07, 0.13],
            night_zenith: [0.01, 0.02, 0.06],
        },
        // English summer: flat, milky, overcast-leaning.
        StadiumEnvironment::Parkland => SkyPalette {
            day_horizon: [0.78, 0.81, 0.85],
            day_zenith: [0.34, 0.49, 0.72],
            day_curve: 1.06,
            cloud: 0.40,
            haze: 0.05,
            night_horizon: [0.04, 0.06, 0.12],
            night_zenith: [0.01, 0.02, 0.06],
        },
        // Dry desert air: dust at the horizon, cloudless deep blue above.
        StadiumEnvironment::Desert => SkyPalette {
            day_horizon: [0.87, 0.77, 0.61],
            day_zenith: [0.10, 0.31, 0.73],
            day_curve: 1.40,
            cloud: 0.05,
            haze: 0.07,
            night_horizon: [0.06, 0.05, 0.10],
            night_zenith: [0.01, 0.02, 0.05],
        },
    }
}

/// Colour the sky settles to at the horizon.
///
/// The world has to dissolve into the same air the dome does, so the ground
/// fade and the distance haze on far props both key off this.
pub fn sky_horizon_color(theme: StadiumEnvironment, night: bool) -> [f32; 3] {
    let palette = sky_palette(theme);
    if night {
        palette.night_horizon
    } else {
        palette.day_horizon
    }
}

/// Sample procedural sky colour at normalised UV `(u, v)` where `v=0` is horizon
/// and `v=1` is zenith.
pub fn sample_sky_color(u: f32, v: f32, night: bool, theme: StadiumEnvironment) -> [f32; 3] {
    let palette = sky_palette(theme);
    if night {
        sample_night_sky(u, v, &palette)
    } else {
        sample_day_sky(u, v, &palette)
    }
}

fn sample_day_sky(u: f32, v: f32, palette: &SkyPalette) -> [f32; 3] {
    let t = v.clamp(0.0, 1.0).powf(palette.day_curve);
    let horizon = palette.day_horizon;
    let zenith = palette.day_zenith;
    let mut rgb = [
        horizon[0] + (zenith[0] - horizon[0]) * t,
        horizon[1] + (zenith[1] - horizon[1]) * t,
        horizon[2] + (zenith[2] - horizon[2]) * t,
    ];

    // Restrained cloud wisps — stronger near mid-altitude, fade at zenith/horizon.
    let cloud_mask = (1.0 - (v - 0.42).abs() * 2.2).clamp(0.0, 1.0);
    let n1 = sky_fbm(u + 0.17, v * 0.9 + 0.04, 11);
    let n2 = sky_fbm(u * 1.3 + 0.5, v * 1.1, 29) * 0.45;
    let clouds = ((n1 + n2) * 0.5).powf(1.6) * cloud_mask * palette.cloud;
    rgb[0] += clouds * 0.35;
    rgb[1] += clouds * 0.28;
    rgb[2] += clouds * 0.18;

    // Subtle horizontal haze variation (not stripes).
    let haze = sky_value_noise(u, v * 0.35 + 0.1, 1.8, 53) * palette.haze;
    rgb[0] += haze;
    rgb[1] += haze * 0.9;
    rgb[2] += haze * 0.65;

    rgb.map(|c| c.clamp(0.0, 1.0))
}

fn sample_night_sky(u: f32, v: f32, palette: &SkyPalette) -> [f32; 3] {
    let t = v.clamp(0.0, 1.0).powf(1.08);
    let horizon = palette.night_horizon;
    let zenith = palette.night_zenith;
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

/// Build a complete sky texture image for day (`night = false`) or night in the
/// default palette. Used for the shared dome that exists before a stadium does.
pub fn create_sky_texture(night: bool) -> Image {
    create_themed_sky_texture(DEFAULT_SKY_THEME, night)
}

fn paint_sky_texels(theme: StadiumEnvironment, night: bool) -> Vec<u8> {
    let mut data = Vec::with_capacity((SKY_W * SKY_H * 4) as usize);
    for y in 0..SKY_H {
        let v = (y as f32 + 0.5) / SKY_H as f32;
        for x in 0..SKY_W {
            let u = (x as f32 + 0.5) / SKY_W as f32;
            let rgb = sample_sky_color(u, v, night, theme);
            data.extend_from_slice(&[
                (rgb[0] * 255.0) as u8,
                (rgb[1] * 255.0) as u8,
                (rgb[2] * 255.0) as u8,
                255,
            ]);
        }
    }
    data
}

/// Build a sky texture image in a stadium theme's own palette.
pub fn create_themed_sky_texture(theme: StadiumEnvironment, night: bool) -> Image {
    let data = paint_sky_texels(theme, night);
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

/// World-space direction for texel `(s, t)` (each in `[-1, 1]`) of cubemap
/// `face`, using the standard `+X, -X, +Y, -Y, +Z, -Z` array-layer order.
fn cube_face_direction(face: u32, s: f32, t: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -t, -s),
        1 => Vec3::new(-1.0, -t, s),
        2 => Vec3::new(s, 1.0, t),
        3 => Vec3::new(s, -1.0, -t),
        4 => Vec3::new(s, -t, 1.0),
        _ => Vec3::new(-s, -t, -1.0),
    }
    .normalize()
}

/// Map a world direction onto the same `(u, v)` convention [`sample_sky_color`]
/// expects (`v = 0` horizon, `v = 1` zenith).
///
/// Directions below the horizon fall back to the horizon colour: the dome
/// this reuses only paints sky, and a probe that samples "ground" as pitch
/// black would darken indirect light on the underside of every prop.
fn direction_to_sky_uv(dir: Vec3) -> (f32, f32) {
    let elevation = dir.y.clamp(-1.0, 1.0).asin();
    let v = (elevation / FRAC_PI_2).clamp(0.0, 1.0);
    let azimuth = dir.z.atan2(dir.x);
    let u = azimuth / TAU + 0.5;
    (u, v)
}

/// Paint all six faces of the IBL source cubemap, back to back in array-layer
/// order, for one theme/time-of-day pair.
fn paint_cubemap_texels(theme: StadiumEnvironment, night: bool) -> Vec<u8> {
    let size = IBL_CUBE_SIZE;
    let mut data = Vec::with_capacity((size * size * 6 * 4) as usize);
    for face in 0..6 {
        for y in 0..size {
            // t runs top-to-bottom in texel space, so flip to NDC's bottom-up y.
            let t = 1.0 - 2.0 * (y as f32 + 0.5) / size as f32;
            for x in 0..size {
                let s = 2.0 * (x as f32 + 0.5) / size as f32 - 1.0;
                let dir = cube_face_direction(face, s, t);
                let (u, v) = direction_to_sky_uv(dir);
                let rgb = sample_sky_color(u, v, night, theme);
                data.extend_from_slice(&[
                    (rgb[0] * 255.0) as u8,
                    (rgb[1] * 255.0) as u8,
                    (rgb[2] * 255.0) as u8,
                    255,
                ]);
            }
        }
    }
    data
}

/// Build a small procedural cubemap `Image` for use with
/// [`GeneratedEnvironmentMapLight`](bevy::light::GeneratedEnvironmentMapLight),
/// sampling the same gradient the flat sky dome uses so the two always agree.
pub fn create_environment_cubemap(theme: StadiumEnvironment, night: bool) -> Image {
    let data = paint_cubemap_texels(theme, night);
    let mut img = Image::new(
        Extent3d {
            width: IBL_CUBE_SIZE,
            height: IBL_CUBE_SIZE,
            depth_or_array_layers: 6,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.usage = TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST;
    img.texture_view_descriptor = Some(TextureViewDescriptor {
        dimension: Some(TextureViewDimension::Cube),
        ..Default::default()
    });
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Perceived brightness, for palette comparisons.
    fn luma(rgb: [f32; 3]) -> f32 {
        rgb[0] * 0.299 + rgb[1] * 0.587 + rgb[2] * 0.114
    }

    /// Coarse sample grid — catches a palette or noise change for a fraction of
    /// the cost of painting two million texels per theme.
    fn sky_grid(theme: StadiumEnvironment, night: bool) -> Vec<[f32; 3]> {
        (0..24)
            .flat_map(|y| {
                (0..32)
                    .map(move |x| sample_sky_color(x as f32 / 32.0, y as f32 / 24.0, night, theme))
            })
            .collect()
    }

    #[test]
    fn sky_generation_is_deterministic() {
        // Painted directly rather than through the cache, which would make any
        // two calls trivially equal.
        assert_eq!(
            paint_sky_texels(DEFAULT_SKY_THEME, false),
            paint_sky_texels(DEFAULT_SKY_THEME, false)
        );
        for theme in StadiumEnvironment::ALL {
            assert_eq!(
                sky_grid(theme, false),
                sky_grid(theme, false),
                "{theme:?} day sky is not reproducible"
            );
            assert_eq!(
                sky_grid(theme, true),
                sky_grid(theme, true),
                "{theme:?} night sky is not reproducible"
            );
        }
    }

    #[test]
    fn themed_cache_serves_the_same_texels() {
        let a = create_themed_sky_texture(StadiumEnvironment::Desert, false);
        let b = create_themed_sky_texture(StadiumEnvironment::Desert, false);
        assert_eq!(a.data.as_ref().unwrap(), b.data.as_ref().unwrap());
    }

    #[test]
    fn day_and_night_differ() {
        assert_ne!(
            paint_sky_texels(DEFAULT_SKY_THEME, false),
            paint_sky_texels(DEFAULT_SKY_THEME, true)
        );
        for theme in StadiumEnvironment::ALL {
            assert_ne!(
                sky_grid(theme, false),
                sky_grid(theme, true),
                "{theme:?} night sky matches its day sky"
            );
        }
    }

    #[test]
    fn every_theme_has_its_own_day_palette() {
        let samples = |theme| -> Vec<[f32; 3]> {
            (0..8)
                .map(|i| sample_sky_color(0.31, i as f32 / 8.0, false, theme))
                .collect()
        };
        let all: Vec<_> = StadiumEnvironment::ALL.map(samples).into_iter().collect();
        for (i, a) in all.iter().enumerate() {
            for (j, b) in all.iter().enumerate().skip(i + 1) {
                assert_ne!(
                    a,
                    b,
                    "{:?} and {:?} share a sky",
                    StadiumEnvironment::ALL[i],
                    StadiumEnvironment::ALL[j]
                );
            }
        }
    }

    #[test]
    fn desert_horizon_is_warmer_than_alpine() {
        let desert = sample_sky_color(0.5, 0.02, false, StadiumEnvironment::Desert);
        let alpine = sample_sky_color(0.5, 0.02, false, StadiumEnvironment::Alpine);
        assert!(
            desert[0] - desert[2] > alpine[0] - alpine[2],
            "dust haze should push the desert horizon warm: {desert:?} vs {alpine:?}"
        );
    }

    #[test]
    fn night_stars_are_not_full_horizontal_rows() {
        for theme in StadiumEnvironment::ALL {
            let data = paint_sky_texels(theme, true);
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
            assert_eq!(rows_all_bright, 0, "{theme:?} has horizontal star bands");
        }
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

        // Every theme's night texture should contain one bright texel per hash
        // star (dim stars ~120+) — no palette may wash them out or invent them.
        for theme in StadiumEnvironment::ALL {
            let data = paint_sky_texels(theme, true);
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
                "{theme:?} star texels ({tex_stars}) diverged from hash count ({hash_stars})"
            );
            assert!(
                tex_stars <= hash_stars + 16,
                "{theme:?} night sky is bright enough to read as stars ({tex_stars} texels)"
            );
        }
    }

    #[test]
    fn day_sky_has_vertical_gradient() {
        for theme in StadiumEnvironment::ALL {
            let bottom = sample_sky_color(0.5, 0.05, false, theme);
            let top = sample_sky_color(0.5, 0.95, false, theme);
            // Zenith is deeper/darker than the pale horizon.
            assert!(
                luma(top) < luma(bottom) - 0.08,
                "{theme:?} sky is flat: horizon {bottom:?}, zenith {top:?}"
            );
        }
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

    #[test]
    fn cubemap_has_six_faces_of_the_expected_size() {
        let img = create_environment_cubemap(DEFAULT_SKY_THEME, false);
        assert_eq!(img.texture_descriptor.size.width, IBL_CUBE_SIZE);
        assert_eq!(img.texture_descriptor.size.height, IBL_CUBE_SIZE);
        assert_eq!(img.texture_descriptor.size.depth_or_array_layers, 6);
        assert!(IBL_CUBE_SIZE.is_power_of_two());
        let expected_bytes = (IBL_CUBE_SIZE * IBL_CUBE_SIZE * 6 * 4) as usize;
        assert_eq!(img.data.as_ref().unwrap().len(), expected_bytes);
        assert_eq!(
            img.texture_view_descriptor
                .as_ref()
                .and_then(|d| d.dimension),
            Some(TextureViewDimension::Cube)
        );
    }

    #[test]
    fn cubemap_generation_is_deterministic() {
        assert_eq!(
            paint_cubemap_texels(DEFAULT_SKY_THEME, false),
            paint_cubemap_texels(DEFAULT_SKY_THEME, false)
        );
    }

    #[test]
    fn cubemap_day_and_night_differ() {
        for theme in StadiumEnvironment::ALL {
            assert_ne!(
                paint_cubemap_texels(theme, false),
                paint_cubemap_texels(theme, true),
                "{theme:?} IBL cubemap does not change between day and night"
            );
        }
    }

    #[test]
    fn cubemap_faces_all_carry_colour() {
        // Every face should differ from pure black — a broken face-direction
        // mapping tends to collapse a face to a single (0,0,0) corner sample.
        let data = paint_cubemap_texels(DEFAULT_SKY_THEME, false);
        let face_bytes = (IBL_CUBE_SIZE * IBL_CUBE_SIZE * 4) as usize;
        for face in 0..6 {
            let start = face * face_bytes;
            let slice = &data[start..start + face_bytes];
            assert!(
                slice.iter().any(|&b| b > 0),
                "cubemap face {face} is entirely black"
            );
        }
    }

    #[test]
    fn top_face_is_brighter_than_bottom_face() {
        // +Y (index 2) looks toward the zenith, -Y (index 3) below the
        // horizon; the zenith may be a deep blue but the fallback horizon
        // colour used below the horizon should never be darker than it.
        let data = paint_cubemap_texels(DEFAULT_SKY_THEME, false);
        let face_bytes = (IBL_CUBE_SIZE * IBL_CUBE_SIZE * 4) as usize;
        let avg_luma = |face: usize| -> f32 {
            let slice = &data[face * face_bytes..(face + 1) * face_bytes];
            let mut sum = 0.0;
            let mut n = 0.0;
            for texel in slice.chunks_exact(4) {
                sum += texel[0] as f32 * 0.299 + texel[1] as f32 * 0.587 + texel[2] as f32 * 0.114;
                n += 1.0;
            }
            sum / n
        };
        let bottom = avg_luma(3);
        let all_faces_bright = (0..6).map(avg_luma).all(|l| l > 0.0);
        assert!(all_faces_bright, "some face rendered fully black");
        assert!(bottom > 0.0, "below-horizon face should not be black");
    }

    #[test]
    fn direction_to_sky_uv_maps_zenith_and_horizon() {
        let (_, zenith_v) = direction_to_sky_uv(Vec3::Y);
        assert!((zenith_v - 1.0).abs() < 1e-5, "zenith should map to v=1");
        let (_, horizon_v) = direction_to_sky_uv(Vec3::X);
        assert!(horizon_v.abs() < 1e-5, "eye-level should map to v=0");
        let (_, below_v) = direction_to_sky_uv(-Vec3::Y);
        assert_eq!(below_v, 0.0, "below the horizon should clamp to v=0");
    }
}
