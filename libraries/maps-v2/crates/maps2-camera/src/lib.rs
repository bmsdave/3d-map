//! Camera, world shape and projection.
//!
//! One camera per map instance, patched atomically: a patch with one
//! invalid field changes no field at all. The world has two shapes —
//! globe far out, Mercator sheet close in — blended by [`Globeness`]
//! rather than switched, and the Mercator `cos(latitude)` compensation
//! fades across the same band, so the zoom does not drift while panning
//! north through the transition. Tilt is stored but not yet applied by
//! [`project`]; it becomes real with the renderer stage.

use maps2_units::{
    Lonlat, MAX_LATITUDE_DEG, ScreenPoint, Zoom, lonlat_at_world_px, world_position_px,
};

/// Zoom below which the world is fully a globe.
pub const GLOBE_FULL_BELOW: f64 = 3.5;
/// Zoom above which the world is fully a flat sheet.
pub const GLOBE_GONE_ABOVE: f64 = 4.5;

const MAX_ZOOM: f64 = 22.0;
const MAX_TILT_DEG: f64 = 60.0;

/// Newton budget of [`unproject`] on the globe blend, and when it may
/// stop early. On the sheet the first guess is already exact and no step
/// is ever taken; six is generous for the blend, which converges in two.
const INVERSE_STEPS: usize = 6;
const INVERSE_TOLERANCE_PX: f64 = 1e-6;
/// Finite-difference step of the Jacobian: small enough to be local at
/// z22, wide enough not to vanish into rounding at z0.
const DERIVATIVE_STEP_DEG: f64 = 1e-6;

/// How much of the globe is left on screen: 1 is a ball, 0 is a sheet.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Globeness(f64);

impl Globeness {
    #[must_use]
    pub fn at(zoom: Zoom) -> Self {
        let span = GLOBE_GONE_ABOVE - GLOBE_FULL_BELOW;
        let t = ((zoom.value() - GLOBE_FULL_BELOW) / span).clamp(0.0, 1.0);
        let smooth = t * t * (3.0 - 2.0 * t);
        Self(1.0 - smooth)
    }

    #[must_use]
    pub fn value(self) -> f64 {
        self.0
    }
}

/// The one camera of a map instance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    centre: Lonlat,
    zoom: Zoom,
    bearing_deg: f64,
    tilt_deg: f64,
}

/// Partial update; `None` leaves a field alone. Applied atomically.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CameraPatch {
    pub centre: Option<Lonlat>,
    pub zoom: Option<f64>,
    pub bearing_deg: Option<f64>,
    pub tilt_deg: Option<f64>,
}

/// Why a patch was refused. Refusal leaves the camera untouched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PatchError {
    NonFinite(&'static str),
}

impl Camera {
    #[must_use]
    pub fn new(centre: Lonlat, zoom: Zoom) -> Self {
        Self {
            centre: clamp_centre(centre),
            zoom: clamp_zoom(zoom.value()),
            bearing_deg: 0.0,
            tilt_deg: 0.0,
        }
    }

    #[must_use]
    pub fn centre(&self) -> Lonlat {
        self.centre
    }

    #[must_use]
    pub fn zoom(&self) -> Zoom {
        self.zoom
    }

    #[must_use]
    pub fn bearing_deg(&self) -> f64 {
        self.bearing_deg
    }

    #[must_use]
    pub fn tilt_deg(&self) -> f64 {
        self.tilt_deg
    }

    /// Apply a patch atomically: every field is validated first, and an
    /// invalid patch changes nothing. Out-of-range finite values are
    /// clamped here — clamps live inside, not in hosts.
    ///
    /// # Errors
    ///
    /// [`PatchError::NonFinite`] if any supplied field is NaN or
    /// infinite; the camera is left exactly as it was.
    pub fn apply(&mut self, patch: &CameraPatch) -> Result<(), PatchError> {
        validate(patch)?;
        if let Some(centre) = patch.centre {
            self.centre = clamp_centre(centre);
        }
        if let Some(zoom) = patch.zoom {
            self.zoom = clamp_zoom(zoom);
        }
        if let Some(bearing) = patch.bearing_deg {
            self.bearing_deg = bearing.rem_euclid(360.0);
        }
        if let Some(tilt) = patch.tilt_deg {
            self.tilt_deg = tilt.clamp(0.0, MAX_TILT_DEG);
        }
        Ok(())
    }
}

fn validate(patch: &CameraPatch) -> Result<(), PatchError> {
    if patch.centre.is_some_and(|c| !c.lon.is_finite() || !c.lat.is_finite()) {
        return Err(PatchError::NonFinite("centre"));
    }
    if patch.zoom.is_some_and(|z| !z.is_finite()) {
        return Err(PatchError::NonFinite("zoom"));
    }
    if patch.bearing_deg.is_some_and(|b| !b.is_finite()) {
        return Err(PatchError::NonFinite("bearing"));
    }
    if patch.tilt_deg.is_some_and(|t| !t.is_finite()) {
        return Err(PatchError::NonFinite("tilt"));
    }
    Ok(())
}

fn clamp_centre(centre: Lonlat) -> Lonlat {
    Lonlat {
        lon: (centre.lon + 180.0).rem_euclid(360.0) - 180.0,
        lat: centre.lat.clamp(-MAX_LATITUDE_DEG, MAX_LATITUDE_DEG),
    }
}

fn clamp_zoom(zoom: f64) -> Zoom {
    Zoom::new(zoom.clamp(0.0, MAX_ZOOM))
}

/// Project a geographic point to logical pixels relative to the viewport
/// centre, y pointing down. Works on both world shapes, mixing by
/// [`Globeness`] of the camera zoom.
#[must_use]
pub fn project(point: Lonlat, camera: &Camera) -> ScreenPoint {
    let (x, y) = rotate(unrotated_offset(point, camera), camera.bearing_deg);
    ScreenPoint {
        x: x as f32,
        y: y as f32,
    }
}

/// The projection before the bearing turns it, in full `f64`: the inverse
/// iterates against this rather than against the `f32` screen point.
fn unrotated_offset(point: Lonlat, camera: &Camera) -> (f64, f64) {
    let g = Globeness::at(camera.zoom).value();
    let flat = flat_offset(point, camera, g);
    let globe = globe_offset(point, camera, g);
    (
        (1.0 - g) * flat.0 + g * globe.0,
        (1.0 - g) * flat.1 + g * globe.1,
    )
}

/// Mercator sheet offset, with the `cos(latitude)` compensation fading in
/// as the globe fades in — so both shapes agree on the local scale at
/// every point of the transition.
fn flat_offset(point: Lonlat, camera: &Camera, g: f64) -> (f64, f64) {
    let world = camera.zoom.world_pixels();
    let (px, py) = world_position_px(point, camera.zoom);
    let (cx, cy) = world_position_px(camera.centre, camera.zoom);
    let mut dx = px - cx;
    if dx > world / 2.0 {
        dx -= world;
    } else if dx < -world / 2.0 {
        dx += world;
    }
    let compensation = (1.0 - g) + g * camera.centre.lat.to_radians().cos();
    (dx * compensation, (py - cy) * compensation)
}

/// Orthographic globe offset, its radius chosen so the scale at the
/// screen centre matches the compensated sheet exactly.
fn globe_offset(point: Lonlat, camera: &Camera, g: f64) -> (f64, f64) {
    let phi0 = camera.centre.lat.to_radians();
    let lam0 = camera.centre.lon.to_radians();
    let phi = point.lat.to_radians();
    let lam = point.lon.to_radians();
    let x = phi.cos() * (lam - lam0).sin();
    let y = phi0.cos() * phi.sin() - phi0.sin() * phi.cos() * (lam - lam0).cos();
    let scale = (1.0 - g) / phi0.cos() + g;
    let radius = scale * camera.zoom.world_pixels() / (2.0 * std::f64::consts::PI);
    (x * radius, -y * radius)
}

/// The ground point that projects to `offset` — the inverse of
/// [`project`], and the whole of what input needs from the camera: a
/// gesture speaks pixels and the camera answers in ground.
///
/// The sheet inverts in closed form and that answer is exact wherever the
/// globe is gone. A blend of two projections has no closed form, so the
/// sheet answer is only the first guess there and Newton walks the
/// residual out against [`project`] itself — the inverse cannot drift
/// away from the forward map that way.
#[must_use]
pub fn unproject(offset: ScreenPoint, camera: &Camera) -> Lonlat {
    let target = unrotate((f64::from(offset.x), f64::from(offset.y)), camera.bearing_deg);
    let mut guess = sheet_inverse(target, camera);
    for _ in 0..INVERSE_STEPS {
        let at = unrotated_offset(guess, camera);
        let residual = (at.0 - target.0, at.1 - target.1);
        if residual.0.hypot(residual.1) < INVERSE_TOLERANCE_PX {
            break;
        }
        guess = corrected(guess, residual, camera);
    }
    guess
}

/// Inverse of [`flat_offset`]: exact when the globe is gone.
fn sheet_inverse(target: (f64, f64), camera: &Camera) -> Lonlat {
    let g = Globeness::at(camera.zoom).value();
    let compensation = (1.0 - g) + g * camera.centre.lat.to_radians().cos();
    let (cx, cy) = world_position_px(camera.centre, camera.zoom);
    lonlat_at_world_px(
        cx + target.0 / compensation,
        cy + target.1 / compensation,
        camera.zoom,
    )
}

/// One Newton step against the projection's numeric Jacobian.
fn corrected(guess: Lonlat, residual: (f64, f64), camera: &Camera) -> Lonlat {
    let at = |point| unrotated_offset(point, camera);
    let base = at(guess);
    let east = at(Lonlat { lon: guess.lon + DERIVATIVE_STEP_DEG, ..guess });
    let north = at(Lonlat { lat: guess.lat + DERIVATIVE_STEP_DEG, ..guess });
    let d_lon = ((east.0 - base.0) / DERIVATIVE_STEP_DEG, (east.1 - base.1) / DERIVATIVE_STEP_DEG);
    let d_lat = (
        (north.0 - base.0) / DERIVATIVE_STEP_DEG,
        (north.1 - base.1) / DERIVATIVE_STEP_DEG,
    );
    let det = d_lon.0 * d_lat.1 - d_lat.0 * d_lon.1;
    if det.abs() < f64::EPSILON {
        return guess;
    }
    Lonlat {
        lon: guess.lon + (residual.1 * d_lat.0 - residual.0 * d_lat.1) / det,
        lat: (guess.lat + (residual.0 * d_lon.1 - residual.1 * d_lon.0) / det)
            .clamp(-MAX_LATITUDE_DEG, MAX_LATITUDE_DEG),
    }
}

fn rotate(offset: (f64, f64), bearing_deg: f64) -> (f64, f64) {
    let a = (-bearing_deg).to_radians();
    (
        offset.0 * a.cos() - offset.1 * a.sin(),
        offset.0 * a.sin() + offset.1 * a.cos(),
    )
}

fn unrotate(offset: (f64, f64), bearing_deg: f64) -> (f64, f64) {
    rotate(offset, -bearing_deg)
}

#[cfg(test)]
mod tests {
    use super::*;

    const LONDON: Lonlat = Lonlat { lon: -0.3049, lat: 51.5149 };

    fn camera(lon: f64, lat: f64, zoom: f64) -> Camera {
        Camera::new(Lonlat { lon, lat }, Zoom::new(zoom))
    }

    fn pixel_span_north_south(centre_lat: f64, zoom: f64) -> f64 {
        // Two points 500 m either side of the centre, along a meridian.
        let dlat = 500.0 / 111_194.9;
        let cam = camera(10.0, centre_lat, zoom);
        let north = project(Lonlat { lon: 10.0, lat: centre_lat + dlat }, &cam);
        let south = project(Lonlat { lon: 10.0, lat: centre_lat - dlat }, &cam);
        f64::from((north.y - south.y).abs())
    }

    #[test]
    fn globeness_fades_between_the_stated_zooms() {
        assert_eq!(Globeness::at(Zoom::new(2.0)).value(), 1.0);
        assert_eq!(Globeness::at(Zoom::new(GLOBE_FULL_BELOW)).value(), 1.0);
        assert_eq!(Globeness::at(Zoom::new(GLOBE_GONE_ABOVE)).value(), 0.0);
        assert_eq!(Globeness::at(Zoom::new(8.0)).value(), 0.0);
        let mid = Globeness::at(Zoom::new(4.0)).value();
        assert!((mid - 0.5).abs() < 1e-9, "midpoint was {mid}");
        let a = Globeness::at(Zoom::new(3.8)).value();
        let b = Globeness::at(Zoom::new(4.2)).value();
        assert!(a > mid && mid > b, "not monotone: {a}, {mid}, {b}");
    }

    #[test]
    fn both_world_shapes_put_the_camera_centre_under_the_screen_centre() {
        // The key test of the stage: globe (z2), sheet (z8) and the
        // blend in between (z4) must all pin the same ground point to
        // the middle of the screen.
        for zoom in [2.0, 4.0, 8.0] {
            let cam = Camera::new(LONDON, Zoom::new(zoom));
            let centre = project(LONDON, &cam);
            assert!(
                f64::from(centre.x).abs() < 1e-6 && f64::from(centre.y).abs() < 1e-6,
                "z{zoom}: centre landed at ({}, {})",
                centre.x,
                centre.y,
            );
        }
    }

    #[test]
    fn projection_does_not_jump_across_the_shape_transition() {
        let east = Lonlat { lon: LONDON.lon + 0.14, lat: LONDON.lat };
        let before = project(east, &Camera::new(LONDON, Zoom::new(3.999)));
        let after = project(east, &Camera::new(LONDON, Zoom::new(4.001)));
        let jump = f64::from((before.x - after.x).abs().max((before.y - after.y).abs()));
        assert!(jump < 0.1, "screen jumped by {jump} px across z4");
    }

    #[test]
    fn mercator_compensation_fades_on_the_same_band_as_the_shape() {
        // On the sheet a ground metre at 60° is 1/cos(60°) = 2× the
        // pixels of a metre at the equator; on the globe it is the same
        // number of pixels. In between, the ratio must be exactly the
        // globeness mix — that is what keeps zoom from drifting north.
        for zoom in [2.0, 4.0, 8.0] {
            let g = Globeness::at(Zoom::new(zoom)).value();
            let expected = (1.0 - g) * 2.0 + g;
            let ratio = pixel_span_north_south(60.0, zoom) / pixel_span_north_south(0.0, zoom);
            assert!(
                (ratio - expected).abs() < 0.01,
                "z{zoom}: ratio {ratio}, expected {expected}",
            );
        }
    }

    #[test]
    fn bearing_rotates_the_geometry_not_just_the_centre() {
        let mut cam = Camera::new(LONDON, Zoom::new(8.0));
        cam.apply(&CameraPatch { bearing_deg: Some(90.0), ..CameraPatch::default() })
            .expect("valid patch");
        let east = Lonlat { lon: LONDON.lon + 0.14, lat: LONDON.lat };
        let p = project(east, &cam);
        // Facing east: what lies east of the centre appears straight up.
        assert!(f64::from(p.y) < -1.0, "east point not above centre: y = {}", p.y);
        assert!(f64::from(p.x).abs() < 0.5, "east point drifted sideways: x = {}", p.x);
    }

    #[test]
    fn a_screen_offset_returns_the_point_that_projects_there() {
        // Both shapes and the blend between them: the offsets stay inside
        // the globe's disc at z2, where the whole world is 1024 px wide.
        for zoom in [2.0, 4.0, 8.0, 14.37] {
            for offset in [(0.0, 0.0), (64.0, -48.0), (-100.0, 80.0)] {
                let cam = Camera::new(LONDON, Zoom::new(zoom));
                let point = ScreenPoint { x: offset.0, y: offset.1 };
                let back = project(unproject(point, &cam), &cam);
                let err = f64::from((back.x - point.x).abs().max((back.y - point.y).abs()));
                assert!(err < 1e-3, "z{zoom} at {offset:?}: off by {err} px");
            }
        }
    }

    #[test]
    fn the_inverse_undoes_the_bearing_too() {
        let mut cam = Camera::new(LONDON, Zoom::new(12.0));
        cam.apply(&CameraPatch { bearing_deg: Some(37.0), ..CameraPatch::default() })
            .expect("valid patch");
        let point = ScreenPoint { x: 120.0, y: -75.0 };
        let back = project(unproject(point, &cam), &cam);
        assert!(f64::from((back.x - point.x).abs()) < 1e-3, "x = {}", back.x);
        assert!(f64::from((back.y - point.y).abs()) < 1e-3, "y = {}", back.y);
    }

    #[test]
    fn a_point_to_the_right_of_the_centre_unprojects_to_the_east() {
        let cam = Camera::new(LONDON, Zoom::new(12.0));
        let east = unproject(ScreenPoint { x: 200.0, y: 0.0 }, &cam);
        assert!(east.lon > LONDON.lon, "lon went {} instead of east", east.lon);
        assert!((east.lat - LONDON.lat).abs() < 1e-9, "lat drifted to {}", east.lat);
    }

    #[test]
    fn a_patch_with_one_invalid_field_changes_no_field() {
        let mut cam = camera(10.0, 50.0, 8.0);
        let untouched = cam;
        let refused = cam.apply(&CameraPatch {
            zoom: Some(f64::NAN),
            bearing_deg: Some(45.0),
            ..CameraPatch::default()
        });
        assert_eq!(refused, Err(PatchError::NonFinite("zoom")));
        assert_eq!(cam, untouched);
    }

    #[test]
    fn finite_out_of_range_values_are_clamped_inside() {
        let mut cam = camera(10.0, 50.0, 8.0);
        cam.apply(&CameraPatch {
            zoom: Some(99.0),
            bearing_deg: Some(370.0),
            tilt_deg: Some(200.0),
            ..CameraPatch::default()
        })
        .expect("finite values are accepted");
        assert!(cam.zoom().value() <= 22.0);
        assert!((cam.bearing_deg() - 10.0).abs() < 1e-9);
        assert!(cam.tilt_deg() <= 60.0);
    }
}
