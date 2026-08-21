//! Which tiles to hold and draw. The zoom rule applied the second of
//! its two times: the pipeline decides what goes *into* a tile, this
//! module decides which tiles are *resident* — v1 only did the first,
//! and buildings fetched at z16 stayed on screen at z6.

use std::{collections::HashSet, hash::BuildHasher};

use maps2_camera::Camera;
use maps2_units::{world_position_px, TileId, Zoom};
use num_traits::ToPrimitive;

/// One extra ring of tiles kept around the viewport so small pans do
/// not immediately evict and refetch the same neighbours.
const KEEP_MARGIN_TILES: i64 = 1;

/// How many zoom levels before the camera actually reaches a deeper
/// source level its tiles start prefetching. Without this, a package
/// boundary with a big level gap (e.g. a world package's z5 handing off
/// to a city package's z12) draws nothing new for the whole climb and
/// then pops in — missing tiles, a grey flash, then streets — the moment
/// the camera crosses z12. Prefetched tiles land in the host's CPU-side
/// store via `load_tile` but are not resident (see [`plan_residency`]'s
/// `draw`), so they carry no visual effect of their own until the camera
/// actually reaches that level, at which point they are already there.
const PREFETCH_LOOKAHEAD_ZOOM: f64 = 4.0;

/// Below this camera zoom, buildings draw as a bare footprint (no walls or
/// roof shape). Buildings enter tiles at `Band::Micro` (z16), so this is the
/// "just arrived" end of that band.
const BUILDING_LOD_SIMPLIFIED_ZOOM: f64 = 17.0;

/// At or above this camera zoom, buildings draw full walls and a shaped
/// roof (gabled/hipped geometry, not just a flat cap).
const BUILDING_LOD_FULL_ZOOM: f64 = 19.0;

/// Building mesh detail. Every resident tile at a given frame shares one
/// target zoom level (see [`target_level`]), so there is no per-building
/// camera distance to key off within a frame; camera zoom itself is the
/// distance proxy instead, exactly as [`target_level`] already uses it to
/// pick which tile pyramid level is "close enough".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildingLod {
    /// Bare footprint, no walls or roof — cheapest, for buildings barely
    /// past their entry zoom.
    Footprint,
    /// Walls and a flat cap, no roof shape.
    Simplified,
    /// Walls and the roof's real shape (gabled ridge, hipped slopes).
    Full,
}

/// Chooses building detail for one frame's camera zoom.
#[must_use]
pub fn building_lod(zoom: f64) -> BuildingLod {
    if zoom >= BUILDING_LOD_FULL_ZOOM {
        BuildingLod::Full
    } else if zoom >= BUILDING_LOD_SIMPLIFIED_ZOOM {
        BuildingLod::Simplified
    } else {
        BuildingLod::Footprint
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResidencyPlan {
    /// Wanted on screen this frame, in draw order: the coarse stand-in
    /// underneath (see [`ResidencyPlan::fallback`]) before the target
    /// level's own tiles, so detail paints over its own backdrop.
    pub draw: Vec<TileId>,
    /// Resident but no longer welcome: wrong level or far off screen.
    pub evict: Vec<TileId>,
    /// Wanted but not yet resident: the host should load these.
    pub missing: Vec<TileId>,
    /// Not wanted yet, but worth having ready before the camera arrives —
    /// see [`PREFETCH_LOOKAHEAD_ZOOM`]. Never drawn until the camera's own
    /// zoom makes them the current [`target_level`].
    pub prefetch: Vec<TileId>,
    /// Coarser ancestors of the viewport, for levels shallower than the
    /// target. Worth fetching but not evidence of a hole: a composed
    /// pyramid is only complete where each package has coverage, so
    /// these are the tiles that stand in where the target level's
    /// package has none. Counted apart from [`ResidencyPlan::missing`]
    /// so an absent one is not reported as a gap in the map.
    pub fallback: Vec<TileId>,
}

/// A tile span in tile indices: `(x0, y0, x1, y1)`, inclusive.
type Span = (i64, i64, i64, i64);

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

/// Below the shallowest source level, `target_level` already keeps the
/// shallowest level's tiles resident (see its doc comment) — but the
/// pixel math here has to agree, or it derives tile size from a true
/// camera zoom far below what any resident tile represents. A world at
/// zoom 2 has far fewer pixels than a single z12 tile normally spans, so
/// "one tile's worth of world pixels" shrinks far below the viewport and
/// the span below enumerates the *entire* z12 grid — millions of tiles,
/// almost all "missing", every frame. Deep overzoom already stretches
/// the deepest level to cover a smaller-than-normal span (comment on
/// `target_level`); this clamp is the same idea the other way — the
/// shallowest level's own zoom floors the math, so the same handful of
/// tiles are asked to cover more than their normal span instead.
fn tile_span(camera: &Camera, level: u8, viewport: (f64, f64)) -> (i64, i64, i64, i64) {
    let zoom = Zoom::new(camera.zoom().value().max(f64::from(level)));
    let world = zoom.world_pixels();
    let tile_px = world / f64::from(1_u32 << level);
    let (cx, cy) = world_position_px(camera.centre(), zoom);
    let x0 = tile_edge(cx - viewport.0 / 2.0, tile_px);
    let x1 = tile_edge(cx + viewport.0 / 2.0, tile_px);
    let y0 = tile_edge(cy - viewport.1 / 2.0, tile_px);
    let y1 = tile_edge(cy + viewport.1 / 2.0, tile_px);
    (x0, y0, x1, y1)
}

fn tile_edge(position: f64, tile_px: f64) -> i64 {
    (position / tile_px).floor().to_i64().unwrap_or_default()
}

fn clamp_tile(value: i64, level: u8) -> Option<u32> {
    let max = i64::from((1_u32 << level) - 1);
    (0..=max).contains(&value).then(|| u32::try_from(value).unwrap_or_default())
}

/// Every valid tile id at `level` within a pixel-space span, in row-major
/// order. Shared by the current draw level and the prefetch level below.
fn tiles_in_span(level: u8, span: Span) -> Vec<TileId> {
    let (x0, y0, x1, y1) = span;
    let mut ids = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            let (Some(x), Some(y)) = (clamp_tile(x, level), clamp_tile(y, level)) else { continue };
            ids.push(TileId { z: level, x, y });
        }
    }
    ids
}

/// Whether a resident tile falls within a level's span plus keep margin.
fn within_span(id: TileId, level: u8, span: Span) -> bool {
    let (x0, y0, x1, y1) = span;
    let m = KEEP_MARGIN_TILES;
    id.z == level
        && (x0 - m..=x1 + m).contains(&i64::from(id.x))
        && (y0 - m..=y1 + m).contains(&i64::from(id.y))
}

/// Every source level shallower than `target`, deepest first, with the
/// span that covers the viewport at that level. Deepest first because
/// the closest ancestor is the best-looking stand-in.
fn coarser_spans(
    camera: &Camera, viewport: (f64, f64), source_levels: &[u8], target: u8,
) -> Vec<(u8, Span)> {
    source_levels
        .iter()
        .copied()
        .filter(|&level| level < target)
        .rev()
        .map(|level| (level, tile_span(camera, level, viewport)))
        .collect()
}

/// The finest already-resident coarse cover, or empty when nothing
/// coarser is loaded yet. One level only: a single stand-in is enough,
/// and drawing every ancestor would repaint the viewport once per level.
fn deepest_resident_cover<S: BuildHasher>(
    coarser: &[(u8, Span)], resident: &HashSet<TileId, S>,
) -> Vec<TileId> {
    coarser
        .iter()
        .find_map(|&(level, span)| {
            let drawable: Vec<TileId> =
                tiles_in_span(level, span).into_iter().filter(|id| resident.contains(id)).collect();
            (!drawable.is_empty()).then_some(drawable)
        })
        .unwrap_or_default()
}

/// Decides draw, evict and missing for one frame. Pure — the caller
/// owns the resident set and applies the plan.
#[must_use]
pub fn plan_residency<S: BuildHasher>(
    camera: &Camera,
    viewport: (f64, f64),
    source_levels: &[u8],
    resident: &HashSet<TileId, S>,
) -> ResidencyPlan {
    let level = target_level(camera.zoom().value(), source_levels);
    let span = tile_span(camera, level, viewport);
    let coarser = coarser_spans(camera, viewport, source_levels, level);
    let mut plan = ResidencyPlan::default();
    let wanted = tiles_in_span(level, span);
    plan.missing = wanted.iter().copied().filter(|id| !resident.contains(id)).collect();

    // A coarse stand-in only earns its draw call where the target level
    // has a hole. Composing a world package (shallow, global) with a
    // city one (deep, local) leaves holes everywhere the city has no
    // coverage, and before this they rendered as the background colour.
    if !plan.missing.is_empty() {
        plan.draw = deepest_resident_cover(&coarser, resident);
    }
    plan.draw.extend(wanted);
    // Every coarser level is worth fetching, not just the one drawn: the
    // deeper ones are better stand-ins once they arrive, and the host
    // skips whichever its package does not carry.
    plan.fallback = coarser
        .iter()
        .flat_map(|&(coarse, coarse_span)| tiles_in_span(coarse, coarse_span))
        .filter(|id| !resident.contains(id))
        .collect();

    // The next deeper source level's span is computed unconditionally (not
    // only once the lookahead gate opens) so a tile prefetched a moment ago
    // stays protected below even if the camera's zoom dips back under the
    // gate before it reaches `level` for real — otherwise it would be
    // fetched, evicted next frame, then re-fetched, forever.
    let next_span = source_levels
        .iter()
        .find(|&&candidate| candidate > level)
        .map(|&next| (next, tile_span(camera, next, viewport)));

    plan.evict = evictions(resident, (level, span), &coarser, next_span);

    if let Some((next, next_span)) = next_span
        && camera.zoom().value() + PREFETCH_LOOKAHEAD_ZOOM >= f64::from(next)
    {
        plan.prefetch = tiles_in_span(next, next_span).into_iter().filter(|id| !resident.contains(id)).collect();
    }

    plan
}

/// Resident tiles no level in play still wants. Coarse stand-ins are
/// kept alongside the target level: they are few (one level's span
/// shrinks to a tile or two once the camera is far below it) and
/// dropping them would refetch the same backdrop on the next frame.
fn evictions<S: BuildHasher>(
    resident: &HashSet<TileId, S>,
    target: (u8, Span),
    coarser: &[(u8, Span)],
    next: Option<(u8, Span)>,
) -> Vec<TileId> {
    let mut evict: Vec<TileId> = resident
        .iter()
        .copied()
        .filter(|&id| {
            let keep = within_span(id, target.0, target.1)
                || coarser.iter().any(|&(level, span)| within_span(id, level, span))
                || next.is_some_and(|(level, span)| within_span(id, level, span));
            !keep
        })
        .collect();
    evict.sort_by_key(|id| (id.z, id.x, id.y));
    evict
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
    fn building_lod_steps_down_as_the_camera_pulls_back() {
        assert_eq!(building_lod(20.0), BuildingLod::Full);
        assert_eq!(building_lod(19.0), BuildingLod::Full);
        assert_eq!(building_lod(18.0), BuildingLod::Simplified);
        assert_eq!(building_lod(17.0), BuildingLod::Simplified);
        assert_eq!(building_lod(16.0), BuildingLod::Footprint);
        assert_eq!(building_lod(0.0), BuildingLod::Footprint);
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
    fn zooming_below_a_packages_shallowest_level_stays_a_handful_of_tiles() {
        // A real bug, found live against the real London package (whose
        // source levels start at 12, not 0): pulling the camera back
        // past zoom 12 made `missing` enumerate the *entire* z12 grid —
        // 16,777,216 tiles — because tile size was derived from the true
        // (far shallower) camera zoom while the level stayed pinned at
        // 12. The browser tab stalled on it every frame.
        let levels = [12, 13, 14, 15, 16];
        let plan = plan_residency(&camera(2.0), (800.0, 600.0), &levels, &HashSet::new());
        assert!(plan.draw.iter().all(|id| id.z == 12));
        assert!(
            plan.draw.len() < 200,
            "expected a viewport's worth of z12 tiles, got {}",
            plan.draw.len()
        );
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

    /// The world+city pyramid the globe demo composes: a global package
    /// with only shallow levels, a city package with only deep ones.
    const COMPOSED: [u8; 11] = [1, 2, 3, 4, 5, 12, 13, 14, 15, 16, 17];

    #[test]
    fn a_deep_zoom_where_only_the_world_package_has_data_draws_the_world_tile() {
        // Reported live: zoom to street level anywhere that is not the
        // one real city and the map went grey. target_level is global,
        // so it picked z13 for Paris too — where no z13 tile exists —
        // and nothing was drawn at all. The world's z5 tile covers that
        // ground and must stand in.
        let paris = Lonlat { lon: 2.3522, lat: 48.8566 };
        let world_tile = locate(paris, 5).tile;
        let resident: HashSet<_> = [world_tile].into();
        let camera = Camera::new(paris, Zoom::new(13.0));

        let plan = plan_residency(&camera, (800.0, 600.0), &COMPOSED, &resident);

        assert_eq!(plan.draw.first(), Some(&world_tile), "the world tile draws underneath");
        assert!(!plan.evict.contains(&world_tile), "and must not be evicted to do it");
        assert!(plan.draw.iter().any(|id| id.z == 13), "the z13 wants are still asked for");
    }

    #[test]
    fn a_half_covered_viewport_still_draws_the_coarse_tile_under_the_hole() {
        // The other half of the same report: at the city package's own
        // edge, half the screen had real streets and half was void.
        let edge = Lonlat { lon: -0.50, lat: 51.50 };
        let camera = Camera::new(edge, Zoom::new(12.0));
        let mut resident: HashSet<TileId> = HashSet::new();
        resident.insert(locate(edge, 5).tile);
        // Only the eastern half of the z12 span is loaded, as if the
        // city package stopped there.
        let centre = locate(edge, 12).tile;
        resident.insert(centre);
        resident.insert(TileId { z: 12, x: centre.x + 1, y: centre.y });

        let plan = plan_residency(&camera, (800.0, 600.0), &COMPOSED, &resident);

        assert!(!plan.missing.is_empty(), "the unloaded half is still a want");
        assert_eq!(plan.draw.first().map(|id| id.z), Some(5), "coarse cover goes down first");
        assert!(plan.draw.contains(&centre), "and the real detail paints over it");
    }

    #[test]
    fn a_fully_covered_viewport_pays_nothing_for_a_coarse_stand_in() {
        let camera = Camera::new(EALING, Zoom::new(14.0));
        let resident: HashSet<TileId> =
            tiles_in_span(14, tile_span(&camera, 14, (800.0, 600.0))).into_iter().collect();

        let plan = plan_residency(&camera, (800.0, 600.0), &LEVELS, &resident);

        assert!(plan.missing.is_empty(), "nothing is missing");
        assert!(plan.draw.iter().all(|id| id.z == 14), "so no coarse tile is drawn: {:?}", plan.draw);
    }

    #[test]
    fn coarser_levels_are_offered_for_fetching_without_counting_as_holes() {
        let paris = Lonlat { lon: 2.3522, lat: 48.8566 };
        let camera = Camera::new(paris, Zoom::new(13.0));

        let plan = plan_residency(&camera, (800.0, 600.0), &COMPOSED, &HashSet::new());

        assert!(plan.fallback.iter().any(|id| id.z == 5), "the world level is worth fetching");
        assert!(plan.fallback.iter().all(|id| id.z < 13));
        assert!(plan.missing.iter().all(|id| id.z == 13), "only the target level counts as missing");
    }

    #[test]
    fn a_big_level_gap_starts_prefetching_before_the_camera_arrives() {
        // The globe-to-city demo's reported bug: world levels stop at 5,
        // city levels start at 12. Climbing that gap must not draw a grey
        // gap and then pop in the city tiles only once z12 is crossed.
        let levels = [0, 1, 2, 3, 4, 5, 12, 13, 14, 15, 16];
        let plan = plan_residency(&camera(8.0), (800.0, 600.0), &levels, &HashSet::new());
        assert!(plan.draw.iter().all(|id| id.z == 5), "still drawing the world level");
        assert!(!plan.prefetch.is_empty(), "z12 tiles should already be prefetching by zoom 8");
        assert!(plan.prefetch.iter().all(|id| id.z == 12));
    }

    #[test]
    fn a_prefetched_tile_is_not_immediately_evicted_next_frame() {
        // Loading a prefetch hint through `load_tile` makes it resident.
        // If the very next residency plan evicted it (it is not at the
        // current draw level), the host would fetch it, throw it away,
        // and fetch it again — forever, right up to the boundary.
        let levels = [0, 1, 2, 3, 4, 5, 12, 13, 14, 15, 16];
        let prefetched = locate(EALING, 12).tile;
        let resident: HashSet<_> = [prefetched].into();
        let plan = plan_residency(&camera(8.0), (800.0, 600.0), &levels, &resident);
        assert!(!plan.evict.contains(&prefetched), "a freshly prefetched tile must survive");
    }

    #[test]
    fn prefetch_stays_empty_far_below_the_next_level() {
        let levels = [0, 1, 2, 3, 4, 5, 12, 13, 14, 15, 16];
        let plan = plan_residency(&camera(2.0), (800.0, 600.0), &levels, &HashSet::new());
        assert!(plan.prefetch.iter().all(|id| id.z != 12), "z12 is far past the lookahead margin at zoom 2");
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
