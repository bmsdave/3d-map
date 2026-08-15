//! The point features that want a name on screen.
//!
//! Built once per resident tile, like the fill bucket, and read every
//! frame by the placement pass. Nothing here decides what is visible —
//! visibility is a property of the frame, not of the feature, and it is
//! settled in `maps2-text` against the whole viewport.

use maps2_style::Class;
use maps2_tile::{TileError, TileView};
use maps2_units::TileCoord;

/// The classes that carry names. Geometry classes have a `name` field
/// too, and it is empty for them.
pub const LABEL_CLASSES: [Class; 2] = [Class::Label, Class::Poi];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelPoint {
    pub id: u32,
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

/// Reads the named point features of one tile, in [`LABEL_CLASSES`]
/// order. A feature without a name is not a label and is dropped.
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
            let coord = feature.vertices().next().ok_or(TileError::Truncated)??;
            if feature.name.is_empty() {
                continue;
            }
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

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::{FeatureDraft, TileBuilder};
    use maps2_units::TileId;

    fn named(id: u32, rank: u8, name: &str, at: TileCoord) -> FeatureDraft {
        FeatureDraft { id, flags: 0, rank, name: name.to_string(), vertices: vec![at] }
    }

    fn sample() -> Vec<u8> {
        let mut builder = TileBuilder::new(TileId { z: 14, x: 8190, y: 5448 });
        builder.push(Class::Poi.code(), named(20, 6, "Bakery", TileCoord(1000, 2000)));
        builder.push(Class::Poi.code(), FeatureDraft::geometry(21, 0, vec![TileCoord(5, 5)]));
        builder.push(Class::Label.code(), named(10, 1, "Ealing", TileCoord(300, 400)));
        builder.push(Class::Water.code(), rect(99));
        builder.build().expect("label fixture fits MT2")
    }

    fn rect(id: u32) -> FeatureDraft {
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
}
