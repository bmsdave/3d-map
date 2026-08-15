//! Which tiles to hold and draw. The zoom rule applied the second of
//! its two times: the pipeline decides what goes *into* a tile, this
//! module decides which tiles are *resident* — v1 only did the first,
//! and buildings fetched at z16 stayed on screen at z6.

use std::collections::HashSet;

use maps2_camera::Camera;
use maps2_units::{world_position_px, TileId};

/// One extra ring of tiles kept around the viewport so small pans do
/// not immediately evict and refetch the same neighbours.
const KEEP_MARGIN_TILES: i64 = 1;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResidencyPlan {
    /// Wanted on screen this frame, in draw order.
    pub draw: Vec<TileId>,
    /// Resident but no longer welcome: wrong level or far off screen.
    pub evict: Vec<TileId>,
    /// Wanted but not yet resident: the host should load these.
    pub missing: Vec<TileId>,
}

/// The deepest available level the camera zoom admits; below the
/// shallowest source the shallowest is used, past the deepest the
/// deepest is stretched (overzoom).
#[must_use]
pub fn target_level(zoom: f64, source_levels: &[u8]) -> u8 {
    let mut level = *source_levels.first().unwrap_or(&0);
    for &candidate in source_levels {
        if zoom >= f64::from(candidate) {
            level = level.max(candidate);
        }
    }
    level
}

/// Adds one host-supplied level while preserving the renderer's sorted,
/// duplicate-free source pyramid.
pub fn register_source_level(levels: &mut Vec<u8>, level: u8) {
    match levels.binary_search(&level) {
        Ok(_) => {}
        Err(index) => levels.insert(index, level),
    }
}

/// Normalises a complete host package pyramid. Empty packages are invalid;
/// callers retain the current levels when this returns `None`.
#[must_use]
pub fn normalise_source_levels(mut levels: Vec<u8>) -> Option<Vec<u8>> {
    levels.sort_unstable();
    levels.dedup();
    (!levels.is_empty()).then_some(levels)
}

fn tile_span(camera: &Camera, level: u8, viewport: (f64, f64)) -> (i64, i64, i64, i64) {
    let world = camera.zoom().world_pixels();
    let tile_px = world / f64::from(1_u32 << level);
    let (cx, cy) = world_position_px(camera.centre(), camera.zoom());
    let x0 = ((cx - viewport.0 / 2.0) / tile_px).floor() as i64;
    let x1 = ((cx + viewport.0 / 2.0) / tile_px).floor() as i64;
    let y0 = ((cy - viewport.1 / 2.0) / tile_px).floor() as i64;
    let y1 = ((cy + viewport.1 / 2.0) / tile_px).floor() as i64;
    (x0, y0, x1, y1)
}

fn clamp_tile(value: i64, level: u8) -> Option<u32> {
    let max = i64::from((1_u32 << level) - 1);
    (0..=max).contains(&value).then_some(value as u32)
}

/// Decides draw, evict and missing for one frame. Pure — the caller
/// owns the resident set and applies the plan.
#[must_use]
pub fn plan_residency(
    camera: &Camera,
    viewport: (f64, f64),
    source_levels: &[u8],
    resident: &HashSet<TileId>,
) -> ResidencyPlan {
    let level = target_level(camera.zoom().value(), source_levels);
    let (x0, y0, x1, y1) = tile_span(camera, level, viewport);
    let mut plan = ResidencyPlan::default();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (Some(x), Some(y)) = (clamp_tile(x, level), clamp_tile(y, level)) else {
                continue;
            };
            let id = TileId { z: level, x, y };
            plan.draw.push(id);
            if !resident.contains(&id) {
                plan.missing.push(id);
            }
        }
    }
    let m = KEEP_MARGIN_TILES;
    for id in resident {
        let keep = id.z == level
            && (x0 - m..=x1 + m).contains(&i64::from(id.x))
            && (y0 - m..=y1 + m).contains(&i64::from(id.y));
        if !keep {
            plan.evict.push(*id);
        }
    }
    plan.evict.sort_by_key(|id| (id.z, id.x, id.y));
    plan
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_camera::Camera;
    use maps2_units::{locate, Lonlat, Zoom};

    const EALING: Lonlat = Lonlat { lon: -0.3049, lat: 51.5149 };
    const LEVELS: [u8; 7] = [0, 5, 8, 10, 12, 14, 16];

    fn camera(zoom: f64) -> Camera {
        Camera::new(EALING, Zoom::new(zoom))
    }

    #[test]
    fn deep_tiles_are_evicted_when_the_camera_leaves() {
        // The v1 fault on record: zoom in until buildings, zoom out,
        // buildings stay. Here they must be evicted.
        let deep = locate(EALING, 16).tile;
        let resident: HashSet<_> = [deep].into();
        let plan = plan_residency(&camera(6.0), (800.0, 600.0), &LEVELS, &resident);
        assert!(plan.evict.contains(&deep), "z16 tile must not survive a z6 camera");
        assert!(plan.draw.iter().all(|id| id.z == 5));
    }

    #[test]
    fn overzoom_stretches_the_deepest_source_level() {
        assert_eq!(target_level(18.7, &LEVELS), 16);
        assert_eq!(target_level(0.5, &LEVELS), 0);
        assert_eq!(target_level(11.9, &LEVELS), 10);
    }

    #[test]
    fn a_host_added_level_is_sorted_without_losing_existing_levels() {
        let mut levels = vec![0, 5, 8, 10, 12, 14, 16];

        register_source_level(&mut levels, 15);

        assert_eq!(levels, [0, 5, 8, 10, 12, 14, 15, 16]);
    }

    #[test]
    fn a_real_package_replaces_the_fixture_pyramid_with_its_own_levels() {
        assert_eq!(normalise_source_levels(vec![16, 12, 16]), Some(vec![12, 16]));
        assert_eq!(normalise_source_levels(Vec::new()), None);
    }

    #[test]
    fn the_viewport_is_covered_and_the_centre_tile_is_wanted() {
        let plan = plan_residency(&camera(14.0), (800.0, 600.0), &LEVELS, &HashSet::new());
        let centre = locate(EALING, 14).tile;
        assert!(plan.draw.contains(&centre));
        // 800×600 at 256 px per tile wants at least a 4×3 cover.
        assert!(plan.draw.len() >= 12, "only {} tiles wanted", plan.draw.len());
        assert_eq!(plan.missing, plan.draw);
    }

    #[test]
    fn a_margin_ring_survives_but_the_second_ring_does_not() {
        let centre = locate(EALING, 14).tile;
        let neighbour = TileId { z: 14, x: centre.x + 2, y: centre.y };
        let far = TileId { z: 14, x: centre.x + 9, y: centre.y };
        let resident: HashSet<_> = [neighbour, far].into();
        let plan = plan_residency(&camera(14.0), (800.0, 600.0), &LEVELS, &resident);
        assert!(!plan.evict.contains(&neighbour), "margin ring must be kept");
        assert!(plan.evict.contains(&far), "far tile must go");
    }
}
