//! Units of the map: zoom, tile addressing, ground metres, screen pixels.
//!
//! The point of this crate is to make the wrong unit unrepresentable.
//! `Metres` and `ScreenPx` are distinct types, and the only way between
//! them is an explicit function that demands latitude and zoom — there is
//! deliberately no other constructor, so "a metre became a pixel" cannot
//! compile. Sizes are stated in ground metres or screen pixels; a fraction
//! of the world is not a unit here (v1 grew a 48 km wide road from one).

use std::f64::consts::PI;

use num_traits::ToPrimitive;

/// One tile is this many CSS pixels at its own zoom level.
pub const TILE_SIZE_PX: f64 = 256.0;

/// Coordinate grid resolution inside one tile: coordinates are integers
/// in `0..TILE_EXTENT`. 65536 steps keep the grid finer than 10 cm at
/// z16; `f32` normalised to world bounds would step 2.4 m and buildings
/// would shiver.
pub const TILE_EXTENT: u32 = 65536;

/// Equatorial circumference of the WGS84 ellipsoid, metres.
pub const EARTH_CIRCUMFERENCE_METRES: f64 = 40_075_016.685_578_49;

/// Web Mercator's latitude limit; the square world ends here.
pub const MAX_LATITUDE_DEG: f64 = 85.051_128_779_806_59;

/// Continuous camera zoom. `z14.37` is a legal state the map must be
/// correct at, not a rounding error.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Zoom(f64);

impl Zoom {
    #[must_use]
    pub fn new(value: f64) -> Self {
        debug_assert!(value.is_finite(), "zoom must be finite, got {value}");
        if !value.is_finite() {
            return Self(0.0);
        }
        Self(value)
    }

    #[must_use]
    pub fn try_new(value: f64) -> Option<Self> {
        if value.is_finite() { Some(Self(value)) } else { None }
    }

    /// Hot path: caller already validated finiteness.
    #[must_use]
    pub fn new_unchecked(value: f64) -> Self {
        Self(value)
    }

    #[must_use]
    pub fn value(self) -> f64 {
        self.0
    }

    /// Width of the whole world in pixels at this zoom: `256 · 2^z`,
    /// without rounding the zoom to an integer.
    #[must_use]
    pub fn world_pixels(self) -> f64 {
        TILE_SIZE_PX * 2_f64.powf(self.0)
    }
}

/// Global Web Mercator tile address.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileId {
    pub z: u8,
    pub x: u32,
    pub y: u32,
}

/// Integer coordinate inside a tile, on the `TILE_EXTENT` grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileCoord(pub u16, pub u16);

/// A point addressed exactly: which tile, and where inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TilePoint {
    pub tile: TileId,
    pub coord: TileCoord,
}

/// Ground distance. Cannot be added to `ScreenPx` by type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metres(pub f64);

/// Screen distance in logical pixels. Cannot be added to `Metres` by type.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPx(pub f32);

/// A position on screen in logical pixels, y pointing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenPoint {
    pub x: f32,
    pub y: f32,
}

/// Geographic input from data sources. It does not leak outward: past the
/// pipeline boundary the world speaks tiles and screen units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lonlat {
    pub lon: f64,
    pub lat: f64,
}

fn mercator_normalised(point: Lonlat) -> (f64, f64) {
    let lat = point
        .lat
        .clamp(-MAX_LATITUDE_DEG, MAX_LATITUDE_DEG)
        .to_radians();
    let x = (point.lon + 180.0) / 360.0;
    let y = (1.0 - ((lat.tan() + 1.0 / lat.cos()).ln()) / PI) / 2.0;
    (x, y)
}

fn grid_index(normalised: f64, grid: u64) -> u64 {
    let scaled = (normalised * grid_as_f64(grid)).round();
    if scaled <= 0.0 {
        0
    } else if scaled >= grid_as_f64(grid) {
        grid - 1
    } else {
        scaled
            .to_u64()
            .expect("a finite grid coordinate within u64 bounds")
    }
}

fn grid_as_f64(grid: u64) -> f64 {
    // Levels stay far below the 2^53 mantissa limit: z22 · 65536 = 2^38.
    grid.to_f64().expect("the map grid is representable as f64")
}

fn tile_component(value: u64) -> u32 {
    u32::try_from(value).expect("tile coordinate fits u32")
}

fn tile_coordinate(value: u64) -> u16 {
    u16::try_from(value).expect("tile grid coordinate fits u16")
}

/// Address a geographic point on the integer grid of tile `level`.
#[must_use]
pub fn locate(point: Lonlat, level: u8) -> TilePoint {
    debug_assert!(level <= 22, "level must be <=22 for grid bounds, got {level}");
    let level = level.min(22);
    let (xn, yn) = mercator_normalised(point);
    let extent = u64::from(TILE_EXTENT);
    let grid = (1_u64 << level) * extent;
    let gx = grid_index(xn, grid);
    let gy = grid_index(yn, grid);
    TilePoint {
        tile: TileId {
            z: level,
            x: tile_component(gx / extent),
            y: tile_component(gy / extent),
        },
        coord: TileCoord(tile_coordinate(gx % extent), tile_coordinate(gy % extent)),
    }
}

/// The geographic point at an exact grid position.
#[must_use]
pub fn to_lonlat(point: TilePoint) -> Lonlat {
    let extent = u64::from(TILE_EXTENT);
    let grid = grid_as_f64((1_u64 << point.tile.z) * extent);
    let gx = u64::from(point.tile.x) * extent + u64::from(point.coord.0);
    let gy = u64::from(point.tile.y) * extent + u64::from(point.coord.1);
    let xn = grid_as_f64(gx) / grid;
    let yn = grid_as_f64(gy) / grid;
    Lonlat {
        lon: xn * 360.0 - 180.0,
        lat: (PI * (1.0 - 2.0 * yn)).sinh().atan().to_degrees(),
    }
}

/// Ground size of one grid step at this tile level and latitude.
#[must_use]
pub fn tile_grid_step_metres(level: u8, lat_deg: f64) -> Metres {
    let grid = grid_as_f64((1_u64 << level) * u64::from(TILE_EXTENT));
    Metres(EARTH_CIRCUMFERENCE_METRES * lat_deg.to_radians().cos() / grid)
}

/// Position of a point in world-pixel space at a zoom: the world is a
/// square `world_pixels()` across with the origin at its north-west
/// corner. A position, not a size — sizes stay in `Metres` or `ScreenPx`.
#[must_use]
pub fn world_position_px(point: Lonlat, zoom: Zoom) -> (f64, f64) {
    let (xn, yn) = mercator_normalised(point);
    let world = zoom.world_pixels();
    (xn * world, yn * world)
}

/// The point at a world-pixel position: the inverse of
/// [`world_position_px`], and the first step of every screen→ground walk.
#[must_use]
pub fn lonlat_at_world_px(x: f64, y: f64, zoom: Zoom) -> Lonlat {
    let world = zoom.world_pixels();
    let yn = y / world;
    Lonlat {
        lon: (x / world) * 360.0 - 180.0,
        lat: (PI * (1.0 - 2.0 * yn)).sinh().atan().to_degrees(),
    }
}

/// The only bridge between ground and screen: it cannot be crossed
/// without stating latitude and zoom.
#[must_use]
pub fn metres_to_screen_px(distance: Metres, lat_deg: f64, zoom: Zoom) -> ScreenPx {
    let ground_px = zoom.world_pixels() / (EARTH_CIRCUMFERENCE_METRES * lat_deg.to_radians().cos());
    ScreenPx(screen_distance_f32(distance.0 * ground_px))
}

fn screen_distance_f32(distance: f64) -> f32 {
    distance.to_f32().unwrap_or_else(|| {
        if distance.is_sign_negative() {
            f32::NEG_INFINITY
        } else {
            f32::INFINITY
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mercator_units(point: Lonlat, level: u8) -> (f64, f64) {
        let n = f64::from(1_u32 << level) * f64::from(TILE_EXTENT);
        let lat = point.lat.to_radians();
        let x = (point.lon + 180.0) / 360.0;
        let y = (1.0 - ((lat.tan() + 1.0 / lat.cos()).ln()) / std::f64::consts::PI) / 2.0;
        (x * n, y * n)
    }

    #[test]
    fn grid_step_at_z16_over_london_is_under_ten_centimetres() {
        let step = tile_grid_step_metres(16, 51.5);
        assert!(step.0 > 0.0);
        assert!(step.0 < 0.10, "step was {} m", step.0);
    }

    #[test]
    fn fractional_zoom_reports_exact_world_pixels() {
        let zoom = Zoom::new(14.37);
        let expected = TILE_SIZE_PX * 2_f64.powf(14.37);
        let relative = (zoom.world_pixels() - expected).abs() / expected;
        assert!(relative < 1e-12, "world_pixels drifted by {relative}");
    }

    #[test]
    fn lonlat_survives_the_grid_round_trip_at_all_latitudes() {
        for level in [0_u8, 4, 10, 16] {
            assert_grid_round_trips_at_level(level);
        }
    }

    fn assert_grid_round_trips_at_level(level: u8) {
        for lat in [0.0, 51.5, -51.5, 85.0, -85.0] {
            for lon in [-179.9, -0.3049, 13.4, 179.9] {
                assert_grid_round_trip(level, lon, lat);
            }
        }
    }

    fn assert_grid_round_trip(level: u8, lon: f64, lat: f64) {
        let point = Lonlat { lon, lat };
        let back = to_lonlat(locate(point, level));
        let (x0, y0) = mercator_units(point, level);
        let (x1, y1) = mercator_units(back, level);
        let err = (x1 - x0).abs().max((y1 - y0).abs());
        assert!(
            err <= 0.51,
            "z{level} ({lon}, {lat}): off by {err} grid units",
        );
    }

    #[test]
    fn a_world_pixel_position_round_trips_back_to_its_point() {
        let points = [
            (-179.9, 0.0),
            (-0.3049, 51.5149),
            (13.4, -33.9),
            (179.9, 84.9),
        ];
        for level in [0.0, 4.0, 12.37, 18.0] {
            for (lon, lat) in points {
                let zoom = Zoom::new(level);
                let (x, y) = world_position_px(Lonlat { lon, lat }, zoom);
                let back = lonlat_at_world_px(x, y, zoom);
                let off = (back.lon - lon).abs().max((back.lat - lat).abs());
                assert!(off < 1e-9, "z{level} ({lon}, {lat}): off by {off}°");
            }
        }
    }

    #[test]
    fn a_known_metre_count_becomes_the_expected_pixels() {
        // At the equator and z0 the whole world is 256 px across.
        let px = metres_to_screen_px(Metres(EARTH_CIRCUMFERENCE_METRES), 0.0, Zoom::new(0.0));
        assert!((f64::from(px.0) - TILE_SIZE_PX).abs() < 1e-3);
    }
}
