//! GPU upload and drawing for terrain-relative building meshes.

use wasm_bindgen::JsValue;
use web_sys::{WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlUniformLocation};

use maps2_render::{BuildingBucket, HeightWindow, MaterialRange};

use crate::gl::{as_bytes, link_program};
use crate::gl_terrain::{heights_glsl, HeightBinding};
use crate::gl_view::{ViewLocations, PROJECTION_GLSL};

fn vertex_source() -> String {
    format!(
        "#version 300 es
in vec2 a_pos;
in float a_height_dm;
{PROJECTION_GLSL}
{HEIGHTS_GLSL}
uniform float u_metres_to_px;
uniform float u_tilt_rad;
out float v_front;

void main() {{
    vec2 unit = a_pos / 65535.0;
    vec3 p = project_nrm(unit, 1.0);
    // The same ground the terrain shaders draw, read the same way: a
    // building whose base came from a different sample of the surface
    // than the surface itself would float or sink into it.
    float metres = height_metres(unit) + a_height_dm * 0.1;
    p.y -= metres * u_metres_to_px * sin(u_tilt_rad);
    v_front = p.z;
    gl_Position = clip_of(p.xy);
}}",
        HEIGHTS_GLSL = heights_glsl(),
    )
}

const FRAGMENT_SRC: &str = r"#version 300 es
precision mediump float;
uniform vec4 u_color;
in float v_front;
out vec4 outColor;
void main() {
    if (v_front < 0.0) { discard; }
    outColor = u_color;
}";

pub struct BuildingProgram {
    program: WebGlProgram,
    pub view: ViewLocations,
    pub color: WebGlUniformLocation,
    has_heights: WebGlUniformLocation,
    height_window: WebGlUniformLocation,
    metres_to_px: WebGlUniformLocation,
    tilt_rad: WebGlUniformLocation,
    pos: u32,
    height_dm: u32,
}

impl BuildingProgram {
    pub fn link(gl: &Gl) -> Result<Self, JsValue> {
        let program = link_program(gl, &vertex_source(), FRAGMENT_SRC)?;
        let uniform = |name: &str| {
            gl.get_uniform_location(&program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
        gl.use_program(Some(&program));
        gl.uniform1i(Some(&uniform("u_heights")?), 0);
        Ok(Self {
            view: ViewLocations::of(gl, &program)?,
            color: uniform("u_color")?,
            has_heights: uniform("u_has_heights")?,
            height_window: uniform("u_height_window")?,
            metres_to_px: uniform("u_metres_to_px")?,
            tilt_rad: uniform("u_tilt_rad")?,
            pos: gl.get_attrib_location(&program, "a_pos") as u32,
            height_dm: gl.get_attrib_location(&program, "a_height_dm") as u32,
            program,
        })
    }

    pub fn bind(&self, gl: &Gl, metres_to_px: f32, tilt_deg: f64) {
        gl.use_program(Some(&self.program));
        gl.uniform1f(Some(&self.metres_to_px), metres_to_px);
        gl.uniform1f(Some(&self.tilt_rad), tilt_deg.to_radians() as f32);
    }

    pub fn bind_heights(&self, gl: &Gl, source: Option<HeightBinding<'_>>) {
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, source.map(|s| &s.texture.texture));
        gl.uniform1f(Some(&self.has_heights), f32::from(u8::from(source.is_some())));
        let window = source.map_or(HeightWindow::IDENTITY, |s| s.window);
        gl.uniform3f(Some(&self.height_window), window.scale, window.offset_x, window.offset_y);
    }
}

pub struct GpuBuildingBucket {
    vertices: WebGlBuffer,
    indices: WebGlBuffer,
    /// One draw per material range, so each can bind its own facade
    /// colour — the mesh itself is one buffer pair, only the draw calls
    /// are split.
    ranges: Vec<MaterialRange>,
}

impl GpuBuildingBucket {
    /// Whether this tile drew any buildings at all — the band readout
    /// asks, because a tile can be resident with nothing to show.
    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }

    pub fn upload(gl: &Gl, bucket: &BuildingBucket) -> Result<Self, JsValue> {
        let vertices = gl.create_buffer().ok_or("no building vertex buffer")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertices));
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, as_bytes(&bucket.vertices), Gl::STATIC_DRAW);
        let indices = gl.create_buffer().ok_or("no building index buffer")?;
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&indices));
        gl.buffer_data_with_u8_array(Gl::ELEMENT_ARRAY_BUFFER, as_bytes(&bucket.indices), Gl::STATIC_DRAW);
        Ok(Self { vertices, indices, ranges: bucket.ranges.clone() })
    }

    pub fn delete(&self, gl: &Gl) {
        gl.delete_buffer(Some(&self.vertices));
        gl.delete_buffer(Some(&self.indices));
    }

    /// The material ranges to draw, in bucket order — the caller sets the
    /// facade colour for each before calling `draw_range`.
    pub fn ranges(&self) -> &[MaterialRange] {
        &self.ranges
    }

    fn bind_attributes(&self, gl: &Gl, program: &BuildingProgram) {
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.vertices));
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&self.indices));
        gl.enable_vertex_attrib_array(program.pos);
        gl.vertex_attrib_pointer_with_i32(program.pos, 2, Gl::UNSIGNED_SHORT, false, 6, 0);
        gl.enable_vertex_attrib_array(program.height_dm);
        gl.vertex_attrib_pointer_with_i32(program.height_dm, 1, Gl::UNSIGNED_SHORT, false, 6, 4);
    }

    /// Draws one material range. `first_index`/`index_count` are index
    /// counts, not bytes — `u32` indices, so the byte offset is ×4.
    pub fn draw_range(&self, gl: &Gl, program: &BuildingProgram, range: MaterialRange) {
        self.bind_attributes(gl, program);
        let offset = i32::try_from(range.first_index).unwrap_or(0) * 4;
        let count = i32::try_from(range.index_count).unwrap_or(0);
        gl.draw_elements_with_i32(Gl::TRIANGLES, count, Gl::UNSIGNED_INT, offset);
    }
}
