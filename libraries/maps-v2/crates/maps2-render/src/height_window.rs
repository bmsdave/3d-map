//! Reading one tile's ground out of an ancestor's raster.
//!
//! The pyramid stops carrying heights at [`maps2_units::TileId`] level
//! `TERRAIN_MAX_Z` (the ingest constant), because the DEM under it stops
//! having anything new to say: Copernicus GLO-30 is 30 m, which a z12
//! tile already samples at 38 m and a z16 tile would only interpolate.
//! Below the cap a tile has no raster of its own and reads the nearest
//! ancestor that has one — the same surface, addressed through a window.
//!
//! Everything the height shaders do to find and sample that window is
//! written here first, in arithmetic that runs under `cargo test`. The
//! shaders mirror it; these tests are what say the mirror is honest.

use maps2_tile::{HeightsRaster, HEIGHTS_SIDE};
use maps2_units::TileId;
use num_traits::ToPrimitive;

/// The highest texel index in a raster, as the shaders spell it. Not a
/// cast: `HEIGHTS_SIDE` is a `usize`, and going through `u16` says out
/// loud that 255 fits in an `f32` exactly.
fn last_texel() -> f32 {
    f32::from(u16::try_from(HEIGHTS_SIDE - 1).unwrap_or(u16::MAX))
}

/// Where a descendant sits inside an ancestor's raster, in unit
/// coordinates: `ancestor_unit = offset + tile_unit * scale`.
///
/// A tile reading its own raster gets the identity window — scale 1,
/// offset 0 — so the sampling path is one path, not two.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HeightWindow {
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl HeightWindow {
    pub const IDENTITY: Self = Self { scale: 1.0, offset_x: 0.0, offset_y: 0.0 };

    /// The window `tile` occupies inside `source`, or `None` when
    /// `source` is not `tile` or one of its ancestors — asking for a
    /// window onto unrelated ground is a bug, not a fallback.
    #[must_use]
    pub fn of(tile: TileId, source: TileId) -> Option<Self> {
        if source.z > tile.z {
            return None;
        }
        let depth = tile.z - source.z;
        let step = 1_u32.checked_shl(u32::from(depth))?;
        if tile.x >> depth != source.x || tile.y >> depth != source.y {
            return None;
        }
        // The arithmetic is exact in f64 and every result is a small
        // multiple of a power of two, so the narrowing is lossless — but
        // it is spelled out rather than cast, like `texel_metres` does.
        let scale = 1.0 / f64::from(step);
        let narrow = |value: f64| value.to_f32().unwrap_or_default();
        Some(Self {
            scale: narrow(scale),
            offset_x: narrow(f64::from(tile.x - (source.x << depth)) * scale),
            offset_y: narrow(f64::from(tile.y - (source.y << depth)) * scale),
        })
    }

    /// Unit position within the tile → sample position within the
    /// source raster, in texels.
    #[must_use]
    pub fn texel_of(&self, unit_x: f32, unit_y: f32) -> (f32, f32) {
        let last = last_texel();
        (
            (self.offset_x + unit_x * self.scale) * last,
            (self.offset_y + unit_y * self.scale) * last,
        )
    }
}

/// How far above a tile its ground may be read from.
///
/// Four, because that is the distance the terrain cap creates: tiles run
/// to z16 and rasters stop at z12. The limit is the point of the
/// constant, not the number — walking further finds *a* raster, and on a
/// world package that raster is a z3 tile covering a quarter of the
/// planet at eleven kilometres a sample. Shading a street from it is not
/// coarse terrain, it is a flat field with a texel spacing that makes
/// every slope read as zero. Better to say there is no terrain here and
/// draw the ground flat, which is honest and looks the same.
pub const MAX_ANCESTOR_DEPTH: u8 = 4;

/// The nearest tile at or above `tile` whose raster is loaded, within
/// [`MAX_ANCESTOR_DEPTH`].
///
/// Walks up one level at a time and stops at the first hit, so a tile
/// four levels below the cap costs four lookups once per frame, not per
/// sample. `None` means nothing close enough above it is resident — the
/// caller draws flat ground, which is what it did before any of this.
#[must_use]
pub fn height_source(tile: TileId, resident: impl Fn(TileId) -> bool) -> Option<TileId> {
    let mut candidate = tile;
    for _ in 0..=MAX_ANCESTOR_DEPTH {
        if resident(candidate) {
            return Some(candidate);
        }
        if candidate.z == 0 {
            return None;
        }
        candidate = TileId { z: candidate.z - 1, x: candidate.x / 2, y: candidate.y / 2 };
    }
    None
}

/// Height at a fractional texel, bilinear between the four around it.
///
/// Nearest-neighbour is what a tile reading its own raster can afford:
/// there, one texel is one sample of the DEM and the steps are below a
/// pixel. Reading an ancestor magnifies those steps by a power of two —
/// at four levels down a single texel spans the whole tile — and
/// nearest-neighbour turns the ground into visible terraces. The shaders
/// cannot lean on hardware filtering to avoid it: the raster is an
/// `R16UI` texture, and WebGL2 will not filter integer textures.
#[must_use]
pub fn sample_bilinear(raster: &HeightsRaster<'_>, x: f32, y: f32) -> f32 {
    let last = last_texel();
    let x = x.clamp(0.0, last);
    let y = y.clamp(0.0, last);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    // Clamped to the raster above, so every index is in range; `metres`
    // clamps again on its own account and cannot be made to panic.
    let at = |dx: f32, dy: f32| {
        raster.metres(
            (x0 + dx).to_i32().unwrap_or_default(),
            (y0 + dy).to_i32().unwrap_or_default(),
        )
    };
    let top = at(0.0, 0.0) + (at(1.0, 0.0) - at(0.0, 0.0)) * fx;
    let bottom = at(0.0, 1.0) + (at(1.0, 1.0) - at(0.0, 1.0)) * fx;
    top + (bottom - top) * fy
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::{encode_height, HEIGHTS_BYTES};

    fn tile(z: u8, x: u32, y: u32) -> TileId {
        TileId { z, x, y }
    }

    #[test]
    fn a_tile_reading_its_own_raster_gets_the_identity_window() {
        let id = tile(12, 2046, 1361);
        assert_eq!(HeightWindow::of(id, id), Some(HeightWindow::IDENTITY));
    }

    #[test]
    fn each_child_gets_its_own_quarter_of_the_parent() {
        let parent = tile(11, 1023, 680);
        let corners = [
            ((2046, 1360), (0.0, 0.0)),
            ((2047, 1360), (0.5, 0.0)),
            ((2046, 1361), (0.0, 0.5)),
            ((2047, 1361), (0.5, 0.5)),
        ];
        for ((x, y), (offset_x, offset_y)) in corners {
            let window = HeightWindow::of(tile(12, x, y), parent).expect("a child of the parent");
            assert_eq!(window, HeightWindow { scale: 0.5, offset_x, offset_y });
        }
    }

    /// Four levels down is a sixteenth of the ancestor per axis, and the
    /// deepest tile the cap makes possible: z16 reading z12.
    #[test]
    fn four_levels_down_is_a_sixteenth_of_the_ancestor() {
        let source = tile(12, 2046, 1361);
        let deep = tile(16, 2046 * 16 + 5, 1361 * 16 + 11);
        let window = HeightWindow::of(deep, source).expect("a descendant");
        assert!((window.scale - 1.0 / 16.0).abs() < f32::EPSILON);
        assert!((window.offset_x - 5.0 / 16.0).abs() < f32::EPSILON);
        assert!((window.offset_y - 11.0 / 16.0).abs() < f32::EPSILON);
    }

    #[test]
    fn ground_that_is_not_an_ancestor_has_no_window() {
        // A neighbour at the same level, its neighbour's child, and a
        // tile deeper than the one asked about.
        assert_eq!(HeightWindow::of(tile(12, 2046, 1361), tile(12, 2047, 1361)), None);
        assert_eq!(HeightWindow::of(tile(13, 4090, 2722), tile(12, 2046, 1361)), None);
        assert_eq!(HeightWindow::of(tile(11, 1023, 680), tile(12, 2046, 1361)), None);
    }

    #[test]
    fn a_window_maps_the_tiles_corners_onto_the_ancestors_texels() {
        let window = HeightWindow::of(tile(12, 2047, 1361), tile(11, 1023, 680)).expect("child");
        let last = last_texel();
        assert_eq!(window.texel_of(0.0, 0.0), (last * 0.5, last * 0.5));
        assert_eq!(window.texel_of(1.0, 1.0), (last, last));
    }

    #[test]
    fn the_search_stops_at_the_first_ancestor_that_has_a_raster() {
        let deep = tile(16, 32744, 21791);
        let cap = tile(12, 2046, 1361);
        assert_eq!(height_source(deep, |id| id == cap), Some(cap));
        // Its own raster wins over any ancestor's.
        assert_eq!(height_source(deep, |_| true), Some(deep));
        // Nothing resident anywhere above: flat ground, not a panic.
        assert_eq!(height_source(deep, |_| false), None);
    }

    /// The failure this constant exists for: on a world package the walk
    /// used to run past the city levels and land on a z3 tile spanning a
    /// quarter of the planet, then shade a street with it.
    #[test]
    fn an_ancestor_further_up_than_the_limit_is_not_terrain() {
        let deep = tile(16, 32744, 21791);
        let too_far = tile(11, 1023, 680);
        assert_eq!(deep.z - too_far.z, MAX_ANCESTOR_DEPTH + 1);
        assert_eq!(height_source(deep, |id| id == too_far), None);

        let just_within = tile(12, 2046, 1361);
        assert_eq!(deep.z - just_within.z, MAX_ANCESTOR_DEPTH);
        assert_eq!(height_source(deep, |id| id == just_within), Some(just_within));
    }

    fn raster_bytes(values: impl Fn(usize, usize) -> f32) -> Vec<u8> {
        let mut bytes = vec![0_u8; HEIGHTS_BYTES];
        for y in 0..HEIGHTS_SIDE {
            for x in 0..HEIGHTS_SIDE {
                let at = (y * HEIGHTS_SIDE + x) * 2;
                bytes[at..at + 2].copy_from_slice(&encode_height(values(x, y)).to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn on_a_texel_centre_bilinear_reads_that_texel() {
        #[allow(clippy::cast_precision_loss)]
        let bytes = raster_bytes(|x, y| (x * 3 + y) as f32);
        let raster = HeightsRaster::parse(&bytes).expect("raster");
        for (x, y) in [(0, 0), (1, 7), (128, 200), (255, 255)] {
            #[allow(clippy::cast_precision_loss)]
            let (fx, fy) = (x as f32, y as f32);
            assert!(
                (sample_bilinear(&raster, fx, fy) - raster.metres(x, y)).abs() < 0.001,
                "at {x},{y}",
            );
        }
    }

    /// The property the shading depends on: on a linear ramp the
    /// interpolated value is the ramp's own value, so an ancestor read
    /// through a window has no terraces in it.
    #[test]
    fn halfway_between_two_texels_is_halfway_up_the_ramp() {
        #[allow(clippy::cast_precision_loss)]
        let bytes = raster_bytes(|x, _| x as f32 * 4.0);
        let raster = HeightsRaster::parse(&bytes).expect("raster");
        assert!((sample_bilinear(&raster, 10.5, 3.0) - 42.0).abs() < 0.01);
        assert!((sample_bilinear(&raster, 10.25, 3.0) - 41.0).abs() < 0.01);
    }

    #[test]
    fn sampling_outside_the_raster_clamps_instead_of_panicking() {
        #[allow(clippy::cast_precision_loss)]
        let bytes = raster_bytes(|x, y| (x + y) as f32);
        let raster = HeightsRaster::parse(&bytes).expect("raster");
        assert!((sample_bilinear(&raster, -5.0, -5.0) - raster.metres(0, 0)).abs() < 0.001);
        assert!((sample_bilinear(&raster, 999.0, 999.0) - raster.metres(255, 255)).abs() < 0.001);
    }
}
