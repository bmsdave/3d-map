//! The height package: a synthetic ridge, deterministic.
//!
//! Two scenes in one package, because relief has two pictures to prove:
//! the world tile carries continent-scale relief for the globe, and a
//! 5×5 block of z8 tiles carries a mountain range over the fixed centre
//! for the flat card. The ground is a function of the normalised world
//! position, not of the tile — so tiles agree on their shared edges by
//! construction, at every level, the same way the vector shapes do.
//!
//! Nothing here is a landscape from anywhere: it is noise plus a cone,
//! shaped until slope, extent and height are in the range real terrain
//! occupies. Real DEMs (Copernicus, GEBCO) arrive with the pipeline.

use maps2_style::Class;
use maps2_tile::{encode_height, TileBuilder, CLASS_HEIGHTS, HEIGHTS_BYTES, HEIGHTS_SIDE};
use maps2_units::TileId;
use num_traits::ToPrimitive;

use crate::{rect_polygon, EALING};

/// The level the mountain scene is cut at: one z8 tile is ~97 km, so a
/// 5×5 block outlasts any pan the card allows.
pub const RIDGE_DETAIL_LEVEL: u8 = 8;

/// Half-width of the detail block, in tiles.
const DETAIL_HALF_SPAN: i64 = 2;

/// Sea level. Below it the fixture does not go: bathymetry is the
/// pipeline's business (GEBCO), and the format already carries it.
const SEA_LEVEL_M: f64 = 0.0;

/// The one place all fixtures look at, in normalised world coordinates.
fn centre_norm() -> (f64, f64) {
    let (x, y) = maps2_units::world_position_px(EALING, maps2_units::Zoom::new(0.0));
    (x / 256.0, y / 256.0)
}

/// Ground height in metres at a normalised world position.
///
/// Amplitudes are chosen so the whole hypsometric ramp gets used: the
/// lowland sits in its green, the flanks in its tans, and only the
/// crest reaches the snow at the top. A scene that spends its life
/// above the last stop is one flat colour, however much relief it has.
#[must_use]
pub fn height_metres(xn: f64, yn: f64) -> f32 {
    let continents = fbm(xn, yn, 3, 4, 1400.0, 0.55).max(SEA_LEVEL_M);
    // The scene stands in its own basin rather than on top of whatever
    // the continental field happens to be worth here: added, it would
    // lift the whole 500 km block onto a plateau and the ramp would
    // spend its life in one colour.
    let (weight, floor) = massif(xn, yn);
    let base = continents * (1.0 - weight) + floor + ridge(xn, yn);
    // Detail rides on land only, so the sea stays a plane.
    let mask = (base / 300.0).clamp(0.0, 1.0);
    (base + fbm(xn, yn, 64, 8, 300.0, 0.6) * mask).max(SEA_LEVEL_M).to_f32().unwrap_or(f32::MAX)
}

/// The broad swell the range stands on: how much of the world it owns
/// here, and the ground it puts underneath.
fn massif(xn: f64, yn: f64) -> (f64, f64) {
    let (cx, cy) = centre_norm();
    let d = (wrap_delta(xn - cx).hypot(yn - cy)) / 0.02;
    let weight = falloff(d);
    (weight, 500.0 * weight)
}

/// The range itself: a cone stretched along a north-east axis.
fn ridge(xn: f64, yn: f64) -> f64 {
    let (cx, cy) = centre_norm();
    let (dx, dy) = (wrap_delta(xn - cx), yn - cy);
    let angle = 35_f64.to_radians();
    let along = dx * angle.cos() + dy * angle.sin();
    let across = -dx * angle.sin() + dy * angle.cos();
    let d = (along / 0.0075).hypot(across / 0.0022);
    2600.0 * falloff(d)
}

/// Smooth 1 → 0 over `0..1`, flat at both ends so no crease shows.
fn falloff(d: f64) -> f64 {
    let t = (1.0 - d).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Shortest distance in x on a world that wraps at the date line.
fn wrap_delta(dx: f64) -> f64 {
    if dx > 0.5 {
        dx - 1.0
    } else if dx < -0.5 {
        dx + 1.0
    } else {
        dx
    }
}

/// Octaves of value noise, each twice as fine and quieter by `gain`.
fn fbm(xn: f64, yn: f64, first_freq: i64, octaves: u32, amplitude: f64, gain: f64) -> f64 {
    let mut sum = 0.0;
    let mut freq = first_freq;
    let mut amp = amplitude;
    for _ in 0..octaves {
        sum += amp * (value_noise(xn, yn, freq) * 2.0 - 1.0);
        freq *= 2;
        amp *= gain;
    }
    sum
}

/// Value noise on an integer lattice, periodic in x so the date line is
/// not a cliff on the globe.
fn value_noise(xn: f64, yn: f64, freq: i64) -> f64 {
    let (fx, fy) = (xn * freq.to_f64().unwrap_or_default(), yn * freq.to_f64().unwrap_or_default());
    let (ix, iy) = (fx.floor(), fy.floor());
    let (tx, ty) = (smooth(fx - ix), smooth(fy - iy));
    let (ix, iy) = (ix.to_i64().unwrap_or_default(), iy.to_i64().unwrap_or_default());
    let at = |dx: i64, dy: i64| lattice((ix + dx).rem_euclid(freq), iy + dy);
    let top = at(0, 0) + (at(1, 0) - at(0, 0)) * tx;
    let bottom = at(0, 1) + (at(1, 1) - at(0, 1)) * tx;
    top + (bottom - top) * ty
}

fn smooth(t: f64) -> f64 {
    t * t * (3.0 - 2.0 * t)
}

/// A deterministic value in `0..1` at a lattice point.
fn lattice(ix: i64, iy: i64) -> f64 {
    let mut h = ix.cast_unsigned()
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ iy.cast_unsigned().wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h >> 11).to_f64().unwrap_or_default() / (1_u64 << 53).to_f64().unwrap_or(1.0)
}

/// The heights section of one tile: samples on the inclusive `0..=255`
/// grid, so `i / 255` of the tile — edge samples are shared with the
/// neighbour rather than half a step short of it.
#[must_use]
pub fn heights_raster(id: TileId) -> Vec<u8> {
    let n = f64::from(1_u32 << id.z);
    let last = (HEIGHTS_SIDE - 1).to_f64().unwrap_or(1.0);
    let mut out = Vec::with_capacity(HEIGHTS_BYTES);
    for j in 0..HEIGHTS_SIDE {
        let yn = (f64::from(id.y) + j.to_f64().unwrap_or_default() / last) / n;
        for i in 0..HEIGHTS_SIDE {
            let xn = (f64::from(id.x) + i.to_f64().unwrap_or_default() / last) / n;
            out.extend_from_slice(&encode_height(height_metres(xn, yn)).to_le_bytes());
        }
    }
    out
}

/// Every tile of the package: the world tile plus the mountain scene.
#[must_use]
pub fn ridge_coverage() -> Vec<TileId> {
    let mut out = vec![TileId { z: 0, x: 0, y: 0 }];
    let centre = maps2_units::locate(EALING, RIDGE_DETAIL_LEVEL).tile;
    for dy in -DETAIL_HALF_SPAN..=DETAIL_HALF_SPAN {
        for dx in -DETAIL_HALF_SPAN..=DETAIL_HALF_SPAN {
            out.push(TileId {
                z: RIDGE_DETAIL_LEVEL,
                x: u32::try_from(i64::from(centre.x) + dx).unwrap_or_default(),
                y: u32::try_from(i64::from(centre.y) + dy).unwrap_or_default(),
            });
        }
    }
    out
}

#[must_use]
pub fn ridge_tiles() -> Vec<(TileId, Vec<u8>)> {
    ridge_coverage().into_iter().map(|id| (id, ridge_tile_bytes(id))).collect()
}

/// One tile: ground under the whole square, and its heights.
#[must_use]
///
/// # Panics
///
/// Panics only if this bounded synthetic fixture cannot fit MT2.
pub fn ridge_tile_bytes(id: TileId) -> Vec<u8> {
    let mut builder = TileBuilder::new(id);
    builder.push(Class::Land.code(), rect_polygon(1, (0, 0, 65535, 65535)));
    builder.push_raster(CLASS_HEIGHTS, heights_raster(id));
    builder.build().expect("ridge fixture fits MT2")
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::{HeightsRaster, TileView, CLASS_HEIGHTS, HEIGHTS_BYTES, HEIGHTS_SIDE};
    use maps2_units::locate;
    use num_traits::ToPrimitive;

    /// The package must be bit-for-bit stable, like the Ealing one.
    const GOLDEN_FNV1A: u64 = 0xD555_7886_32CA_7AA4;

    fn fnv1a(bytes: &[u8], mut hash: u64) -> u64 {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        hash
    }

    fn raster_of(id: TileId) -> Vec<u8> {
        let bytes = ridge_tile_bytes(id);
        let tile = TileView::parse(&bytes).expect("parses");
        tile.raster(CLASS_HEIGHTS).expect("heights section").to_vec()
    }

    fn sample(raster: &HeightsRaster<'_>, x: usize, y: usize) -> f32 {
        raster.metres(i32::try_from(x).unwrap_or_default(), i32::try_from(y).unwrap_or_default())
    }

    fn column(bytes: &[u8], x: usize) -> Vec<f32> {
        let raster = HeightsRaster::parse(bytes).expect("full raster");
        (0..HEIGHTS_SIDE).map(|y| sample(&raster, x, y)).collect()
    }

    fn row(bytes: &[u8], y: usize) -> Vec<f32> {
        let raster = HeightsRaster::parse(bytes).expect("full raster");
        (0..HEIGHTS_SIDE).map(|x| sample(&raster, x, y)).collect()
    }

    #[test]
    fn every_tile_carries_a_full_heights_section() {
        for (id, bytes) in ridge_tiles() {
            let tile = TileView::parse(&bytes).expect("parses");
            let raster = tile.raster(CLASS_HEIGHTS).expect("heights section");
            assert_eq!(raster.len(), HEIGHTS_BYTES, "z{} {} {}", id.z, id.x, id.y);
            assert!(HeightsRaster::parse(raster).is_ok());
        }
    }

    #[test]
    fn neighbouring_tiles_share_their_edge_samples() {
        // The grid is inclusive of both edges, so a surface built from
        // two tiles has no cliff along the seam — the same rule the
        // vector shapes follow.
        let centre = locate(EALING, RIDGE_DETAIL_LEVEL).tile;
        let east = TileId { z: centre.z, x: centre.x + 1, y: centre.y };
        let south = TileId { z: centre.z, x: centre.x, y: centre.y + 1 };
        assert_eq!(column(&raster_of(centre), HEIGHTS_SIDE - 1), column(&raster_of(east), 0));
        assert_eq!(row(&raster_of(centre), HEIGHTS_SIDE - 1), row(&raster_of(south), 0));
    }

    #[test]
    fn the_ridge_stands_where_the_camera_looks_and_the_lowland_is_at_sea_level() {
        let centre = locate(EALING, RIDGE_DETAIL_LEVEL);
        let raster_bytes = raster_of(centre.tile);
        let raster = HeightsRaster::parse(&raster_bytes).expect("raster");
        let peak = (0..HEIGHTS_SIDE)
            .flat_map(|y| (0..HEIGHTS_SIDE).map(move |x| (x, y)))
            .map(|(x, y)| sample(&raster, x, y))
            .fold(f32::MIN, f32::max);
        assert!(peak > 2000.0, "the centre tile has no mountain: {peak} m");
        // The ramp's last stop is snow at 3600 m. A scene whose every
        // sample is past it is one flat white, so the crest may reach
        // the top of the ramp but the tile must not live above it.
        let above_snow = (0..HEIGHTS_SIDE)
            .flat_map(|y| (0..HEIGHTS_SIDE).map(move |x| (x, y)))
            .filter(|(x, y)| sample(&raster, *x, *y) > 3600.0)
            .count();
        let share = above_snow.to_f32().unwrap_or_default()
            / (HEIGHTS_SIDE * HEIGHTS_SIDE).to_f32().unwrap_or(1.0);
        assert!(share < 0.15, "{:.0}% of the scene is above the ramp", share * 100.0);
        // Somewhere on the world tile the ground is at sea level: the
        // hypsometric ramp needs both ends of its scale.
        let world_bytes = raster_of(TileId { z: 0, x: 0, y: 0 });
        let world = HeightsRaster::parse(&world_bytes).expect("raster");
        let lowest = (0..HEIGHTS_SIDE)
            .flat_map(|y| (0..HEIGHTS_SIDE).map(move |x| (x, y)))
            .map(|(x, y)| sample(&world, x, y))
            .fold(f32::MAX, f32::min);
        assert!(lowest.abs() < f32::EPSILON, "no lowland on the world tile: {lowest}");
    }

    #[test]
    fn the_world_tile_has_relief_worth_a_globe() {
        let bytes = raster_of(TileId { z: 0, x: 0, y: 0 });
        let raster = HeightsRaster::parse(&bytes).expect("raster");
        let peak = (0..HEIGHTS_SIDE)
            .flat_map(|y| (0..HEIGHTS_SIDE).map(move |x| (x, y)))
            .map(|(x, y)| sample(&raster, x, y))
            .fold(f32::MIN, f32::max);
        assert!(peak > 1500.0, "the globe is a billiard ball: {peak} m");
    }

    #[test]
    fn the_package_covers_the_globe_and_the_mountain_scene() {
        let ids = ridge_coverage();
        assert!(ids.contains(&TileId { z: 0, x: 0, y: 0 }), "no world tile");
        let centre = locate(EALING, RIDGE_DETAIL_LEVEL).tile;
        let detail = ids.iter().filter(|id| id.z == RIDGE_DETAIL_LEVEL).count();
        assert_eq!(detail, 25, "the scene must outlast a viewport pan");
        assert!(ids.contains(&centre));
    }

    #[test]
    fn the_package_bytes_are_golden() {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for (id, bytes) in ridge_tiles() {
            hash = fnv1a(&[id.z], hash);
            hash = fnv1a(&id.x.to_le_bytes(), hash);
            hash = fnv1a(&id.y.to_le_bytes(), hash);
            hash = fnv1a(&bytes, hash);
        }
        assert_eq!(hash, GOLDEN_FNV1A, "package changed: new hash {hash:#x}");
    }
}
