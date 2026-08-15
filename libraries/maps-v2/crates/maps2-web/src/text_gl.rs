//! The screen-space passes: SDF text with a halo, and the collision
//! boxes the debug card draws over it.
//!
//! Both take vertices already in logical pixels — no tile transform, no
//! bearing, no tilt. That is not a shortcut, it is the contract: type
//! belongs to the screen, not to the world mesh.

use wasm_bindgen::JsValue;
use web_sys::{WebGl2RenderingContext as Gl, WebGlBuffer, WebGlTexture, WebGlUniformLocation};

use maps2_text::{Atlas, GlyphQuad, ScreenBox, EM_PX, SDF_RANGE_PX};

use crate::gl::link_program;

/// Field units per atlas pixel: the byte range spans ±`SDF_RANGE_PX`
/// around 128, so half the range is 127/255 of the texture's scale.
const FIELD_PER_ATLAS_PX: f32 = (127.0 / 255.0) / SDF_RANGE_PX;

/// Where the field reads as the outline, in texture units.
const EDGE: f32 = 128.0 / 255.0;

const TEXT_VERTEX_SRC: &str = r"#version 300 es
in vec2 a_pos;
in vec2 a_uv;
uniform vec2 u_viewport;
out vec2 v_uv;
void main() {
    vec2 clip = a_pos / u_viewport * 2.0 - 1.0;
    gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
    v_uv = a_uv;
}";

// The point of a distance field: the cutoff is found per fragment, so
// one texture is sharp at any size instead of one bitmap per size.
const TEXT_FRAGMENT_SRC: &str = r"#version 300 es
precision mediump float;
uniform sampler2D u_atlas;
uniform vec4 u_color;
uniform vec4 u_halo;
uniform float u_halo_width;
uniform float u_edge;
in vec2 v_uv;
out vec4 outColor;
void main() {
    float d = texture(u_atlas, v_uv).r;
    float w = fwidth(d) * 0.8 + 0.0001;
    float fill = smoothstep(u_edge - w, u_edge + w, d);
    float halo = smoothstep(u_edge - u_halo_width - w, u_edge - u_halo_width + w, d);
    float alpha = max(fill * u_color.a, halo * u_halo.a);
    if (alpha < 0.004) discard;
    outColor = vec4(mix(u_halo.rgb, u_color.rgb, fill), alpha);
}";

const BOX_VERTEX_SRC: &str = r"#version 300 es
in vec2 a_pos;
uniform vec2 u_viewport;
void main() {
    vec2 clip = a_pos / u_viewport * 2.0 - 1.0;
    gl_Position = vec4(clip.x, -clip.y, 0.0, 1.0);
}";

const BOX_FRAGMENT_SRC: &str = r"#version 300 es
precision mediump float;
uniform vec4 u_color;
out vec4 outColor;
void main() { outColor = u_color; }";

pub struct TextProgram {
    program: web_sys::WebGlProgram,
    atlas: WebGlTexture,
    buffer: WebGlBuffer,
    u_viewport: WebGlUniformLocation,
    u_color: WebGlUniformLocation,
    u_halo: WebGlUniformLocation,
    u_halo_width: WebGlUniformLocation,
    u_edge: WebGlUniformLocation,
    a_pos: u32,
    a_uv: u32,
}

impl TextProgram {
    pub fn link(gl: &Gl, atlas: &Atlas) -> Result<Self, JsValue> {
        let program = link_program(gl, TEXT_VERTEX_SRC, TEXT_FRAGMENT_SRC)?;
        let uniform = |name: &str| {
            gl.get_uniform_location(&program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
        Ok(Self {
            u_viewport: uniform("u_viewport")?,
            u_color: uniform("u_color")?,
            u_halo: uniform("u_halo")?,
            u_halo_width: uniform("u_halo_width")?,
            u_edge: uniform("u_edge")?,
            a_pos: gl.get_attrib_location(&program, "a_pos") as u32,
            a_uv: gl.get_attrib_location(&program, "a_uv") as u32,
            atlas: upload_atlas(gl, atlas)?,
            buffer: gl.create_buffer().ok_or("no text buffer")?,
            program,
        })
    }

    /// One draw call per colour group. `halo_em` is the halo's width in
    /// em, so it grows with the type instead of thinning out on it.
    pub fn draw(&self, gl: &Gl, request: &TextDraw<'_>) {
        if request.quads.is_empty() {
            return;
        }
        gl.use_program(Some(&self.program));
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, Some(&self.atlas));
        gl.uniform2f(Some(&self.u_viewport), request.viewport.0, request.viewport.1);
        set_colour(gl, &self.u_color, request.colour, request.opacity);
        set_colour(gl, &self.u_halo, request.halo, request.opacity);
        gl.uniform1f(Some(&self.u_halo_width), request.halo_em * EM_PX * FIELD_PER_ATLAS_PX);
        gl.uniform1f(Some(&self.u_edge), EDGE);

        let vertices = quad_vertices(request.quads);
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.buffer));
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, as_bytes(&vertices), Gl::DYNAMIC_DRAW);
        gl.enable_vertex_attrib_array(self.a_pos);
        gl.vertex_attrib_pointer_with_i32(self.a_pos, 2, Gl::FLOAT, false, 16, 0);
        gl.enable_vertex_attrib_array(self.a_uv);
        gl.vertex_attrib_pointer_with_i32(self.a_uv, 2, Gl::FLOAT, false, 16, 8);
        gl.draw_arrays(Gl::TRIANGLES, 0, (request.quads.len() * 6) as i32);
    }
}

/// Everything one text draw call needs. Five fields rather than five
/// arguments, because the colours travel together.
pub struct TextDraw<'a> {
    pub quads: &'a [GlyphQuad],
    pub viewport: (f32, f32),
    pub colour: [u8; 4],
    pub halo: [u8; 4],
    pub halo_em: f32,
    pub opacity: f32,
}

pub struct BoxProgram {
    program: web_sys::WebGlProgram,
    buffer: WebGlBuffer,
    u_viewport: WebGlUniformLocation,
    u_color: WebGlUniformLocation,
    a_pos: u32,
}

impl BoxProgram {
    pub fn link(gl: &Gl) -> Result<Self, JsValue> {
        let program = link_program(gl, BOX_VERTEX_SRC, BOX_FRAGMENT_SRC)?;
        let uniform = |name: &str| {
            gl.get_uniform_location(&program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
        Ok(Self {
            u_viewport: uniform("u_viewport")?,
            u_color: uniform("u_color")?,
            a_pos: gl.get_attrib_location(&program, "a_pos") as u32,
            buffer: gl.create_buffer().ok_or("no box buffer")?,
            program,
        })
    }

    pub fn draw(&self, gl: &Gl, boxes: &[ScreenBox], viewport: (f32, f32), colour: [u8; 4]) {
        if boxes.is_empty() {
            return;
        }
        gl.use_program(Some(&self.program));
        gl.uniform2f(Some(&self.u_viewport), viewport.0, viewport.1);
        set_colour(gl, &self.u_color, colour, 1.0);
        let vertices = box_vertices(boxes);
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.buffer));
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, as_bytes(&vertices), Gl::DYNAMIC_DRAW);
        gl.enable_vertex_attrib_array(self.a_pos);
        gl.vertex_attrib_pointer_with_i32(self.a_pos, 2, Gl::FLOAT, false, 8, 0);
        gl.draw_arrays(Gl::TRIANGLES, 0, (boxes.len() * 6) as i32);
    }
}

fn set_colour(gl: &Gl, at: &WebGlUniformLocation, colour: [u8; 4], opacity: f32) {
    gl.uniform4f(
        Some(at),
        f32::from(colour[0]) / 255.0,
        f32::from(colour[1]) / 255.0,
        f32::from(colour[2]) / 255.0,
        f32::from(colour[3]) / 255.0 * opacity,
    );
}

fn quad_vertices(quads: &[GlyphQuad]) -> Vec<f32> {
    let mut out = Vec::with_capacity(quads.len() * 24);
    for q in quads {
        for (x, y, u, v) in [
            (q.x0, q.y0, q.u0, q.v0),
            (q.x1, q.y0, q.u1, q.v0),
            (q.x1, q.y1, q.u1, q.v1),
            (q.x0, q.y0, q.u0, q.v0),
            (q.x1, q.y1, q.u1, q.v1),
            (q.x0, q.y1, q.u0, q.v1),
        ] {
            out.extend_from_slice(&[x, y, u, v]);
        }
    }
    out
}

fn box_vertices(boxes: &[ScreenBox]) -> Vec<f32> {
    let mut out = Vec::with_capacity(boxes.len() * 12);
    for b in boxes {
        out.extend_from_slice(&[
            b.x0, b.y0, b.x1, b.y0, b.x1, b.y1, b.x0, b.y0, b.x1, b.y1, b.x0, b.y1,
        ]);
    }
    out
}

fn upload_atlas(gl: &Gl, atlas: &Atlas) -> Result<WebGlTexture, JsValue> {
    let texture = gl.create_texture().ok_or("no atlas texture")?;
    gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
    gl.pixel_storei(Gl::UNPACK_ALIGNMENT, 1);
    gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
        Gl::TEXTURE_2D,
        0,
        Gl::R8 as i32,
        atlas.width as i32,
        atlas.height as i32,
        0,
        Gl::RED,
        Gl::UNSIGNED_BYTE,
        Some(&atlas.pixels),
    )?;
    // Linear between texels, clamped at the edges: the field is meant to
    // be interpolated, and a wrapped sample would fetch another glyph.
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MIN_FILTER, Gl::LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE as i32);
    gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
    Ok(texture)
}

fn as_bytes(slice: &[f32]) -> &[u8] {
    // Safety: f32 is plain old data, the length is exact, and the slice
    // outlives the call.
    unsafe {
        std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), std::mem::size_of_val(slice))
    }
}
