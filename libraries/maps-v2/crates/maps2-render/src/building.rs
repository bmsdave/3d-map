//! Roof and wall mesh generation for MT2 building footprints.
//!
//! Detail scales with [`BuildingLod`]: `Footprint` skips walls entirely
//! (a flat fill at base height), `Simplified` extrudes walls under a flat
//! cap, and `Full` additionally shapes gabled/hipped roofs. The roof shaper
//! is an approximation, not a straight-skeleton solver: it works from the
//! footprint's axis-aligned bounding box rather than its true medial axis,
//! which is exact for rectangular footprints and a reasonable ridge-line
//! guess for anything else.

use maps2_style::Class;
use maps2_tile::{MaterialClass, RoofType, TileError, TileView};
use maps2_units::TileCoord;
use num_traits::ToPrimitive;

use crate::{residency::BuildingLod, triangulate, Point};

/// One terrain-relative vertex in a building mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingVertex {
    pub x: u16,
    pub y: u16,
    pub height_dm: u16,
}

/// A contiguous index range drawn with one facade material's colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialRange {
    pub material: MaterialClass,
    pub first_index: u32,
    pub index_count: u32,
}

/// The roofs and walls for one tile, uploaded once while it is resident.
/// Indices are grouped by `ranges` so a host can draw each material with
/// its own colour in one pass per range, without a per-vertex colour
/// attribute.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildingBucket {
    pub vertices: Vec<BuildingVertex>,
    pub indices: Vec<u32>,
    pub ranges: Vec<MaterialRange>,
}

/// Builds terrain-relative roofs and walls from MT2 v2+ building features
/// at the given level of detail.
///
/// # Errors
///
/// Returns [`TileError`] for malformed tile geometry.
pub fn build_building_bucket(tile: &TileView, lod: BuildingLod) -> Result<BuildingBucket, TileError> {
    let Some(section) = tile.section(Class::Building.code()) else {
        return Ok(BuildingBucket::default());
    };
    // Buildings are sorted by material so each material's triangles land in
    // one contiguous index range; a tile mixing many materials still draws
    // in as many passes as distinct materials it actually uses, not one per
    // building.
    let mut buildings = Vec::new();
    for feature in section.features() {
        let feature = feature?;
        if let Some(building) = feature.building {
            let ring: Vec<TileCoord> = feature.vertices().collect::<Result<Vec<_>, _>>()?;
            buildings.push((building, ring));
        }
    }
    buildings.sort_by_key(|(building, _)| building.material as u8);

    let mut bucket = BuildingBucket::default();
    for (material, group) in group_by_material(&buildings) {
        let start = index_len(&bucket);
        for (building, ring) in group {
            append_building(&mut bucket, ring, *building, lod);
        }
        let end = index_len(&bucket);
        if end > start {
            bucket.ranges.push(MaterialRange { material, first_index: start, index_count: end - start });
        }
    }
    Ok(bucket)
}

type PreparedBuilding = (maps2_tile::BuildingView, Vec<TileCoord>);

fn group_by_material(buildings: &[PreparedBuilding]) -> Vec<(MaterialClass, &[PreparedBuilding])> {
    let mut groups = Vec::new();
    let mut start = 0;
    while start < buildings.len() {
        let material = buildings[start].0.material;
        let end = buildings[start..].iter().position(|(building, _)| building.material != material).map_or(buildings.len(), |offset| start + offset);
        groups.push((material, &buildings[start..end]));
        start = end;
    }
    groups
}

fn index_len(bucket: &BuildingBucket) -> u32 {
    u32::try_from(bucket.indices.len()).unwrap_or(u32::MAX)
}

fn append_building(bucket: &mut BuildingBucket, ring: &[TileCoord], building: maps2_tile::BuildingView, lod: BuildingLod) {
    let mut ring = ring.to_vec();
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 {
        return;
    }
    match lod {
        BuildingLod::Footprint => append_flat(bucket, &ring, building.base_height_dm),
        BuildingLod::Simplified => {
            append_flat(bucket, &ring, building.top_height_dm);
            append_walls(bucket, &ring, building.base_height_dm, building.top_height_dm);
        }
        BuildingLod::Full => {
            append_roof(bucket, &ring, building.top_height_dm, building.roof);
            append_walls(bucket, &ring, building.base_height_dm, building.top_height_dm);
        }
    }
}

/// A flat fill at one height — the roof cap at `Simplified`/`Full` when the
/// roof has no shape, and the whole mesh at `Footprint`.
fn append_flat(bucket: &mut BuildingBucket, ring: &[TileCoord], height_dm: u16) {
    let base = index_base(bucket);
    bucket.vertices.extend(ring.iter().map(|point| BuildingVertex { x: point.0, y: point.1, height_dm }));
    let points = ring.iter().map(|point| (f64::from(point.0), f64::from(point.1))).collect::<Vec<Point>>();
    for triangle in triangulate(&points) {
        bucket.indices.extend(triangle.iter().map(|index| base + index));
    }
}

/// Extra rise from eave to ridge: a third of the wall height, clamped to a
/// visually sane 1–4 m so a very tall or very short building does not
/// produce a spike or a barely-visible bump.
fn ridge_rise_dm(base_dm: u16, top_dm: u16) -> u16 {
    let wall = top_dm.saturating_sub(base_dm);
    (wall / 3).clamp(10, 40)
}

fn append_roof(bucket: &mut BuildingBucket, ring: &[TileCoord], top_dm: u16, roof: RoofType) {
    match roof {
        RoofType::Gabled | RoofType::Hipped => append_shaped_roof(bucket, ring, top_dm, roof),
        RoofType::Flat | RoofType::Other => append_flat(bucket, ring, top_dm),
    }
}

/// A ridge-line roof approximated from the footprint's bounding box: the
/// ridge runs along the box's longer axis. Gabled uses the full length
/// (vertical gable ends); hipped insets the ridge so the ends slope too.
fn append_shaped_roof(bucket: &mut BuildingBucket, ring: &[TileCoord], top_dm: u16, roof: RoofType) {
    let Some((min_x, max_x, min_y, max_y)) = bounds(ring) else {
        return append_flat(bucket, ring, top_dm);
    };
    let rise = ridge_rise_dm(0, top_dm).min(top_dm);
    let ridge_dm = top_dm.saturating_add(rise);
    let along_x = (max_x - min_x) >= (max_y - min_y);
    let inset = if roof == RoofType::Hipped { 0.25 } else { 0.0 };
    let (ridge_a, ridge_b) = ridge_endpoints(min_x, max_x, min_y, max_y, along_x, inset);

    let base = index_base(bucket);
    bucket.vertices.extend(ring.iter().map(|point| BuildingVertex { x: point.0, y: point.1, height_dm: top_dm }));
    let ridge_start = base + u32::try_from(ring.len()).unwrap_or(u32::MAX);
    let ridge_end = ridge_start + 1;
    bucket.vertices.push(BuildingVertex { x: ridge_a.0, y: ridge_a.1, height_dm: ridge_dm });
    bucket.vertices.push(BuildingVertex { x: ridge_b.0, y: ridge_b.1, height_dm: ridge_dm });

    for i in 0..ring.len() {
        let point = (f64::from(ring[i].0), f64::from(ring[i].1));
        let ridge_index = if nearer_to_a(point, ridge_a, ridge_b) { ridge_start } else { ridge_end };
        let next = base + u32::try_from((i + 1) % ring.len()).unwrap_or(u32::MAX);
        bucket.indices.extend_from_slice(&[base + u32::try_from(i).unwrap_or(u32::MAX), next, ridge_index]);
    }
}

fn bounds(ring: &[TileCoord]) -> Option<(f64, f64, f64, f64)> {
    let xs = ring.iter().map(|p| f64::from(p.0));
    let ys = ring.iter().map(|p| f64::from(p.1));
    let min_x = xs.clone().fold(f64::INFINITY, f64::min);
    let max_x = xs.fold(f64::NEG_INFINITY, f64::max);
    let min_y = ys.clone().fold(f64::INFINITY, f64::min);
    let max_y = ys.fold(f64::NEG_INFINITY, f64::max);
    (min_x.is_finite() && max_x.is_finite() && min_y.is_finite() && max_y.is_finite()).then_some((min_x, max_x, min_y, max_y))
}

fn ridge_endpoints(min_x: f64, max_x: f64, min_y: f64, max_y: f64, along_x: bool, inset: f64) -> (TileCoord, TileCoord) {
    let centre_x = f64::midpoint(min_x, max_x);
    let centre_y = f64::midpoint(min_y, max_y);
    if along_x {
        let span = (max_x - min_x) * inset;
        (coord(min_x + span, centre_y), coord(max_x - span, centre_y))
    } else {
        let span = (max_y - min_y) * inset;
        (coord(centre_x, min_y + span), coord(centre_x, max_y - span))
    }
}

fn coord(x: f64, y: f64) -> TileCoord {
    let clamp = |v: f64| v.clamp(0.0, f64::from(u16::MAX)).round().to_u16().unwrap_or(u16::MAX);
    TileCoord(clamp(x), clamp(y))
}

fn nearer_to_a(point: Point, a: TileCoord, b: TileCoord) -> bool {
    let distance = |p: Point, c: TileCoord| {
        let dx = p.0 - f64::from(c.0);
        let dy = p.1 - f64::from(c.1);
        dx * dx + dy * dy
    };
    distance(point, a) <= distance(point, b)
}

fn append_walls(bucket: &mut BuildingBucket, ring: &[TileCoord], base_dm: u16, top_dm: u16) {
    for edge in ring.windows(2).chain(std::iter::once(&[ring[ring.len() - 1], ring[0]][..])) {
        append_wall(bucket, edge[0], edge[1], base_dm, top_dm);
    }
}

fn append_wall(bucket: &mut BuildingBucket, first: TileCoord, second: TileCoord, base_dm: u16, top_dm: u16) {
    let base = index_base(bucket);
    for (point, height_dm) in [(first, base_dm), (second, base_dm), (second, top_dm), (first, top_dm)] {
        bucket.vertices.push(BuildingVertex { x: point.0, y: point.1, height_dm });
    }
    bucket.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn index_base(bucket: &BuildingBucket) -> u32 {
    u32::try_from(bucket.vertices.len()).unwrap_or(u32::MAX)
}
