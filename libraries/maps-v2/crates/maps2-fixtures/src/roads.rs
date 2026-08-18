//! The road pathology scene: one tile holding every defect worth
//! looking at, so a screenshot can be compared against it.
//!
//! Ealing's street grid is a fair map and a useless test: its corners
//! are all right angles and nothing crosses anything. The cases that
//! break line rendering are rare in real data and easy to miss —
//! a corner sharp enough to grow a spike, three arms meeting at one
//! node, a closed ring, two carriageways close enough for their casings
//! to touch, and a road passing over another. Here they sit side by
//! side at a fixed zoom.
//!
//! The scene lives inside a single z16 tile and is laid out in that
//! tile's own grid, not in world coordinates: it is a specimen, not
//! geography, and nothing about it has to line up with a neighbour.

use maps2_style::{Class, FLAG_BRIDGE, FLAG_TUNNEL};
use maps2_tile::{FeatureDraft, TileBuilder};
use maps2_units::{locate, to_lonlat, Lonlat, TileCoord, TileId, TilePoint};
use num_traits::ToPrimitive;

use crate::{rect_polygon, EALING};

/// The zoom the scene is composed for: a z16 tile spans 512 px there,
/// so the whole specimen fits a lab canvas with room around it.
pub const ROADS_ZOOM: f64 = 17.0;

/// Points of the roundabout. Enough that every corner miters, which is
/// the case a ring is here to cover.
const RING_SIDES: usize = 24;

/// The single tile the scene lives in.
#[must_use]
pub fn roads_tile() -> TileId {
    locate(EALING, 16).tile
}

/// Where the camera has to sit to frame the scene: the tile's middle.
/// The lab reads this from the package rather than repeating Mercator
/// in TypeScript.
#[must_use]
pub fn roads_centre() -> Lonlat {
    to_lonlat(TilePoint { tile: roads_tile(), coord: TileCoord(32768, 32768) })
}

/// The package, deterministically — one tile.
#[must_use]
pub fn roads_tiles() -> Vec<(TileId, Vec<u8>)> {
    vec![(roads_tile(), roads_tile_bytes())]
}

#[must_use]
///
/// # Panics
///
/// Panics only if this bounded synthetic fixture cannot fit MT2.
pub fn roads_tile_bytes() -> Vec<u8> {
    let mut builder = TileBuilder::new(roads_tile());
    builder.push(Class::Land.code(), rect_polygon(1, (0, 0, 65535, 65535)));
    for (class, feature) in scene() {
        builder.push(class.code(), feature);
    }
    builder.build().expect("roads fixture fits MT2")
}

fn road(id: u64, flags: u8, name: &str, points: &[(u16, u16)]) -> FeatureDraft {
    FeatureDraft {
        id,
        flags,
        rank: 0,
        name: name.to_string(),
        vertices: points.iter().map(|(x, y)| TileCoord(*x, *y)).collect(),
        holes: Vec::new(),
    }
}

fn scene() -> Vec<(Class, FeatureDraft)> {
    let mut out = Vec::new();
    out.extend(sharp_corner());
    out.extend(y_junction());
    out.push(roundabout());
    out.extend(dual_carriageway());
    out.push(chicane());
    out.extend(crossings());
    out
}

/// Two arms meeting at about 9°: far past any miter limit, so the join
/// has to bevel or it grows a spike across the tile.
fn sharp_corner() -> Vec<(Class, FeatureDraft)> {
    vec![(
        Class::RoadPrimary,
        road(10, 0, "", &[(5000, 20000), (22000, 12000), (6000, 16500)]),
    )]
}

/// Three arms at one node, as three features — the node is shared by
/// geometry, not by a topology the renderer knows about.
fn y_junction() -> Vec<(Class, FeatureDraft)> {
    vec![
        (Class::RoadTrunk, road(20, 0, "", &[(33000, 29000), (33000, 13000)])),
        (Class::RoadSecondary, road(21, 0, "", &[(33000, 13000), (25000, 5000)])),
        (Class::RoadSecondary, road(22, 0, "", &[(33000, 13000), (41000, 5000)])),
    ]
}

/// A closed ring: no ends to cap, and a seam that must not show.
fn roundabout() -> (Class, FeatureDraft) {
    let mut points: Vec<(u16, u16)> = (0..RING_SIDES)
        .map(|i| {
            let angle = std::f64::consts::TAU * i.to_f64().unwrap_or_default()
                / RING_SIDES.to_f64().unwrap_or(1.0);
            (
                (52000.0 + 8000.0 * angle.cos()).round().to_u16().unwrap_or(u16::MAX),
                (16000.0 + 8000.0 * angle.sin()).round().to_u16().unwrap_or(u16::MAX),
            )
        })
        .collect();
    points.push(points[0]);
    (Class::RoadResidential, road(30, 0, "", &points))
}

/// A motorway and its service road, close enough that the casings all
/// but touch: the pair that shows whether class order is respected.
fn dual_carriageway() -> Vec<(Class, FeatureDraft)> {
    vec![
        (Class::RoadMotorway, road(40, 0, "North Circular", &[(4000, 34000), (62000, 34000)])),
        (Class::RoadService, road(41, 0, "", &[(6000, 38000), (60000, 38000)])),
    ]
}

/// Two corners chosen so that each position of the card's miter-limit
/// knob changes the answer: 40° needs a miter of 2.9 and 75° one of
/// 1.6, so at 1.5 both bevel, at 2 only the sharper one, at 4 neither.
fn chicane() -> (Class, FeatureDraft) {
    (
        Class::RoadResidential,
        road(
            50, 0, "",
            &[(6000, 46000), (24000, 46000), (12510, 55645), (20703, 61380)],
        ),
    )
}

/// A street with a bridge over it and a tunnel under it.
fn crossings() -> Vec<(Class, FeatureDraft)> {
    vec![
        (Class::RoadResidential, road(60, 0, "", &[(30000, 58000), (63000, 46000)])),
        (Class::RoadSecondary, road(61, FLAG_BRIDGE, "", &[(36000, 43000), (48000, 62000)])),
        (Class::RoadSecondary, road(62, FLAG_TUNNEL, "", &[(52000, 43000), (62000, 62000)])),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::TileView;

    /// The scene must be bit-for-bit stable: it is the input of a
    /// golden screenshot, and a silent change there is a silent change
    /// to the picture the reviewer approved.
    // MT2 v5 (2026-08-18): building features gained a material byte, changing
    // tile bytes even for scenes that never set one explicitly.
    const GOLDEN_FNV1A: u64 = 0x3A3D_4D78_8D08_55C7;

    fn fnv1a(bytes: &[u8]) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in bytes {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
        }
        hash
    }

    fn features_of(bytes: &[u8], class: Class) -> Vec<(u8, Vec<TileCoord>)> {
        let tile = TileView::parse(bytes).expect("parses");
        let Some(section) = tile.section(class.code()) else {
            return Vec::new();
        };
        section
            .features()
            .map(|f| {
                let f = f.expect("feature decodes");
                (f.flags, f.vertices().collect::<Result<Vec<_>, _>>().expect("vertices"))
            })
            .collect()
    }

    fn interior_angle_deg(points: &[TileCoord], at: usize) -> f64 {
        let point = |i: usize| (f64::from(points[i].0), f64::from(points[i].1));
        let (bx, by) = point(at - 1);
        let (px, py) = point(at);
        let (nx, ny) = point(at + 1);
        let arriving = ((bx - px), (by - py));
        let leaving = ((nx - px), (ny - py));
        let dot = arriving.0 * leaving.0 + arriving.1 * leaving.1;
        let lengths = arriving.0.hypot(arriving.1) * leaving.0.hypot(leaving.1);
        (dot / lengths).clamp(-1.0, 1.0).acos().to_degrees()
    }

    #[test]
    fn the_scene_bytes_are_golden() {
        let hash = fnv1a(&roads_tile_bytes());
        assert_eq!(hash, GOLDEN_FNV1A, "scene changed: new hash {hash:#x}");
    }

    #[test]
    fn the_sharp_corner_really_is_sharper_than_fifteen_degrees() {
        let primary = features_of(&roads_tile_bytes(), Class::RoadPrimary);
        let (_, points) = primary.first().expect("the sharp corner");
        assert_eq!(points.len(), 3);
        let angle = interior_angle_deg(points, 1);
        assert!(angle < 15.0, "corner is {angle}°, which no longer bevels");
    }

    fn open_residential(points: usize) -> Vec<TileCoord> {
        let residential = features_of(&roads_tile_bytes(), Class::RoadResidential);
        residential
            .into_iter()
            .map(|(_, p)| p)
            .find(|p| p.len() == points && p.first() != p.last())
            .expect("an open residential road of that length")
    }

    #[test]
    fn each_chicane_corner_answers_a_different_miter_limit() {
        let points = open_residential(4);
        let miter_of = |at: usize| {
            let angle = interior_angle_deg(&points, at);
            1.0 / ((180.0 - angle) / 2.0).to_radians().cos()
        };
        // One corner sits between the card's middle and widest limit,
        // the other between its narrowest and middle: every position of
        // the knob then has something to change.
        let (sharper, gentler) = (miter_of(1), miter_of(2));
        assert!(sharper > 2.0 && sharper < 4.0, "sharper corner miters at {sharper}");
        assert!(gentler > 1.5 && gentler < 2.0, "gentler corner miters at {gentler}");
    }

    #[test]
    fn the_roundabout_is_a_closed_ring_of_gentle_corners() {
        let residential = features_of(&roads_tile_bytes(), Class::RoadResidential);
        let (_, ring) = residential
            .iter()
            .find(|(_, p)| p.first() == p.last())
            .expect("the roundabout");
        assert_eq!(ring.len(), RING_SIDES + 1);
        // Every corner of the ring must miter at the narrowest limit,
        // or the ring stops being the "all miters" case it is here for.
        for at in 1..ring.len() - 1 {
            let angle = interior_angle_deg(ring, at);
            let miter = 1.0 / ((180.0 - angle) / 2.0).to_radians().cos();
            assert!(miter < 1.5, "ring corner {at} miters at {miter}");
        }
    }

    #[test]
    fn a_bridge_and_a_tunnel_are_both_in_the_scene() {
        let secondary = features_of(&roads_tile_bytes(), Class::RoadSecondary);
        let flags: Vec<u8> = secondary.iter().map(|(f, _)| *f).collect();
        assert!(flags.contains(&FLAG_BRIDGE), "no bridge in the scene");
        assert!(flags.contains(&FLAG_TUNNEL), "no tunnel in the scene");
        assert!(flags.contains(&0), "no plain secondary to compare against");
    }

    #[test]
    fn the_camera_centre_falls_inside_the_scene_tile() {
        let centre = roads_centre();
        assert_eq!(locate(centre, 16).tile, roads_tile());
    }
}
