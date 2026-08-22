//! The terrain program: the ground surface, its relief and its light.
//!
//! The surface is one grid mesh, uploaded once and drawn for every tile
//! with different uniforms — a tile's identity is its frame and its
//! heights, not its geometry. Heights ride in an integer texture
//! (`R16UI`, the wire values untouched): `texelFetch` gives the shader
//! the exact sample the pipeline wrote, with no filtering inventing
//! slopes that are not in the data.

use wasm_bindgen::JsValue;
use web_sys::{
    WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlTexture, WebGlUniformLocation,
};

use maps2_render::{ground_mesh, GroundMesh, HeightWindow, GROUND_MESH_CELLS, HIGHLIGHT_GAIN};
use maps2_style::{HYPSOMETRIC_STOPS, LIGHT_ALTITUDE_DEG, LIGHT_AZIMUTH_DEG};
use maps2_tile::{HEIGHTS_SIDE, HEIGHT_OFFSET_M};

use crate::gl::{as_bytes, link_program};
use crate::gl_view::{ViewLocations, PROJECTION_GLSL};

const STOPS: usize = HYPSOMETRIC_STOPS.len();

/// Reading heights, for every shader that does.
///
/// A tile below the terrain cap has no raster of its own and reads the
/// nearest ancestor that has one, through a window: `u_height_window` is
/// `(scale, offset_x, offset_y)`, the identity `(1, 0, 0)` when the tile
/// reads itself. `maps2_render::HeightWindow` computes it, and
/// `maps2_render::sample_bilinear` is this arithmetic in Rust, under test.
///
/// The interpolation is written out because it has to be: the raster is
/// an `R16UI` texture and WebGL2 does not filter integer textures. Read
/// nearest-neighbour, an ancestor magnified four levels puts one texel
/// across the whole tile and the ground comes out in terraces.
///
/// Every shader here shares one copy so the ground, its shading and the
/// buildings standing on it cannot disagree about where the surface is.
pub fn heights_glsl() -> String {
    format!(
        "uniform highp usampler2D u_heights;
uniform float u_has_heights;
uniform vec3 u_height_window;

vec2 height_texel(vec2 unit) {{
    return (u_height_window.yz + unit * u_height_window.x) * {last}.0;
}}

float height_at(vec2 texel) {{
    vec2 within = clamp(texel, vec2(0.0), vec2({last}.0));
    vec2 low = floor(within);
    vec2 frac = within - low;
    ivec2 near = ivec2(low);
    ivec2 far = min(near + ivec2(1), ivec2({last}));
    float top = mix(
        float(texelFetch(u_heights, ivec2(near.x, near.y), 0).r),
        float(texelFetch(u_heights, ivec2(far.x, near.y), 0).r),
        frac.x);
    float bottom = mix(
        float(texelFetch(u_heights, ivec2(near.x, far.y), 0).r),
        float(texelFetch(u_heights, ivec2(far.x, far.y), 0).r),
        frac.x);
    return (mix(top, bottom, frac.y) - {offset:.1}) * u_has_heights;
}}

float height_metres(vec2 unit) {{
    return height_at(height_texel(unit));
}}",
        last = HEIGHTS_SIDE - 1,
        offset = HEIGHT_OFFSET_M,
    )
}

fn vertex_source() -> String {
    format!(
        "#version 300 es
in vec2 a_pos;
{PROJECTION_GLSL}
{HEIGHTS_GLSL}
uniform float u_exaggeration;
uniform float u_earth_radius_m;
out vec2 v_unit;
out float v_front;

void main() {{
    vec2 unit = a_pos / {span:.1};
    v_unit = unit;
    float metres = height_metres(unit);
    float relief = 1.0 + (metres / u_earth_radius_m) * u_exaggeration * u_globeness;
    vec3 p = project_nrm(unit, relief);
    v_front = p.z;
    gl_Position = clip_of(p.xy);
}}",
        HEIGHTS_GLSL = heights_glsl(),
        span = 65535.0_f32,
    )
}

fn fragment_source() -> String {
    format!(
        "#version 300 es
precision highp float;
precision highp usampler2D;
{HEIGHTS_GLSL}
uniform float u_texel_metres;
uniform float u_z_factor;
uniform float u_shading;
uniform float u_hypsometric;
uniform vec3 u_light;
uniform vec4 u_base_color;
uniform float u_stop_metres[{STOPS}];
uniform vec3 u_stop_color[{STOPS}];
in vec2 v_unit;
in float v_front;
out vec4 outColor;

vec3 hypsometric(float metres) {{
    vec3 tint = u_stop_color[0];
    for (int i = 1; i < {STOPS}; i++) {{
        float t = clamp(
            (metres - u_stop_metres[i - 1]) / (u_stop_metres[i] - u_stop_metres[i - 1]),
            0.0, 1.0
        );
        tint = mix(tint, u_stop_color[i], t);
    }}
    return tint;
}}

void main() {{
    if (v_front < 0.0) {{ discard; }}
    vec2 texel = height_texel(v_unit);
    // A raster has no border ring, so at the very edge a central
    // difference would fall back on a one-sided one and the two tiles
    // would disagree by a visible line. Stepping the gradient one
    // sample inwards makes both sides read the same field instead.
    // Inwards means inside the raster being read — which is the
    // ancestor's, for a tile below the cap, and `u_texel_metres` is that
    // ancestor's sample spacing to match.
    vec2 slope_at = clamp(texel, vec2(1.0), vec2({last}.0 - 1.0));
    float span = 2.0 * u_texel_metres;
    float east = (height_at(slope_at + vec2(1.0, 0.0)) - height_at(slope_at - vec2(1.0, 0.0))) / span;
    float north = (height_at(slope_at - vec2(0.0, 1.0)) - height_at(slope_at + vec2(0.0, 1.0))) / span;
    vec3 normal = normalize(vec3(-east * u_z_factor, -north * u_z_factor, 1.0));
    float raw = max(dot(normal, u_light), 0.0) / u_light.z;
    // Highlights compressed as in maps2_render::relative_shade: on a
    // near-white ground a full lambert highlight is just white.
    float lambert = raw > 1.0 ? 1.0 + (raw - 1.0) * {highlight} : raw;
    float shade = mix(1.0, lambert, u_shading);
    vec3 base = mix(u_base_color.rgb, hypsometric(height_at(texel)), u_hypsometric);
    outColor = vec4(base * shade, u_base_color.a);
}}",
        HEIGHTS_GLSL = heights_glsl(),
        last = HEIGHTS_SIDE - 1,
        highlight = HIGHLIGHT_GAIN,
    )
}

/// Per-draw settings of the ground surface.
#[derive(Clone, Copy, Debug)]
pub struct TerrainSettings {
    pub shading: f32,
    pub hypsometric: f32,
    pub z_factor: f32,
    pub exaggeration: f32,
    pub base_color: [u8; 4],
}

/// One tile's answer to "where is the ground": the raster to read, the
/// window into it, and how far apart that raster's samples are on the
/// ground. Passed whole so the three shaders that read terrain cannot be
/// handed different halves of it.
#[derive(Clone, Copy)]
pub struct HeightBinding<'a> {
    pub texture: &'a HeightTexture,
    pub window: HeightWindow,
    pub texel_metres: f32,
}

pub struct TerrainProgram {
    program: WebGlProgram,
    pub view: ViewLocations,
    a_pos: u32,
    has_heights: WebGlUniformLocation,
    height_window: WebGlUniformLocation,
    texel_metres: WebGlUniformLocation,
    z_factor: WebGlUniformLocation,
    shading: WebGlUniformLocation,
    hypsometric: WebGlUniformLocation,
    exaggeration: WebGlUniformLocation,
    base_color: WebGlUniformLocation,
}

impl TerrainProgram {
    pub fn link(gl: &Gl) -> Result<Self, JsValue> {
        let program = link_program(gl, &vertex_source(), &fragment_source())?;
        let uniform = |name: &str| {
            gl.get_uniform_location(&program, name)
                .ok_or_else(|| JsValue::from_str(&format!("missing uniform {name}")))
        };
        let parts = (
            ViewLocations::of(gl, &program)?,
            gl.get_attrib_location(&program, "a_pos") as u32,
            uniform("u_has_heights")?,
            uniform("u_height_window")?,
            uniform("u_texel_metres")?,
            uniform("u_z_factor")?,
            uniform("u_shading")?,
            uniform("u_hypsometric")?,
            uniform("u_exaggeration")?,
            uniform("u_base_color")?,
        );
        set_constants(gl, &program, &uniform)?;
        Ok(Self {
            view: parts.0,
            a_pos: parts.1,
            has_heights: parts.2,
            height_window: parts.3,
            texel_metres: parts.4,
            z_factor: parts.5,
            shading: parts.6,
            hypsometric: parts.7,
            exaggeration: parts.8,
            base_color: parts.9,
            program,
        })
    }

    pub fn bind(&self, gl: &Gl, settings: TerrainSettings) {
        gl.use_program(Some(&self.program));
        gl.uniform1f(Some(&self.shading), settings.shading);
        gl.uniform1f(Some(&self.hypsometric), settings.hypsometric);
        gl.uniform1f(Some(&self.z_factor), settings.z_factor);
        gl.uniform1f(Some(&self.exaggeration), settings.exaggeration);
        let c = settings.base_color;
        gl.uniform4f(
            Some(&self.base_color),
            f32::from(c[0]) / 255.0,
            f32::from(c[1]) / 255.0,
            f32::from(c[2]) / 255.0,
            f32::from(c[3]) / 255.0,
        );
    }

    /// Per tile: the raster it reads — its own, or an ancestor's seen
    /// through a window — and that raster's ground distance between two
    /// samples. `None` is flat ground, which is what a tile with no
    /// terrain above it has always drawn.
    pub fn bind_heights(&self, gl: &Gl, source: Option<HeightBinding<'_>>) {
        gl.active_texture(Gl::TEXTURE0);
        gl.bind_texture(Gl::TEXTURE_2D, source.map(|s| &s.texture.texture));
        gl.uniform1f(Some(&self.has_heights), f32::from(u8::from(source.is_some())));
        let window = source.map_or(HeightWindow::IDENTITY, |s| s.window);
        gl.uniform3f(Some(&self.height_window), window.scale, window.offset_x, window.offset_y);
        gl.uniform1f(Some(&self.texel_metres), source.map_or(1.0, |s| s.texel_metres));
    }

    pub fn attribute(&self) -> u32 {
        self.a_pos
    }
}

/// Light direction, Earth radius and the hypsometric ramp never change
/// while the map lives, so they are set once, at link time.
fn set_constants(
    gl: &Gl,
    program: &WebGlProgram,
    uniform: &impl Fn(&str) -> Result<WebGlUniformLocation, JsValue>,
) -> Result<(), JsValue> {
    gl.use_program(Some(program));
    let azimuth = LIGHT_AZIMUTH_DEG.to_radians();
    let altitude = LIGHT_ALTITUDE_DEG.to_radians();
    gl.uniform3f(
        Some(&uniform("u_light")?),
        azimuth.sin() * altitude.cos(),
        azimuth.cos() * altitude.cos(),
        altitude.sin(),
    );
    gl.uniform1f(
        Some(&uniform("u_earth_radius_m")?),
        (maps2_units::EARTH_CIRCUMFERENCE_METRES / std::f64::consts::TAU) as f32,
    );
    let metres: Vec<f32> = HYPSOMETRIC_STOPS.iter().map(|s| s.0).collect();
    let colours: Vec<f32> = HYPSOMETRIC_STOPS
        .iter()
        .flat_map(|s| s.1.map(|c| f32::from(c) / 255.0))
        .collect();
    gl.uniform1fv_with_f32_array(Some(&uniform("u_stop_metres[0]")?), &metres);
    gl.uniform3fv_with_f32_array(Some(&uniform("u_stop_color[0]")?), &colours);
    gl.uniform1i(Some(&uniform("u_heights")?), 0);
    Ok(())
}

/// One tile's heights on the GPU, wire values untouched.
pub struct HeightTexture {
    pub(crate) texture: WebGlTexture,
}

impl HeightTexture {
    pub fn upload(gl: &Gl, bytes: &[u8]) -> Result<Self, JsValue> {
        let texture = gl.create_texture().ok_or("no texture")?;
        gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
        let words: Vec<u16> =
            bytes.chunks_exact(2).map(|w| u16::from_le_bytes([w[0], w[1]])).collect();
        // Safety: the view does not outlive `words`, and nothing
        // allocates on the JS heap between making it and using it.
        let view = unsafe { js_sys::Uint16Array::view(&words) };
        let side = HEIGHTS_SIDE as i32;
        gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_array_buffer_view(
            Gl::TEXTURE_2D,
            0,
            Gl::R16UI as i32,
            side,
            side,
            0,
            Gl::RED_INTEGER,
            Gl::UNSIGNED_SHORT,
            Some(&view),
        )?;
        // Integer textures cannot filter, and should not: an invented
        // sample between two real ones is an invented slope.
        for (name, value) in [
            (Gl::TEXTURE_MIN_FILTER, Gl::NEAREST),
            (Gl::TEXTURE_MAG_FILTER, Gl::NEAREST),
            (Gl::TEXTURE_WRAP_S, Gl::CLAMP_TO_EDGE),
            (Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE),
        ] {
            gl.tex_parameteri(Gl::TEXTURE_2D, name, value as i32);
        }
        Ok(Self { texture })
    }

    pub fn delete(&self, gl: &Gl) {
        gl.delete_texture(Some(&self.texture));
    }
}

/// The one ground mesh every tile is drawn with.
pub struct GroundMeshGpu {
    vertices: WebGlBuffer,
    indices: WebGlBuffer,
    index_count: i32,
}

impl GroundMeshGpu {
    pub fn upload(gl: &Gl) -> Result<Self, JsValue> {
        let mesh: GroundMesh = ground_mesh(GROUND_MESH_CELLS);
        let vertices = gl.create_buffer().ok_or("no ground vertex buffer")?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&vertices));
        let flat: Vec<u16> = mesh.vertices.iter().flat_map(|v| [v.x, v.y]).collect();
        gl.buffer_data_with_u8_array(Gl::ARRAY_BUFFER, as_bytes(&flat), Gl::STATIC_DRAW);
        let indices = gl.create_buffer().ok_or("no ground index buffer")?;
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&indices));
        gl.buffer_data_with_u8_array(
            Gl::ELEMENT_ARRAY_BUFFER,
            as_bytes(&mesh.indices),
            Gl::STATIC_DRAW,
        );
        Ok(Self { vertices, indices, index_count: mesh.indices.len() as i32 })
    }

    pub fn bind(&self, gl: &Gl, a_pos: u32) {
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&self.vertices));
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&self.indices));
        gl.enable_vertex_attrib_array(a_pos);
        gl.vertex_attrib_pointer_with_i32(a_pos, 2, Gl::UNSIGNED_SHORT, false, 4, 0);
    }

    pub fn draw(&self, gl: &Gl) {
        gl.draw_elements_with_i32(Gl::TRIANGLES, self.index_count, Gl::UNSIGNED_INT, 0);
    }
}
