//! Procedural day/night sky textures (generated once at startup).
//!
//! Each [`StadiumEnvironment`] gets its own palette: the air over a desert
//! plateau is not the air over an alpine valley, and the sky is the largest
//! single surface on screen, so it carries most of the theme.

use std::sync::{Arc, Mutex, PoisonError};

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat, TextureUsages};

use crate::core::stadiums::StadiumEnvironment;

const SKY_W: u32 = 2048;
const SKY_H: u32 = 1024;

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
    rgb[2] += haze * 0.7;

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

/// Cached texels for the theme most recently asked for.
///
/// A stadium is rebuilt whenever the match scene is (innings change, replay
/// setup), and painting two million texels of fractal noise on each rebuild
/// would land squarely in `build_stadium`'s frame budget. One slot is enough:
/// a build only ever asks for its own theme.
static SKY_TEXEL_CACHE: Mutex<Option<CachedSky>> = Mutex::new(None);

struct CachedSky {
    theme: StadiumEnvironment,
    night: bool,
    texels: Arc<Vec<u8>>,
}

/// Sky texels for `theme`, painting them only if the cache holds another sky.
fn themed_sky_texels(theme: StadiumEnvironment, night: bool) -> Arc<Vec<u8>> {
    let mut slot = SKY_TEXEL_CACHE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    if let Some(cached) = slot.as_ref()
        && cached.theme == theme
        && cached.night == night
    {
        return cached.texels.clone();
    }
    let texels = Arc::new(paint_sky_texels(theme, night));
    *slot = Some(CachedSky {
        theme,
        night,
        texels: texels.clone(),
    });
    texels
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
    let data = themed_sky_texels(theme, night).as_ref().clone();
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
}
