//! Which labels get the screen.
//!
//! The maximum set of non-overlapping labels is NP-hard, so nobody
//! computes it: production renderers order the candidates by importance
//! and give each the first free spot, and so does this. The order is
//! `(rank, id)` — the id only so that two equally important labels
//! resolve the same way every frame, which is what makes a frame
//! reproducible at all.
//!
//! The other half is the budget. The scarce resource is not the number
//! of features but **screen area**: one "London" costs what five small
//! POI cost. So placement stops when the placed boxes have eaten their
//! share of the viewport, and everything past that point is rejected
//! for budget rather than for collision.

use std::collections::HashMap;

use num_traits::ToPrimitive;

/// Side of the screen index cell, in pixels. Collision tests only look
/// at the cells a box touches, so the cost is local to the box.
pub const GRID_CELL_PX: f32 = 64.0;

/// Candidates whose box lies further than this outside the viewport are
/// dropped. The margin exists so a label does not blink on and off as
/// its anchor crosses the edge.
pub const LABEL_MARGIN_PX: f32 = 96.0;

/// An axis-aligned screen rectangle, y down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl ScreenBox {
    /// Touching edges do not overlap: two labels may sit flush.
    #[must_use]
    pub fn intersects(self, other: Self) -> bool {
        self.x0 < other.x1 && other.x0 < self.x1 && self.y0 < other.y1 && other.y0 < self.y1
    }

    #[must_use]
    pub fn area(self) -> f32 {
        (self.x1 - self.x0).max(0.0) * (self.y1 - self.y0).max(0.0)
    }

    #[must_use]
    pub fn clipped_to(self, width: f32, height: f32) -> Self {
        Self {
            x0: self.x0.clamp(0.0, width),
            y0: self.y0.clamp(0.0, height),
            x1: self.x1.clamp(0.0, width),
            y1: self.y1.clamp(0.0, height),
        }
    }
}

/// A label asking for screen space. `id` is the feature's identity, not
/// the tile's copy of it: the same shop seen from two tiles arrives
/// twice with one id and is placed once.
#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub id: u64,
    pub rank: u8,
    pub text: String,
    /// Where the label wants to sit, already projected.
    pub anchor: (f32, f32),
    pub size: (f32, f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLabel {
    pub id: u64,
    pub rank: u8,
    pub text: String,
    pub bounds: ScreenBox,
    /// Baseline start of the line, in screen pixels.
    pub pen: (f32, f32),
}

/// Why a candidate did not make the frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rejection {
    /// Blocked by an already placed, better ranked label.
    Collision { by: u64 },
    /// The screen budget was spent before this label's turn.
    Budget,
    /// Too far outside the viewport to matter.
    OffScreen,
    /// Another copy of the same feature, from a neighbouring tile.
    Duplicate,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RejectedLabel {
    pub id: u64,
    pub rank: u8,
    pub bounds: ScreenBox,
    pub reason: Rejection,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    pub placed: Vec<PlacedLabel>,
    pub rejected: Vec<RejectedLabel>,
    /// Share of the viewport the placed boxes cover, 0..1.
    pub occupancy: f32,
}

/// The frame's answer: who is on screen, who is not, and why.
///
/// `budget` is the share of the viewport labels may cover, 0..1. When
/// the next label in rank order would spend more than what is left, the
/// frame stops choosing: that label and everything below it is rejected
/// for budget. Stopping rather than skipping ahead is what keeps the
/// promise that nothing on screen is less important than what was left
/// off it.
#[must_use]
pub fn place(candidates: &[Candidate], viewport: (f32, f32), budget: f32) -> Placement {
    let screen = viewport.0 * viewport.1;
    let mut state = Frame {
        grid: Grid::default(),
        placed: Vec::new(),
        rejected: Vec::new(),
        used: 0.0,
        seen: Vec::new(),
    };
    let mut spent = false;
    for candidate in rank_order(candidates) {
        let bounds = bounds_of(candidate);
        if spent {
            state.reject(candidate, bounds, Rejection::Budget);
            continue;
        }
        spent = state.consider(candidate, bounds, viewport, budget * screen);
    }
    Placement {
        placed: state.placed,
        rejected: state.rejected,
        occupancy: if screen > 0.0 { state.used / screen } else { 0.0 },
    }
}

/// `(rank, id)`: importance first, identity only to break ties. Without
/// the tie-break two equally ranked labels would swap places between
/// frames and the map would flicker.
fn rank_order(candidates: &[Candidate]) -> Vec<&Candidate> {
    let mut ordered: Vec<&Candidate> = candidates.iter().collect();
    ordered.sort_by_key(|c| (c.rank, c.id));
    ordered
}

/// The label's box, centred on its anchor.
fn bounds_of(candidate: &Candidate) -> ScreenBox {
    let (half_w, half_h) = (candidate.size.0 / 2.0, candidate.size.1 / 2.0);
    ScreenBox {
        x0: candidate.anchor.0 - half_w,
        y0: candidate.anchor.1 - half_h,
        x1: candidate.anchor.0 + half_w,
        y1: candidate.anchor.1 + half_h,
    }
}

struct Frame {
    grid: Grid,
    placed: Vec<PlacedLabel>,
    rejected: Vec<RejectedLabel>,
    used: f32,
    seen: Vec<u64>,
}

impl Frame {
    fn reject(&mut self, candidate: &Candidate, bounds: ScreenBox, reason: Rejection) {
        self.rejected.push(RejectedLabel {
            id: candidate.id,
            rank: candidate.rank,
            bounds,
            reason,
        });
    }

    /// Returns true when this candidate exhausted the budget.
    fn consider(
        &mut self,
        candidate: &Candidate,
        bounds: ScreenBox,
        viewport: (f32, f32),
        allowance: f32,
    ) -> bool {
        if !within_margin(bounds, viewport) {
            self.reject(candidate, bounds, Rejection::OffScreen);
            return false;
        }
        if self.seen.contains(&candidate.id) {
            self.reject(candidate, bounds, Rejection::Duplicate);
            return false;
        }
        self.seen.push(candidate.id);
        if let Some(by) = self.grid.first_hit(bounds, &self.placed) {
            self.reject(candidate, bounds, Rejection::Collision { by });
            return false;
        }
        let area = bounds.clipped_to(viewport.0, viewport.1).area();
        if self.used + area > allowance {
            self.reject(candidate, bounds, Rejection::Budget);
            return true;
        }
        self.grid.insert(bounds, self.placed.len());
        self.used += area;
        self.placed.push(PlacedLabel {
            id: candidate.id,
            rank: candidate.rank,
            text: candidate.text.clone(),
            bounds,
            pen: (bounds.x0, bounds.y1),
        });
        false
    }
}

fn within_margin(bounds: ScreenBox, viewport: (f32, f32)) -> bool {
    bounds.x1 > -LABEL_MARGIN_PX
        && bounds.y1 > -LABEL_MARGIN_PX
        && bounds.x0 < viewport.0 + LABEL_MARGIN_PX
        && bounds.y0 < viewport.1 + LABEL_MARGIN_PX
}

/// A screen-space bucket index. It changes what a collision test costs,
/// never what it answers: every box in every cell the query touches is
/// still compared exactly.
#[derive(Default)]
struct Grid {
    cells: HashMap<(i32, i32), Vec<usize>>,
}

impl Grid {
    fn span(bounds: ScreenBox) -> (i32, i32, i32, i32) {
        let cell = |v: f32| (v / GRID_CELL_PX).floor().to_i32().unwrap_or_default();
        (cell(bounds.x0), cell(bounds.y0), cell(bounds.x1), cell(bounds.y1))
    }

    fn keys(bounds: ScreenBox) -> impl Iterator<Item = (i32, i32)> {
        let (x0, y0, x1, y1) = Self::span(bounds);
        (y0..=y1).flat_map(move |y| (x0..=x1).map(move |x| (x, y)))
    }

    fn insert(&mut self, bounds: ScreenBox, at: usize) {
        for key in Self::keys(bounds) {
            self.cells.entry(key).or_default().push(at);
        }
    }

    /// The best-ranked label already on screen that this box hits.
    /// Placement order is rank order, so the smallest index wins.
    fn first_hit(&self, bounds: ScreenBox, placed: &[PlacedLabel]) -> Option<u64> {
        Self::keys(bounds)
            .filter_map(|key| self.cells.get(&key))
            .flatten()
            .filter(|at| placed[**at].bounds.intersects(bounds))
            .min()
            .map(|at| placed[*at].id)
    }
}

#[cfg(test)]
#[expect(clippy::float_cmp, reason = "areas here are exact sums of exact inputs")]
mod tests {
    use super::*;

    const VIEWPORT: (f32, f32) = (800.0, 600.0);

    /// A deterministic pseudo-random field of labels, sized like real
    /// ones and dense enough that most of them cannot fit.
    fn crowd(count: u64, spread: f32) -> Vec<Candidate> {
        let mut out = Vec::new();
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        for i in 0..count {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let a = ((seed >> 33) % 100_000) as f32 / 100_000.0;
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let b = ((seed >> 33) % 100_000) as f32 / 100_000.0;
            out.push(Candidate {
                id: i,
                // Heavy tail: a few important ones, many unimportant.
                rank: (i % 10) as u8,
                text: format!("label {i}"),
                anchor: (a * spread - (spread - VIEWPORT.0) / 2.0, b * spread - (spread - VIEWPORT.1) / 2.0),
                size: (40.0 + (i % 7) as f32 * 12.0, 14.0),
            });
        }
        out
    }

    fn shifted(candidates: &[Candidate], dx: f32) -> Vec<Candidate> {
        candidates
            .iter()
            .map(|c| Candidate { anchor: (c.anchor.0 + dx, c.anchor.1), ..c.clone() })
            .collect()
    }

    fn ids(placement: &Placement) -> Vec<u64> {
        placement.placed.iter().map(|p| p.id).collect()
    }

    #[test]
    fn no_two_placed_boxes_ever_overlap() {
        let placement = place(&crowd(600, 900.0), VIEWPORT, 1.0);
        assert!(!placement.placed.is_empty(), "nothing was placed at all");
        for (i, a) in placement.placed.iter().enumerate() {
            for b in &placement.placed[i + 1..] {
                assert!(
                    !a.bounds.intersects(b.bounds),
                    "{} and {} overlap: {:?} {:?}",
                    a.id,
                    b.id,
                    a.bounds,
                    b.bounds,
                );
            }
        }
    }

    #[test]
    fn the_same_frame_is_the_same_whatever_order_the_tiles_arrived_in() {
        let candidates = crowd(600, 900.0);
        let mut reversed = candidates.clone();
        reversed.reverse();
        let straight = place(&candidates, VIEWPORT, 1.0);
        let backwards = place(&reversed, VIEWPORT, 1.0);
        assert_eq!(ids(&straight), ids(&backwards));
        assert_eq!(straight.occupancy, backwards.occupancy);
        assert_eq!(place(&candidates, VIEWPORT, 1.0), straight, "not reproducible");
    }

    #[test]
    fn a_feature_seen_from_two_tiles_is_placed_once() {
        let one = Candidate {
            id: 77,
            rank: 1,
            text: "Ealing Broadway".into(),
            anchor: (400.0, 300.0),
            size: (120.0, 14.0),
        };
        // The neighbouring tile's copy: same feature, same place, and
        // its own rounding of the anchor.
        let other = Candidate { anchor: (400.4, 300.2), ..one.clone() };
        let placement = place(&[one, other], VIEWPORT, 1.0);
        assert_eq!(placement.placed.len(), 1);
        assert_eq!(placement.rejected.len(), 1);
        assert_eq!(placement.rejected[0].reason, Rejection::Duplicate);
    }

    #[test]
    fn nothing_is_rejected_for_a_label_that_matters_less() {
        let placement = place(&crowd(600, 900.0), VIEWPORT, 1.0);
        let key = |id: u64, rank: u8| (rank, id);
        for rejected in &placement.rejected {
            let Rejection::Collision { by } = rejected.reason else {
                continue;
            };
            let blocker = placement
                .placed
                .iter()
                .find(|p| p.id == by)
                .expect("the blocker is on screen");
            assert!(
                key(blocker.id, blocker.rank) < key(rejected.id, rejected.rank),
                "{} (rank {}) lost to worse {} (rank {})",
                rejected.id,
                rejected.rank,
                blocker.id,
                blocker.rank,
            );
        }
    }

    /// The roadmap's stability contract: panning a few pixels legally
    /// changes the set — but only at the edges, never wholesale.
    #[test]
    fn a_shift_of_up_to_twenty_pixels_reshuffles_under_a_tenth_of_the_labels() {
        let candidates = crowd(900, 1400.0);
        let before = place(&candidates, VIEWPORT, 1.0);
        let baseline: std::collections::HashSet<u64> = ids(&before).into_iter().collect();
        for dx in [1.0, 5.0, 12.0, 20.0] {
            let after = place(&shifted(&candidates, dx), VIEWPORT, 1.0);
            let now: std::collections::HashSet<u64> = ids(&after).into_iter().collect();
            let churn = baseline.symmetric_difference(&now).count();
            let share = churn as f32 / baseline.len() as f32;
            assert!(share < 0.10, "{dx} px moved {share:.3} of the labels");
        }
    }

    #[test]
    fn occupancy_is_the_placed_area_and_never_passes_the_budget() {
        for budget in [0.02_f32, 0.05, 0.12, 0.3] {
            let placement = place(&crowd(900, 900.0), VIEWPORT, budget);
            let summed: f32 = placement
                .placed
                .iter()
                .map(|p| p.bounds.clipped_to(VIEWPORT.0, VIEWPORT.1).area())
                .sum();
            let expected = summed / (VIEWPORT.0 * VIEWPORT.1);
            assert!((placement.occupancy - expected).abs() < 1e-4);
            assert!(placement.occupancy <= budget, "{} over budget {budget}", placement.occupancy);
        }
    }

    #[test]
    fn a_tighter_budget_keeps_a_subset_of_the_looser_one() {
        let candidates = crowd(900, 900.0);
        let loose: Vec<u64> = ids(&place(&candidates, VIEWPORT, 0.30));
        let tight = place(&candidates, VIEWPORT, 0.06);
        assert!(!tight.placed.is_empty());
        assert!(tight.placed.len() < loose.len(), "the budget changed nothing");
        for id in ids(&tight) {
            assert!(loose.contains(&id), "{id} appeared only under the tighter budget");
        }
        assert!(
            tight.rejected.iter().any(|r| r.reason == Rejection::Budget),
            "nothing was rejected for budget",
        );
    }

    #[test]
    fn labels_far_outside_the_viewport_are_dropped_before_anything_else() {
        let far = Candidate {
            id: 5,
            rank: 0,
            text: "elsewhere".into(),
            anchor: (-4000.0, 300.0),
            size: (60.0, 14.0),
        };
        let placement = place(&[far], VIEWPORT, 1.0);
        assert!(placement.placed.is_empty());
        assert_eq!(placement.rejected[0].reason, Rejection::OffScreen);
        assert_eq!(placement.occupancy, 0.0);
    }

    #[test]
    fn the_screen_index_finds_the_same_collisions_a_full_scan_would() {
        // The 64 px grid is an optimisation; it must not be a second
        // definition of "overlap". A crowd wider than one cell proves it.
        let candidates = crowd(300, 700.0);
        let placement = place(&candidates, VIEWPORT, 1.0);
        let mut by_id: HashMap<u64, ScreenBox> = HashMap::new();
        for placed in &placement.placed {
            by_id.insert(placed.id, placed.bounds);
        }
        for rejected in &placement.rejected {
            if matches!(rejected.reason, Rejection::Collision { .. }) {
                assert!(
                    by_id.values().any(|b| b.intersects(rejected.bounds)),
                    "{} was rejected for a collision with nothing",
                    rejected.id,
                );
            }
        }
    }
}
