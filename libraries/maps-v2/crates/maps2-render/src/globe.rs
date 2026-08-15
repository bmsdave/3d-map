//! The projection the vertex shader runs, written once in Rust.
//!
//! `maps2-camera` projects a geographic point; a vertex arrives as a
//! position inside a tile instead, and it arrives in GLSL. This module
//! is the same projection expressed the way the shader needs it — from
//! normalised world coordinates, with the relief displacement and the
//! back-face test the sphere adds — and its tests pin it to the camera's
//! answer. The GLSL below it is a transcription; when they disagree,
//! this is the one that is right.

use maps2_camera::{Camera, Globeness};
use maps2_units::{world_position_px, Lonlat, TileId, Zoom};

/// Everything a frame's projection depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub centre: Lonlat,
    pub centre_normalised: (f64, f64),
    pub world_px: f64,
    pub bearing_deg: f64,
    pub globeness: f64,
    pub viewport: (f64, f64),
}

impl View {
    #[must_use]
    pub fn of(camera: &Camera, viewport: (f64, f64)) -> Self {
        let (x, y) = world_position_px(camera.centre(), Zoom::new(0.0));
        Self {
            centre: camera.centre(),
            centre_normalised: (x / 256.0, y / 256.0),
            world_px: camera.zoom().world_pixels(),
            bearing_deg: camera.bearing_deg(),
            globeness: Globeness::at(camera.zoom()).value(),
            viewport,
        }
    }
}

/// A projected vertex: logical pixels from the viewport centre, and
/// which side of the globe it sits on (negative is the far side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Projected {
    pub x: f64,
    pub y: f64,
    pub front: f64,
}

/// Project a normalised world position. `relief` scales the globe's
/// radius at this vertex (1.0 is the sphere itself).
#[must_use]
pub fn project_normalised(nrm: (f64, f64), view: &View, relief: f64) -> Projected {
    let g = view.globeness;
    let flat = flat_offset(nrm, view, g);
    let (globe, front) = globe_offset(nrm, view, g, relief);
    let mixed = (
        (1.0 - g) * flat.0 + g * globe.0,
        (1.0 - g) * flat.1 + g * globe.1,
    );
    let (x, y) = rotate(mixed, view.bearing_deg);
    Projected { x, y, front: front * g + (1.0 - g) }
}

fn flat_offset(nrm: (f64, f64), view: &View, g: f64) -> (f64, f64) {
    let world = view.world_px;
    let mut dx = (nrm.0 - view.centre_normalised.0) * world;
    if dx > world / 2.0 {
        dx -= world;
    } else if dx < -world / 2.0 {
        dx += world;
    }
    let dy = (nrm.1 - view.centre_normalised.1) * world;
    let compensation = (1.0 - g) + g * view.centre.lat.to_radians().cos();
    (dx * compensation, dy * compensation)
}

/// Orthographic globe, radius matched to the compensated sheet at the
/// screen centre; the second value is the cosine of the angle away from
/// the camera, which is what tells front from back.
fn globe_offset(nrm: (f64, f64), view: &View, g: f64, relief: f64) -> ((f64, f64), f64) {
    let (lam, phi) = normalised_to_radians(nrm);
    let phi0 = view.centre.lat.to_radians();
    let lam0 = view.centre.lon.to_radians();
    let dlam = lam - lam0;
    let x = phi.cos() * dlam.sin();
    let y = phi0.cos() * phi.sin() - phi0.sin() * phi.cos() * dlam.cos();
    let front = phi0.sin() * phi.sin() + phi0.cos() * phi.cos() * dlam.cos();
    let scale = (1.0 - g) / phi0.cos() + g;
    let radius = scale * view.world_px / std::f64::consts::TAU * relief;
    ((x * radius, -y * radius), front)
}

/// Inverse Web Mercator: normalised world → longitude and latitude in
/// radians.
fn normalised_to_radians(nrm: (f64, f64)) -> (f64, f64) {
    let lam = nrm.0 * std::f64::consts::TAU - std::f64::consts::PI;
    let phi = (std::f64::consts::PI * (1.0 - 2.0 * nrm.1)).sinh().atan();
    (lam, phi)
}

fn rotate(offset: (f64, f64), bearing_deg: f64) -> (f64, f64) {
    let a = (-bearing_deg).to_radians();
    (
        offset.0 * a.cos() - offset.1 * a.sin(),
        offset.0 * a.sin() + offset.1 * a.cos(),
    )
}

/// Where a tile sits in the normalised world, and how much of it it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileFrame {
    pub origin_x: f64,
    pub origin_y: f64,
    pub span: f64,
}

#[must_use]
pub fn tile_frame(id: TileId) -> TileFrame {
    let tiles = f64::from(1_u32 << id.z);
    TileFrame {
        origin_x: f64::from(id.x) / tiles,
        origin_y: f64::from(id.y) / tiles,
        span: 1.0 / tiles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_camera::{project, Camera, Globeness};
    use maps2_units::{world_position_px, Lonlat, TileId, Zoom};

    const EALING: Lonlat = Lonlat { lon: -0.3049, lat: 51.5149 };

    fn normalised(point: Lonlat) -> (f64, f64) {
        let (x, y) = world_position_px(point, Zoom::new(0.0));
        (x / 256.0, y / 256.0)
    }

    fn view_at(zoom: f64) -> View {
        View::of(&Camera::new(EALING, Zoom::new(zoom)), (800.0, 600.0))
    }

    #[test]
    fn the_shader_projection_is_the_camera_projection() {
        // The vertex shader cannot be unit-tested; this twin of it can,
        // and it must agree with the camera on both shapes and on the
        // blend between them, or the sphere would drift from the maths
        // the rest of the SDK reasons with.
        let points = [
            EALING,
            Lonlat { lon: EALING.lon + 12.0, lat: EALING.lat + 7.0 },
            Lonlat { lon: EALING.lon - 40.0, lat: -20.0 },
            Lonlat { lon: 179.0, lat: 0.0 },
        ];
        for zoom in [2.0, 4.0, 8.0, 14.0] {
            let view = view_at(zoom);
            let camera = Camera::new(EALING, Zoom::new(zoom));
            for point in points {
                let mine = project_normalised(normalised(point), &view, 1.0);
                let theirs = project(point, &camera);
                let dx = (mine.x - f64::from(theirs.x)).abs();
                let dy = (mine.y - f64::from(theirs.y)).abs();
                // The camera hands out `f32` pixels, and a point twelve
                // degrees off centre at z14 is a million pixels away —
                // where `f32` itself steps by a sixteenth of a pixel.
                let tolerance = 1e-3 + mine.x.abs().max(mine.y.abs()) * 1e-6;
                let off = dx.max(dy);
                assert!(off < tolerance, "z{zoom} {point:?}: off by {off} px");
            }
        }
    }

    #[test]
    fn the_camera_centre_lands_in_the_middle_on_both_shapes() {
        for zoom in [2.0, 4.0, 8.0] {
            let projected = project_normalised(normalised(EALING), &view_at(zoom), 1.0);
            assert!(projected.x.abs() < 1e-9 && projected.y.abs() < 1e-9, "z{zoom}");
            assert!(projected.front > 0.0, "the centre faces the camera");
        }
    }

    #[test]
    fn the_far_side_of_the_globe_faces_away() {
        let view = view_at(2.0);
        let antipode = Lonlat { lon: EALING.lon + 180.0, lat: -EALING.lat };
        assert!(project_normalised(normalised(antipode), &view, 1.0).front < 0.0);
        // The limb, a quarter turn away, is the boundary case.
        let limb = Lonlat { lon: EALING.lon + 90.0, lat: 0.0 };
        let front = project_normalised(normalised(limb), &view, 1.0).front;
        assert!(front.abs() < 0.8, "the limb should sit near zero, not {front}");
        // On the sheet nothing is ever behind the world.
        let flat = View::of(&Camera::new(EALING, Zoom::new(12.0)), (800.0, 600.0));
        assert!(project_normalised(normalised(antipode), &flat, 1.0).front > 0.0);
    }

    #[test]
    fn relief_lifts_a_point_off_the_sphere_and_leaves_the_sheet_alone() {
        let globe = view_at(2.0);
        let point = normalised(Lonlat { lon: EALING.lon + 30.0, lat: EALING.lat });
        let ground = project_normalised(point, &globe, 1.0);
        let lifted = project_normalised(point, &globe, 1.05);
        let ratio = lifted.x / ground.x;
        assert!((ratio - 1.05).abs() < 1e-9, "radius did not scale: {ratio}");

        let sheet = View::of(&Camera::new(EALING, Zoom::new(12.0)), (800.0, 600.0));
        let near = normalised(Lonlat { lon: EALING.lon + 0.01, lat: EALING.lat });
        assert_close(project_normalised(near, &sheet, 1.0).x, project_normalised(near, &sheet, 1.05).x);
    }

    #[test]
    fn a_tile_frame_is_its_share_of_the_normalised_world() {
        let world = tile_frame(TileId { z: 0, x: 0, y: 0 });
        assert_close(world.origin_x, 0.0);
        assert_close(world.origin_y, 0.0);
        assert_close(world.span, 1.0);
        let deep = tile_frame(TileId { z: 14, x: 8190, y: 5448 });
        assert_close(deep.span, 1.0 / 16384.0);
        assert_close(deep.origin_x, 8190.0 / 16384.0);
        assert_close(deep.origin_y, 5448.0 / 16384.0);
    }

    #[test]
    fn the_view_reports_the_shape_the_camera_is_in() {
        assert_close(view_at(2.0).globeness, 1.0);
        assert_close(view_at(8.0).globeness, 0.0);
        assert_close(view_at(4.0).globeness, Globeness::at(Zoom::new(4.0)).value());
    }

    fn assert_close(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < f64::EPSILON, "{actual} != {expected}");
    }
}
