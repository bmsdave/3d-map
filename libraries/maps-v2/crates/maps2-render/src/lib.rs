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
mod height_window;
mod residency;
mod terrain;
mod triangulate;

pub use line::{
    build_line_bucket, miter_length, road_passes, Cap, JoinCounts, LineBucket, LineOptions,
    LineRange, LineVertex, Pass, RoadLevel, RoadPass, LINESOFAR_STEP, MITER_LIMIT_MAX,
    NORMAL_SCALE, POS_BIAS, ROAD_LEVELS, ROAD_ORDER, ROUND_CAP_SEGMENTS,
};
pub use building::{BuildingBucket, BuildingVertex, MaterialRange, build_building_bucket};
pub use labels::{build_label_bucket, LabelBucket, LabelPoint, LABEL_CLASSES};
pub use globe::{project_normalised, tile_frame, Projected, TileFrame, View};
pub use height_window::{height_source, sample_bilinear, HeightWindow, MAX_ANCESTOR_DEPTH};
pub use residency::{
    building_lod, normalise_source_levels, plan_residency, register_source_level, target_level,
    BuildingLod, ResidencyPlan,
};
pub use terrain::{
    gradient_at, HIGHLIGHT_GAIN, ground_mesh, relative_shade, relief_radius_scale, shading_z_factor, texel_metres,
    GroundMesh, GROUND_MESH_CELLS,
};
pub use triangulate::{ring_area, triangles_area, triangulate, triangulate_with_holes, Point};

/// Fill draw order, bottom to top. Roads are not fills — they get
/// their own bucket and passes at stage 4.
///
/// Buildings are not fills either, and used to be listed here anyway:
/// every footprint was triangulated into this bucket, uploaded to the
/// GPU, and then skipped at draw time because [`build_building_bucket`]
/// had already drawn it in three dimensions. A building is described in
/// one place now.
pub const FILL_ORDER: [Class; 3] = [Class::Land, Class::Water, Class::Park];

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
    let outer = open_tile_ring(feature.vertices().collect::<Result<Vec<_>, _>>()?);
    if outer.len() < 3 {
        return Ok(());
    }
    let holes = feature.holes().map(|hole| hole?.collect::<Result<Vec<_>, _>>().map(open_tile_ring))
        .collect::<Result<Vec<Vec<maps2_units::TileCoord>>, TileError>>()?;
    let ring = points(&outer);
    let hole_points = holes.iter().map(|hole| points(hole)).collect::<Vec<_>>();
    let mut coords = outer;
    coords.extend(holes.into_iter().flatten());
    let hole_refs = hole_points.iter().map(Vec::as_slice).collect::<Vec<_>>();
    let (_, triangles) = triangulate_with_holes(&ring, &hole_refs);
    let base = u32::try_from(bucket.vertices.len()).map_err(|_| TileError::TooLarge)?;
    bucket
        .vertices
        .extend(coords.iter().map(|v| FillVertex { x: v.0, y: v.1 }));
    for triangle in triangles {
        bucket.indices.extend(triangle.iter().map(|i| base + i));
    }
    Ok(())
}

fn open_tile_ring(mut ring: Vec<maps2_units::TileCoord>) -> Vec<maps2_units::TileCoord> {
    if ring.len() > 1 && ring.first() == ring.last() {
        ring.pop();
    }
    ring
}

fn points(ring: &[maps2_units::TileCoord]) -> Vec<Point> {
    ring.iter().map(|v| (f64::from(v.0), f64::from(v.1))).collect()
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
    fn fill_bucket_excludes_a_feature_hole() {
        let feature = maps2_tile::FeatureDraft::polygon_with_holes(
            1,
            0,
            vec![
                maps2_units::TileCoord(0, 0), maps2_units::TileCoord(10, 0),
                maps2_units::TileCoord(10, 10), maps2_units::TileCoord(0, 10), maps2_units::TileCoord(0, 0),
            ],
            vec![vec![
                maps2_units::TileCoord(3, 3), maps2_units::TileCoord(7, 3),
                maps2_units::TileCoord(7, 7), maps2_units::TileCoord(3, 7), maps2_units::TileCoord(3, 3),
            ]],
        );
        let mut builder = maps2_tile::TileBuilder::new(maps2_units::TileId { z: 12, x: 0, y: 0 });
        builder.push(Class::Water.code(), feature);
        let bytes = builder.build().expect("tile");
        let bucket = build_fill_bucket(&TileView::parse(&bytes).expect("view")).expect("bucket");
        let points = bucket.vertices.iter().map(|vertex| (f64::from(vertex.x), f64::from(vertex.y))).collect::<Vec<_>>();
        let triangles = bucket.indices.chunks_exact(3).map(|triangle| [triangle[0], triangle[1], triangle[2]]).collect::<Vec<_>>();

        assert!((triangles_area(&points, &triangles) - 84.0).abs() < f64::EPSILON);
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
        assert!(!classes.contains(&Class::Building), "buildings belong to the building bucket, not the fills");

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

        let bucket = build_building_bucket(&tile, BuildingLod::Full).expect("building bucket");

        assert_eq!(bucket.indices.len(), 21);
        assert!(bucket.vertices.iter().any(|vertex| vertex.height_dm == 0));
        assert!(bucket.vertices.iter().any(|vertex| vertex.height_dm == 120));
        assert_eq!(bucket.ranges.len(), 1, "one material, one range");
        assert_eq!(bucket.ranges[0].material, maps2_tile::MaterialClass::Unknown);
    }

    #[test]
    fn footprint_lod_has_no_walls_and_simplified_lod_has_a_flat_cap() {
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

        let footprint = build_building_bucket(&tile, BuildingLod::Footprint).expect("bucket");
        // Roof triangle only, no walls: 1 triangle = 3 indices.
        assert_eq!(footprint.indices.len(), 3);
        assert!(footprint.vertices.iter().all(|vertex| vertex.height_dm == 0), "footprint sits at base height");

        let simplified = build_building_bucket(&tile, BuildingLod::Simplified).expect("bucket");
        // Flat cap (3) + three walls (6 each) = 21, same shape as Full with a
        // flat roof — Simplified only differs from Full on shaped roofs.
        assert_eq!(simplified.indices.len(), 21);
    }

    #[test]
    fn full_lod_gives_a_gabled_roof_a_ridge_above_the_eave() {
        let mut builder = maps2_tile::TileBuilder::new(maps2_units::TileId { z: 16, x: 0, y: 0 });
        builder.push_building(
            Class::Building.code(),
            maps2_tile::FeatureDraft::geometry(
                17,
                0,
                vec![
                    maps2_units::TileCoord(1000, 1000),
                    maps2_units::TileCoord(5000, 1000),
                    maps2_units::TileCoord(5000, 3000),
                    maps2_units::TileCoord(1000, 3000),
                    maps2_units::TileCoord(1000, 1000),
                ],
            ),
            maps2_tile::BuildingDraft { base_height_dm: 0, top_height_dm: 120, roof: maps2_tile::RoofType::Gabled, material: maps2_tile::MaterialClass::Unknown },
        );
        let bytes = builder.build().expect("building tile");
        let tile = TileView::parse(&bytes).expect("tile parses");

        let bucket = build_building_bucket(&tile, BuildingLod::Full).expect("bucket");

        assert!(bucket.vertices.iter().any(|vertex| vertex.height_dm > 120), "a ridge must rise above the eave");
    }
}
