//! CPU decode: building buckets + heights without `Gl`.
//! `Map::load_tile` was synchronous on the main thread (`map.rs:445`);
//! this module isolates the CPU work so a future `Worker` can call it
//! off the main thread and post `DecodedTile` back. `upload_gpu` stays
//! on the main thread.

use maps2_render::{build_building_bucket, build_fill_bucket, build_label_bucket, build_line_bucket, BuildingBucket, BuildingLod, FillBucket, LabelBucket, LineBucket, LineOptions};
use maps2_tile::{unpack, HeightsRaster, TileView, TileError, CLASS_HEIGHTS, CLASS_HEIGHTS_PACKED};
use std::ops::Range;

/// Result of CPU decode — no `Gl` touched.
pub struct DecodedTile {
    pub id: maps2_units::TileId,
    pub fills: FillBucket,
    pub buildings: BuildingBucket,
    pub lines: LineBucket,
    pub names: LabelBucket,
    pub height: Option<HeightDecoded>,
}

/// Height handling after decode.
pub enum HeightDecoded {
    Plain(Range<usize>),
    Unpacked(Box<[u8]>),
}

/// Decode all CPU buckets and heights from `view`.
/// Pure — no `Gl` — so it can run in a `Worker`.
#[allow(clippy::missing_errors_doc)]
pub fn decode_tile(view: &TileView, building_lod: BuildingLod, line_options: LineOptions) -> Result<DecodedTile, TileError> {
    let id = view.header().id;
    let fills = build_fill_bucket(view)?;
    let buildings = build_building_bucket(view, building_lod)?;
    let lines = build_line_bucket(view, line_options)?;
    let names = build_label_bucket(view)?;
    let height = if let Some(packed) = view.raster(CLASS_HEIGHTS_PACKED) {
        let raster = unpack(packed)?;
        HeightsRaster::parse(&raster)?;
        Some(HeightDecoded::Unpacked(raster.into_boxed_slice()))
    } else if let Some(raster) = view.raster(CLASS_HEIGHTS) {
        HeightsRaster::parse(raster)?;
        view.section_span(CLASS_HEIGHTS).map(HeightDecoded::Plain)
    } else {
        None
    };
    Ok(DecodedTile { id, fills, buildings, lines, names, height })
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_render::{LineOptions, BuildingLod};
    use maps2_tile::TileBuilder;
    use maps2_units::TileId;

    fn minimal_tile() -> Vec<u8> {
        let id = TileId { z: 14, x: 0, y: 0 };
        TileBuilder::new(id).build().unwrap()
    }

    #[test]
    fn decode_minimal_tile_without_gl() {
        let bytes = minimal_tile();
        let view = TileView::parse(&bytes).unwrap();
        let decoded = decode_tile(&view, BuildingLod::Footprint, LineOptions::default()).unwrap();
        assert_eq!(decoded.id, TileId { z: 14, x: 0, y: 0 });
    }

    #[test]
    fn decode_invalid_bytes_is_error_not_panic() {
        let bad = vec![0u8; 10];
        assert!(TileView::parse(&bad).is_err());
    }
}
