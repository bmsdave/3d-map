//! Tile → screen placement. Pure math, tested natively; the GL side
//! only feeds these numbers into uniforms.
//!
//! Flat Mercator only for now: the globe blend joins when the renderer
//! reaches stage 3's globe card. The camera crate already holds the
//! blended projection this will switch to.

use maps2_camera::Camera;
use maps2_units::{world_position_px, TileId, TILE_EXTENT};

/// Screen placement of one tile: `screen = translate + grid · scale`,
/// in logical pixels with the origin at the viewport's top-left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TilePlacement {
    pub translate_x: f64,
    pub translate_y: f64,
    /// Pixels per one tile grid unit.
    pub scale: f64,
}

#[must_use]
pub fn place_tile(id: TileId, camera: &Camera, viewport_w: f64, viewport_h: f64) -> TilePlacement {
    let world = camera.zoom().world_pixels();
    let tiles = f64::from(1_u32 << id.z);
    let scale = world / (tiles * f64::from(TILE_EXTENT));
    let tile_px = world / tiles;
    let (centre_x, centre_y) = world_position_px(camera.centre(), camera.zoom());
    TilePlacement {
        translate_x: f64::from(id.x) * tile_px - centre_x + viewport_w / 2.0,
        translate_y: f64::from(id.y) * tile_px - centre_y + viewport_h / 2.0,
        scale,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_units::{locate, Lonlat, Zoom};

    const EALING: Lonlat = Lonlat { lon: -0.3049, lat: 51.5149 };

    #[test]
    fn the_camera_centre_lands_in_the_middle_of_the_viewport() {
        for zoom in [8.0, 14.37, 16.0] {
            let camera = Camera::new(EALING, Zoom::new(zoom));
            let level = zoom.floor() as u8;
            let point = locate(EALING, level);
            let placement = place_tile(point.tile, &camera, 800.0, 600.0);
            let x = placement.translate_x + f64::from(point.coord.0) * placement.scale;
            let y = placement.translate_y + f64::from(point.coord.1) * placement.scale;
            // Within half a grid step of the exact middle.
            assert!((x - 400.0).abs() < placement.scale, "z{zoom}: x = {x}");
            assert!((y - 300.0).abs() < placement.scale, "z{zoom}: y = {y}");
        }
    }

    #[test]
    fn a_tile_spans_exactly_its_share_of_the_world() {
        let camera = Camera::new(EALING, Zoom::new(14.0));
        let id = locate(EALING, 14).tile;
        let placement = place_tile(id, &camera, 800.0, 600.0);
        // At integer zoom equal to the tile level, one tile is 256 px.
        let span = placement.scale * f64::from(TILE_EXTENT);
        assert!((span - 256.0).abs() < 1e-9, "span = {span}");
        // The next tile east starts exactly where this one ends.
        let east = place_tile(TileId { z: id.z, x: id.x + 1, y: id.y }, &camera, 800.0, 600.0);
        assert!((east.translate_x - (placement.translate_x + span)).abs() < 1e-9);
    }

    #[test]
    fn fractional_zoom_scales_the_tile_continuously() {
        let id = locate(EALING, 14).tile;
        let at = |zoom: f64| {
            let camera = Camera::new(EALING, Zoom::new(zoom));
            place_tile(id, &camera, 800.0, 600.0).scale * f64::from(TILE_EXTENT)
        };
        let mid = at(14.5);
        assert!(mid > at(14.0) && mid < at(15.0));
        assert!((at(15.0) / at(14.0) - 2.0).abs() < 1e-9);
    }
}
