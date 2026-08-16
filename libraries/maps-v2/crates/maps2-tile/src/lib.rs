//! The tile on the wire: a flat binary layout read through views.
//!
//! A tile is a dumb container: sections keyed by a raw class code
//! (semantics live in `maps2-style`), features with integer geometry on
//! the `TILE_EXTENT` grid. No JSON anywhere — parsing JSON was 54% of a
//! frame in v1. Reading allocates nothing and copies nothing: a
//! [`TileView`] is a window over the caller's bytes, sections are found
//! through the header table rather than by scanning, and geometry is
//! decoded lazily as it is iterated.
//!
//! Format v2 — **frozen**. Version 1 remains readable; changing anything
//! below means a new version in the header and a knowing migration, never a
//! silent edit. The byte-exact layout lives in `../../docs/tile-format.md`.
//!
//! ```text
//! 0   magic  "MT2\0"
//! 4   version u16 LE (= 1)
//! 6   z u8, reserved u8
//! 8   x u32, y u32
//! 16  section_count u16, reserved u16
//! 20  table: section_count × { class u16, offset u32, len u32 }
//! …   payload: sections back to back (offsets relative to payload base)
//!
//! vector section (class < 0xFF00): feature_count u16, then features:
//!   id u32, flags u8, rank u8, base_dm u16, top_dm u16, roof u8,
//!   name_len u16, name utf8,
//!   vertex_count u16, first vertex (x u16, y u16),
//!   deltas: (vertex_count − 1) × (dx, dy) zigzag varint
//!
//! raster section (class ≥ 0xFF00): opaque payload owned by the class
//!   (heights: 256×256 u16 LE, row-major)
//! ```

use maps2_units::{TileCoord, TileId};

mod build;
mod heights;
mod varint;
mod view;

pub use build::TileBuilder;
pub use heights::{
    decode_height, encode_height, HeightsRaster, HEIGHTS_BYTES, HEIGHTS_SIDE, HEIGHT_OFFSET_M,
};
pub use view::{FeatureView, SectionView, TileView};

pub const MAGIC: [u8; 4] = *b"MT2\0";
pub const FORMAT_VERSION: u16 = 4;
pub const PREVIOUS_FORMAT_VERSION: u16 = 3;
pub const HOLES_FORMAT_VERSION: u16 = 3;
pub const LEGACY_FORMAT_VERSION: u16 = 1;

/// Class codes from here up are raster sections: opaque payloads, no
/// feature structure. Below — vector sections with features.
pub const RASTER_CLASS_BASE: ClassCode = 0xFF00;

/// The heights raster: 256×256 `u16` LE metres offset by +11000
/// (GEBCO depths stay positive), row-major.
pub const CLASS_HEIGHTS: ClassCode = 0xFF00;

/// Feature flags on the wire. Meaning of the bits is owned by the
/// style; the format only carries them.
pub type FeatureFlags = u8;

/// Raw class code of a section. `maps2-style` maps it to semantics.
pub type ClassCode = u16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileError {
    TooShort,
    BadMagic,
    UnsupportedVersion(u16),
    SectionOutOfBounds(ClassCode),
    Truncated,
    BadVarint,
    DeltaOutOfRange,
    BadText,
    BadBuilding,
    TooLarge,
    EmptyGeometry,
}

/// One feature as fed to the builder; the reading side never allocates
/// this — it decodes vertices lazily instead. `rank` orders point
/// classes for selection (0 = most important); `name` is non-empty for
/// labels and POI, empty for geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeatureDraft {
    pub id: u64,
    pub flags: FeatureFlags,
    pub rank: u8,
    pub name: String,
    pub vertices: Vec<TileCoord>,
    pub holes: Vec<Vec<TileCoord>>,
}

/// A building's vertical extent, in decimetres above the terrain datum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingData {
    pub base_height_dm: u16,
    pub top_height_dm: u16,
    pub roof: RoofType,
}

/// The builder-side name for [`BuildingData`].
pub type BuildingDraft = BuildingData;
/// The reader-side name for [`BuildingData`].
pub type BuildingView = BuildingData;

impl BuildingData {
    #[must_use]
    pub const fn flat(base_height_dm: u16, top_height_dm: u16) -> Self {
        Self {
            base_height_dm,
            top_height_dm,
            roof: RoofType::Flat,
        }
    }
}

/// The roof form exposed by MT2 v2 and later.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RoofType {
    Flat = 0,
    Gabled = 1,
    Hipped = 2,
    Other = 3,
}

impl RoofType {
    pub(crate) const fn from_wire(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Flat),
            1 => Some(Self::Gabled),
            2 => Some(Self::Hipped),
            3 => Some(Self::Other),
            _ => None,
        }
    }
}

impl FeatureDraft {
    /// A nameless, rankless geometry feature.
    #[must_use]
    pub fn geometry(id: u64, flags: FeatureFlags, vertices: Vec<TileCoord>) -> Self {
        Self {
            id,
            flags,
            rank: 0,
            name: String::new(),
            vertices,
            holes: Vec::new(),
        }
    }

    /// A polygon whose interior rings are excluded from its fill.
    #[must_use]
    pub fn polygon_with_holes(
        id: u64,
        flags: FeatureFlags,
        vertices: Vec<TileCoord>,
        holes: Vec<Vec<TileCoord>>,
    ) -> Self {
        Self {
            id,
            flags,
            rank: 0,
            name: String::new(),
            vertices,
            holes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileHeader {
    pub version: u16,
    pub id: TileId,
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_units::TileCoord;

    fn water_feature() -> FeatureDraft {
        FeatureDraft::geometry(
            7,
            0,
            vec![
                TileCoord(100, 200),
                TileCoord(65535, 0),
                TileCoord(0, 65535),
                TileCoord(30000, 30001),
                TileCoord(100, 200),
            ],
        )
    }

    fn poi_features() -> Vec<FeatureDraft> {
        vec![
            FeatureDraft {
                id: 40,
                flags: 0b101,
                rank: 2,
                name: "Пожарная станция".to_string(),
                vertices: vec![TileCoord(5, 5)],
                holes: Vec::new(),
            },
            FeatureDraft {
                id: 41,
                flags: 0,
                rank: 7,
                name: String::new(),
                vertices: vec![TileCoord(60000, 12)],
                holes: Vec::new(),
            },
        ]
    }

    fn build_sample() -> Vec<u8> {
        let mut builder = TileBuilder::new(TileId {
            z: 14,
            x: 8190,
            y: 5448,
        });
        builder.push(2, water_feature());
        for poi in poi_features() {
            builder.push(9, poi);
        }
        builder.build().expect("sample fits MT2")
    }

    fn collect(section: SectionView<'_>) -> Vec<FeatureDraft> {
        section
            .features()
            .map(|f| {
                let f = f.expect("feature decodes");
                FeatureDraft {
                    id: f.id,
                    flags: f.flags,
                    rank: f.rank,
                    name: f.name.to_string(),
                    vertices: f
                        .vertices()
                        .collect::<Result<Vec<_>, _>>()
                        .expect("vertices decode"),
                    holes: f
                        .holes()
                        .map(|hole| hole.and_then(Iterator::collect))
                        .collect::<Result<Vec<_>, _>>()
                        .expect("holes decode"),
                }
            })
            .collect()
    }

    #[test]
    fn a_built_tile_reads_back_identically() {
        let bytes = build_sample();
        let tile = TileView::parse(&bytes).expect("parses");
        assert_eq!(
            tile.header(),
            TileHeader {
                version: FORMAT_VERSION,
                id: TileId {
                    z: 14,
                    x: 8190,
                    y: 5448
                }
            },
        );
        assert_eq!(
            collect(tile.section(2).expect("water section")),
            vec![water_feature()]
        );
        assert_eq!(
            collect(tile.section(9).expect("poi section")),
            poi_features()
        );
        assert_eq!(tile.section(1), None);
    }

    #[test]
    fn extreme_deltas_survive_the_round_trip() {
        // 0 → 65535 → 0 exercises the widest zigzag deltas both ways.
        let feature = FeatureDraft::geometry(
            1,
            0,
            vec![
                TileCoord(0, 65535),
                TileCoord(65535, 0),
                TileCoord(0, 65535),
            ],
        );
        let mut builder = TileBuilder::new(TileId { z: 0, x: 0, y: 0 });
        builder.push(0, feature.clone());
        let bytes = builder.build().expect("feature fits MT2");
        let tile = TileView::parse(&bytes).expect("parses");
        assert_eq!(collect(tile.section(0).expect("section")), vec![feature]);
    }

    #[test]
    fn a_polygon_hole_survives_the_round_trip() {
        let feature = FeatureDraft::polygon_with_holes(
            9,
            0,
            vec![
                TileCoord(0, 0),
                TileCoord(10, 0),
                TileCoord(10, 10),
                TileCoord(0, 0),
            ],
            vec![vec![
                TileCoord(2, 2),
                TileCoord(8, 2),
                TileCoord(8, 8),
                TileCoord(2, 2),
            ]],
        );
        let mut builder = TileBuilder::new(TileId { z: 12, x: 1, y: 2 });
        builder.push(1, feature);
        let bytes = builder.build().expect("tile");
        let tile = TileView::parse(&bytes).expect("view");
        let feature = tile
            .section(1)
            .expect("section")
            .features()
            .next()
            .expect("feature")
            .expect("valid");

        assert_eq!(
            feature
                .holes()
                .collect::<Result<Vec<_>, _>>()
                .expect("holes")
                .len(),
            1
        );
    }

    #[test]
    fn a_feature_id_beyond_u32_survives_the_round_trip() {
        let id = u64::from(u32::MAX) + 1;
        let feature = FeatureDraft::geometry(id, 0, vec![TileCoord(1, 1)]);
        let mut builder = TileBuilder::new(TileId { z: 12, x: 1, y: 2 });
        builder.push(1, feature);
        let bytes = builder.build().expect("tile");
        let tile = TileView::parse(&bytes).expect("view");

        assert_eq!(
            tile.section(1)
                .expect("section")
                .features()
                .next()
                .expect("feature")
                .expect("valid")
                .id,
            id
        );
    }

    #[test]
    fn reading_one_section_does_not_depend_on_the_bytes_of_another() {
        // Corrupt the POI section's payload entirely; the water section
        // must still read — proof that lookup goes through the table
        // and never scans or validates unrelated bytes.
        let mut bytes = build_sample();
        let clean = TileView::parse(&bytes).expect("parses");
        let poi = clean.section_span(9).expect("poi span");
        for b in &mut bytes[poi.clone()] {
            *b = 0xFF;
        }
        let tile = TileView::parse(&bytes).expect("still parses");
        assert_eq!(
            collect(tile.section(2).expect("water section")),
            vec![water_feature()]
        );
    }

    #[test]
    fn malformed_bytes_are_an_error_never_a_panic() {
        assert_eq!(TileView::parse(&[]), Err(TileError::TooShort));
        let mut bad_magic = build_sample();
        bad_magic[0] = b'X';
        assert_eq!(TileView::parse(&bad_magic), Err(TileError::BadMagic));

        let mut bad_version = build_sample();
        bad_version[4] = 99;
        assert_eq!(
            TileView::parse(&bad_version),
            Err(TileError::UnsupportedVersion(99))
        );

        // Cut the buffer inside the payload: parse succeeds (header is
        // intact), the damaged section errors on access, iteration of
        // its features reports Truncated rather than panicking.
        let full = build_sample();
        let cut = &full[..full.len() - 3];
        let tile = TileView::parse(cut).expect("header intact");
        let section = tile.section(9).expect("section table remains intact");
        let outcome: Result<Vec<_>, _> = section
            .features()
            .map(|f| f.and_then(|f| f.vertices().collect::<Result<Vec<_>, _>>()))
            .collect();
        assert!(outcome.is_err(), "truncated section must error");
    }

    #[test]
    fn a_raster_section_round_trips_and_is_not_a_vector_section() {
        let mut builder = TileBuilder::new(TileId { z: 10, x: 1, y: 2 });
        builder.push(0, water_feature());
        let heights: Vec<u8> = (0_u16..512).flat_map(u16::to_le_bytes).collect();
        builder.push_raster(CLASS_HEIGHTS, heights.clone());
        let bytes = builder.build().expect("raster fits MT2");
        let tile = TileView::parse(&bytes).expect("parses");
        assert_eq!(tile.raster(CLASS_HEIGHTS), Some(heights.as_slice()));
        assert_eq!(tile.section(CLASS_HEIGHTS), None, "raster is not features");
        assert_eq!(tile.raster(0), None, "vector is not raster");
    }

    #[test]
    fn a_building_carries_its_base_top_and_roof_through_the_tile() {
        let mut builder = TileBuilder::new(TileId {
            z: 16,
            x: 32744,
            y: 21792,
        });
        builder.push_building(
            3,
            FeatureDraft::geometry(
                17,
                0,
                vec![TileCoord(10, 10), TileCoord(100, 10), TileCoord(10, 10)],
            ),
            BuildingDraft::flat(125, 547),
        );

        let bytes = builder.build().expect("building tile");
        let tile = TileView::parse(&bytes).expect("parses");
        let building = tile
            .section(3)
            .expect("building section")
            .features()
            .next()
            .expect("building")
            .expect("feature")
            .building;

        assert_eq!(building, Some(BuildingView::flat(125, 547)));
    }

    #[test]
    fn garbage_varint_reports_an_error() {
        // A section whose delta stream is all 0xFF cannot decode into
        // u16 coordinates; iteration must surface an error.
        let mut bytes = build_sample();
        let clean = TileView::parse(&bytes).expect("parses");
        let water = clean.section_span(2).expect("water span");
        // Keep feature_count/id/flags/vertex_count intact (first 11
        // bytes of the section), poison the delta stream after the
        // first vertex (4 more bytes).
        let poison_from = water.start + 2 + 4 + 1 + 1 + 2 + 2 + 4;
        for b in &mut bytes[poison_from..water.end] {
            *b = 0xFF;
        }
        let tile = TileView::parse(&bytes).expect("parses");
        let outcome: Result<Vec<_>, _> = tile
            .section(2)
            .expect("water section")
            .features()
            .map(|f| f.and_then(|f| f.vertices().collect::<Result<Vec<_>, _>>()))
            .collect();
        assert!(outcome.is_err());
    }

    #[test]
    fn every_declared_roof_form_round_trips() {
        for roof in [RoofType::Gabled, RoofType::Hipped, RoofType::Other] {
            let mut builder = TileBuilder::new(TileId { z: 16, x: 1, y: 2 });
            builder.push_building(
                3,
                FeatureDraft::geometry(17, 0, vec![TileCoord(10, 10)]),
                BuildingDraft {
                    base_height_dm: 10,
                    top_height_dm: 20,
                    roof,
                },
            );
            let bytes = builder.build().expect("building tile");
            let feature = TileView::parse(&bytes)
                .expect("parses")
                .section(3)
                .expect("section")
                .features()
                .next()
                .expect("feature")
                .expect("valid");

            assert_eq!(feature.building.map(|building| building.roof), Some(roof));
        }
    }
}
