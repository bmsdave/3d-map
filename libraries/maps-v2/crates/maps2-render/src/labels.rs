//! The named map features that want a name on screen.
//!
//! Built once per resident tile, like the fill bucket, and read every
//! frame by the placement pass. Nothing here decides what is visible —
//! visibility is a property of the frame, not of the feature, and it is
//! settled in `maps2-text` against the whole viewport.

use maps2_style::Class;
use maps2_tile::{TileError, TileView};
use maps2_units::TileCoord;
use num_traits::ToPrimitive;

/// The classes that carry names. Geometry classes have a `name` field
/// too, and it is empty for them.
pub const LABEL_CLASSES: [Class; 9] = [
    Class::Label,
    Class::Poi,
    Class::RoadMotorway,
    Class::RoadTrunk,
    Class::RoadPrimary,
    Class::RoadSecondary,
    Class::RoadResidential,
    Class::RoadService,
    Class::RoadPath,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelPoint {
    pub id: u64,
    pub rank: u8,
    pub class: Class,
    pub name: String,
    /// The anchor, on the tile's own integer grid.
    pub coord: TileCoord,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LabelBucket {
    pub points: Vec<LabelPoint>,
}

/// Reads named point and road features of one tile, in [`LABEL_CLASSES`]
/// order. Road names anchor at their geometric midpoint; text stays upright.
/// A feature without a name is not a label and is dropped.
///
/// # Errors
///
/// Returns [`TileError`] when the tile's label data is truncated or malformed.
pub fn build_label_bucket(tile: &TileView) -> Result<LabelBucket, TileError> {
    let mut bucket = LabelBucket::default();
    for class in LABEL_CLASSES {
        let Some(section) = tile.section(class.code()) else {
            continue;
        };
        for feature in section.features() {
            let feature = feature?;
            if feature.name.is_empty() {
                continue;
            }
            let vertices = feature.vertices().collect::<Result<Vec<_>, _>>()?;
            let coord = label_anchor(class, &vertices)?;
            bucket.points.push(LabelPoint {
                id: feature.id,
                rank: feature.rank,
                class,
                name: feature.name.to_string(),
                coord,
            });
        }
    }
    Ok(bucket)
}

fn label_anchor(class: Class, vertices: &[TileCoord]) -> Result<TileCoord, TileError> {
    let first = *vertices.first().ok_or(TileError::Truncated)?;
    if class.road_rank().is_none() || vertices.len() == 1 {
        return Ok(first);
    }
    Ok(road_midpoint(vertices))
}

fn road_midpoint(vertices: &[TileCoord]) -> TileCoord {
    let length = vertices.windows(2).map(segment_length).sum::<f64>();
    let mut remaining = length / 2.0;
    for segment in vertices.windows(2) {
        let segment_length = segment_length(segment);
        if segment_length == 0.0 {
            continue;
        }
        if remaining <= segment_length {
            return interpolate(segment[0], segment[1], remaining / segment_length);
        }
        remaining -= segment_length;
    }
    *vertices.last().expect("a road label has a first vertex")
}

fn segment_length(segment: &[TileCoord]) -> f64 {
    let dx = f64::from(segment[1].0) - f64::from(segment[0].0);
    let dy = f64::from(segment[1].1) - f64::from(segment[0].1);
    dx.hypot(dy)
}

fn interpolate(start: TileCoord, end: TileCoord, fraction: f64) -> TileCoord {
    let x = (f64::from(end.0) - f64::from(start.0)).mul_add(fraction, f64::from(start.0));
    let y = (f64::from(end.1) - f64::from(start.1)).mul_add(fraction, f64::from(start.1));
    TileCoord(
        x.round().to_u16().expect("interpolated x stays in tile extent"),
        y.round().to_u16().expect("interpolated y stays in tile extent"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::{FeatureDraft, TileBuilder};
    use maps2_units::TileId;

    fn named(id: u64, rank: u8, name: &str, at: TileCoord) -> FeatureDraft {
        FeatureDraft { id, flags: 0, rank, name: name.to_string(), vertices: vec![at], holes: Vec::new() }
    }

    fn sample() -> Vec<u8> {
        let mut builder = TileBuilder::new(TileId { z: 14, x: 8190, y: 5448 });
        builder.push(Class::Poi.code(), named(20, 6, "Bakery", TileCoord(1000, 2000)));
        builder.push(Class::Poi.code(), FeatureDraft::geometry(21, 0, vec![TileCoord(5, 5)]));
        builder.push(Class::Label.code(), named(10, 1, "Ealing", TileCoord(300, 400)));
        builder.push(
            Class::RoadPrimary.code(),
            FeatureDraft {
                id: 30,
                flags: 0,
                rank: 3,
                name: "Uxbridge Road".to_string(),
                vertices: vec![TileCoord(100, 200), TileCoord(300, 400), TileCoord(500, 600)],
                holes: Vec::new(),
            },
        );
        builder.push(Class::Water.code(), rect(99));
        builder.build().expect("label fixture fits MT2")
    }

    fn rect(id: u64) -> FeatureDraft {
        FeatureDraft::geometry(
            id,
            0,
            vec![TileCoord(0, 0), TileCoord(10, 0), TileCoord(10, 10), TileCoord(0, 0)],
        )
    }

    #[test]
    fn only_named_point_features_become_labels() {
        let bytes = sample();
        let tile = TileView::parse(&bytes).expect("parses");
        let bucket = build_label_bucket(&tile).expect("builds");
        assert_eq!(
            bucket.points,
            vec![
                LabelPoint {
                    id: 10,
                    rank: 1,
                    class: Class::Label,
                    name: "Ealing".into(),
                    coord: TileCoord(300, 400),
                },
                LabelPoint {
                    id: 20,
                    rank: 6,
                    class: Class::Poi,
                    name: "Bakery".into(),
                    coord: TileCoord(1000, 2000),
                },
                LabelPoint {
                    id: 30,
                    rank: 3,
                    class: Class::RoadPrimary,
                    name: "Uxbridge Road".into(),
                    coord: TileCoord(300, 400),
                },
            ],
        );
    }

    #[test]
    fn a_tile_of_pure_geometry_has_no_labels() {
        let mut builder = TileBuilder::new(TileId { z: 8, x: 1, y: 2 });
        builder.push(Class::Water.code(), rect(1));
        let bytes = builder.build().expect("label fixture fits MT2");
        let tile = TileView::parse(&bytes).expect("parses");
        assert_eq!(build_label_bucket(&tile).expect("builds"), LabelBucket::default());
    }

    #[test]
    fn damaged_label_bytes_are_an_error_not_a_panic() {
        let mut bytes = sample();
        let clean = TileView::parse(&bytes).expect("parses");
        let span = clean.section_span(Class::Label.code()).expect("label span");
        for b in &mut bytes[span.start + 2..span.end] {
            *b = 0xFF;
        }
        let tile = TileView::parse(&bytes).expect("header intact");
        assert!(build_label_bucket(&tile).is_err());
    }

    #[test]
    fn a_degenerate_road_anchors_at_its_only_distinct_coordinate() {
        assert_eq!(
            road_midpoint(&[TileCoord(44, 88), TileCoord(44, 88)]),
            TileCoord(44, 88),
        );
    }

    #[test]
    fn a_midpoint_skips_an_empty_leading_segment() {
        assert_eq!(
            road_midpoint(&[TileCoord(0, 0), TileCoord(0, 0), TileCoord(8, 0)]),
            TileCoord(4, 0),
        );
    }

    #[test]
    fn a_midpoint_crosses_a_short_first_segment() {
        assert_eq!(
            road_midpoint(&[TileCoord(0, 0), TileCoord(2, 0), TileCoord(10, 0)]),
            TileCoord(5, 0),
        );
    }
}
