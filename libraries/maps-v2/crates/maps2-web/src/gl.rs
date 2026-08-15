//! Thin WebGL2 wrapper: one fill program, one buffer pair per tile.

use wasm_bindgen::JsValue;
use web_sys::{WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlUniformLocation};

use maps2_render::{ClassRange, FillBucket};
use maps2_units::TILE_EXTENT;

use crate::gl_view::{ViewLocations, PROJECTION_GLSL};

/// Vertices arrive on the tile's own integer grid; the shared
/// projection puts them on whichever shape the world is in.
fn vertex_source() -> String {
    format!(
        "#version 300 es
in vec2 a_pos;
{PROJECTION_GLSL}
out float v_front;
void main() {{
    vec3 p = project_nrm(a_pos / {extent:.1}, 1.0);
    v_front = p.z;
    gl_Position = clip_of(p.xy);
}}",
        extent = f64::from(TILE_EXTENT),
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

pub struct FillProgram {
    program: WebGlProgram,
    pub view: ViewLocations,
    pub u_color: WebGlUniformLocation,
    pub a_pos: u32,
}

impl FillProgram {
    pub fn link(gl: &Gl) -> Result<Self, JsValue> {
        let program = link_program(gl, &vertex_source(), FRAGMENT_SRC)?;
        let uniform = |name: &str| {
            gl.get_uniform_location(&program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
        Ok(Self {
            view: ViewLocations::of(gl, &program)?,
            u_color: uniform("u_color")?,
            a_pos: gl.get_attrib_location(&program, "a_pos") as u32,
            program,
        })
    }

    pub fn bind(&self, gl: &Gl) {
        gl.use_program(Some(&self.program));
    }
}

/// A tile's mesh on the GPU. Uploaded once; frames only bind it.
pub struct GpuBucket {
    vertices: WebGlBuffer,
    indices: WebGlBuffer,
    pub ranges: Vec<ClassRange>,
}

impl GpuBucket {
    pub fn upload(gl: &Gl, bucket: &FillBucket) -> Result<Self, JsValue> {
        let vertices = gl.create_buffer().ok_or("no vertex buffer")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertices));
        let flat: Vec<u16> = bucket.vertices.iter().flat_map(|v| [v.x, v.y]).collect();
        // A copy at upload time, once per tile — never per frame.
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, as_bytes(&flat), Gl::STATIC_DRAW);

        let indices = gl.create_buffer().ok_or("no index buffer")?;
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&indices));
        gl.buffer_data_with_u8_array(
            Gl::ELEMENT_ARRAY_BUFFER,
            as_bytes(&bucket.indices),
            Gl::STATIC_DRAW,
        );
        Ok(Self { vertices, indices, ranges: bucket.ranges.clone() })
    }

    /// Frees the GPU buffers on eviction; the struct is dropped after.
    pub fn delete(&self, gl: &Gl) {
        gl.delete_buffer(Some(&self.vertices));
        gl.delete_buffer(Some(&self.indices));
    }

    pub fn bind(&self, gl: &Gl, a_pos: u32) {
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.vertices));
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&self.indices));
        gl.enable_vertex_attrib_array(a_pos);
        gl.vertex_attrib_pointer_with_i32(a_pos, 2, Gl::UNSIGNED_SHORT, false, 4, 0);
    }
}

pub(crate) fn as_bytes<T: bytemuck_like::Pod>(slice: &[T]) -> &[u8] {
    // Safety: T is a plain-old-data numeric type (u16/u32), the length
    // is exact, and the slice lives for the duration of the call.
    unsafe {
        std::slice::from_raw_parts(slice.as_ptr().cast::<u8>(), std::mem::size_of_val(slice))
    }
}

/// The plain-old-data types we upload; a private stand-in for bytemuck.
pub(crate) mod bytemuck_like {
    pub trait Pod {}
    impl Pod for u16 {}
    impl Pod for u32 {}
    impl Pod for maps2_render::LineVertex {}
}

pub(crate) fn link_program(gl: &Gl, vertex: &str, fragment: &str) -> Result<WebGlProgram, JsValue> {
    let compile = |kind: u32, src: &str| -> Result<web_sys::WebGlShader, JsValue> {
        let shader = gl.create_shader(kind).ok_or("no shader")?;
        gl.shader_source(&shader, src);
        gl.compile_shader(&shader);
        if gl
            .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
            .as_bool()
            .unwrap_or(false)
        {
            Ok(shader)
        } else {
            Err(JsValue::from_str(
                &gl.get_shader_info_log(&shader).unwrap_or_default(),
            ))
        }
    };
    let program = gl.create_program().ok_or("no program")?;
    gl.attach_shader(&program, &compile(Gl::VERTEX_SHADER, vertex)?);
    gl.attach_shader(&program, &compile(Gl::FRAGMENT_SHADER, fragment)?);
    gl.link_program(&program);
    if gl
        .get_program_parameter(&program, Gl::LINK_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        Ok(program)
    } else {
        Err(JsValue::from_str(
            &gl.get_program_info_log(&program).unwrap_or_default(),
        ))
    }
}
