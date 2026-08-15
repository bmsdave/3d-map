//! Gestures → camera patches. Every number a gesture produces is decided
//! here, natively tested; the wasm layer only forwards DOM events and the
//! clock, so nothing about input needs a browser to be checked.
//!
//! Two invariants hold all of it together: a drag moves the ground with
//! the pointer, and a zoom leaves the ground under the cursor where it
//! is. Both are stated against [`maps2_camera::project`] in the tests
//! below rather than against a formula, so a change of projection has to
//! keep them true.

use maps2_camera::{Camera, CameraPatch, project, unproject};
use maps2_units::{Lonlat, ScreenPoint};
use num_traits::ToPrimitive;

/// Pointer position in the canvas' own pixels, origin top-left.
pub type Point = (f64, f64);
/// Canvas size in those same pixels.
pub type Viewport = (f64, f64);

/// Wheel pixels to one zoom level. A mouse notch of 100 px is 0.4 of a
/// level; the trackpad pinch arrives as a wheel with `ctrlKey` and much
/// smaller deltas, so it gets its own divisor.
const WHEEL_PX_PER_ZOOM: f64 = 250.0;
const PINCH_PX_PER_ZOOM: f64 = 40.0;
/// However violent the device, one event moves the zoom no further.
const MAX_ZOOM_PER_EVENT: f64 = 2.0;

/// Keyboard pan step, logical pixels.
const KEY_PAN_PX: f64 = 80.0;

/// Throw velocity decays by `1/e` every this many milliseconds, and is
/// dropped below `MIN_SPEED_PX_PER_MS` — a third of a pixel a frame is
/// already invisible.
const INERTIA_TAU_MS: f64 = 160.0;
const MIN_SPEED_PX_PER_MS: f64 = 0.02;
/// A hand cannot flick faster than this, but a synthetic pointer can —
/// and without a ceiling one stuck event queue throws the map a county
/// away. Three pixels a millisecond is already a very fast flick.
const MAX_THROW_PX_PER_MS: f64 = 3.0;
/// A pointer sample cannot be closer in time than this to the one before.
const MIN_SAMPLE_MS: f64 = 1.0;

/// A double click climbs one level over this long.
const DOUBLE_CLICK_MS: f64 = 250.0;

/// Budget for holding a point under the cursor. The sheet is exact on the
/// first try; the globe blend takes one correction more.
const HOLD_STEPS: usize = 4;
const HOLD_TOLERANCE_PX: f32 = 1e-3;

/// Live gesture state: at most one drag, one throw and one zoom animation.
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    drag: Option<Drag>,
    throw: Option<Throw>,
    zoom: Option<ZoomRun>,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    at: Point,
    when_ms: f64,
    velocity: Point,
}

#[derive(Clone, Copy, Debug)]
struct Throw {
    velocity: Point,
}

#[derive(Clone, Copy, Debug)]
struct ZoomRun {
    at: Point,
    from: f64,
    to: f64,
    elapsed_ms: f64,
}

impl Input {
    /// A new grip cancels whatever the map was still doing on its own.
    pub fn pointer_down(&mut self, at: Point, now_ms: f64) {
        self.throw = None;
        self.zoom = None;
        self.drag = Some(Drag { at, when_ms: now_ms, velocity: (0.0, 0.0) });
    }

    pub fn pointer_move(
        &mut self,
        camera: &Camera,
        at: Point,
        now_ms: f64,
    ) -> Option<CameraPatch> {
        let drag = self.drag.as_mut()?;
        let delta = (at.0 - drag.at.0, at.1 - drag.at.1);
        let dt = (now_ms - drag.when_ms).max(MIN_SAMPLE_MS);
        drag.velocity = (delta.0 / dt, delta.1 / dt);
        drag.at = at;
        drag.when_ms = now_ms;
        Some(pan_patch(camera, delta))
    }

    /// Release: a pointer still moving throws the map, a pointer that had
    /// already stopped leaves it where it is.
    pub fn pointer_up(&mut self) {
        let Some(drag) = self.drag.take() else { return };
        let speed = speed(drag.velocity);
        if speed < MIN_SPEED_PX_PER_MS {
            return;
        }
        let scale = (MAX_THROW_PX_PER_MS / speed).min(1.0);
        self.throw = Some(Throw {
            velocity: (drag.velocity.0 * scale, drag.velocity.1 * scale),
        });
    }

    pub fn wheel(
        &mut self,
        camera: &Camera,
        view: Viewport,
        at: Point,
        delta_y: f64,
        pinch: bool,
    ) -> CameraPatch {
        self.throw = None;
        self.zoom = None;
        zoom_about(camera, view, at, camera.zoom().value() + wheel_zoom_step(delta_y, pinch))
    }

    /// A double click is worth one level, walked over `DOUBLE_CLICK_MS`
    /// by [`Input::tick`] rather than jumped.
    pub fn double_click(&mut self, camera: &Camera, at: Point) {
        self.throw = None;
        self.zoom = Some(ZoomRun {
            at,
            from: camera.zoom().value(),
            to: camera.zoom().value() + 1.0,
            elapsed_ms: 0.0,
        });
    }

    /// Arrows pan, `+`/`-` step a level about the middle of the screen.
    /// Anything else is not ours: `None`, and the host keeps the key.
    pub fn key(&mut self, camera: &Camera, view: Viewport, key: &str) -> Option<CameraPatch> {
        let pan = |dx: f64, dy: f64| Some(pan_patch(camera, (dx, dy)));
        let step = |by: f64| Some(zoom_about(camera, view, centre_of(view), camera.zoom().value() + by));
        match key {
            "ArrowLeft" => pan(KEY_PAN_PX, 0.0),
            "ArrowRight" => pan(-KEY_PAN_PX, 0.0),
            "ArrowUp" => pan(0.0, KEY_PAN_PX),
            "ArrowDown" => pan(0.0, -KEY_PAN_PX),
            "+" | "=" => step(1.0),
            "-" => step(-1.0),
            _ => None,
        }
    }

    /// One frame of whatever is still running by itself.
    pub fn tick(&mut self, camera: &Camera, view: Viewport, dt_ms: f64) -> Option<CameraPatch> {
        self.advance_zoom(camera, view, dt_ms).or_else(|| self.coast(camera, dt_ms))
    }

    /// True when the map is not moving on its own — the readout of a card
    /// and the settling point of an e2e test.
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.drag.is_none() && self.throw.is_none() && self.zoom.is_none()
    }

    fn advance_zoom(
        &mut self,
        camera: &Camera,
        view: Viewport,
        dt_ms: f64,
    ) -> Option<CameraPatch> {
        let run = self.zoom.as_mut()?;
        run.elapsed_ms += dt_ms;
        let t = (run.elapsed_ms / DOUBLE_CLICK_MS).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - t).powi(3);
        let patch = zoom_about(camera, view, run.at, run.from + (run.to - run.from) * eased);
        if t >= 1.0 {
            self.zoom = None;
        }
        Some(patch)
    }

    /// One frame of the throw: the map travels the current velocity and
    /// the velocity loses the same fraction of itself every millisecond,
    /// so the distance per frame only ever falls.
    fn coast(&mut self, camera: &Camera, dt_ms: f64) -> Option<CameraPatch> {
        let throw = self.throw.as_mut()?;
        let delta = (throw.velocity.0 * dt_ms, throw.velocity.1 * dt_ms);
        let decay = (-dt_ms / INERTIA_TAU_MS).exp();
        throw.velocity = (throw.velocity.0 * decay, throw.velocity.1 * decay);
        if speed(throw.velocity) < MIN_SPEED_PX_PER_MS {
            self.throw = None;
        }
        Some(pan_patch(camera, delta))
    }
}

/// Centre after dragging the map by `delta` pixels: the ground that was
/// `delta` away from the middle of the screen is now in the middle of it.
#[must_use]
pub fn pan_patch(camera: &Camera, delta: Point) -> CameraPatch {
    let back = ScreenPoint { x: screen_scalar(-delta.0), y: screen_scalar(-delta.1) };
    CameraPatch { centre: Some(unproject(back, camera)), ..CameraPatch::default() }
}

/// Change the zoom while the ground under `at` stays under `at`. The zoom
/// travels through the camera's own clamp first, so a patch at the limit
/// still holds the anchor instead of drifting.
#[must_use]
pub fn zoom_about(camera: &Camera, view: Viewport, at: Point, zoom: f64) -> CameraPatch {
    let cursor = centre_offset(at, view);
    let anchor = unproject(cursor, camera);
    let zoomed = with(camera, camera.centre(), zoom);
    CameraPatch {
        centre: Some(centre_holding(anchor, cursor, &zoomed)),
        zoom: Some(zoomed.zoom().value()),
        ..CameraPatch::default()
    }
}

/// The centre that leaves `anchor` at screen `offset`. On the sheet the
/// projection is a plain difference of world positions, so unprojecting
/// the reversed offset from a camera parked on the anchor is the exact
/// answer; on the globe blend it is a first answer, and re-projecting
/// walks the rest out.
fn centre_holding(anchor: Lonlat, offset: ScreenPoint, camera: &Camera) -> Lonlat {
    let mut aim = offset;
    let mut centre = anchor;
    for _ in 0..HOLD_STEPS {
        let parked = with(camera, anchor, camera.zoom().value());
        centre = unproject(ScreenPoint { x: -aim.x, y: -aim.y }, &parked);
        let at = project(anchor, &with(camera, centre, camera.zoom().value()));
        let residual = (at.x - offset.x, at.y - offset.y);
        if residual.0.hypot(residual.1) < HOLD_TOLERANCE_PX {
            break;
        }
        aim = ScreenPoint { x: aim.x - residual.0, y: aim.y - residual.1 };
    }
    centre
}

fn with(camera: &Camera, centre: Lonlat, zoom: f64) -> Camera {
    let mut next = *camera;
    next.apply(&CameraPatch { centre: Some(centre), zoom: Some(zoom), ..CameraPatch::default() })
        .expect("centre and zoom come from finite arithmetic");
    next
}

/// Wheel delta to zoom levels.
#[must_use]
pub fn wheel_zoom_step(delta_y: f64, pinch: bool) -> f64 {
    let per_zoom = if pinch { PINCH_PX_PER_ZOOM } else { WHEEL_PX_PER_ZOOM };
    (-delta_y / per_zoom).clamp(-MAX_ZOOM_PER_EVENT, MAX_ZOOM_PER_EVENT)
}

/// Canvas position → the offset from the viewport centre that the camera
/// projects in.
#[must_use]
pub fn centre_offset(at: Point, view: Viewport) -> ScreenPoint {
    ScreenPoint {
        x: screen_scalar(at.0 - view.0 / 2.0),
        y: screen_scalar(at.1 - view.1 / 2.0),
    }
}

fn screen_scalar(value: f64) -> f32 {
    value.to_f32().expect("gesture coordinates fit f32")
}

fn centre_of(view: Viewport) -> Point {
    (view.0 / 2.0, view.1 / 2.0)
}

fn speed(velocity: Point) -> f64 {
    velocity.0.hypot(velocity.1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_camera::project;
    use maps2_units::Zoom;

    const EALING: Lonlat = Lonlat { lon: -0.3049, lat: 51.5149 };
    const VIEW: Viewport = (720.0, 480.0);

    fn camera(zoom: f64) -> Camera {
        Camera::new(EALING, Zoom::new(zoom))
    }

    fn applied(camera: &Camera, patch: &CameraPatch) -> Camera {
        let mut next = *camera;
        next.apply(patch).expect("a patch of finite numbers");
        next
    }

    fn drift_px(ground: Lonlat, camera: &Camera, want: ScreenPoint) -> f64 {
        let now = project(ground, camera);
        f64::from((now.x - want.x).abs().max((now.y - want.y).abs()))
    }

    #[test]
    fn zoom_to_the_cursor_leaves_the_ground_under_the_cursor() {
        for zoom in [4.0, 8.0, 14.37] {
            for at in [(180.0, 120.0), (700.0, 10.0), (360.0, 240.0)] {
                let before = camera(zoom);
                let cursor = centre_offset(at, VIEW);
                let anchor = unproject(cursor, &before);
                let after = applied(&before, &zoom_about(&before, VIEW, at, zoom + 0.7));
                let drift = drift_px(anchor, &after, cursor);
                assert!(drift < 0.01, "z{zoom} at {at:?}: the ground slid {drift} px");
            }
        }
    }

    #[test]
    fn a_drag_of_n_pixels_moves_the_centre_by_exactly_those_pixels() {
        let before = camera(12.0);
        for delta in [(120.0, 0.0), (-40.0, 65.0)] {
            let after = applied(&before, &pan_patch(&before, delta));
            // The new centre is the ground that stood `-delta` from the
            // old one, which is the same thing as the drag having carried
            // the ground under the pointer exactly `delta` px.
            let moved = project(after.centre(), &before);
            assert!(
                (f64::from(moved.x) + delta.0).abs() < 1e-3,
                "{delta:?}: centre went {} px, not {}",
                moved.x,
                -delta.0,
            );
            assert!((f64::from(moved.y) + delta.1).abs() < 1e-3, "y went {}", moved.y);
            let carried = drift_px(
                before.centre(),
                &after,
                ScreenPoint { x: screen_scalar(delta.0), y: screen_scalar(delta.1) },
            );
            assert!(carried < 1e-3, "{delta:?}: the ground lagged {carried} px");
        }
    }

    #[test]
    fn the_wheel_zooms_in_scrolling_up_and_the_pinch_bites_harder() {
        assert!(wheel_zoom_step(-100.0, false) > 0.0);
        assert!(wheel_zoom_step(100.0, false) < 0.0);
        assert!(wheel_zoom_step(-10.0, true) > wheel_zoom_step(-10.0, false));
        assert!(wheel_zoom_step(-100_000.0, false) <= MAX_ZOOM_PER_EVENT);
        assert!(wheel_zoom_step(100_000.0, true) >= -MAX_ZOOM_PER_EVENT);
    }

    #[test]
    fn a_throw_decays_monotonically_and_comes_to_rest() {
        let mut input = Input::default();
        let mut cam = camera(12.0);
        input.pointer_down((100.0, 100.0), 0.0);
        for sample in 1..=5 {
            let at = (100.0 + f64::from(sample) * 20.0, 100.0);
            let patch = input
                .pointer_move(&cam, at, f64::from(sample) * 16.0)
                .expect("the pointer is down");
            cam = applied(&cam, &patch);
        }
        input.pointer_up();
        let mut previous = f64::INFINITY;
        let mut frames = 0;
        while let Some(patch) = input.tick(&cam, VIEW, 16.0) {
            let next = applied(&cam, &patch);
            let step = f64::from(project(next.centre(), &cam).x).abs();
            assert!(step < previous, "frame {frames}: {step} px after {previous} px");
            previous = step;
            cam = next;
            frames += 1;
            assert!(frames < 200, "the throw never came to rest");
        }
        assert!(frames > 3, "the throw was over in {frames} frames");
        assert!(input.is_idle());
    }

    #[test]
    fn a_flick_faster_than_a_hand_is_capped_before_it_is_thrown() {
        // Synthetic pointers — a test driver, a stuck event queue —
        // deliver a hundred pixels in a millisecond. The map may not fly
        // to the next county because of it.
        let mut input = Input::default();
        let cam = camera(12.0);
        input.pointer_down((0.0, 0.0), 0.0);
        input.pointer_move(&cam, (400.0, 0.0), 1.0);
        input.pointer_up();
        let mut travelled = 0.0;
        let mut moving = cam;
        while let Some(patch) = input.tick(&moving, VIEW, 16.0) {
            let next = applied(&moving, &patch);
            travelled += f64::from(project(next.centre(), &moving).x).abs();
            moving = next;
        }
        let ceiling = MAX_THROW_PX_PER_MS * INERTIA_TAU_MS * 1.1;
        assert!(travelled <= ceiling, "the throw carried {travelled} px, over {ceiling}");
        assert!(travelled > 100.0, "the throw carried only {travelled} px");
    }

    #[test]
    fn a_pointer_that_had_stopped_before_release_is_not_thrown() {
        let mut input = Input::default();
        let cam = camera(12.0);
        input.pointer_down((100.0, 100.0), 0.0);
        input.pointer_move(&cam, (200.0, 100.0), 16.0);
        input.pointer_move(&cam, (200.0, 100.0), 200.0);
        input.pointer_up();
        assert!(input.is_idle());
        assert!(input.tick(&cam, VIEW, 16.0).is_none());
    }

    #[test]
    fn a_double_click_climbs_exactly_one_level_and_holds_its_point() {
        let mut input = Input::default();
        let mut cam = camera(12.0);
        let at = (600.0, 150.0);
        let cursor = centre_offset(at, VIEW);
        let anchor = unproject(cursor, &cam);
        input.double_click(&cam, at);
        let mut frames = 0;
        while let Some(patch) = input.tick(&cam, VIEW, 16.0) {
            cam = applied(&cam, &patch);
            frames += 1;
            assert!(frames < 100, "the animation never finished");
        }
        assert!(frames > 1, "the zoom jumped in {frames} frame");
        assert!((cam.zoom().value() - 13.0).abs() < 1e-9, "zoom = {}", cam.zoom().value());
        let drift = drift_px(anchor, &cam, cursor);
        assert!(drift < 0.01, "the point under the click slid {drift} px");
    }

    #[test]
    fn the_arrow_keys_pan_by_the_stated_pixels() {
        let before = camera(12.0);
        let mut input = Input::default();
        let east = applied(&before, &input.key(&before, VIEW, "ArrowRight").expect("a pan"));
        let moved = project(east.centre(), &before);
        assert!(east.centre().lon > before.centre().lon, "ArrowRight went west");
        assert!((f64::from(moved.x) - KEY_PAN_PX).abs() < 1e-3, "moved {} px", moved.x);
        let north = applied(&before, &input.key(&before, VIEW, "ArrowUp").expect("a pan"));
        assert!(north.centre().lat > before.centre().lat, "ArrowUp went south");
        assert!(input.key(&before, VIEW, "a").is_none(), "'a' is not ours to keep");
    }

    #[test]
    fn plus_and_minus_step_one_level_about_the_middle_of_the_screen() {
        let before = camera(12.0);
        let mut input = Input::default();
        for (key, want) in [("+", 13.0), ("=", 13.0), ("-", 11.0)] {
            let after = applied(&before, &input.key(&before, VIEW, key).expect("a zoom"));
            assert!((after.zoom().value() - want).abs() < 1e-9, "{key} → {}", after.zoom().value());
            let slid = drift_px(before.centre(), &after, ScreenPoint { x: 0.0, y: 0.0 });
            assert!(slid < 0.01, "{key}: the middle of the screen slid {slid} px");
        }
    }
}
