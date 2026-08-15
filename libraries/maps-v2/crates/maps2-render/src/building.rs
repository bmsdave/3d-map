//! Roof and wall mesh generation for MT2 building footprints.

use maps2_style::Class;
use maps2_tile::{TileError, TileView};

use crate::{Point, triangulate};

/// One terrain-relative vertex in a building mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingVertex {
    pub x: u16,
    pub y: u16,
    pub height_dm: u16,
}

/// The roofs and walls for one tile, uploaded once while it is resident.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildingBucket {
    pub vertices: Vec<BuildingVertex>,
    pub indices: Vec<u32>,
}

/// Builds terrain-relative roofs and walls from MT2 v2 through v4 building features.
///
/// # Errors
///
/// Returns [`TileError`] for malformed tile geometry.
pub fn build_building_bucket(tile: &TileView) -> Result<BuildingBucket, TileError> {
    let Some(section) = tile.section(Class::Building.code()) else {
        return Ok(BuildingBucket::default());
    };
    let mut bucket = BuildingBucket::default();
    for feature in section.features() {
        let feature = feature?;
        if let Some(building) = feature.building {
            append_building(&mut bucket, feature.vertices().collect::<Result<Vec<_>, _>>()?, building);
        }
    }
    Ok(bucket)
}

fn append_building(
    bucket: &mut BuildingBucket,
    mut ring: Vec<maps2_units::TileCoord>,
    building: maps2_tile::BuildingView,
) {
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    if ring.len() < 3 {
        return;
    }
    append_roof(bucket, &ring, building.top_height_dm);
    append_walls(bucket, &ring, building.base_height_dm, building.top_height_dm);
}

fn append_roof(bucket: &mut BuildingBucket, ring: &[maps2_units::TileCoord], height_dm: u16) {
    let base = index_base(bucket);
    bucket.vertices.extend(ring.iter().map(|point| BuildingVertex {
        x: point.0,
        y: point.1,
        height_dm,
    }));
    let points = ring.iter().map(|point| (f64::from(point.0), f64::from(point.1))).collect::<Vec<Point>>();
    for triangle in triangulate(&points) {
        bucket.indices.extend(triangle.iter().map(|index| base + index));
    }
}

fn append_walls(bucket: &mut BuildingBucket, ring: &[maps2_units::TileCoord], base_dm: u16, top_dm: u16) {
    for edge in ring.windows(2).chain(std::iter::once(&[ring[ring.len() - 1], ring[0]][..])) {
        append_wall(bucket, edge[0], edge[1], base_dm, top_dm);
    }
}

fn append_wall(
    bucket: &mut BuildingBucket,
    first: maps2_units::TileCoord,
    second: maps2_units::TileCoord,
    base_dm: u16,
    top_dm: u16,
) {
    let base = index_base(bucket);
    for (point, height_dm) in [(first, base_dm), (second, base_dm), (second, top_dm), (first, top_dm)] {
        bucket.vertices.push(BuildingVertex { x: point.0, y: point.1, height_dm });
    }
    bucket.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

fn index_base(bucket: &BuildingBucket) -> u32 {
    u32::try_from(bucket.vertices.len()).unwrap_or(u32::MAX)
}
