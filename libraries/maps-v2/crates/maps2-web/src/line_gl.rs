//! The road program: one ribbon unfolded per vertex, two passes per
//! storey.
//!
//! The shader is written from the packing constants of `maps2-render`
//! rather than beside them, so a change to the vertex layout cannot
//! quietly disagree with the code that reads it.

use wasm_bindgen::JsValue;
use web_sys::{WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlUniformLocation};

use maps2_render::{
    JoinCounts, LineBucket, LineRange, RoadLevel, LINESOFAR_STEP, NORMAL_SCALE, POS_BIAS,
};
use maps2_style::Class;

use crate::gl::{as_bytes, link_program};
use crate::gl_view::{ViewLocations, PROJECTION_GLSL};
use maps2_units::TILE_EXTENT;

const VERTEX_STRIDE: i32 = 8;

fn vertex_src() -> String {
    format!(
        r"#version 300 es
in vec2 a_pos;
in vec2 a_normal;
in float a_linesofar;
{PROJECTION_GLSL}
uniform float u_half_width;
out vec2 v_normal;
out float v_along_px;
out float v_front;
void main() {{
    vec2 extrusion = a_normal / {normal_scale:.1};
    // The centreline goes through the shared projection, so a ribbon
    // follows whatever shape the world is in; the extrusion stays in
    // screen pixels, because a road's width is a screen width and not
    // a distance on the ground.
    vec3 centre = project_nrm((a_pos + {bias}.0) / {extent:.1}, 1.0);
    v_front = centre.z;
    float reach = length(extrusion);
    v_normal = reach > 0.001 ? extrusion / reach : vec2(0.0);
    v_along_px = a_linesofar * {step:.1} * u_tile_scale_px / {extent:.1};
    gl_Position = clip_of(centre.xy + extrusion * u_half_width);
}}",
        normal_scale = NORMAL_SCALE,
        bias = POS_BIAS,
        step = LINESOFAR_STEP,
        extent = f64::from(TILE_EXTENT),
    )
}

/// `v_normal` is the unit normal, so its interpolated length is the
/// distance across the ribbon in half-widths — zero on the centreline,
/// one at either edge, whatever the miter did to the geometry.
const FRAGMENT_SRC: &str = r"#version 300 es
precision highp float;
uniform vec4 u_color;
uniform float u_half_width;
uniform float u_feather;
uniform vec2 u_dash;
in vec2 v_normal;
in float v_along_px;
in float v_front;
out vec4 outColor;
void main() {
    if (v_front < 0.0) { discard; }
    if (u_dash.x > 0.0 && mod(v_along_px, u_dash.x) > u_dash.y) discard;
    float dist = length(v_normal) * u_half_width;
    float inner = max(u_half_width - u_feather, 0.0);
    float outer = max(u_half_width, inner + 0.001);
    float alpha = 1.0 - smoothstep(inner, outer, dist);
    outColor = vec4(u_color.rgb, u_color.a * alpha);
}";

pub struct LineProgram {
    program: WebGlProgram,
    /// The shared projection's uniforms — the same ones the fills and
    /// the terrain bind, so a road cannot end up on a different shape
    /// from the ground it runs over.
    pub view: ViewLocations,
    pub u_viewport: WebGlUniformLocation,
    pub u_half_width: WebGlUniformLocation,
    pub u_color: WebGlUniformLocation,
    pub u_feather: WebGlUniformLocation,
    pub u_dash: WebGlUniformLocation,
    a_pos: u32,
    a_normal: u32,
    a_linesofar: u32,
}

impl LineProgram {
    pub fn link(gl: &Gl) -> Result<Self, JsValue> {
        let program = link_program(gl, &vertex_src(), FRAGMENT_SRC)?;
        let uniform = |name: &str| {
            gl.get_uniform_location(&program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
        Ok(Self {
            view: ViewLocations::of(gl, &program)?,
            u_viewport: uniform("u_viewport")?,
            u_half_width: uniform("u_half_width")?,
            u_color: uniform("u_color")?,
            u_feather: uniform("u_feather")?,
            u_dash: uniform("u_dash")?,
            a_pos: gl.get_attrib_location(&program, "a_pos") as u32,
            a_normal: gl.get_attrib_location(&program, "a_normal") as u32,
            a_linesofar: gl.get_attrib_location(&program, "a_linesofar") as u32,
            program,
        })
    }

    pub fn bind(&self, gl: &Gl) {
        gl.use_program(Some(&self.program));
    }

    /// The fill program owns only `a_pos`; leaving the ribbon's other
    /// two arrays enabled would point them at a freed buffer.
    pub fn unbind(&self, gl: &Gl) {
        gl.disable_vertex_attrib_array(self.a_normal);
        gl.disable_vertex_attrib_array(self.a_linesofar);
    }
}

/// A tile's road ribbons on the GPU. Uploaded when the tile becomes
/// resident, and again only if the join geometry itself changes.
pub struct GpuLineBucket {
    vertices: WebGlBuffer,
    indices: WebGlBuffer,
    pub ranges: Vec<LineRange>,
    pub joins: JoinCounts,
}

impl GpuLineBucket {
    pub fn upload(gl: &Gl, bucket: &LineBucket) -> Result<Self, JsValue> {
        let vertices = gl.create_buffer().ok_or("no line vertex buffer")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertices));
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, as_bytes(&bucket.vertices), Gl::STATIC_DRAW);

        let indices = gl.create_buffer().ok_or("no line index buffer")?;
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&indices));
        gl.buffer_data_with_u8_array(
            Gl::ELEMENT_ARRAY_BUFFER,
            as_bytes(&bucket.indices),
            Gl::STATIC_DRAW,
        );
        Ok(Self { vertices, indices, ranges: bucket.ranges.clone(), joins: bucket.joins })
    }

    pub fn delete(&self, gl: &Gl) {
        gl.delete_buffer(Some(&self.vertices));
        gl.delete_buffer(Some(&self.indices));
    }

    #[must_use]
    pub fn range(&self, class: Class, level: RoadLevel) -> Option<LineRange> {
        self.ranges.iter().copied().find(|r| r.class == class && r.level == level)
    }

    pub fn bind(&self, gl: &Gl, program: &LineProgram) {
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.vertices));
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&self.indices));
        let attribute = |location: u32, size: i32, kind: u32, offset: i32| {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_with_i32(location, size, kind, false, VERTEX_STRIDE, offset);
        };
        attribute(program.a_pos, 2, Gl::SHORT, 0);
        attribute(program.a_normal, 2, Gl::BYTE, 4);
        attribute(program.a_linesofar, 1, Gl::UNSIGNED_SHORT, 6);
    }
}
