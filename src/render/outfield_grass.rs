//! Outfield turf tiling and mowing-band helpers for the authored grass albedo.

use bevy::image::Image;
use bevy::math::{Affine2, Vec2};
use bevy::prelude::Color;
use bevy::render::render_resource::{TextureDataOrder, TextureFormat};

/// Metres of real turf covered by one repeat of the grass albedo tile.
pub const OUTFIELD_GRASS_TILE_METERS: f32 = 4.0;

/// Number of alternating mow bands across the square outfield.
pub const MOW_BAND_COUNT: u32 = 16;

/// Legacy outfield green the authored albedo PNG was balanced against (personality anchor).
const LEGACY_REFERENCE_OUTFIELD_COLOR: Color = Color::srgb_u8(0x2F, 0x7D, 0x32);

/// Canonical bright fairway green for material base colours (golf-course groomed turf).
pub const REFERENCE_OUTFIELD_COLOR: Color = Color::srgb_u8(0x52, 0xA6, 0x42);

/// Expected authored grass albedo resolution (verified at load time in tests).
pub const AUTHORED_GRASS_ALBEDO_SIZE: u32 = 1254;

const MODULATION_CLAMP_LOW: f32 = 0.88;
const MODULATION_CLAMP_HIGH: f32 = 1.12;

/// UV repeats along one outfield axis for a given span in metres.
#[inline]
pub fn outfield_grass_uv_scale(span_m: f32) -> f32 {
    span_m / OUTFIELD_GRASS_TILE_METERS
}

/// Subtle luminance lift/dip for professional alternating mow stripes.
#[inline]
pub fn mow_stripe_multiplier(band_index: u32) -> f32 {
    if band_index.is_multiple_of(2) {
        1.10
    } else {
        0.90
    }
}

/// Fairway base colour preserving stadium personality relative to [`REFERENCE_OUTFIELD_COLOR`].
///
/// Stadium outfield tints are still authored against the legacy PNG anchor; ratio each
/// channel against that anchor and apply the delta to the bright fairway reference so
/// personality survives the retune without crushing the albedo multiply.
pub fn stadium_modulation_tint(stadium_tint: Color) -> Color {
    let legacy = LEGACY_REFERENCE_OUTFIELD_COLOR.to_srgba();
    let reference = REFERENCE_OUTFIELD_COLOR.to_srgba();
    let stadium = stadium_tint.to_srgba();
    let mut s = bevy::color::Srgba {
        red: (reference.red * (stadium.red / legacy.red)).clamp(
            reference.red * MODULATION_CLAMP_LOW,
            reference.red * MODULATION_CLAMP_HIGH,
        ),
        green: (reference.green * (stadium.green / legacy.green)).clamp(
            reference.green * MODULATION_CLAMP_LOW,
            reference.green * MODULATION_CLAMP_HIGH,
        ),
        blue: (reference.blue * (stadium.blue / legacy.blue)).clamp(
            reference.blue * MODULATION_CLAMP_LOW,
            reference.blue * MODULATION_CLAMP_HIGH,
        ),
        alpha: stadium.alpha,
    };
    // Leave headroom for the brightest mow stripe without clipping to white.
    let lum = 0.2126 * s.red + 0.7152 * s.green + 0.0722 * s.blue;
    let max_lum = 1.0 / mow_stripe_multiplier(0);
    if lum > max_lum {
        let scale = max_lum / lum;
        s.red *= scale;
        s.green *= scale;
        s.blue *= scale;
    }
    Color::srgba(s.red, s.green, s.blue, s.alpha)
}

/// Stadium modulation tint with alternating mow-band luminance (not baked into the PNG).
pub fn tinted_mow_band_color(stadium_tint: Color, band_index: u32) -> Color {
    let mul = mow_stripe_multiplier(band_index);
    let mut s = stadium_modulation_tint(stadium_tint).to_srgba();
    let lum = 0.2126 * s.red + 0.7152 * s.green + 0.0722 * s.blue;
    let scaled_lum = (lum * mul).clamp(0.0, 1.0);
    if lum > 1e-6 {
        let scale = scaled_lum / lum;
        s.red = (s.red * scale).min(1.0);
        s.green = (s.green * scale).min(1.0);
        s.blue = (s.blue * scale).min(1.0);
    }
    Color::srgba(s.red, s.green, s.blue, s.alpha)
}

/// UV transform for one X-aligned mowing strip so the grass albedo tiles continuously
/// across adjacent bands (repeat addressing on the material sampler).
pub fn strip_uv_transform(field_span_m: f32, strip_width_m: f32, strip_x_min_m: f32) -> Affine2 {
    let scale_u = outfield_grass_uv_scale(strip_width_m);
    let scale_v = outfield_grass_uv_scale(field_span_m);
    let origin_u = (strip_x_min_m + field_span_m / 2.0) / OUTFIELD_GRASS_TILE_METERS;
    Affine2::from_scale_angle_translation(
        Vec2::new(scale_u, scale_v),
        0.0,
        Vec2::new(origin_u, 0.0),
    )
}

/// Mip level count for a 2D texture (includes the base level).
#[inline]
pub fn mip_level_count(width: u32, height: u32) -> u32 {
    let max_dim = width.max(height).max(1);
    32 - max_dim.leading_zeros()
}

#[inline]
pub fn next_mip_dimension(dim: u32) -> u32 {
    (dim / 2).max(1)
}

#[inline]
fn srgb_byte_to_linear(b: u8) -> f32 {
    let c = b as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
fn linear_to_srgb_byte(c: f32) -> u8 {
    let s = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Box-filter one mip level from an sRGB RGBA8 source (linear-light averaging).
pub fn box_downsample_rgba8_srgb(src: &[u8], src_w: u32, src_h: u32) -> (Vec<u8>, u32, u32) {
    let dst_w = next_mip_dimension(src_w);
    let dst_h = next_mip_dimension(src_h);
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];

    for y in 0..dst_h {
        for x in 0..dst_w {
            let mut lin = [0.0f32; 4];
            let mut count = 0.0f32;
            for dy in 0..2 {
                let sy = y * 2 + dy;
                if sy >= src_h {
                    continue;
                }
                for dx in 0..2 {
                    let sx = x * 2 + dx;
                    if sx >= src_w {
                        continue;
                    }
                    let idx = ((sy * src_w + sx) * 4) as usize;
                    for c in 0..3 {
                        lin[c] += srgb_byte_to_linear(src[idx + c]);
                    }
                    lin[3] += src[idx + 3] as f32;
                    count += 1.0;
                }
            }
            let dst_idx = ((y * dst_w + x) * 4) as usize;
            if count > 0.0 {
                dst[dst_idx + 3] = (lin[3] / count).round() as u8;
                for c in 0..3 {
                    dst[dst_idx + c] = linear_to_srgb_byte(lin[c] / count);
                }
            }
        }
    }
    (dst, dst_w, dst_h)
}

/// Total byte length for a tightly packed RGBA8 mip chain down to 1×1.
pub fn expected_rgba8_mip_data_len(width: u32, height: u32) -> usize {
    let mut total = 0usize;
    let mut w = width;
    let mut h = height;
    loop {
        total += (w as usize) * (h as usize) * 4;
        if w == 1 && h == 1 {
            break;
        }
        w = next_mip_dimension(w);
        h = next_mip_dimension(h);
    }
    total
}

/// Append a CPU-generated box-filtered mip chain to an RGBA8 sRGB image.
pub fn append_rgba8_srgb_mip_chain(image: &mut Image) {
    assert_eq!(
        image.texture_descriptor.format,
        TextureFormat::Rgba8UnormSrgb,
        "mip generation only supports Rgba8UnormSrgb"
    );
    let base_w = image.texture_descriptor.size.width;
    let base_h = image.texture_descriptor.size.height;
    let mips = mip_level_count(base_w, base_h);
    image.texture_descriptor.mip_level_count = mips;
    image.data_order = TextureDataOrder::LayerMajor;

    let mut packed = image.data.take().expect("image must have CPU data");
    let mut w = base_w;
    let mut h = base_h;
    let mut current = std::mem::take(&mut packed);
    packed = Vec::with_capacity(expected_rgba8_mip_data_len(base_w, base_h));
    packed.extend_from_slice(&current);
    while w > 1 || h > 1 {
        let (next, nw, nh) = box_downsample_rgba8_srgb(&current, w, h);
        packed.extend_from_slice(&next);
        current = next;
        w = nw;
        h = nh;
    }
    debug_assert_eq!(packed.len(), expected_rgba8_mip_data_len(base_w, base_h));
    image.data = Some(packed);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_uv(affine: Affine2, u: f32, v: f32) -> Vec2 {
        affine.transform_point2(Vec2::new(u, v))
    }

    #[test]
    fn uv_scale_matches_world_metres() {
        assert!((outfield_grass_uv_scale(40.0) - 10.0).abs() < 1e-5);
        assert!((outfield_grass_uv_scale(OUTFIELD_GRASS_TILE_METERS) - 1.0).abs() < 1e-5);
    }

    #[test]
    fn mow_stripes_alternate_brightness() {
        assert!(mow_stripe_multiplier(0) > mow_stripe_multiplier(1));
        assert!((mow_stripe_multiplier(0) - mow_stripe_multiplier(2)).abs() < 1e-5);
        assert!((mow_stripe_multiplier(1) - mow_stripe_multiplier(3)).abs() < 1e-5);
    }

    #[test]
    fn reference_stadium_modulation_matches_fairway() {
        let tint = stadium_modulation_tint(LEGACY_REFERENCE_OUTFIELD_COLOR).to_srgba();
        let fairway = REFERENCE_OUTFIELD_COLOR.to_srgba();
        assert!((tint.red - fairway.red).abs() < 0.02);
        assert!((tint.green - fairway.green).abs() < 0.02);
        assert!((tint.blue - fairway.blue).abs() < 0.02);
        // Headroom for the +10% mow stripe keeps reference luminance in range.
        assert!(tint.red > 0.30 && tint.red <= 1.0);
        assert!(tint.green > 0.60 && tint.green <= 1.0);
        assert!(tint.blue > 0.20 && tint.blue <= 1.0);
    }

    #[test]
    fn modulation_preserves_stadium_personality_within_bounds() {
        let rose = Color::srgb_u8(0x35, 0x82, 0x36);
        let tint = stadium_modulation_tint(rose).to_srgba();
        let fairway = stadium_modulation_tint(LEGACY_REFERENCE_OUTFIELD_COLOR).to_srgba();
        assert!(tint.red > fairway.red);
        assert!(tint.green >= fairway.green * MODULATION_CLAMP_LOW);
        assert!(tint.blue >= fairway.blue * MODULATION_CLAMP_LOW);
        assert!(tint.red <= fairway.red * MODULATION_CLAMP_HIGH);
        assert!(tint.green <= fairway.green * MODULATION_CLAMP_HIGH);
        assert!(tint.blue <= fairway.blue * MODULATION_CLAMP_HIGH);
    }

    #[test]
    fn tinted_mow_bands_alternate_and_stay_bounded() {
        let stadium = Color::srgb_u8(0x35, 0x82, 0x36);
        let lum = |c: bevy::color::Srgba| 0.2126 * c.red + 0.7152 * c.green + 0.0722 * c.blue;
        let even = tinted_mow_band_color(stadium, 0).to_srgba();
        let odd = tinted_mow_band_color(stadium, 1).to_srgba();
        assert!(lum(even) > lum(odd));
        for band in 0..8 {
            let c = tinted_mow_band_color(stadium, band).to_srgba();
            assert!(c.red >= 0.0 && c.red <= 1.0);
            assert!(c.green >= 0.0 && c.green <= 1.0);
            assert!(c.blue >= 0.0 && c.blue <= 1.0);
        }
    }

    #[test]
    fn strip_uvs_tile_continuously_across_bands() {
        let span = 128.0;
        let band_w = span / MOW_BAND_COUNT as f32;
        let half = span / 2.0;
        for band in 0..MOW_BAND_COUNT {
            let x_min = -half + band as f32 * band_w;
            let t = strip_uv_transform(span, band_w, x_min);
            let end_u = map_uv(t, 1.0, 0.0).x;
            if band + 1 < MOW_BAND_COUNT {
                let next_x = -half + (band + 1) as f32 * band_w;
                let next_t = strip_uv_transform(span, band_w, next_x);
                let start_u = map_uv(next_t, 0.0, 0.0).x;
                assert!(
                    (end_u - start_u).abs() < 1e-4,
                    "band {band}: end {end_u} != next start {start_u}"
                );
            }
        }
    }

    #[test]
    fn full_field_uv_span_matches_repeat_count() {
        let span = 140.0;
        let scale = outfield_grass_uv_scale(span);
        let t = strip_uv_transform(span, span, -span / 2.0);
        let corner = map_uv(t, 1.0, 1.0);
        assert!((corner.x - scale).abs() < 1e-4);
        assert!((corner.y - scale).abs() < 1e-4);
    }

    #[test]
    fn mip_level_count_for_authored_grass_size() {
        assert_eq!(
            mip_level_count(AUTHORED_GRASS_ALBEDO_SIZE, AUTHORED_GRASS_ALBEDO_SIZE),
            11
        );
        assert_eq!(mip_level_count(1024, 1024), 11);
        assert_eq!(mip_level_count(1, 1), 1);
    }

    #[test]
    fn box_downsample_handles_odd_dimensions() {
        // 3×3 checker — odd edge pixels must not panic; result is 1×1.
        let mut src = vec![0u8; 3 * 3 * 4];
        for y in 0..3 {
            for x in 0..3 {
                let v = if (x + y) % 2 == 0 { 200 } else { 40 };
                let i = ((y * 3 + x) * 4) as usize;
                src[i..i + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let (dst, w, h) = box_downsample_rgba8_srgb(&src, 3, 3);
        assert_eq!((w, h), (1, 1));
        assert_eq!(dst.len(), 4);

        // Full chain from authored size reaches 1×1.
        let mut w = AUTHORED_GRASS_ALBEDO_SIZE;
        let mut h = AUTHORED_GRASS_ALBEDO_SIZE;
        let mut levels = 1u32;
        while w > 1 || h > 1 {
            w = next_mip_dimension(w);
            h = next_mip_dimension(h);
            levels += 1;
        }
        assert_eq!(
            levels,
            mip_level_count(AUTHORED_GRASS_ALBEDO_SIZE, AUTHORED_GRASS_ALBEDO_SIZE)
        );
        assert_eq!((w, h), (1, 1));
    }

    #[test]
    fn grass_image_has_true_mip_chain() {
        let image = crate::render::create_outfield_grass_image();
        assert_eq!(
            image.texture_descriptor.size.width,
            AUTHORED_GRASS_ALBEDO_SIZE
        );
        assert_eq!(
            image.texture_descriptor.size.height,
            AUTHORED_GRASS_ALBEDO_SIZE
        );
        assert!(
            image.texture_descriptor.mip_level_count > 1,
            "expected a full mip chain, got {}",
            image.texture_descriptor.mip_level_count
        );
        let data = image.data.as_ref().expect("grass image must have CPU data");
        assert_eq!(
            data.len(),
            expected_rgba8_mip_data_len(AUTHORED_GRASS_ALBEDO_SIZE, AUTHORED_GRASS_ALBEDO_SIZE)
        );
    }
}
