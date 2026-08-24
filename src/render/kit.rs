//! Procedural shirt texture for the named-material-slot player asset: one
//! composited [`Image`] carrying pattern, name, squad number and crest,
//! built from a [`ShirtSpec`].
//!
//! ## UV layout
//!
//! Measured from the exported MPFB GLB (not assumed — the asset is built and
//! this is its real unwrap):
//!
//! - `u` wraps once around the torso. `u = 0.0` / `u = 1.0` is the seam
//!   under the character's **left arm**, and the texture must tile
//!   horizontally (the sampler wraps that seam edge back onto itself).
//! - `u = 0.25` is **back centre** — player name and squad number go here.
//! - `u = 0.5` is the seam under the character's right arm (diametrically
//!   opposite the left-arm seam).
//! - `u = 0.75` is **front centre** — the crest goes here.
//! - `v = 0` is the shoulder line (top of the image); `v = 1` is the hem
//!   (bottom of the image).
//!
//! So going left to right across the image: left-arm seam, back panel,
//! right-arm seam, front panel, left-arm seam again. The pattern (stripes/
//! hoops/chevron/etc.) is generated per quarter-panel so it repeats
//! identically on the back and front halves and stays continuous at the
//! `u = 0.0` / `u = 1.0` tiling edge.

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

use crate::core::teams::KitStyle;
use crate::render::glyphs;

/// Shirt texture is square and this large — sixteen times the pixel count of
/// the old 64x64 per-part atlas, since name/number/crest all have to be
/// legible in the same map.
pub const SHIRT_TEXTURE_SIZE: u32 = 256;

/// `u` of the back panel centre — see the module doc for the full layout.
pub const BACK_CENTER_U: f32 = 0.25;
/// `u` of the front panel centre.
pub const FRONT_CENTER_U: f32 = 0.75;

/// Front-chest crest placement, `(u_min, u_max, v_min, v_max)`, centred on
/// [`FRONT_CENTER_U`].
pub const CREST_REGION_UV: (f32, f32, f32, f32) = (0.66, 0.84, 0.06, 0.24);
/// Upper-back name placement, `(u_min, u_max, v_min, v_max)`, centred on
/// [`BACK_CENTER_U`].
pub const NAME_REGION_UV: (f32, f32, f32, f32) = (0.04, 0.48, 0.06, 0.16);
/// Centre-back squad number placement, `(u_min, u_max, v_min, v_max)`,
/// centred on [`BACK_CENTER_U`].
pub const NUMBER_REGION_UV: (f32, f32, f32, f32) = (0.08, 0.44, 0.20, 0.62);

/// Longest name that reliably fits [`NAME_REGION_UV`] at the chosen glyph
/// scale; longer names are truncated to this many characters.
const NAME_MAX_CHARS: usize = 9;
/// Glyph pixel scale used for the back-of-shirt name text.
const NAME_GLYPH_SCALE: u32 = 2;

/// Everything needed to composite one shirt texture.
#[derive(Clone, Debug)]
pub struct ShirtSpec {
    pub primary: Color,
    pub secondary: Color,
    pub pattern: KitStyle,
    /// Shown truncated and centred across the upper back. `None` leaves the
    /// name row blank.
    pub name: Option<String>,
    /// Shown large and centred on the back. `None` leaves the number blank.
    pub number: Option<u8>,
    /// Sponsor/team crest, composited onto the front chest. `None` leaves
    /// the chest plain. Compositing needs the decoded pixels, so callers
    /// resolve the handle via `Assets<Image>` and pass the image itself to
    /// [`build_shirt_image`] — the handle lives on the spec purely as the
    /// asset identity a caller threads through.
    pub crest: Option<Handle<Image>>,
}

impl ShirtSpec {
    pub fn new(primary: Color, secondary: Color, pattern: KitStyle) -> Self {
        ShirtSpec {
            primary,
            secondary,
            pattern,
            name: None,
            number: None,
            crest: None,
        }
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_number(mut self, number: u8) -> Self {
        self.number = Some(number);
        self
    }

    pub fn with_crest(mut self, crest: Handle<Image>) -> Self {
        self.crest = Some(crest);
        self
    }
}

/// Pixel-space bounds `(x0, x1, y0, y1)` of a `(u_min, u_max, v_min, v_max)`
/// UV region over a `size` x `size` texture.
fn region_px(region: (f32, f32, f32, f32), size: u32) -> (u32, u32, u32, u32) {
    let (u0, u1, v0, v1) = region;
    let f = size as f32;
    (
        (u0 * f).round() as u32,
        (u1 * f).round() as u32,
        (v0 * f).round() as u32,
        (v1 * f).round() as u32,
    )
}

/// Whether style `pattern` paints the secondary colour at normalised
/// position `(u, v)`. `local_u` is `u` re-mapped to `0..1` *within*
/// whichever quarter-panel (back for `u < 0.5`, front for `u >= 0.5`) the
/// texel falls in — `0`/`1` at the arm seams, `0.5` at the panel centre.
///
/// Patterns that just repeat around the cylinder (stripes, the horizontal
/// band, hoops) are computed from the full `u`/`v` so their period divides
/// `1.0` evenly and the `u = 0` / `u = 1` tiling edge falls exactly on a
/// period boundary — no half-width sliver stripe at the seam. Patterns
/// meant to read as symmetric on each panel (the chevron) use `local_u`
/// instead, which is itself continuous at that same edge (it's a period-0.5
/// triangle wave, and 0.5 divides 1.0 evenly too).
fn pattern_uses_secondary(pattern: KitStyle, u: f32, local_u: f32, v: f32) -> bool {
    match pattern {
        KitStyle::Solid => false,
        KitStyle::VerticalStripes => ((u * 16.0) as u32).is_multiple_of(2),
        KitStyle::HorizontalBand => v > 0.28 && v < 0.52,
        KitStyle::Chevron => v < 0.22 + (local_u - 0.5).abs() * 0.55,
        // A diagonal band spiralling once around the whole cylinder rather
        // than mirrored per panel — `local_u` would tear at the seam here
        // (the fold isn't symmetric for this formula), whereas wrapping the
        // full `u` with `% 1.0` is continuous by construction.
        KitStyle::DiagonalSplit => (u + v * 0.85) % 1.0 > 0.55,
        KitStyle::Hoops => ((v * 10.0) as u32).is_multiple_of(2),
    }
}

/// Ink colour for name/number text: white on a dark primary, near black on a
/// light one, so lettering always reads against the shirt.
fn ink_color(primary: Color) -> [u8; 3] {
    let p = primary.to_srgba();
    let luma = 0.299 * p.red + 0.587 * p.green + 0.114 * p.blue;
    if luma < 0.55 {
        [0xF4, 0xF4, 0xF0]
    } else {
        [0x14, 0x14, 0x18]
    }
}

/// Uppercase `name`, drop anything that isn't a letter or space, and
/// truncate to [`NAME_MAX_CHARS`] so it always fits [`NAME_REGION_UV`].
fn sanitise_name(name: &str) -> String {
    name.to_ascii_uppercase()
        .chars()
        .filter(|c| c.is_ascii_uppercase() || *c == ' ')
        .take(NAME_MAX_CHARS)
        .collect()
}

/// Paint the back-of-shirt name, centred in [`NAME_REGION_UV`].
fn paint_name(data: &mut [u8], size: u32, name: &str, color: [u8; 3]) {
    let clean = sanitise_name(name);
    if clean.is_empty() {
        return;
    }
    let (x0, x1, y0, y1) = region_px(NAME_REGION_UV, size);
    let region_w = x1.saturating_sub(x0);
    let region_h = y1.saturating_sub(y0);
    let scale = NAME_GLYPH_SCALE;
    let advance = (glyphs::GLYPH_5X7_COLS + 1) * scale;
    let glyph_h = glyphs::GLYPH_5X7_ROWS * scale;
    let len = clean.len() as u32;
    let total_w = len * advance - scale; // no trailing inter-glyph gap
    let ox = x0 + region_w.saturating_sub(total_w) / 2;
    let oy = y0 + region_h.saturating_sub(glyph_h) / 2;
    for (i, ch) in clean.bytes().enumerate() {
        glyphs::draw_glyph_5x7(
            data,
            size,
            size,
            ch,
            ox + i as u32 * advance,
            oy,
            scale,
            color,
        );
    }
}

/// Pick a digit scale that fills [`NUMBER_REGION_UV`] as large as it can,
/// for a number with `digit_count` digits.
fn number_scale(digit_count: u32, region_w: u32, region_h: u32) -> u32 {
    let total_units_w = glyphs::DIGIT_3X5_COLS * digit_count + digit_count.saturating_sub(1);
    let by_width = if total_units_w > 0 {
        region_w / total_units_w
    } else {
        1
    };
    let by_height = region_h / glyphs::DIGIT_3X5_ROWS;
    by_width.min(by_height).max(1)
}

/// Paint the large back-of-shirt squad number, centred in
/// [`NUMBER_REGION_UV`].
fn paint_number(data: &mut [u8], size: u32, number: u8, color: [u8; 3]) {
    let digits: Vec<usize> = if number >= 10 {
        vec![(number / 10) as usize % 10, number as usize % 10]
    } else {
        vec![number as usize % 10]
    };
    let (x0, x1, y0, y1) = region_px(NUMBER_REGION_UV, size);
    let region_w = x1.saturating_sub(x0);
    let region_h = y1.saturating_sub(y0);
    let scale = number_scale(digits.len() as u32, region_w, region_h);
    let glyph_w = glyphs::DIGIT_3X5_COLS * scale;
    let gap = scale;
    let total_w = glyph_w * digits.len() as u32 + gap * (digits.len() as u32 - 1);
    let total_h = glyphs::DIGIT_3X5_ROWS * scale;
    let ox = x0 + region_w.saturating_sub(total_w) / 2;
    let oy = y0 + region_h.saturating_sub(total_h) / 2;
    for (i, &d) in digits.iter().enumerate() {
        let gx = ox + i as u32 * (glyph_w + gap);
        glyphs::draw_digit_3x5(data, size, size, d, gx, oy, scale, color);
    }
}

/// Nearest-neighbour sample of an RGBA8 image at normalised `(u, v)`.
/// Returns `None` if the image isn't a plain RGBA8 buffer we know how to
/// read (defensive — a crest asset in an unexpected format is skipped
/// rather than panicking the render loop).
fn sample_rgba8(image: &Image, u: f32, v: f32) -> Option<[u8; 4]> {
    if image.texture_descriptor.format != TextureFormat::Rgba8UnormSrgb
        && image.texture_descriptor.format != TextureFormat::Rgba8Unorm
    {
        return None;
    }
    let data = image.data.as_ref()?;
    let w = image.texture_descriptor.size.width;
    let h = image.texture_descriptor.size.height;
    if w == 0 || h == 0 {
        return None;
    }
    let x = ((u.clamp(0.0, 0.999_999) * w as f32) as u32).min(w - 1);
    let y = ((v.clamp(0.0, 0.999_999) * h as f32) as u32).min(h - 1);
    let idx = ((y * w + x) * 4) as usize;
    data.get(idx..idx + 4).map(|s| [s[0], s[1], s[2], s[3]])
}

/// Alpha-blend the crest image into [`CREST_REGION_UV`] on the front chest.
fn paint_crest(data: &mut [u8], size: u32, crest: &Image) {
    let (x0, x1, y0, y1) = region_px(CREST_REGION_UV, size);
    for y in y0..y1.min(size) {
        for x in x0..x1.min(size) {
            let u = (x - x0) as f32 / (x1 - x0).max(1) as f32;
            let v = (y - y0) as f32 / (y1 - y0).max(1) as f32;
            let Some([r, g, b, a]) = sample_rgba8(crest, u, v) else {
                continue;
            };
            if a == 0 {
                continue;
            }
            let idx = ((y * size + x) * 4) as usize;
            let a = a as f32 / 255.0;
            data[idx] = (r as f32 * a + data[idx] as f32 * (1.0 - a)) as u8;
            data[idx + 1] = (g as f32 * a + data[idx + 1] as f32 * (1.0 - a)) as u8;
            data[idx + 2] = (b as f32 * a + data[idx + 2] as f32 * (1.0 - a)) as u8;
        }
    }
}

/// Composite `spec` into one shirt [`Image`]. `crest_image` is the decoded
/// crest referenced by `spec.crest` (resolved by the caller through
/// `Assets<Image>`); pass `None` to leave the chest plain, e.g. while the
/// crest is still streaming in.
pub fn build_shirt_image(spec: &ShirtSpec, crest_image: Option<&Image>) -> Image {
    const S: u32 = SHIRT_TEXTURE_SIZE;
    let p = spec.primary.to_srgba();
    let s = spec.secondary.to_srgba();
    let mut data = Vec::with_capacity((S * S * 4) as usize);
    for y in 0..S {
        for x in 0..S {
            let u = x as f32 / S as f32;
            let v = y as f32 / S as f32;
            // Fold each half (back: u<0.5, front: u>=0.5) into 0..1 with the
            // panel centre at 0.5 and the arm seams at 0/1 — see
            // `pattern_uses_secondary`.
            let local_u = if u < 0.5 { u * 2.0 } else { (u - 0.5) * 2.0 };
            let (r, g, b) = if pattern_uses_secondary(spec.pattern, u, local_u, v) {
                (s.red, s.green, s.blue)
            } else {
                (p.red, p.green, p.blue)
            };
            data.extend_from_slice(&[(r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8, 255]);
        }
    }

    let ink = ink_color(spec.primary);
    if let Some(name) = &spec.name {
        paint_name(&mut data, S, name, ink);
    }
    if let Some(number) = spec.number {
        paint_number(&mut data, S, number, ink);
    }
    if let Some(crest) = crest_image {
        paint_crest(&mut data, S, crest);
    }

    Image::new(
        Extent3d {
            width: S,
            height: S,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(image: &Image, x: u32, y: u32) -> [u8; 4] {
        let data = image.data.as_ref().unwrap();
        let idx = ((y * SHIRT_TEXTURE_SIZE + x) * 4) as usize;
        [data[idx], data[idx + 1], data[idx + 2], data[idx + 3]]
    }

    fn region_has_ink(image: &Image, region: (f32, f32, f32, f32), ink: [u8; 3]) -> bool {
        let (x0, x1, y0, y1) = region_px(region, SHIRT_TEXTURE_SIZE);
        (y0..y1).any(|y| {
            (x0..x1).any(|x| {
                let px = pixel(image, x, y);
                px[0] == ink[0] && px[1] == ink[1] && px[2] == ink[2]
            })
        })
    }

    fn outside_region_matches_baseline(
        with_extra: &Image,
        baseline: &Image,
        region: (f32, f32, f32, f32),
    ) -> bool {
        let (x0, x1, y0, y1) = region_px(region, SHIRT_TEXTURE_SIZE);
        for y in 0..SHIRT_TEXTURE_SIZE {
            for x in 0..SHIRT_TEXTURE_SIZE {
                let inside = (x0..x1).contains(&x) && (y0..y1).contains(&y);
                if inside {
                    continue;
                }
                if pixel(with_extra, x, y) != pixel(baseline, x, y) {
                    return false;
                }
            }
        }
        true
    }

    fn plain_spec() -> ShirtSpec {
        ShirtSpec::new(
            Color::srgb(0.05, 0.15, 0.75),
            Color::srgb(0.95, 0.85, 0.10),
            KitStyle::Solid,
        )
    }

    #[test]
    fn regions_sit_on_the_documented_panel_centres() {
        // Name/number straddle the back centre (u = 0.25); the crest
        // straddles the front centre (u = 0.75), and none of the three
        // regions cross either arm seam (u = 0, 0.5, 1).
        for (region, centre) in [
            (NAME_REGION_UV, BACK_CENTER_U),
            (NUMBER_REGION_UV, BACK_CENTER_U),
            (CREST_REGION_UV, FRONT_CENTER_U),
        ] {
            assert!(region.0 < centre && centre < region.1);
        }
        assert!(NAME_REGION_UV.1 <= 0.5);
        assert!(NUMBER_REGION_UV.1 <= 0.5);
        assert!(CREST_REGION_UV.0 >= 0.5);
    }

    #[test]
    fn digits_render_only_inside_the_number_region() {
        let base = build_shirt_image(&plain_spec(), None);
        let numbered = build_shirt_image(&plain_spec().with_number(7), None);
        assert_ne!(base.data, numbered.data, "number should change the texture");
        assert!(
            outside_region_matches_baseline(&numbered, &base, NUMBER_REGION_UV),
            "digit ink leaked outside NUMBER_REGION_UV"
        );
        assert!(region_has_ink(
            &numbered,
            NUMBER_REGION_UV,
            ink_color(plain_spec().primary)
        ));
    }

    #[test]
    fn two_digit_numbers_stay_inside_the_number_region_too() {
        let base = build_shirt_image(&plain_spec(), None);
        let numbered = build_shirt_image(&plain_spec().with_number(99), None);
        assert!(outside_region_matches_baseline(
            &numbered,
            &base,
            NUMBER_REGION_UV
        ));
        assert!(region_has_ink(
            &numbered,
            NUMBER_REGION_UV,
            ink_color(plain_spec().primary)
        ));
    }

    #[test]
    fn name_renders_only_inside_the_name_region() {
        let base = build_shirt_image(&plain_spec(), None);
        let named = build_shirt_image(&plain_spec().with_name("Shanker"), None);
        assert!(
            outside_region_matches_baseline(&named, &base, NAME_REGION_UV),
            "name ink leaked outside NAME_REGION_UV"
        );
        assert!(region_has_ink(
            &named,
            NAME_REGION_UV,
            ink_color(plain_spec().primary)
        ));
    }

    #[test]
    fn long_name_is_truncated_and_still_fits_the_region() {
        let base = build_shirt_image(&plain_spec(), None);
        let long = build_shirt_image(
            &plain_spec().with_name("SUPERCALIFRAGILISTICEXPIALIDOCIOUS"),
            None,
        );
        assert!(outside_region_matches_baseline(
            &long,
            &base,
            NAME_REGION_UV
        ));
    }

    #[test]
    fn short_name_is_horizontally_centred_in_its_region() {
        let image = build_shirt_image(&plain_spec().with_name("AB"), None);
        let ink = ink_color(plain_spec().primary);
        let (x0, x1, y0, y1) = region_px(NAME_REGION_UV, SHIRT_TEXTURE_SIZE);
        let mut min_x = None;
        let mut max_x = None;
        for y in y0..y1 {
            for x in x0..x1 {
                let px = pixel(&image, x, y);
                if [px[0], px[1], px[2]] == ink {
                    min_x = Some(min_x.map_or(x, |m: u32| m.min(x)));
                    max_x = Some(max_x.map_or(x, |m: u32| m.max(x)));
                }
            }
        }
        let (min_x, max_x) = (min_x.unwrap(), max_x.unwrap());
        let region_centre = (x0 + x1) as f32 / 2.0;
        let ink_centre = (min_x + max_x) as f32 / 2.0;
        assert!(
            (ink_centre - region_centre).abs() < 4.0,
            "name not centred: ink centre {ink_centre}, region centre {region_centre}"
        );
    }

    #[test]
    fn every_non_solid_style_uses_the_secondary_colour() {
        let secondary = Color::srgb(0.95, 0.85, 0.10);
        let s = secondary.to_srgba();
        let secondary_u8 = [
            (s.red * 255.0) as u8,
            (s.green * 255.0) as u8,
            (s.blue * 255.0) as u8,
        ];
        for style in [
            KitStyle::VerticalStripes,
            KitStyle::HorizontalBand,
            KitStyle::Chevron,
            KitStyle::DiagonalSplit,
            KitStyle::Hoops,
        ] {
            let spec = ShirtSpec::new(Color::srgb(0.05, 0.15, 0.75), secondary, style);
            let image = build_shirt_image(&spec, None);
            let data = image.data.as_ref().unwrap();
            let uses_secondary = data.chunks_exact(4).any(|px| {
                px[0] == secondary_u8[0] && px[1] == secondary_u8[1] && px[2] == secondary_u8[2]
            });
            assert!(
                uses_secondary,
                "{style:?} never painted the secondary colour"
            );
        }
    }

    #[test]
    fn solid_style_never_uses_the_secondary_colour() {
        let secondary = Color::srgb(0.95, 0.85, 0.10);
        let s = secondary.to_srgba();
        let secondary_u8 = [
            (s.red * 255.0) as u8,
            (s.green * 255.0) as u8,
            (s.blue * 255.0) as u8,
        ];
        let spec = ShirtSpec::new(Color::srgb(0.05, 0.15, 0.75), secondary, KitStyle::Solid);
        let image = build_shirt_image(&spec, None);
        let data = image.data.as_ref().unwrap();
        assert!(
            data.chunks_exact(4).all(|px| !(px[0] == secondary_u8[0]
                && px[1] == secondary_u8[1]
                && px[2] == secondary_u8[2])),
            "solid kit must never show the secondary colour"
        );
    }

    #[test]
    fn pattern_is_mirrored_across_back_and_front_panels() {
        let spec = ShirtSpec::new(
            Color::srgb(0.05, 0.15, 0.75),
            Color::srgb(0.95, 0.85, 0.10),
            KitStyle::VerticalStripes,
        );
        let image = build_shirt_image(&spec, None);
        // A texel a quarter of the way into the back panel and the
        // equivalent texel a quarter of the way into the front panel should
        // agree, since the pattern re-maps u into 0..1 per panel.
        let y = 100; // outside the name/number rows
        let back = pixel(&image, (0.125 * SHIRT_TEXTURE_SIZE as f32) as u32, y);
        let front = pixel(&image, (0.625 * SHIRT_TEXTURE_SIZE as f32) as u32, y);
        assert_eq!(back, front);
    }

    #[test]
    fn vertical_stripes_tile_seamlessly_at_the_arm_seam() {
        // u = 0 and u = 1 are the same physical seam on the mesh (the
        // texture must tile horizontally). A seamless 16-stripe wrap has
        // exactly 16 colour transitions scanning all the way around,
        // wrap-inclusive — not 15 (a stripe swallowed by the seam) or 17
        // (an extra sliver stripe at the seam).
        let spec = ShirtSpec::new(
            Color::srgb(0.05, 0.15, 0.75),
            Color::srgb(0.95, 0.85, 0.10),
            KitStyle::VerticalStripes,
        );
        let image = build_shirt_image(&spec, None);
        let y = 100;
        let mut transitions = 0;
        for x in 0..SHIRT_TEXTURE_SIZE {
            let a = pixel(&image, x, y);
            let b = pixel(&image, (x + 1) % SHIRT_TEXTURE_SIZE, y);
            if a != b {
                transitions += 1;
            }
        }
        assert_eq!(
            transitions, 16,
            "seam should not clip or duplicate a stripe"
        );
    }

    #[test]
    fn diagonal_split_wraps_continuously_at_the_arm_seam() {
        let spec = ShirtSpec::new(
            Color::srgb(0.05, 0.15, 0.75),
            Color::srgb(0.95, 0.85, 0.10),
            KitStyle::DiagonalSplit,
        );
        let image = build_shirt_image(&spec, None);
        let y = 60;
        let left_edge = pixel(&image, 0, y);
        let right_edge = pixel(&image, SHIRT_TEXTURE_SIZE - 1, y);
        assert_eq!(left_edge, right_edge);
    }

    #[test]
    fn image_dimensions_match_the_documented_size() {
        let image = build_shirt_image(&plain_spec(), None);
        assert_eq!(image.texture_descriptor.size.width, SHIRT_TEXTURE_SIZE);
        assert_eq!(image.texture_descriptor.size.height, SHIRT_TEXTURE_SIZE);
    }
}
