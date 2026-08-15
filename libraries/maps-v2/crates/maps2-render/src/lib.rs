//! Building GPU-ready buckets from tiles, and the order they draw in.
//!
//! Everything here is plain math, tested natively; the WebGL2 binding
//! lives in `maps2-web`, where it can actually execute. A bucket is
//! built once per resident tile and lives between frames — a new frame
//! is never a reason to rebuild (VISION: zero copies per frame).

use maps2_style::Class;
use maps2_tile::{TileError, TileView};

mod line;
mod building;
mod labels;
mod globe;
mod residency;
mod terrain;
mod triangulate;

pub use line::{
    build_line_bucket, miter_length, road_passes, Cap, JoinCounts, LineBucket, LineOptions,
    LineRange, LineVertex, Pass, RoadLevel, RoadPass, LINESOFAR_STEP, MITER_LIMIT_MAX,
    NORMAL_SCALE, POS_BIAS, ROAD_LEVELS, ROAD_ORDER, ROUND_CAP_SEGMENTS,
};
pub use building::{BuildingBucket, BuildingVertex, build_building_bucket};
pub use labels::{build_label_bucket, LabelBucket, LabelPoint, LABEL_CLASSES};
pub use globe::{project_normalised, tile_frame, Projected, TileFrame, View};
pub use residency::{normalise_source_levels, plan_residency, register_source_level, target_level, ResidencyPlan};
pub use terrain::{
    gradient_at, HIGHLIGHT_GAIN, ground_mesh, relative_shade, relief_radius_scale, shading_z_factor, texel_metres,
    GroundMesh, GROUND_MESH_CELLS,
};
pub use triangulate::{ring_area, triangles_area, triangulate, Point};

/// Fill draw order, bottom to top. Roads are not fills — they get
/// their own bucket and passes at stage 4.
pub const FILL_ORDER: [Class; 4] = [Class::Land, Class::Water, Class::Park, Class::Building];

/// One vertex of the fill mesh, in tile grid coordinates. Colour is a
/// per-range style lookup at draw time, not vertex data — repainting
/// must not touch the mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillVertex {
    pub x: u16,
    pub y: u16,
}

/// A contiguous index range drawn with one class's fill colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClassRange {
    pub class: Class,
    pub first_index: u32,
    pub index_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FillBucket {
    pub vertices: Vec<FillVertex>,
    pub indices: Vec<u32>,
    pub ranges: Vec<ClassRange>,
}

/// Builds the fill mesh of one tile in [`FILL_ORDER`]. Classes absent
/// from the tile are absent from the ranges.
///
/// # Errors
///
/// Returns [`TileError`] when a feature cannot be decoded.
pub fn build_fill_bucket(tile: &TileView) -> Result<FillBucket, TileError> {
    let mut bucket = FillBucket::default();
    for class in FILL_ORDER {
        let Some(section) = tile.section(class.code()) else {
            continue;
        };
        let first_index = u32::try_from(bucket.indices.len()).map_err(|_| TileError::TooLarge)?;
        for feature in section.features() {
            let feature = feature?;
            append_polygon(&mut bucket, &feature)?;
        }
        let index_count = u32::try_from(bucket.indices.len()).map_err(|_| TileError::TooLarge)? - first_index;
        if index_count > 0 {
            bucket.ranges.push(ClassRange { class, first_index, index_count });
        }
    }
    Ok(bucket)
}

fn append_polygon(
    bucket: &mut FillBucket,
    feature: &maps2_tile::FeatureView<'_>,
) -> Result<(), TileError> {
    let mut ring: Vec<Point> = Vec::new();
    let mut coords = Vec::new();
    for vertex in feature.vertices() {
        let v = vertex?;
        ring.push((f64::from(v.0), f64::from(v.1)));
        coords.push(v);
    }
    // The wire format closes rings with a duplicate vertex; the
    // triangulator wants them open.
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
        coords.pop();
    }
    if ring.len() < 3 {
        return Ok(());
    }
    let base = u32::try_from(bucket.vertices.len()).map_err(|_| TileError::TooLarge)?;
    bucket
        .vertices
        .extend(coords.iter().map(|v| FillVertex { x: v.0, y: v.1 }));
    for triangle in triangulate(&ring) {
        bucket.indices.extend(triangle.iter().map(|i| base + i));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_fixtures::{ealing_tiles, EALING};
    use maps2_units::locate;

    #[test]
    fn triangulation_conserves_area_for_convex_and_concave_rings() {
        let rect = vec![(0.0, 0.0), (10.0, 0.0), (10.0, 4.0), (0.0, 4.0)];
        let l_shape = vec![
            (0.0, 0.0),
            (6.0, 0.0),
            (6.0, 2.0),
            (2.0, 2.0),
            (2.0, 6.0),
            (0.0, 6.0),
        ];
        let clockwise: Vec<_> = rect.iter().rev().copied().collect();
        for ring in [rect, l_shape, clockwise] {
            let triangles = triangulate(&ring);
            assert_eq!(triangles.len(), ring.len() - 2);
            let diff = (triangles_area(&ring, &triangles) - ring_area(&ring)).abs();
            assert!(diff < 1e-9, "area drifted by {diff}");
        }
    }

    #[test]
    fn the_centre_micro_tile_builds_a_bucket_in_fill_order() {
        let centre = locate(EALING, 16).tile;
        let (_, bytes) = ealing_tiles()
            .into_iter()
            .find(|(id, _)| *id == centre)
            .expect("centre tile in package");
        let tile = maps2_tile::TileView::parse(&bytes).expect("parses");
        let bucket = build_fill_bucket(&tile).expect("builds");

        let classes: Vec<_> = bucket.ranges.iter().map(|r| r.class).collect();
        let order_of = |class| FILL_ORDER.iter().position(|c| *c == class).expect("in order");
        assert!(classes.windows(2).all(|w| order_of(w[0]) < order_of(w[1])));
        assert!(classes.contains(&Class::Land));
        assert!(classes.contains(&Class::Building));

        assert_eq!(bucket.indices.len() % 3, 0);
        let max = bucket.indices.iter().max().expect("indices");
        assert!((*max as usize) < bucket.vertices.len());
        let ranges_total: u32 = bucket.ranges.iter().map(|r| r.index_count).sum();
        assert_eq!(ranges_total as usize, bucket.indices.len());
    }

    #[test]
    fn land_range_area_is_the_full_tile() {
        let centre = locate(EALING, 10).tile;
        let (_, bytes) = ealing_tiles()
            .into_iter()
            .find(|(id, _)| *id == centre)
            .expect("centre tile");
        let tile = maps2_tile::TileView::parse(&bytes).expect("parses");
        let bucket = build_fill_bucket(&tile).expect("builds");
        let land = bucket
            .ranges
            .iter()
            .find(|r| r.class == Class::Land)
            .expect("land range");
        let area: f64 = bucket.indices[land.first_index as usize..]
            [..land.index_count as usize]
            .chunks(3)
            .map(|t| {
                let p = |i: u32| {
                    let v = bucket.vertices[i as usize];
                    (f64::from(v.x), f64::from(v.y))
                };
                let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
                ((b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)).abs() / 2.0
            })
            .sum();
        let full = 65535.0_f64 * 65535.0;
        assert!((area - full).abs() / full < 1e-9, "land area {area} vs {full}");
    }

    #[test]
    fn a_building_bucket_has_a_roof_and_closed_walls() {
        let mut builder = maps2_tile::TileBuilder::new(maps2_units::TileId { z: 16, x: 0, y: 0 });
        builder.push_building(
            Class::Building.code(),
            maps2_tile::FeatureDraft::geometry(
                17,
                0,
                vec![
                    maps2_units::TileCoord(10, 10),
                    maps2_units::TileCoord(30, 10),
                    maps2_units::TileCoord(20, 30),
                    maps2_units::TileCoord(10, 10),
                ],
            ),
            maps2_tile::BuildingDraft::flat(0, 120),
        );
        let bytes = builder.build().expect("building tile");
        let tile = TileView::parse(&bytes).expect("tile parses");

        let bucket = build_building_bucket(&tile).expect("building bucket");

        assert_eq!(bucket.indices.len(), 21);
        assert!(bucket.vertices.iter().any(|vertex| vertex.height_dm == 0));
        assert!(bucket.vertices.iter().any(|vertex| vertex.height_dm == 120));
    }
}
