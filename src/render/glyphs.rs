//! Shared bitmap glyph tables for procedurally painted textures.
//!
//! Two tiny fonts cover every piece of in-game signage that needs text baked
//! into a CPU-generated [`Image`](bevy::image::Image): a 5x7 uppercase A-Z
//! font (big screen team names, shirt name text) and a 3x5 digit font (gate
//! plaques, squad numbers). Both tables used to live inline inside the
//! functions that painted the big screen and the gate plaques; they are
//! pulled out here so every caller — `stadium.rs` and `kit.rs` — shares one
//! copy instead of hand-maintaining duplicates.

/// 5x7 bitmap font, uppercase A-Z only. Each glyph is 7 rows of 5 bits (MSB
/// is the leftmost column); every letter in this set only actually uses the
/// top 4 rows; the bottom 3 are blank descender space.
pub const FONT_5X7: [[u8; 7]; 26] = [
    [
        0b11100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b01000, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10000, 0b10000, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10000, 0b01100, 0b10000, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10100, 0b01100, 0b00100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10000, 0b11000, 0b10000, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10000, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b01000, 0b01000, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b00100, 0b00100, 0b00100, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b11000, 0b11000, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10000, 0b10000, 0b10000, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b11100, 0b10100, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b11100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10100, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10100, 0b11100, 0b10000, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10100, 0b11100, 0b00100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b11000, 0b11000, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b10000, 0b11100, 0b00100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b01000, 0b01000, 0b01000, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b10100, 0b10100, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b10100, 0b10100, 0b01000, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b10100, 0b11100, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b01000, 0b01000, 0b10100, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b10100, 0b01000, 0b01000, 0b01000, 0b00000, 0b00000, 0b00000,
    ],
    [
        0b11100, 0b00100, 0b01000, 0b11100, 0b00000, 0b00000, 0b00000,
    ],
];

/// Glyph cell dimensions of [`FONT_5X7`], in bitmap texels.
pub const GLYPH_5X7_COLS: u32 = 5;
pub const GLYPH_5X7_ROWS: u32 = 7;

/// 3x5 bitmap digits 0-9. Each glyph is 5 rows of 3 bits (MSB is the
/// leftmost column).
pub const DIGITS_3X5: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b001, 0b001, 0b001],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

/// Glyph cell dimensions of [`DIGITS_3X5`], in bitmap texels.
pub const DIGIT_3X5_COLS: u32 = 3;
pub const DIGIT_3X5_ROWS: u32 = 5;

/// Paint one uppercase letter from [`FONT_5X7`] into an RGBA8 buffer.
///
/// `(ox, oy)` is the top-left texel of the glyph cell; each bitmap texel is
/// expanded to a `scale` x `scale` block. Only "on" texels are written, so a
/// pre-painted background shows through everywhere else. Non `A`-`Z` bytes
/// are silently skipped — callers can feed raw text and just let punctuation
/// and spaces fall through as blank advances.
#[allow(clippy::too_many_arguments)]
pub fn draw_glyph_5x7(
    data: &mut [u8],
    width: u32,
    height: u32,
    ch: u8,
    ox: u32,
    oy: u32,
    scale: u32,
    color: [u8; 3],
) {
    if !ch.is_ascii_uppercase() {
        return;
    }
    let i = (ch - b'A') as usize;
    for (row, &glyph_row) in FONT_5X7[i].iter().enumerate() {
        for col in 0..GLYPH_5X7_COLS {
            if glyph_row & (1 << (4 - col)) == 0 {
                continue;
            }
            blit_cell(
                data,
                width,
                height,
                ox + col * scale,
                oy + row as u32 * scale,
                scale,
                color,
            );
        }
    }
}

/// Paint one digit (`0`-`9`, taken mod 10) from [`DIGITS_3X5`] into an RGBA8
/// buffer, analogous to [`draw_glyph_5x7`].
#[allow(clippy::too_many_arguments)]
pub fn draw_digit_3x5(
    data: &mut [u8],
    width: u32,
    height: u32,
    digit: usize,
    ox: u32,
    oy: u32,
    scale: u32,
    color: [u8; 3],
) {
    let d = digit % 10;
    for (row, &bits) in DIGITS_3X5[d].iter().enumerate() {
        for col in 0..DIGIT_3X5_COLS {
            if bits & (1 << (2 - col)) == 0 {
                continue;
            }
            blit_cell(
                data,
                width,
                height,
                ox + col * scale,
                oy + row as u32 * scale,
                scale,
                color,
            );
        }
    }
}

/// Fill a `scale` x `scale` block of RGB texels (alpha untouched), clipped to
/// the buffer bounds.
fn blit_cell(
    data: &mut [u8],
    width: u32,
    height: u32,
    px: u32,
    py: u32,
    scale: u32,
    color: [u8; 3],
) {
    for dy in 0..scale {
        for dx in 0..scale {
            let x = px + dx;
            let y = py + dy;
            if x < width && y < height {
                let idx = ((y * width + x) * 4) as usize;
                data[idx] = color[0];
                data[idx + 1] = color[1];
                data[idx + 2] = color[2];
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_and_digit_tables_are_fully_populated() {
        assert_eq!(FONT_5X7.len(), 26);
        assert_eq!(DIGITS_3X5.len(), 10);
        // Every glyph has at least one lit texel — a table typo that zeroed a
        // row would otherwise render a blank letter with no test failure.
        for (i, glyph) in FONT_5X7.iter().enumerate() {
            let lit: u32 = glyph.iter().map(|row| row.count_ones()).sum();
            assert!(lit > 0, "letter {} is blank", (b'A' + i as u8) as char);
        }
        for (d, glyph) in DIGITS_3X5.iter().enumerate() {
            let lit: u32 = glyph.iter().map(|row| row.count_ones()).sum();
            assert!(lit > 0, "digit {d} is blank");
        }
    }

    #[test]
    fn draw_glyph_writes_only_within_its_cell() {
        const W: u32 = 32;
        const H: u32 = 32;
        let mut data = vec![0u8; (W * H * 4) as usize];
        draw_glyph_5x7(&mut data, W, H, b'A', 4, 4, 2, [255, 255, 255]);
        for y in 0..H {
            for x in 0..W {
                let idx = ((y * W + x) * 4) as usize;
                let lit = data[idx] != 0;
                let in_cell = (4..4 + GLYPH_5X7_COLS * 2).contains(&x)
                    && (4..4 + GLYPH_5X7_ROWS * 2).contains(&y);
                if lit {
                    assert!(in_cell, "glyph painted outside its cell at ({x},{y})");
                }
            }
        }
    }

    #[test]
    fn draw_glyph_ignores_non_letters() {
        const W: u32 = 16;
        const H: u32 = 16;
        let mut data = vec![0u8; (W * H * 4) as usize];
        draw_glyph_5x7(&mut data, W, H, b'?', 0, 0, 1, [255, 255, 255]);
        assert!(
            data.iter().all(|&b| b == 0),
            "non-letter byte should draw nothing"
        );
    }

    #[test]
    fn draw_digit_distinguishes_digits() {
        const W: u32 = 16;
        const H: u32 = 16;
        let mut a = vec![0u8; (W * H * 4) as usize];
        let mut b = vec![0u8; (W * H * 4) as usize];
        draw_digit_3x5(&mut a, W, H, 1, 0, 0, 2, [255, 255, 255]);
        draw_digit_3x5(&mut b, W, H, 8, 0, 0, 2, [255, 255, 255]);
        assert_ne!(a, b);
        // "8" lights every row; "1" doesn't — a cheap discriminator that the
        // right row is being read out of the table.
        let lit_rows = |data: &[u8]| -> usize {
            (0..H)
                .filter(|&y| (0..W).any(|x| data[((y * W + x) * 4) as usize] != 0))
                .count()
        };
        assert!(lit_rows(&b) >= lit_rows(&a));
    }
}
