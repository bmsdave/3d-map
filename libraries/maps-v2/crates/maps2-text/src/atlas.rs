//! The signed distance field atlas: one cell per glyph, one texture for
//! every size. Built in process — the field is a few hundred kilobytes
//! of arithmetic, deterministic on every machine, and generated once
//! per map rather than per frame. The offline `gen-atlas` binary writes
//! the very same bytes out as PNG plus metrics for eyeballing.

use crate::font::{advance_em, strokes, CHARSET, STROKE_HALF_EM};
use num_traits::ToPrimitive;

/// Side of one glyph cell in atlas pixels.
pub const ATLAS_CELL_PX: u32 = 32;
/// Cells per atlas row.
pub const ATLAS_COLUMNS: u32 = 16;
/// One em in atlas pixels. The rest of the cell is the field's room to
/// fall off, which is what a halo needs.
pub const EM_PX: f32 = 18.0;
/// Distance encoded across the byte range: 0 is `-SDF_RANGE_PX`, 255 is
/// `+SDF_RANGE_PX`, 128 is exactly on the outline.
pub const SDF_RANGE_PX: f32 = 4.0;

/// Where a glyph's origin sits inside its cell, in atlas pixels from the
/// cell's top-left corner, y down. Chosen so that an ascender's halo and
/// a descender's halo both stay inside the cell.
pub const CELL_ORIGIN_PX: (f32, f32) = (7.0, 23.0);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GlyphMetrics {
    pub ch: char,
    /// Index of the glyph's cell, row-major.
    pub cell: u32,
    /// Pen movement after this glyph, in em units.
    pub advance: f32,
}

pub struct Atlas {
    pub width: u32,
    pub height: u32,
    /// One byte per pixel, row-major: the encoded distance.
    pub pixels: Vec<u8>,
    glyphs: Vec<GlyphMetrics>,
}

/// A glyph's skeleton in atlas pixels, y down, together with the box it
/// can possibly influence.
struct Skeleton {
    segments: Vec<((f32, f32), (f32, f32))>,
    reach: (f32, f32, f32, f32),
}

impl Atlas {
    #[must_use]
    pub fn build() -> Self {
        let count = CHARSET.chars().count().to_u32().unwrap_or_default();
        let rows = count.div_ceil(ATLAS_COLUMNS);
        let width = ATLAS_COLUMNS * ATLAS_CELL_PX;
        let height = rows * ATLAS_CELL_PX;
        let mut atlas = Self {
            width,
            height,
            pixels: vec![0; usize::try_from(width * height).unwrap_or_default()],
            glyphs: Vec::with_capacity(count.to_usize().unwrap_or_default()),
        };
        for (cell, ch) in CHARSET.chars().enumerate() {
            let cell = cell.to_u32().unwrap_or_default();
            atlas.glyphs.push(GlyphMetrics { ch, cell, advance: advance_em(ch) });
            atlas.draw_cell(cell, &skeleton(ch));
        }
        atlas
    }

    /// Metrics of one character, or `None` when it is outside the
    /// charset — a caller substitutes, this crate never guesses.
    #[must_use]
    pub fn glyph(&self, ch: char) -> Option<GlyphMetrics> {
        self.glyphs.iter().copied().find(|g| g.ch == ch)
    }

    /// Texture coordinates of a cell: `(u0, v0, u1, v1)`.
    #[must_use]
    pub fn cell_uv(&self, cell: u32) -> (f32, f32, f32, f32) {
        let x = (cell % ATLAS_COLUMNS) * ATLAS_CELL_PX;
        let y = (cell / ATLAS_COLUMNS) * ATLAS_CELL_PX;
        let (w, h) = (self.width.to_f32().unwrap_or(1.0), self.height.to_f32().unwrap_or(1.0));
        (
            x.to_f32().unwrap_or_default() / w,
            y.to_f32().unwrap_or_default() / h,
            (x + ATLAS_CELL_PX).to_f32().unwrap_or_default() / w,
            (y + ATLAS_CELL_PX).to_f32().unwrap_or_default() / h,
        )
    }

    #[must_use]
    pub fn glyphs(&self) -> &[GlyphMetrics] {
        &self.glyphs
    }

    fn draw_cell(&mut self, cell: u32, skeleton: &Skeleton) {
        if skeleton.segments.is_empty() {
            return;
        }
        let base_x = (cell % ATLAS_COLUMNS) * ATLAS_CELL_PX;
        let base_y = (cell / ATLAS_COLUMNS) * ATLAS_CELL_PX;
        for (row, col) in cell_pixels() {
            let Some(value) = skeleton.encoded_at((col.to_f32().unwrap_or_default() + 0.5, row.to_f32().unwrap_or_default() + 0.5)) else {
                continue;
            };
            let index = usize::try_from((base_y + row) * self.width + base_x + col).unwrap_or_default();
            self.pixels[index] = value;
        }
    }
}

fn cell_pixels() -> impl Iterator<Item = (u32, u32)> {
    (0..ATLAS_CELL_PX).flat_map(|row| (0..ATLAS_CELL_PX).map(move |col| (row, col)))
}

impl Skeleton {
    /// The encoded field at a point of the cell, or `None` where the
    /// glyph is too far away to have anything to say.
    fn encoded_at(&self, point: (f32, f32)) -> Option<u8> {
        if point.0 < self.reach.0
            || point.1 < self.reach.1
            || point.0 > self.reach.2
            || point.1 > self.reach.3
        {
            return None;
        }
        let nearest = self
            .segments
            .iter()
            .map(|(a, b)| distance_to_segment(point, *a, *b))
            .fold(f32::INFINITY, f32::min);
        let signed = STROKE_HALF_EM.mul_add(EM_PX, -nearest);
        Some((signed / SDF_RANGE_PX).mul_add(127.0, 128.0).clamp(0.0, 255.0).to_u8().unwrap_or(u8::MAX))
    }
}

/// Em units, y up, to cell pixels, y down.
fn skeleton(ch: char) -> Skeleton {
    let to_cell = |p: &(f32, f32)| {
        (
            EM_PX.mul_add(p.0, CELL_ORIGIN_PX.0),
            CELL_ORIGIN_PX.1 - EM_PX * p.1,
        )
    };
    let mut segments = Vec::new();
    let mut reach = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for polyline in strokes(ch) {
        let points: Vec<(f32, f32)> = polyline.iter().map(to_cell).collect();
        for point in &points {
            reach.0 = reach.0.min(point.0);
            reach.1 = reach.1.min(point.1);
            reach.2 = reach.2.max(point.0);
            reach.3 = reach.3.max(point.1);
        }
        segments.extend(points.windows(2).map(|w| (w[0], w[1])));
        if points.len() == 1 {
            segments.push((points[0], points[0]));
        }
    }
    let margin = STROKE_HALF_EM.mul_add(EM_PX, SDF_RANGE_PX);
    Skeleton {
        segments,
        reach: (reach.0 - margin, reach.1 - margin, reach.2 + margin, reach.3 + margin),
    }
}

fn distance_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let length2 = dx.mul_add(dx, dy * dy);
    let t = if length2 <= f32::EPSILON {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / length2).clamp(0.0, 1.0)
    };
    let (nx, ny) = (dx.mul_add(t, a.0), dy.mul_add(t, a.1));
    (p.0 - nx).hypot(p.1 - ny)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(atlas: &Atlas, ch: char, dx: f32, dy: f32) -> u8 {
        let cell = atlas.glyph(ch).expect("charset glyph").cell;
        let col = cell % ATLAS_COLUMNS;
        let row = cell / ATLAS_COLUMNS;
        let x = col * ATLAS_CELL_PX + EM_PX.mul_add(dx, CELL_ORIGIN_PX.0) as u32;
        let y = row * ATLAS_CELL_PX + (CELL_ORIGIN_PX.1 - dy * EM_PX) as u32;
        atlas.pixels[(y * atlas.width + x) as usize]
    }

    #[test]
    fn the_atlas_carries_exactly_the_documented_charset() {
        let atlas = Atlas::build();
        for ch in CHARSET.chars() {
            assert!(atlas.glyph(ch).is_some(), "{ch:?} missing from the atlas");
        }
        assert_eq!(atlas.glyph('Ж'), None, "outside the charset must be None");
        assert_eq!(atlas.glyphs().len(), CHARSET.chars().count());
        let cells: Vec<u32> = atlas.glyphs().iter().map(|g| g.cell).collect();
        let mut sorted = cells.clone();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), cells.len(), "two glyphs share a cell");
    }

    #[test]
    fn the_field_is_positive_inside_the_stroke_and_falls_off_outside() {
        let atlas = Atlas::build();
        // 'I' is a single vertical stroke through the middle of the cap
        // height: on it the field is inside, and it only gets darker as
        // the sample walks away from it.
        let on_stroke = sample(&atlas, 'I', 0.08, 0.35);
        assert!(on_stroke > 128, "centre of the stroke read {on_stroke}");
        let near = sample(&atlas, 'I', 0.20, 0.35);
        let far = sample(&atlas, 'I', 0.34, 0.35);
        assert!(near < 128, "0.2em off the stroke read {near}");
        assert!(far < near, "field not monotone: {near} then {far}");
    }

    #[test]
    fn a_blank_glyph_still_advances_the_pen() {
        let atlas = Atlas::build();
        let space = atlas.glyph(' ').expect("space is in the charset");
        assert!(space.advance > 0.0, "space advance {}", space.advance);
        for ch in CHARSET.chars() {
            let advance = atlas.glyph(ch).expect("charset").advance;
            assert!(advance > 0.0 && advance < 1.2, "{ch:?} advances {advance}");
        }
    }

    #[test]
    fn cells_tile_the_texture_without_overlapping() {
        let atlas = Atlas::build();
        assert_eq!(atlas.width, ATLAS_COLUMNS * ATLAS_CELL_PX);
        assert_eq!(atlas.pixels.len(), (atlas.width * atlas.height) as usize);
        for glyph in atlas.glyphs() {
            let (u0, v0, u1, v1) = atlas.cell_uv(glyph.cell);
            assert!((0.0..=1.0).contains(&u0) && (0.0..=1.0).contains(&u1));
            assert!((0.0..=1.0).contains(&v0) && (0.0..=1.0).contains(&v1));
            let width = (u1 - u0) * atlas.width as f32;
            assert!((width - ATLAS_CELL_PX as f32).abs() < 1e-3, "cell width {width}");
        }
    }

    /// The atlas is generated, not shipped, so it has to come out
    /// identical on every machine — otherwise every golden downstream
    /// of it is a lottery.
    #[test]
    fn the_atlas_bytes_are_the_same_on_every_machine() {
        let first = Atlas::build();
        let second = Atlas::build();
        assert_eq!(first.pixels, second.pixels);
        let ink = first.pixels.iter().filter(|p| **p > 128).count();
        let total = first.pixels.len();
        assert!(ink * 40 > total, "only {ink} of {total} pixels carry ink");
        assert!(ink * 3 < total, "{ink} of {total} pixels are ink — the field leaked");
    }

    /// A glyph's field must not bleed into its neighbour's cell, or a
    /// quad would sample the letter next door at its edges.
    #[test]
    fn no_glyph_reaches_the_border_of_its_cell() {
        let atlas = Atlas::build();
        let last = ATLAS_CELL_PX - 1;
        let border = |cell: u32| {
            let (bx, by) = (cell % ATLAS_COLUMNS * ATLAS_CELL_PX, cell / ATLAS_COLUMNS * ATLAS_CELL_PX);
            (0..ATLAS_CELL_PX).flat_map(move |i| {
                [(bx + i, by), (bx + i, by + last), (bx, by + i), (bx + last, by + i)]
            })
        };
        for glyph in atlas.glyphs() {
            for (x, y) in border(glyph.cell) {
                let value = atlas.pixels[(y * atlas.width + x) as usize];
                assert!(value < 128, "{:?} inked its cell border: {value}", glyph.ch);
            }
        }
    }
}
