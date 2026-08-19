//! Roads as ribbons: the line bucket, its joins, and the pass order.
//!
//! A road travels as a centreline. The ribbon is unfolded in the vertex
//! shader from a normal and a width in screen pixels, so zoom and tilt
//! cost nothing and the mesh survives every camera move — v1 baked
//! metres into the mesh and grew a 48 km wide road. What cannot be
//! deferred to the shader is the corner: a join needs different
//! geometry depending on how sharp it is, so joins are decided here,
//! once, when the bucket is built.
//!
//! Vertex packing, 8 bytes: `pos i16×2`, `normal i8×2`, `linesofar u16`.
//! Two of those need a word. The grid is unsigned `0..65535` and `i16`
//! is not, so positions carry a −[`POS_BIAS`] offset that the shader
//! adds back. The normal is the whole extrusion vector — unit along a
//! straight run, up to the miter limit at a corner — scaled by
//! [`NORMAL_SCALE`]; the shader recovers its direction (the
//! anti-aliasing coordinate across the ribbon) and its length (the
//! offset) from that one pair of bytes.

use std::f64::consts::PI;

use maps2_style::{Class, FLAG_BRIDGE, FLAG_TUNNEL};
use maps2_tile::{SectionView, TileError, TileView};
use maps2_units::TileCoord;
use num_traits::ToPrimitive;

/// Packing scale of the normal. 31 rather than 63 because the extrusion
/// carries the miter length with it: `MITER_LIMIT_MAX · 31` still fits
/// in `i8`, and the direction stays accurate to under a tenth of a
/// pixel at the widest road we draw.
pub const NORMAL_SCALE: f32 = 31.0;

/// The longest miter the packed normal can express.
pub const MITER_LIMIT_MAX: f32 = 4.0;

/// Positions are stored biased by this, to fit the unsigned grid in `i16`.
pub const POS_BIAS: i32 = 32768;

/// `linesofar` counts grid units in steps of this many: a road far
/// longer than a tile diagonal still fits `u16`, and the step is under
/// two metres at z16.
pub const LINESOFAR_STEP: f32 = 16.0;

/// Triangles in the fan of one round cap.
pub const ROUND_CAP_SEGMENTS: u32 = 8;

/// Road classes bottom to top: a motorway draws over a service road.
pub const ROAD_ORDER: [Class; 8] = [
    // Boundaries go down first: a border is a reference mark under the
    // map's own furniture, never something a road disappears beneath.
    Class::Boundary,
    Class::RoadPath,
    Class::RoadService,
    Class::RoadResidential,
    Class::RoadSecondary,
    Class::RoadPrimary,
    Class::RoadTrunk,
    Class::RoadMotorway,
];

/// One vertex of a road ribbon, 8 bytes, as the shader reads it.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineVertex {
    pub x: i16,
    pub y: i16,
    pub nx: i8,
    pub ny: i8,
    pub linesofar: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LineOptions {
    pub miter_limit: f32,
    pub cap: Cap,
}

impl Default for LineOptions {
    fn default() -> Self {
        Self { miter_limit: 2.0, cap: Cap::Butt }
    }
}

/// How every corner of the bucket was resolved. A readout, not a
/// counter the frame pays for: it is fixed when the bucket is built.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct JoinCounts {
    pub miter: u32,
    pub bevel: u32,
}

/// Which storey a road runs on. Ground roads draw in class order, a
/// bridge draws over all of them, a tunnel under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum RoadLevel {
    Tunnel,
    Ground,
    Bridge,
}

pub const ROAD_LEVELS: [RoadLevel; 3] = [RoadLevel::Tunnel, RoadLevel::Ground, RoadLevel::Bridge];

impl RoadLevel {
    /// Bridge wins over tunnel: a feature carrying both is malformed
    /// data, and drawing it on top is the visible failure.
    #[must_use]
    pub fn of(flags: u8) -> Self {
        if flags & FLAG_BRIDGE != 0 {
            RoadLevel::Bridge
        } else if flags & FLAG_TUNNEL != 0 {
            RoadLevel::Tunnel
        } else {
            RoadLevel::Ground
        }
    }
}

/// Casings of a storey all draw before any of its fills, so the outline
/// of a junction is one shape rather than a seam through it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pass {
    Casing,
    Fill,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LineRange {
    pub class: Class,
    pub level: RoadLevel,
    pub first_index: u32,
    pub index_count: u32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct LineBucket {
    pub vertices: Vec<LineVertex>,
    pub indices: Vec<u32>,
    pub ranges: Vec<LineRange>,
    pub joins: JoinCounts,
}

/// One draw call of the road stack, in the order it happens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoadPass {
    pub level: RoadLevel,
    pub pass: Pass,
    pub class: Class,
}

/// Every road draw call of a frame, bottom to top.
#[must_use]
pub fn road_passes() -> Vec<RoadPass> {
    let mut out = Vec::with_capacity(ROAD_LEVELS.len() * 2 * ROAD_ORDER.len());
    for level in ROAD_LEVELS {
        for pass in [Pass::Casing, Pass::Fill] {
            out.extend(ROAD_ORDER.map(|class| RoadPass { level, pass, class }));
        }
    }
    out
}

/// How far the outer corner of a miter reaches, in half-widths:
/// `1/cos(θ/2)` of the angle the line turns through. Infinite when the
/// line doubles back exactly.
#[must_use]
pub fn miter_length(prev_dir: [f64; 2], next_dir: [f64; 2]) -> f64 {
    let (nx, ny) = miter_vector(left_normal(prev_dir), left_normal(next_dir));
    nx.hypot(ny)
}

fn left_normal(dir: [f64; 2]) -> [f64; 2] {
    [-dir[1], dir[0]]
}

fn miter_vector(a: [f64; 2], b: [f64; 2]) -> (f64, f64) {
    let denominator = 1.0 + a[0] * b[0] + a[1] * b[1];
    if denominator <= 1e-9 {
        return (f64::INFINITY, f64::INFINITY);
    }
    ((a[0] + b[0]) / denominator, (a[1] + b[1]) / denominator)
}

fn pack(point: [f64; 2], normal: [f64; 2], distance: f64) -> LineVertex {
    let coordinate = |value: f64| {
        let biased = value.round() - f64::from(POS_BIAS);
        biased.clamp(f64::from(i16::MIN), f64::from(i16::MAX)).to_i16().unwrap_or_default()
    };
    let component = |value: f64| {
        (value * f64::from(NORMAL_SCALE))
            .round()
            .clamp(f64::from(i8::MIN + 1), f64::from(i8::MAX)).to_i8().unwrap_or_default()
    };
    LineVertex {
        x: coordinate(point[0]),
        y: coordinate(point[1]),
        nx: component(normal[0]),
        ny: component(normal[1]),
        linesofar: (distance / f64::from(LINESOFAR_STEP)).round().clamp(0.0, 65535.0).to_u16().unwrap_or_default(),
    }
}

/// A polyline prepared for extrusion: duplicates dropped, closure
/// detected, directions and running length precomputed.
struct Shape {
    points: Vec<[f64; 2]>,
    directions: Vec<[f64; 2]>,
    distances: Vec<f64>,
    closed: bool,
}

impl Shape {
    fn new(points: &[TileCoord]) -> Option<Self> {
        let mut cleaned: Vec<[f64; 2]> = Vec::with_capacity(points.len());
        for point in points {
            let next = [f64::from(point.0), f64::from(point.1)];
            if cleaned.last() != Some(&next) {
                cleaned.push(next);
            }
        }
        let closed = cleaned.len() >= 4 && cleaned.first() == cleaned.last();
        if closed {
            cleaned.pop();
        }
        if cleaned.len() < 2 {
            return None;
        }
        Some(Self::measured(cleaned, closed))
    }

    fn measured(points: Vec<[f64; 2]>, closed: bool) -> Self {
        let segments = if closed { points.len() } else { points.len() - 1 };
        let mut directions = Vec::with_capacity(segments);
        let mut distances = vec![0.0; points.len()];
        for i in 0..segments {
            let from = points[i];
            let to = points[(i + 1) % points.len()];
            let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
            let length = dx.hypot(dy).max(f64::MIN_POSITIVE);
            directions.push([dx / length, dy / length]);
            if i + 1 < points.len() {
                distances[i + 1] = distances[i] + length;
            }
        }
        Self { points, directions, distances, closed }
    }

    fn segments(&self) -> usize {
        self.directions.len()
    }

    /// Directions meeting at vertex `i`: the segment arriving and the
    /// one leaving. Wraps for a closed ring.
    fn corner(&self, i: usize) -> ([f64; 2], [f64; 2]) {
        let segments = self.segments();
        (self.directions[(i + segments - 1) % segments], self.directions[i % segments])
    }
}

impl LineBucket {
    /// Extrudes one centreline into the bucket. A ring — first point
    /// repeated as the last — is joined all the way round and gets no
    /// caps; `linesofar` restarts at the seam, which only a dashed ring
    /// could notice.
    pub fn push_polyline(&mut self, points: &[TileCoord], options: LineOptions) {
        let Some(shape) = Shape::new(points) else {
            return;
        };
        if shape.closed {
            self.extrude_ring(&shape, options);
        } else {
            self.extrude_open(&shape, options);
        }
    }

    #[must_use]
    pub fn range(&self, class: Class, level: RoadLevel) -> Option<LineRange> {
        self.ranges.iter().copied().find(|r| r.class == class && r.level == level)
    }

    fn extrude_ring(&mut self, shape: &Shape, options: LineOptions) {
        let count = shape.points.len();
        let (seam, mut previous) = self.join(shape, 0, options);
        for i in 1..count {
            let (incoming, outgoing) = self.join(shape, i, options);
            self.quad(previous, incoming);
            previous = outgoing;
        }
        self.quad(previous, seam);
    }

    fn extrude_open(&mut self, shape: &Shape, options: LineOptions) {
        let count = shape.points.len();
        let first_normal = left_normal(shape.directions[0]);
        let last_normal = left_normal(shape.directions[shape.segments() - 1]);
        let start = self.pair(shape.points[0], first_normal, 0.0);
        let mut previous = start;
        for i in 1..count - 1 {
            let (incoming, outgoing) = self.join(shape, i, options);
            self.quad(previous, incoming);
            previous = outgoing;
        }
        let end = self.pair(shape.points[count - 1], last_normal, shape.distances[count - 1]);
        self.quad(previous, end);
        if options.cap == Cap::Round {
            let back = shape.directions[0];
            self.round_cap(shape.points[0], first_normal, [-back[0], -back[1]], 0.0, start);
            let forward = shape.directions[shape.segments() - 1];
            let tail = shape.points[count - 1];
            self.round_cap(tail, last_normal, forward, shape.distances[count - 1], end);
        }
    }

    /// The pair a segment arrives at, and the pair the next one leaves
    /// from. They are the same pair for a miter; a bevel keeps them
    /// apart and fills the gap between with one triangle.
    fn join(&mut self, shape: &Shape, i: usize, options: LineOptions) -> (u32, u32) {
        let (arriving, leaving) = shape.corner(i);
        let (before, after) = (left_normal(arriving), left_normal(leaving));
        let (mx, my) = miter_vector(before, after);
        let point = shape.points[i];
        let distance = shape.distances[i];
        if mx.hypot(my) <= f64::from(options.miter_limit.min(MITER_LIMIT_MAX)) {
            self.joins.miter += 1;
            let pair = self.pair(point, [mx, my], distance);
            return (pair, pair);
        }
        self.joins.bevel += 1;
        let incoming = self.pair(point, before, distance);
        let outgoing = self.pair(point, after, distance);
        let centre = self.vertex(point, [0.0, 0.0], distance);
        let turns_left = arriving[0] * leaving[1] - arriving[1] * leaving[0] > 0.0;
        let outer = u32::from(turns_left);
        self.triangle(centre, incoming + outer, outgoing + outer);
        (incoming, outgoing)
    }

    fn round_cap(&mut self, point: [f64; 2], normal: [f64; 2], out: [f64; 2], at: f64, pair: u32) {
        let centre = self.vertex(point, [0.0, 0.0], at);
        let mut previous = pair;
        for step in 1..ROUND_CAP_SEGMENTS {
            let angle = PI * f64::from(step) / f64::from(ROUND_CAP_SEGMENTS);
            let (sin, cos) = angle.sin_cos();
            let rim = [normal[0] * cos + out[0] * sin, normal[1] * cos + out[1] * sin];
            let vertex = self.vertex(point, rim, at);
            self.triangle(centre, previous, vertex);
            previous = vertex;
        }
        self.triangle(centre, previous, pair + 1);
    }

    /// Both sides of the ribbon at one point; the index returned is the
    /// left vertex, the right one follows it.
    fn pair(&mut self, point: [f64; 2], normal: [f64; 2], distance: f64) -> u32 {
        let left = self.vertex(point, normal, distance);
        self.vertex(point, [-normal[0], -normal[1]], distance);
        left
    }

    fn vertex(&mut self, point: [f64; 2], normal: [f64; 2], distance: f64) -> u32 {
        let index = u32::try_from(self.vertices.len()).unwrap_or(u32::MAX);
        self.vertices.push(pack(point, normal, distance));
        index
    }

    fn quad(&mut self, from: u32, to: u32) {
        self.triangle(from, from + 1, to + 1);
        self.triangle(from, to + 1, to);
    }

    fn triangle(&mut self, a: u32, b: u32, c: u32) {
        self.indices.extend([a, b, c]);
    }
}

/// Builds every road ribbon of a tile, one range per class and storey.
///
/// # Errors
///
/// Returns [`TileError`] when a road feature cannot be decoded.
pub fn build_line_bucket(tile: &TileView, options: LineOptions) -> Result<LineBucket, TileError> {
    let mut bucket = LineBucket::default();
    for class in ROAD_ORDER {
        let Some(section) = tile.section(class.code()) else {
            continue;
        };
        for level in ROAD_LEVELS {
            let first_index = u32::try_from(bucket.indices.len()).map_err(|_| TileError::TooLarge)?;
            append_level(&mut bucket, &section, level, options)?;
            let index_count = u32::try_from(bucket.indices.len()).map_err(|_| TileError::TooLarge)? - first_index;
            if index_count > 0 {
                bucket.ranges.push(LineRange { class, level, first_index, index_count });
            }
        }
    }
    Ok(bucket)
}

fn append_level(
    bucket: &mut LineBucket,
    section: &SectionView<'_>,
    level: RoadLevel,
    options: LineOptions,
) -> Result<(), TileError> {
    for feature in section.features() {
        let feature = feature?;
        if RoadLevel::of(feature.flags) != level {
            continue;
        }
        let points = feature.vertices().collect::<Result<Vec<_>, _>>()?;
        bucket.push_polyline(&points, options);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn corner(interior_degrees: f64) -> Vec<TileCoord> {
        // Two arms of 12000 grid units meeting at the given interior
        // angle: the smaller the angle, the sharper the corner.
        let turn = (180.0 - interior_degrees).to_radians();
        let (sin, cos) = turn.sin_cos();
        let apex = [32000.0, 32000.0];
        let arm = 12000.0;
        let quantise = |p: [f64; 2]| TileCoord(
            p[0].round().to_u16().expect("corner x fits tile coordinates"),
            p[1].round().to_u16().expect("corner y fits tile coordinates"),
        );
        vec![
            quantise([apex[0] - arm, apex[1]]),
            quantise(apex),
            quantise([apex[0] + arm * cos, apex[1] + arm * sin]),
        ]
    }

    fn extruded(points: &[TileCoord], options: LineOptions) -> LineBucket {
        let mut bucket = LineBucket::default();
        bucket.push_polyline(points, options);
        bucket
    }

    fn ring(sides: usize) -> Vec<TileCoord> {
        let mut points: Vec<TileCoord> = (0..sides)
            .map(|i| {
                let angle = 2.0 * PI * i.to_f64().expect("ring index fits f64") / sides.to_f64().expect("side count fits f64");
                TileCoord(
                    (32000.0 + 9000.0 * angle.cos()).round().to_u16().expect("ring x fits tile coordinates"),
                    (32000.0 + 9000.0 * angle.sin()).round().to_u16().expect("ring y fits tile coordinates"),
                )
            })
            .collect();
        points.push(points[0]);
        points
    }

    #[test]
    fn a_right_angle_miters_to_the_square_root_of_two() {
        assert!((miter_length([1.0, 0.0], [0.0, 1.0]) - 2.0_f64.sqrt()).abs() < 1e-12);
        assert!((miter_length([1.0, 0.0], [1.0, 0.0]) - 1.0).abs() < 1e-12);
        // A line doubling back on itself has no miter at all.
        assert!(miter_length([1.0, 0.0], [-1.0, 0.0]).is_infinite());
    }

    #[test]
    fn a_corner_sharper_than_the_limit_becomes_a_bevel() {
        let limit = LineOptions::default();
        assert_eq!(extruded(&corner(10.0), limit).joins, JoinCounts { miter: 0, bevel: 1 });
        assert_eq!(extruded(&corner(150.0), limit).joins, JoinCounts { miter: 1, bevel: 0 });
        // 40° needs a miter of 2.9: bevelled at the default limit,
        // mitred once the card raises it.
        let wide = LineOptions { miter_limit: 4.0, ..limit };
        assert_eq!(extruded(&corner(40.0), limit).joins, JoinCounts { miter: 0, bevel: 1 });
        assert_eq!(extruded(&corner(40.0), wide).joins, JoinCounts { miter: 1, bevel: 0 });
    }

    #[test]
    fn a_bevel_costs_three_vertices_more_than_a_miter() {
        let options = LineOptions::default();
        let mitred = extruded(&corner(150.0), options);
        let bevelled = extruded(&corner(10.0), options);
        // Both ends plus the join: two vertices per pair.
        assert_eq!(mitred.vertices.len(), 6);
        // A bevel keeps the two pairs apart and adds the centre of the
        // triangle that fills the gap.
        assert_eq!(bevelled.vertices.len(), 9);
        assert_eq!(bevelled.indices.len(), mitred.indices.len() + 3);
    }

    #[test]
    fn linesofar_never_goes_backwards_along_a_line() {
        let points = vec![
            TileCoord(1000, 1000),
            TileCoord(9000, 3000),
            TileCoord(9200, 30000),
            TileCoord(40000, 41000),
        ];
        let bucket = extruded(&points, LineOptions::default());
        assert!(bucket.vertices.windows(2).all(|w| w[0].linesofar <= w[1].linesofar));
        let total = 8246.0 + 27004.0 + 32723.0;
        let last = f64::from(bucket.vertices.last().expect("vertices").linesofar);
        assert!(
            (last * f64::from(LINESOFAR_STEP) - total).abs() / total < 0.01,
            "linesofar ended at {last} steps, expected about {}",
            total / f64::from(LINESOFAR_STEP),
        );
    }

    #[test]
    fn a_ring_is_joined_all_the_way_round_and_an_open_line_is_not() {
        let options = LineOptions::default();
        let closed = extruded(&ring(24), options);
        let open = extruded(&ring(24)[..24], options);
        assert_eq!(closed.joins.miter + closed.joins.bevel, 24);
        assert_eq!(open.joins.miter + open.joins.bevel, 22);
        assert_eq!(closed.joins.bevel, 0, "a wide ring only needs miters");
    }

    #[test]
    fn round_caps_add_a_fan_at_each_end() {
        let straight = vec![TileCoord(10000, 10000), TileCoord(40000, 10000)];
        let butt = extruded(&straight, LineOptions::default());
        let round =
            extruded(&straight, LineOptions { cap: Cap::Round, ..LineOptions::default() });
        let fan = ROUND_CAP_SEGMENTS as usize;
        assert_eq!(round.vertices.len(), butt.vertices.len() + 2 * fan);
        assert_eq!(round.indices.len(), butt.indices.len() + 2 * fan * 3);
    }

    #[test]
    fn the_packed_normal_survives_the_widest_miter() {
        let bucket = extruded(
            &corner(30.0),
            LineOptions { miter_limit: MITER_LIMIT_MAX, ..LineOptions::default() },
        );
        let longest = bucket
            .vertices
            .iter()
            .map(|v| f64::from(v.nx).hypot(f64::from(v.ny)) / f64::from(NORMAL_SCALE))
            .fold(0.0_f64, f64::max);
        let expected = 1.0 / (150.0_f64 / 2.0).to_radians().cos();
        assert!((longest - expected).abs() < 0.05, "packed {longest}, wanted {expected}");
        assert!(bucket.vertices.iter().all(|v| v.nx != i8::MIN && v.ny != i8::MIN));
    }

    #[test]
    fn positions_come_back_from_the_bias_unchanged() {
        let points = vec![TileCoord(0, 65535), TileCoord(65535, 0)];
        let bucket = extruded(&points, LineOptions::default());
        let restored: Vec<(i32, i32)> = bucket
            .vertices
            .iter()
            .map(|v| (i32::from(v.x) + POS_BIAS, i32::from(v.y) + POS_BIAS))
            .collect();
        assert!(restored.contains(&(0, 65535)));
        assert!(restored.contains(&(65535, 0)));
    }

    #[test]
    fn every_index_points_at_a_vertex_that_exists() {
        let bucket = extruded(&ring(9), LineOptions { cap: Cap::Round, miter_limit: 2.0 });
        assert_eq!(bucket.indices.len() % 3, 0);
        assert!(bucket.indices.iter().all(|i| (*i as usize) < bucket.vertices.len()));
    }

    #[test]
    fn casings_draw_under_fills_and_a_bridge_over_both() {
        let passes = road_passes();
        let at = |level, pass, class| {
            passes
                .iter()
                .position(|p| p.level == level && p.pass == pass && p.class == class)
                .expect("pass listed")
        };
        let ground_fill = |class| at(RoadLevel::Ground, Pass::Fill, class);
        // A motorway draws over a service road, and every casing of the
        // storey is down before the first fill.
        assert!(ground_fill(Class::RoadMotorway) > ground_fill(Class::RoadService));
        assert!(
            at(RoadLevel::Ground, Pass::Casing, Class::RoadMotorway)
                < ground_fill(Class::RoadPath)
        );
        assert!(at(RoadLevel::Tunnel, Pass::Fill, Class::RoadMotorway) < ground_fill(Class::RoadPath));
        assert!(
            at(RoadLevel::Bridge, Pass::Casing, Class::RoadPath)
                > ground_fill(Class::RoadMotorway)
        );
    }

    #[test]
    fn the_pathology_scene_splits_into_ranges_by_class_and_storey() {
        let bytes = maps2_fixtures::roads_tile_bytes();
        let tile = TileView::parse(&bytes).expect("fixture parses");
        let bucket = build_line_bucket(&tile, LineOptions::default()).expect("builds");

        assert!(bucket.range(Class::RoadMotorway, RoadLevel::Ground).is_some());
        assert!(bucket.range(Class::RoadSecondary, RoadLevel::Bridge).is_some());
        assert!(bucket.range(Class::RoadSecondary, RoadLevel::Tunnel).is_some());
        assert!(bucket.range(Class::RoadMotorway, RoadLevel::Bridge).is_none());

        // The sharp corner and the chicane bevel at the default limit;
        // the roundabout miters all the way round.
        assert!(bucket.joins.bevel >= 2, "joins were {:?}", bucket.joins);
        assert!(bucket.joins.miter >= RING_SIDES_IN_FIXTURE);

        let ranges_total: u32 = bucket.ranges.iter().map(|r| r.index_count).sum();
        assert_eq!(ranges_total as usize, bucket.indices.len());
        assert!(bucket.indices.iter().all(|i| (*i as usize) < bucket.vertices.len()));
    }

    /// The roundabout's corners, each of which must miter.
    const RING_SIDES_IN_FIXTURE: u32 = 24;

    #[test]
    fn raising_the_miter_limit_turns_bevels_into_miters() {
        let bytes = maps2_fixtures::roads_tile_bytes();
        let tile = TileView::parse(&bytes).expect("fixture parses");
        let at = |limit| {
            let options = LineOptions { miter_limit: limit, ..LineOptions::default() };
            build_line_bucket(&tile, options).expect("builds").joins
        };
        let (tight, loose) = (at(2.0), at(MITER_LIMIT_MAX));
        assert!(loose.bevel < tight.bevel, "{tight:?} vs {loose:?}");
        // The corner under 15° is past every limit the card offers.
        assert!(loose.bevel > 0, "the sharp corner must still bevel");
        assert_eq!(tight.miter + tight.bevel, loose.miter + loose.bevel);
    }

    #[test]
    fn a_feature_travels_on_the_storey_its_flags_say() {
        assert_eq!(RoadLevel::of(0), RoadLevel::Ground);
        assert_eq!(RoadLevel::of(FLAG_BRIDGE), RoadLevel::Bridge);
        assert_eq!(RoadLevel::of(FLAG_TUNNEL), RoadLevel::Tunnel);
        assert_eq!(RoadLevel::of(maps2_style::FLAG_ONE_WAY), RoadLevel::Ground);
    }
}
