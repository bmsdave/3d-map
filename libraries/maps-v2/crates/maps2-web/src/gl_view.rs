//! The projection every program shares, in GLSL and in uniforms.
//!
//! A vertex arrives as a position inside a tile and has to land on the
//! screen through whichever shape the world is in — sheet, sphere, or
//! the blend between them. Doing that on the GPU is what lets one mesh
//! serve both shapes; doing it in one place is what keeps the sphere
//! honest, because this GLSL is a transcription of
//! `maps2_render::project_normalised`, which is pinned to the camera by
//! native tests.

use wasm_bindgen::JsValue;
use web_sys::{WebGl2RenderingContext as Gl, WebGlProgram, WebGlUniformLocation};

use maps2_render::{TileFrame, View};

/// Declarations and functions pasted into every vertex shader.
pub const PROJECTION_GLSL: &str = r"
uniform vec2 u_tile_origin;
uniform float u_tile_span;
uniform vec2 u_tile_offset_px;
uniform float u_tile_scale_px;
uniform vec2 u_centre_rad;
uniform float u_world_px;
uniform float u_globeness;
uniform float u_bearing_rad;
uniform vec2 u_viewport;

const float PI = 3.141592653589793;
const float TAU = 6.283185307179586;

vec2 tile_to_nrm(vec2 unit) {
    return u_tile_origin + unit * u_tile_span;
}

// xy: logical pixels from the viewport centre. z: which side of the
// globe the vertex is on — negative is behind the world.
//
// The sheet offset comes in already measured in pixels, because at z16
// the world is 17 million pixels wide and `float` would lose half of
// one working from normalised coordinates. The sphere has no such
// trouble: it only exists where the whole world fits on screen.
vec3 project_nrm(vec2 unit, float relief) {
    float g = u_globeness;
    float world = u_world_px;
    vec2 nrm = tile_to_nrm(unit);

    float cphi0 = cos(u_centre_rad.y);
    float sphi0 = sin(u_centre_rad.y);
    vec2 sheet = (u_tile_offset_px + unit * u_tile_scale_px) * ((1.0 - g) + g * cphi0);

    float lam = nrm.x * TAU - PI;
    float phi = atan(sinh(PI * (1.0 - 2.0 * nrm.y)));
    float dlam = lam - u_centre_rad.x;
    float cphi = cos(phi);
    float sphi = sin(phi);
    float scale = (1.0 - g) / cphi0 + g;
    float radius = scale * world / TAU * relief;
    vec2 ball = vec2(cphi * sin(dlam), -(cphi0 * sphi - sphi0 * cphi * cos(dlam))) * radius;
    float front = sphi0 * sphi + cphi0 * cphi * cos(dlam);

    vec2 mixed = mix(sheet, ball, g);
    float a = -u_bearing_rad;
    vec2 turned = vec2(
        mixed.x * cos(a) - mixed.y * sin(a),
        mixed.x * sin(a) + mixed.y * cos(a)
    );
    return vec3(turned, front * g + (1.0 - g));
}

vec4 clip_of(vec2 offset_px) {
    vec2 clip = (u_viewport * 0.5 + offset_px) / u_viewport * 2.0 - 1.0;
    return vec4(clip.x, -clip.y, 0.0, 1.0);
}
";

/// The uniform locations of [`PROJECTION_GLSL`] inside one program.
pub struct ViewLocations {
    tile_origin: WebGlUniformLocation,
    tile_span: WebGlUniformLocation,
    tile_offset_px: WebGlUniformLocation,
    tile_scale_px: WebGlUniformLocation,
    centre_rad: WebGlUniformLocation,
    world_px: WebGlUniformLocation,
    globeness: WebGlUniformLocation,
    bearing_rad: WebGlUniformLocation,
    viewport: WebGlUniformLocation,
}

impl ViewLocations {
    pub fn of(gl: &Gl, program: &WebGlProgram) -> Result<Self, JsValue> {
        let uniform = |name: &str| {
            gl.get_uniform_location(program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
        Ok(Self {
            tile_origin: uniform("u_tile_origin")?,
            tile_span: uniform("u_tile_span")?,
            tile_offset_px: uniform("u_tile_offset_px")?,
            tile_scale_px: uniform("u_tile_scale_px")?,
            centre_rad: uniform("u_centre_rad")?,
            world_px: uniform("u_world_px")?,
            globeness: uniform("u_globeness")?,
            bearing_rad: uniform("u_bearing_rad")?,
            viewport: uniform("u_viewport")?,
        })
    }

    /// Once per frame, after the program is bound.
    pub fn set_view(&self, gl: &Gl, view: &View) {
        gl.uniform2f(
            Some(&self.centre_rad),
            view.centre.lon.to_radians() as f32,
            view.centre.lat.to_radians() as f32,
        );
        gl.uniform1f(Some(&self.world_px), view.world_px as f32);
        gl.uniform1f(Some(&self.globeness), view.globeness as f32);
        gl.uniform1f(Some(&self.bearing_rad), view.bearing_deg.to_radians() as f32);
        gl.uniform2f(
            Some(&self.viewport),
            view.viewport.0 as f32,
            view.viewport.1 as f32,
        );
    }

    /// Once per tile. The pixel offset and scale are worked out in
    /// `f64` by the caller; only their small results reach the GPU.
    pub fn set_tile(&self, gl: &Gl, frame: TileFrame, placement: TilePixels) {
        gl.uniform2f(
            Some(&self.tile_origin),
            frame.origin_x as f32,
            frame.origin_y as f32,
        );
        gl.uniform1f(Some(&self.tile_span), frame.span as f32);
        gl.uniform2f(
            Some(&self.tile_offset_px),
            placement.offset_x as f32,
            placement.offset_y as f32,
        );
        gl.uniform1f(Some(&self.tile_scale_px), placement.scale_px as f32);
    }
}

/// A tile on the flat sheet: where its north-west corner sits relative
/// to the viewport centre, and how many pixels its whole side spans.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TilePixels {
    pub offset_x: f64,
    pub offset_y: f64,
    pub scale_px: f64,
}

impl TilePixels {
    pub fn of(placement: &crate::transform::TilePlacement, viewport: (f64, f64)) -> Self {
        Self {
            offset_x: placement.translate_x - viewport.0 / 2.0,
            offset_y: placement.translate_y - viewport.1 / 2.0,
            scale_px: placement.scale * f64::from(maps2_units::TILE_EXTENT),
        }
    }
}
