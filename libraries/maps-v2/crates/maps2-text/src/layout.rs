//! A string becomes quads in screen pixels. There is no shaping: one
//! character, one advance, left to right — recorded debt against the
//! world map, honest for Latin names.

use crate::atlas::{Atlas, ATLAS_CELL_PX, CELL_ORIGIN_PX, EM_PX};
use crate::font::{ASCENDER_EM, DESCENDER_EM};
use num_traits::ToPrimitive;

/// Height of one line in em units: the whole em box, ascender to
/// descender.
pub const LINE_HEIGHT_EM: f32 = ASCENDER_EM - DESCENDER_EM;

/// One glyph as the renderer wants it: a screen-pixel rectangle and the
/// patch of the atlas it samples. `y` grows downwards, like the canvas.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphQuad {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
}

/// Width and height a line will occupy at this size, in screen pixels.
/// The height is the em box, not the ink — a row of labels must not
/// jitter because one of them happens to have no descender.
#[must_use]
pub fn measure_line(atlas: &Atlas, text: &str, size_px: f32) -> (f32, f32) {
    let advances: f32 = text
        .chars()
        .filter_map(|ch| atlas.glyph(ch))
        .map(|g| g.advance)
        .sum();
    (advances * size_px, LINE_HEIGHT_EM * size_px)
}

/// Quads for a line whose left edge sits at `x` and whose baseline sits
/// at `y`, both in screen pixels. Characters outside the charset are
/// dropped, and their advance with them.
#[must_use]
pub fn layout_line(atlas: &Atlas, text: &str, x: f32, y: f32, size_px: f32) -> Vec<GlyphQuad> {
    let scale = size_px / EM_PX;
    let side = ATLAS_CELL_PX.to_f32().unwrap_or_default() * scale;
    let mut pen = x;
    let mut quads = Vec::new();
    for ch in text.chars() {
        let Some(glyph) = atlas.glyph(ch) else {
            continue;
        };
        let (u0, v0, u1, v1) = atlas.cell_uv(glyph.cell);
        let x0 = CELL_ORIGIN_PX.0.mul_add(-scale, pen);
        let y0 = CELL_ORIGIN_PX.1.mul_add(-scale, y);
        quads.push(GlyphQuad { x0, y0, x1: x0 + side, y1: y0 + side, u0, v0, u1, v1 });
        pen = glyph.advance.mul_add(size_px, pen);
    }
    quads
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_line_walks_left_to_right_one_quad_per_glyph() {
        let atlas = Atlas::build();
        let quads = layout_line(&atlas, "Ealing", 100.0, 200.0, 16.0);
        assert_eq!(quads.len(), 6);
        assert!(quads.windows(2).all(|w| w[1].x0 > w[0].x0), "pen went backwards");
        for quad in &quads {
            assert!(quad.x1 > quad.x0 && quad.y1 > quad.y0);
        }
    }

    #[test]
    fn measuring_is_linear_in_size_and_additive_in_text() {
        let atlas = Atlas::build();
        let (w16, h16) = measure_line(&atlas, "Ealing Broadway", 16.0);
        let (w32, h32) = measure_line(&atlas, "Ealing Broadway", 32.0);
        assert!((w32 - 2.0 * w16).abs() < 1e-3, "{w16} then {w32}");
        assert!((h32 - 2.0 * h16).abs() < 1e-3, "{h16} then {h32}");
        let (a, _) = measure_line(&atlas, "Ealing", 16.0);
        let (b, _) = measure_line(&atlas, " Broadway", 16.0);
        assert!((w16 - (a + b)).abs() < 1e-3, "{w16} vs {a} + {b}");
    }

    #[test]
    fn the_measured_width_is_the_span_the_quads_actually_use() {
        let atlas = Atlas::build();
        let (width, _) = measure_line(&atlas, "Ealing", 16.0);
        let quads = layout_line(&atlas, "Ealing", 100.0, 200.0, 16.0);
        let pen_end = 100.0 + width;
        // Every quad is a whole cell, so it overhangs the pen span by
        // the field's padding; nothing may start beyond the pen.
        assert!(quads.iter().all(|q| q.x0 < pen_end), "a quad started past the pen");
        let overhang = 16.0 / EM_PX * ATLAS_CELL_PX.to_f32().expect("cell size fits f32");
        assert!(quads.iter().all(|q| q.x1 <= pen_end + overhang));
    }

    #[test]
    fn each_glyph_samples_only_its_own_cell() {
        let atlas = Atlas::build();
        let quads = layout_line(&atlas, "Ab", 0.0, 0.0, 16.0);
        let a = atlas.glyph('A').expect("A").cell;
        let b = atlas.glyph('b').expect("b").cell;
        assert_eq!((quads[0].u0, quads[0].v0), (atlas.cell_uv(a).0, atlas.cell_uv(a).1));
        assert_eq!((quads[1].u0, quads[1].v0), (atlas.cell_uv(b).0, atlas.cell_uv(b).1));
        assert!((quads[0].u0 - quads[1].u0).abs() > f32::EPSILON);
    }

    #[test]
    fn characters_outside_the_charset_are_dropped_not_guessed() {
        let atlas = Atlas::build();
        assert_eq!(layout_line(&atlas, "Ealing", 0.0, 0.0, 16.0).len(), 6);
        assert_eq!(layout_line(&atlas, "Eaли", 0.0, 0.0, 16.0).len(), 2);
        let cyrillic = layout_line(&atlas, "Илинг", 0.0, 0.0, 16.0);
        assert!(cyrillic.is_empty(), "cyrillic silently rendered as something");
        let (width, _) = measure_line(&atlas, "Илинг", 16.0);
        assert!(width.abs() < f32::EPSILON, "a dropped glyph must not advance the pen");
    }
}
