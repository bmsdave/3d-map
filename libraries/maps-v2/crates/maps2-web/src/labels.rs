//! The label pass, without a GL context in sight.
//!
//! Every frame: project each resident tile's named points to screen,
//! measure them with the atlas, and let `maps2-text` decide who gets the
//! room. Text is laid out **after** projection, from a screen anchor —
//! that is the whole reason it does not rotate with the map and stands
//! upright under tilt. Put the glyphs in the world mesh instead and both
//! properties are gone.

use maps2_camera::Camera;
use maps2_render::LabelBucket;
use maps2_style::{entry_band, Class, ALL_BANDS};
use maps2_text::{
    layout_line, measure_line, place, Atlas, Candidate, GlyphQuad, Placement, ASCENDER_EM,
    LINE_HEIGHT_EM,
};
use maps2_units::{TileCoord, TileId};
use num_traits::ToPrimitive;

use crate::transform::place_tile;

/// Breathing room around a line, in screen pixels. Two labels may sit
/// flush against each other's boxes; this is what keeps their ink apart.
pub const LABEL_PADDING_PX: f32 = 3.0;

/// Text size by class and rank, in screen pixels. Importance is a size
/// difference before it is anything else.
#[must_use]
pub fn label_size_px(class: Class, rank: u8) -> f32 {
    match class {
        Class::Label => match rank {
            0 => 20.0,
            1 => 17.0,
            2 => 15.0,
            _ => 13.0,
        },
        _ => 12.0,
    }
}

/// The identity a label competes under: the feature, not the tile's
/// copy of it. The class rides in the high half so the frame can colour
/// a placed label without carrying a second table.
#[must_use]
pub fn label_identity(class: Class, feature: u64) -> u128 {
    u128::from(class.code()) << 64 | u128::from(feature)
}

#[must_use]
pub fn class_of(identity: u128) -> Option<Class> {
    Class::from_code(u16::try_from(identity >> 64).ok()?)
}

/// Candidates of one frame, in tile order. Order does not matter to the
/// answer — placement sorts by `(rank, id)` — but it does keep the
/// assembly cheap.
///
/// Text shaping (`measure_line`) is the expensive part of building a
/// candidate, so a class is skipped below [`label_ready_zoom`] rather
/// than measured and then rejected by the placer. A real city can carry
/// tens of thousands of named residential streets the moment their
/// geometry enters at [`maps2_style::entry_band`]'s zoom; at that same
/// zoom they are city-wide and every one of them loses to place names on
/// both budget and collision, so shaping their text is pure waste. See
/// [`label_ready_zoom`] for the rule.
#[must_use]
pub fn frame_candidates(
    buckets: &[(TileId, &LabelBucket)],
    camera: &Camera,
    viewport: (f64, f64),
    atlas: &Atlas,
) -> Vec<Candidate> {
    let zoom = camera.zoom().value();
    let mut out = Vec::new();
    for (id, bucket) in buckets {
        for point in &bucket.points {
            if zoom < label_ready_zoom(point.class) {
                continue;
            }
            let size_px = label_size_px(point.class, point.rank);
            let (width, height) = measure_line(atlas, &point.name, size_px);
            out.push(Candidate {
                id: label_identity(point.class, point.id),
                rank: point.rank,
                text: point.name.clone(),
                anchor: anchor_px(*id, point.coord, camera, viewport),
                size: (
                    width + 2.0 * LABEL_PADDING_PX,
                    height + 2.0 * LABEL_PADDING_PX,
                ),
            });
        }
    }
    out
}

/// The zoom at which a class's name becomes worth shaping at all.
///
/// A road's geometry and its name do not have to earn the screen at the
/// same zoom: the name only needs to compete once the class has had a
/// full band to itself, i.e. from the entry zoom of the band *after*
/// the one its geometry entered at. `Label` and `Poi` keep their
/// existing entry zoom — `Label`'s candidate pool is already rank-gated
/// by zoom in ingest, and `Poi` was never the volume problem.
#[must_use]
fn label_ready_zoom(class: Class) -> f64 {
    match class {
        Class::Label | Class::Poi => entry_band(class).entry_zoom(),
        _ => {
            let entry = entry_band(class);
            ALL_BANDS
                .iter()
                .position(|band| *band == entry)
                .and_then(|index| ALL_BANDS.get(index + 1))
                .map_or(f64::INFINITY, |band| band.entry_zoom())
        }
    }
}

/// Where a tile-grid point lands on screen, in logical pixels.
fn anchor_px(id: TileId, coord: TileCoord, camera: &Camera, viewport: (f64, f64)) -> (f32, f32) {
    let placement = place_tile(id, camera, viewport.0, viewport.1);
    (
        screen_scalar(f64::from(coord.0).mul_add(placement.scale, placement.translate_x)),
        screen_scalar(f64::from(coord.1).mul_add(placement.scale, placement.translate_y)),
    )
}

/// The whole frame in one call: who is placed, and the quads to draw.
#[must_use]
pub fn place_frame(
    buckets: &[(TileId, &LabelBucket)],
    camera: &Camera,
    viewport: (f64, f64),
    atlas: &Atlas,
    budget: f32,
) -> Placement {
    let candidates = frame_candidates(buckets, camera, viewport, atlas);
    place(&candidates, (screen_scalar(viewport.0), screen_scalar(viewport.1)), budget)
}

fn screen_scalar(value: f64) -> f32 {
    value.to_f32().expect("screen values fit f32")
}

/// Glyph quads of everything placed, grouped by class so the renderer
/// can change colour once per group instead of once per label.
#[must_use]
pub fn frame_quads(atlas: &Atlas, placement: &Placement, class: Class) -> Vec<GlyphQuad> {
    let mut quads = Vec::new();
    for label in &placement.placed {
        if class_of(label.id) != Some(class) {
            continue;
        }
        let size_px = size_from_box(label.bounds.y1 - label.bounds.y0);
        let baseline = ASCENDER_EM.mul_add(size_px, label.bounds.y0 + LABEL_PADDING_PX);
        quads.extend(layout_line(
            atlas,
            &label.text,
            label.bounds.x0 + LABEL_PADDING_PX,
            baseline,
            size_px,
        ));
    }
    quads
}

/// The box height is the em box plus padding, so it gives the size back.
fn size_from_box(height: f32) -> f32 {
    2.0_f32.mul_add(-LABEL_PADDING_PX, height) / LINE_HEIGHT_EM
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_camera::CameraPatch;
    use maps2_fixtures::{coverage, tile_bytes, BOUNDARY_ID, EALING};
    use maps2_render::{build_label_bucket, LabelPoint};
    use maps2_text::Rejection;
    use maps2_tile::TileView;
    use maps2_units::{locate, Zoom};

    const VIEWPORT: (f64, f64) = (800.0, 600.0);

    fn buckets_at(level: u8) -> Vec<(TileId, LabelBucket)> {
        coverage()
            .into_iter()
            .filter(|id| id.z == level)
            .map(|id| {
                let bytes = tile_bytes(id);
                let tile = TileView::parse(&bytes).expect("fixture parses");
                (id, build_label_bucket(&tile).expect("labels build"))
            })
            .collect()
    }

    fn borrowed(owned: &[(TileId, LabelBucket)]) -> Vec<(TileId, &LabelBucket)> {
        owned.iter().map(|(id, b)| (*id, b)).collect()
    }

    fn camera_at(zoom: f64) -> Camera {
        Camera::new(EALING, Zoom::new(zoom))
    }

    /// The roadmap's promise about type: it is not part of the world
    /// mesh. Turning or tilting the camera may move a label, but it may
    /// never rotate, shear or squash one.
    #[test]
    fn glyph_quads_stay_axis_aligned_and_unsheared_whatever_the_camera_does() {
        let atlas = Atlas::build();
        let owned = buckets_at(14);
        let mut upright = camera_at(14.0);
        let mut turned = camera_at(14.0);
        turned
            .apply(&CameraPatch {
                bearing_deg: Some(37.0),
                tilt_deg: Some(55.0),
                ..CameraPatch::default()
            })
            .expect("valid patch");
        let sizes = |camera: &Camera| {
            let placement = place_frame(&borrowed(&owned), camera, VIEWPORT, &atlas, 1.0);
            frame_quads(&atlas, &placement, Class::Label)
                .iter()
                .map(|q| {
                    (
                        (q.x1 - q.x0).round().to_i32().expect("glyph width fits i32"),
                        (q.y1 - q.y0).round().to_i32().expect("glyph height fits i32"),
                    )
                })
                .collect::<Vec<_>>()
        };
        upright.apply(&CameraPatch::default()).expect("no-op patch");
        let flat = sizes(&upright);
        assert!(!flat.is_empty(), "no label quads at all");
        // Every quad is a square of the cell: no rotation can produce
        // that, and no shear can survive it.
        assert!(flat.iter().all(|(w, h)| w == h), "quads are not square: {flat:?}");
        assert_eq!(flat, sizes(&turned), "bearing and tilt changed the type");
    }

    #[test]
    fn the_same_place_seen_from_four_tiles_is_placed_once() {
        let atlas = Atlas::build();
        let owned = buckets_at(14);
        let identity = label_identity(Class::Label, u64::from(BOUNDARY_ID));
        let copies = owned
            .iter()
            .flat_map(|(_, b)| &b.points)
            .filter(|p| p.id == u64::from(BOUNDARY_ID))
            .count();
        assert!(copies >= 2, "the fixture offers {copies} copies");
        let placement =
            place_frame(&borrowed(&owned), &camera_at(14.0), VIEWPORT, &atlas, 1.0);
        let placed = placement.placed.iter().filter(|p| p.id == identity).count();
        let duplicates = placement
            .rejected
            .iter()
            .filter(|r| r.id == identity && r.reason == Rejection::Duplicate)
            .count();
        assert!(placed <= 1, "the same place was drawn {placed} times");
        // Exactly one copy competes; the others never reach the grid at
        // all, whatever happens to the one that does.
        assert_eq!(duplicates, copies - 1, "{duplicates} of {copies} copies were deduped");
    }

    #[test]
    fn a_road_class_stops_shaping_text_at_its_own_entry_zoom() {
        // RoadResidential's geometry enters at the Street band (12.0);
        // its name only becomes a candidate one band later, at Address
        // (14.0). At exactly its own entry zoom, real cities carry tens
        // of thousands of these — none of them can ever win a place
        // name's budget, so they must not be shaped at all.
        let atlas = Atlas::build();
        let tile = locate(EALING, 12).tile;
        let bucket = LabelBucket {
            points: vec![LabelPoint {
                id: 1,
                rank: 0,
                class: Class::RoadResidential,
                name: "Any Street".to_string(),
                coord: TileCoord(32768, 32768),
            }],
        };
        let owned = vec![(tile, bucket)];
        let at_entry = frame_candidates(&borrowed(&owned), &camera_at(12.0), VIEWPORT, &atlas);
        assert!(at_entry.is_empty(), "RoadResidential was shaped at its own entry zoom");
        let one_band_later =
            frame_candidates(&borrowed(&owned), &camera_at(14.0), VIEWPORT, &atlas);
        assert_eq!(one_band_later.len(), 1, "RoadResidential never becomes a candidate at all");
    }

    #[test]
    fn a_full_width_feature_identity_keeps_its_class() {
        let feature = u64::from(u32::MAX) + 1;
        let identity = label_identity(Class::Label, feature);

        assert_eq!(class_of(identity), Some(Class::Label));
        assert_ne!(identity, label_identity(Class::Label, feature - 1));
    }

    #[test]
    fn a_crowded_micro_frame_offers_a_thousand_candidates_and_keeps_a_fraction() {
        let atlas = Atlas::build();
        let owned = buckets_at(16);
        let camera = Camera::new(EALING, Zoom::new(16.0));
        let candidates = frame_candidates(&borrowed(&owned), &camera, VIEWPORT, &atlas);
        assert!(candidates.len() > 1000, "only {} candidates", candidates.len());
        let placement = place_frame(&borrowed(&owned), &camera, VIEWPORT, &atlas, 0.08);
        assert!(placement.occupancy <= 0.08);
        assert!(
            placement.placed.len() * 5 < candidates.len(),
            "{} of {} survived — the budget did nothing",
            placement.placed.len(),
            candidates.len(),
        );
    }

    #[test]
    fn a_longer_name_asks_for_a_wider_box() {
        let atlas = Atlas::build();
        let owned = buckets_at(14);
        let candidates = frame_candidates(&borrowed(&owned), &camera_at(14.0), VIEWPORT, &atlas);
        let short = candidates.iter().find(|c| c.text == "Ealing").expect("Ealing");
        let long = candidates
            .iter()
            .find(|c| c.text == "Ealing Broadway")
            .expect("Ealing Broadway");
        assert!(long.size.0 > short.size.0 * 1.5, "{} vs {}", long.size.0, short.size.0);
        // Same class, so the box height only follows the rank's size.
        assert!(short.size.1 > long.size.1, "rank 0 must set bigger type");
    }

    /// The centre tile of the fixture sits under the camera centre, so
    /// its label lands near the middle of the viewport — the one place
    /// arithmetic, not cartography, has to be right.
    #[test]
    fn a_place_at_the_camera_centre_lands_in_the_middle_of_the_viewport() {
        let atlas = Atlas::build();
        let owned = buckets_at(14);
        let candidates = frame_candidates(&borrowed(&owned), &camera_at(14.0), VIEWPORT, &atlas);
        let centre = candidates.iter().find(|c| c.text == "Ealing").expect("Ealing");
        let home = locate(EALING, 14).tile;
        assert!(owned.iter().any(|(id, _)| *id == home));
        assert!((centre.anchor.0 - 400.0).abs() < 2.0, "x = {}", centre.anchor.0);
        assert!((centre.anchor.1 - 300.0).abs() < 2.0, "y = {}", centre.anchor.1);
    }
}
