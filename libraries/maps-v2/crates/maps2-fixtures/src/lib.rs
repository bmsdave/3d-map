//! The synthetic world for the lab: procedural, deterministic, tiny.
//!
//! Shapes are defined in normalised world coordinates and cut per tile,
//! so edges align across tile boundaries for free. The zoom rule is
//! applied here for the first of its two times: a class is written into
//! a tile only if the tile's level admits its band — a z8 tile contains
//! no buildings, and no client code has to hide them.
//!
//! Feature ids are stable within a build but carry no cross-level
//! identity yet; that arrives with the real pipeline (stage 8).

mod ridge;

pub use ridge::{
    height_metres, ridge_coverage, ridge_tile_bytes, ridge_tiles, RIDGE_DETAIL_LEVEL,
};

use maps2_style::{Class, entry_band};
use maps2_tile::{FeatureDraft, TileBuilder};
use maps2_units::{Lonlat, TileCoord, TileId, Zoom, locate, world_position_px};
use num_traits::ToPrimitive;

mod roads;

pub use roads::{roads_centre, roads_tile, roads_tile_bytes, roads_tiles, ROADS_ZOOM};

/// The one fixed place of all fixtures (see lab bands.ts).
pub const EALING: Lonlat = Lonlat { lon: -0.3049, lat: 51.5149 };

/// Levels the package is cut at: the seven band entries.
pub const COVERAGE_LEVELS: [u8; 7] = [0, 5, 8, 10, 12, 14, 16];

const EXTENT_F: f64 = 65536.0;
/// Street grid spacing in world units: one street per z15 tile (~190 m
/// over London) — guarantees at least one street in any 3×3 window of
/// z16 tiles, whatever the grid phase.
const STREET_SPACING: f64 = 1.0 / 32768.0;

/// The place deliberately sat on a tile corner, so that the renderer's
/// cross-border dedup has a real duplicate to work on.
pub const BOUNDARY_ID: u32 = 60;

/// Normalised world rectangle, x/y in 0..1 (Mercator square).
#[derive(Clone, Copy, Debug)]
struct WorldRect {
    x0: f64,
    y0: f64,
    x1: f64,
    y1: f64,
}

fn centre_norm() -> (f64, f64) {
    let (x, y) = world_position_px(EALING, Zoom::new(0.0));
    (x / 256.0, y / 256.0)
}

// Both shapes sit close enough to the centre to intersect the 3×3
// coverage even at z16, where one tile is only ~1.5e-5 of the world.

fn water_ribbon() -> WorldRect {
    let (_, yc) = centre_norm();
    WorldRect { x0: 0.0, y0: yc + 5.0e-6, x1: 1.0, y1: yc + 1.2e-4 }
}

fn park_rect() -> WorldRect {
    let (xc, yc) = centre_norm();
    WorldRect { x0: xc - 1.5e-4, y0: yc - 1.6e-4, x1: xc - 1.5e-5, y1: yc - 1.0e-5 }
}

fn tile_window(id: TileId) -> WorldRect {
    let n = f64::from(1_u32 << id.z);
    WorldRect {
        x0: f64::from(id.x) / n,
        y0: f64::from(id.y) / n,
        x1: (f64::from(id.x) + 1.0) / n,
        y1: (f64::from(id.y) + 1.0) / n,
    }
}

fn to_local(value: f64, w0: f64, w1: f64) -> f64 {
    ((value - w0) / (w1 - w0)) * EXTENT_F
}

fn quantise(value: f64) -> u16 {
    value.round().clamp(0.0, 65535.0).to_u16().unwrap_or(u16::MAX)
}

/// A world rect clipped into a tile; None when nothing visible remains.
fn clip(rect: WorldRect, window: WorldRect) -> Option<(u16, u16, u16, u16)> {
    let x0 = quantise(to_local(rect.x0.max(window.x0), window.x0, window.x1));
    let x1 = quantise(to_local(rect.x1.min(window.x1), window.x0, window.x1));
    let y0 = quantise(to_local(rect.y0.max(window.y0), window.y0, window.y1));
    let y1 = quantise(to_local(rect.y1.min(window.y1), window.y0, window.y1));
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

pub(crate) fn rect_polygon(id: u32, (x0, y0, x1, y1): (u16, u16, u16, u16)) -> FeatureDraft {
    FeatureDraft::geometry(
        id,
        0,
        vec![
            TileCoord(x0, y0),
            TileCoord(x1, y0),
            TileCoord(x1, y1),
            TileCoord(x0, y1),
            TileCoord(x0, y0),
        ],
    )
}

fn admitted(class: Class, level: u8) -> bool {
    f64::from(level) >= entry_band(class).entry_zoom()
}

/// All tile ids of the package: 3×3 around the centre at each level.
#[must_use]
pub fn coverage() -> Vec<TileId> {
    let mut out = Vec::new();
    for level in COVERAGE_LEVELS {
        let centre = locate(EALING, level).tile;
        let max = (1_u32 << level) - 1;
        for id in (-1_i64..=1)
            .flat_map(|dy| (-1_i64..=1).map(move |dx| coverage_tile(level, centre, max, dx, dy)))
            .flatten()
        {
            if !out.contains(&id) { out.push(id); }
        }
    }
    out
}

fn coverage_tile(level: u8, centre: TileId, max: u32, dx: i64, dy: i64) -> Option<TileId> {
    let x = i64::from(centre.x) + dx;
    let y = i64::from(centre.y) + dy;
    (0..=i64::from(max)).contains(&x).then_some(())?;
    (0..=i64::from(max)).contains(&y).then_some(())?;
    Some(TileId { z: level, x: u32::try_from(x).ok()?, y: u32::try_from(y).ok()? })
}

/// The whole package, deterministically.
#[must_use]
pub fn ealing_tiles() -> Vec<(TileId, Vec<u8>)> {
    coverage().into_iter().map(|id| (id, tile_bytes(id))).collect()
}

/// One tile of the synthetic world.
///
/// # Panics
///
/// Panics only if this bounded synthetic fixture cannot fit MT2.
#[must_use]
pub fn tile_bytes(id: TileId) -> Vec<u8> {
    let mut builder = TileBuilder::new(id);
    let window = tile_window(id);
    push_ground(&mut builder, id.z, window);
    push_roads(&mut builder, id.z, window);
    push_buildings(&mut builder, id.z, window);
    push_places(&mut builder, window);
    push_poi(&mut builder, id);
        builder.build().expect("fixture tile fits MT2")
}

/// Named places: offsets from the fixed centre in normalised world
/// units, with the rank the selection stage competes on.
const PLACES: [(&str, u8, f64, f64); 6] = [
    ("Ealing", 0, 0.0, 0.0),
    ("Ealing Broadway", 1, 6.0e-6, -4.0e-6),
    ("West Ealing", 1, -9.0e-6, 2.0e-6),
    ("Northfields", 2, 2.0e-6, 9.0e-6),
    ("Walpole Park", 2, -8.0e-5, -8.5e-5),
    ("The Brent", 3, 0.0, 6.0e-5),
];

/// How far past its own edges a tile still carries a place, as a share
/// of the tile. A real cutter overlaps its tiles for exactly this
/// reason: a name must not vanish because its anchor fell on a seam.
const PLACE_OVERLAP: f64 = 0.04;

/// The corner of the centre z14 tile — a point four tiles share.
fn boundary_place() -> (f64, f64) {
    let tile = locate(EALING, 14).tile;
    let n = f64::from(1_u32 << 14);
    ((f64::from(tile.x) + 1.0) / n, (f64::from(tile.y) + 1.0) / n)
}

fn push_places(builder: &mut TileBuilder, window: WorldRect) {
    let (xc, yc) = centre_norm();
    let corner = boundary_place();
    let named = PLACES
        .iter()
        .enumerate()
        .map(|(i, (name, rank, dx, dy))| (50 + u32::try_from(i).unwrap_or_default(), *name, *rank, xc + dx, yc + dy))
        .chain([(BOUNDARY_ID, "Boundary Oak", 4_u8, corner.0, corner.1)]);
    for (id, name, rank, x, y) in named {
        let Some(coord) = place_in(window, x, y) else {
            continue;
        };
        builder.push(
            Class::Label.code(),
            FeatureDraft { id, flags: 0, rank, name: name.to_string(), vertices: vec![coord] },
        );
    }
}

/// The tile coordinate of a world point, or None when it is outside the
/// tile and its overlap.
fn place_in(window: WorldRect, x: f64, y: f64) -> Option<TileCoord> {
    let (w, h) = (window.x1 - window.x0, window.y1 - window.y0);
    let inside = x >= window.x0 - w * PLACE_OVERLAP
        && x <= window.x1 + w * PLACE_OVERLAP
        && y >= window.y0 - h * PLACE_OVERLAP
        && y <= window.y1 + h * PLACE_OVERLAP;
    inside.then(|| {
        TileCoord(
            quantise(to_local(x, window.x0, window.x1)),
            quantise(to_local(y, window.y0, window.y1)),
        )
    })
}

/// POI per tile edge. Eleven squared is a hundred and twenty one per
/// tile, and a thousand across the nine a viewport holds — the density
/// the budget card is there to argue with.
const POI_PER_EDGE: u32 = 11;

const POI_NAMES: [&str; 14] = [
    "Bakery",
    "Chemist",
    "Cafe",
    "School",
    "Library",
    "Post Office",
    "Fire Station",
    "Clinic",
    "Museum",
    "Bank",
    "Market",
    "Gallery",
    "Depot",
    "Chapel",
];

/// A regular POI grid, one per level, so that every tile carries the
/// same count whatever its zoom — the thinning a real pipeline does by
/// rank happens at cutting time, not here.
fn push_poi(builder: &mut TileBuilder, id: TileId) {
    if !admitted(Class::Poi, id.z) {
        return;
    }
    let step = EXTENT_F / f64::from(POI_PER_EDGE);
    for j in 0..POI_PER_EDGE {
        for i in 0..POI_PER_EDGE {
            let global = (id.x * POI_PER_EDGE + i, id.y * POI_PER_EDGE + j);
            builder.push(
                Class::Poi.code(),
                FeatureDraft {
                    id: (global.0 % 65536) << 16 | (global.1 % 65536),
                    flags: 0,
                    rank: poi_rank(global),
                    name: poi_name(global),
                    vertices: vec![TileCoord(
                        quantise((f64::from(i) + 0.5) * step),
                        quantise((f64::from(j) + 0.5) * step),
                    )],
                },
            );
        }
    }
}

/// Heavy tail: a handful of landmarks, a great many corner shops.
fn poi_rank(global: (u32, u32)) -> u8 {
    ((global.0.wrapping_mul(7).wrapping_add(global.1.wrapping_mul(13))) % 10) as u8
}

fn poi_name(global: (u32, u32)) -> String {
    let base = POI_NAMES[(global.0.wrapping_add(global.1.wrapping_mul(3)) as usize) % 14];
    if (global.0 + global.1).is_multiple_of(5) {
        format!("{base} {}", (global.0.wrapping_mul(3).wrapping_add(global.1)) % 90 + 10)
    } else {
        base.to_string()
    }
}

fn push_ground(builder: &mut TileBuilder, level: u8, window: WorldRect) {
    builder.push(Class::Land.code(), rect_polygon(1, (0, 0, 65535, 65535)));
    if let Some(span) = clip(water_ribbon(), window) {
        builder.push(Class::Water.code(), rect_polygon(2, span));
    }
    if admitted(Class::Park, level)
        && let Some(span) = clip(park_rect(), window)
    {
        builder.push(Class::Park.code(), rect_polygon(3, span));
    }
}

/// Two primary centrelines cross at the centre; residential streets sit
/// on a fixed world grid, one per z14 tile.
fn push_roads(builder: &mut TileBuilder, level: u8, window: WorldRect) {
    let (xc, yc) = centre_norm();
    if admitted(Class::RoadPrimary, level) {
        if (window.y0..window.y1).contains(&yc) {
            let y = quantise(to_local(yc, window.y0, window.y1));
            builder.push(Class::RoadPrimary.code(), line(10, TileCoord(0, y), TileCoord(65535, y)));
        }
        if (window.x0..window.x1).contains(&xc) {
            let x = quantise(to_local(xc, window.x0, window.x1));
            builder.push(Class::RoadPrimary.code(), line(11, TileCoord(x, 0), TileCoord(x, 65535)));
        }
    }
    if admitted(Class::RoadResidential, level) {
        push_street_grid(builder, window);
    }
}

fn push_street_grid(builder: &mut TileBuilder, window: WorldRect) {
    let mut id = 100;
    let mut k = (window.x0 / STREET_SPACING).ceil().to_i64().unwrap_or_default();
    while k.to_f64().unwrap_or_default() * STREET_SPACING < window.x1 {
        let x = quantise(to_local(k.to_f64().unwrap_or_default() * STREET_SPACING, window.x0, window.x1));
        builder.push(Class::RoadResidential.code(), line(id, TileCoord(x, 0), TileCoord(x, 65535)));
        id += 1;
        k += 1;
    }
    let mut k = (window.y0 / STREET_SPACING).ceil().to_i64().unwrap_or_default();
    while k.to_f64().unwrap_or_default() * STREET_SPACING < window.y1 {
        let y = quantise(to_local(k.to_f64().unwrap_or_default() * STREET_SPACING, window.y0, window.y1));
        builder.push(Class::RoadResidential.code(), line(id, TileCoord(0, y), TileCoord(65535, y)));
        id += 1;
        k += 1;
    }
}

/// 6×6 block grid per tile, skipping squares under water or park.
fn push_buildings(builder: &mut TileBuilder, level: u8, window: WorldRect) {
    if !admitted(Class::Building, level) {
        return;
    }
    let cells = 6_u32;
    let cell = EXTENT_F / f64::from(cells);
    let margin = cell * 0.21;
    let mut id = 1000;
    for j in 0..cells {
        for i in 0..cells {
            let x0 = f64::from(i) * cell + margin;
            let y0 = f64::from(j) * cell + margin;
            let x1 = f64::from(i + 1) * cell - margin;
            let y1 = f64::from(j + 1) * cell - margin;
            let world = WorldRect {
                x0: window.x0 + (x0 / EXTENT_F) * (window.x1 - window.x0),
                y0: window.y0 + (y0 / EXTENT_F) * (window.y1 - window.y0),
                x1: window.x0 + (x1 / EXTENT_F) * (window.x1 - window.x0),
                y1: window.y0 + (y1 / EXTENT_F) * (window.y1 - window.y0),
            };
            if overlaps(world, water_ribbon()) || overlaps(world, park_rect()) {
                continue;
            }
            let span = (quantise(x0), quantise(y0), quantise(x1), quantise(y1));
            builder.push_building(
                Class::Building.code(),
                rect_polygon(id, span),
                maps2_tile::BuildingDraft::flat(0, fixture_building_height_dm(i, j)),
            );
            id += 1;
        }
    }
}

fn fixture_building_height_dm(x: u32, y: u32) -> u16 {
    90 + u16::try_from((x * 17 + y * 11) % 8).unwrap_or_default() * 30
}

fn overlaps(a: WorldRect, b: WorldRect) -> bool {
    a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1
}

fn line(id: u32, from: TileCoord, to: TileCoord) -> FeatureDraft {
    FeatureDraft::geometry(id, 0, vec![from, to])
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::TileView;

    /// The package must be bit-for-bit stable: any format or content
    /// change shows up as a hash change and is made knowingly.
    ///
    /// Changed when the micro fixture gained deterministic building heights.
    const GOLDEN_FNV1A: u64 = 0xB836_99B5_93C0_B551;

    fn fnv1a(bytes: &[u8], mut hash: u64) -> u64 {
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        hash
    }

    #[test]
    fn the_package_bytes_are_golden() {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for (id, bytes) in ealing_tiles() {
            hash = fnv1a(&[id.z], hash);
            hash = fnv1a(&id.x.to_le_bytes(), hash);
            hash = fnv1a(&id.y.to_le_bytes(), hash);
            hash = fnv1a(&bytes, hash);
        }
        assert_eq!(hash, GOLDEN_FNV1A, "package changed: new hash {hash:#x}");
    }

    #[test]
    fn a_tile_contains_no_class_below_its_band() {
        for (id, bytes) in ealing_tiles() {
            let tile = TileView::parse(&bytes).expect("fixture parses");
            let forbidden = [
                (Class::Building, 16_u8),
                (Class::RoadResidential, 12),
                (Class::RoadPrimary, 8),
                (Class::Park, 8),
            ];
            for (class, _) in forbidden.into_iter().filter(|(_, min_level)| id.z < *min_level) {
                assert_eq!(
                    tile.section(class.code()),
                    None,
                    "z{} tile must not carry {class:?}",
                    id.z,
                );
            }
        }
    }

    #[test]
    fn micro_buildings_carry_a_nonzero_top_height() {
        let centre = locate(EALING, 16).tile;
        let (_, bytes) = ealing_tiles()
            .into_iter()
            .find(|(id, _)| *id == centre)
            .expect("centre tile");
        let tile = TileView::parse(&bytes).expect("fixture parses");
        let building = tile
            .section(Class::Building.code())
            .expect("building section")
            .features()
            .next()
            .expect("building")
            .expect("feature")
            .building;

        assert!(building.is_some_and(|data| data.top_height_dm > 0));
    }

    #[test]
    fn admitted_classes_do_appear_at_their_levels() {
        let by_level = |level: u8| {
            ealing_tiles()
                .into_iter()
                .filter(move |(id, _)| id.z == level)
                .map(|(_, bytes)| bytes)
                .collect::<Vec<_>>()
        };
        let has = |bytes: &[Vec<u8>], class: Class| {
            bytes
                .iter()
                .any(|b| TileView::parse(b).expect("parses").section(class.code()).is_some())
        };
        for level in COVERAGE_LEVELS {
            let tiles = by_level(level);
            assert!(has(&tiles, Class::Land), "z{level}: no land");
            assert!(has(&tiles, Class::Water), "z{level}: no water");
            if level >= 8 {
                assert!(has(&tiles, Class::Park), "z{level}: no park");
                assert!(has(&tiles, Class::RoadPrimary), "z{level}: no primary");
            }
            if level >= 12 {
                assert!(has(&tiles, Class::RoadResidential), "z{level}: no streets");
            }
            if level >= 16 {
                assert!(has(&tiles, Class::Building), "z{level}: no buildings");
            }
        }
    }

    fn labels_of(bytes: &[u8], class: Class) -> Vec<(u32, u8, String)> {
        let tile = TileView::parse(bytes).expect("parses");
        let Some(section) = tile.section(class.code()) else {
            return Vec::new();
        };
        section
            .features()
            .map(|f| {
                let f = f.expect("decodes");
                (f.id, f.rank, f.name.to_string())
            })
            .collect()
    }

    #[test]
    fn every_level_carries_named_places() {
        for level in COVERAGE_LEVELS {
            let found: Vec<_> = ealing_tiles()
                .iter()
                .filter(|(id, _)| id.z == level)
                .flat_map(|(_, bytes)| labels_of(bytes, Class::Label))
                .collect();
            assert!(!found.is_empty(), "z{level}: no places");
            assert!(found.iter().all(|(_, _, name)| !name.is_empty()), "z{level}: a nameless place");
        }
    }

    #[test]
    fn poi_arrive_at_their_band_and_crowd_the_deepest_level() {
        for (id, bytes) in ealing_tiles() {
            let poi = labels_of(&bytes, Class::Poi);
            if id.z < 14 {
                assert!(poi.is_empty(), "z{} carries POI below their band", id.z);
            } else {
                assert!(poi.len() > 100, "z{} tile carries only {} POI", id.z, poi.len());
            }
        }
        // The density card needs a thousand candidates in one 3×3 window.
        let micro: usize = ealing_tiles()
            .iter()
            .filter(|(id, _)| id.z == 16)
            .map(|(_, bytes)| labels_of(bytes, Class::Poi).len())
            .sum();
        assert!(micro >= 1000, "only {micro} POI at z16");
    }

    /// Dedup across tile borders has nothing to dedup unless the package
    /// actually writes a border feature into both tiles.
    #[test]
    fn a_place_on_a_tile_corner_is_written_into_every_tile_that_meets_it() {
        let mut homes: Vec<TileId> = Vec::new();
        for (id, bytes) in ealing_tiles() {
            if id.z != 14 {
                continue;
            }
            if labels_of(&bytes, Class::Label).iter().any(|(fid, _, _)| *fid == BOUNDARY_ID) {
                homes.push(id);
            }
        }
        assert!(homes.len() >= 2, "the corner place lives in {} tiles", homes.len());
    }

    #[test]
    fn poi_ranks_are_a_head_and_a_long_tail() {
        let centre = locate(EALING, 16).tile;
        let ranks: Vec<u8> =
            labels_of(&tile_bytes(centre), Class::Poi).iter().map(|(_, r, _)| *r).collect();
        let head = ranks.iter().filter(|r| **r <= 1).count();
        assert!(head > 0, "no important POI at all");
        assert!(head * 3 < ranks.len(), "{head} of {} POI are head", ranks.len());
        let distinct: std::collections::HashSet<u8> = ranks.iter().copied().collect();
        assert!(distinct.len() >= 8, "ranks collapse to {} values", distinct.len());
    }

    #[test]
    fn water_edges_align_across_a_tile_boundary() {
        let centre = locate(EALING, 14).tile;
        let ribbon_rows = |x: u32| {
            let bytes = tile_bytes(TileId { z: 14, x, y: centre.y + 1 });
            let tile = TileView::parse(&bytes).expect("parses");
            let section = tile.section(Class::Water.code()).expect("water in ribbon row");
            let feature = section.features().next().expect("one feature").expect("decodes");
            feature
                .vertices()
                .collect::<Result<Vec<_>, _>>()
                .expect("vertices")
                .iter()
                .map(|v| v.1)
                .collect::<Vec<_>>()
        };
        assert_eq!(ribbon_rows(centre.x), ribbon_rows(centre.x + 1));
    }

    #[test]
    fn land_always_covers_the_full_tile() {
        for (_, bytes) in ealing_tiles() {
            let tile = TileView::parse(&bytes).expect("parses");
            let land = tile.section(Class::Land.code()).expect("land");
            let feature = land.features().next().expect("one").expect("decodes");
            let xs: Vec<_> = feature.vertices().collect::<Result<Vec<_>, _>>().expect("verts");
            assert_eq!(xs[0], TileCoord(0, 0));
            assert_eq!(xs[2], TileCoord(65535, 65535));
        }
    }
}
