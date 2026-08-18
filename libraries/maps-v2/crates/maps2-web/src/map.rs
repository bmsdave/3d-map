//! The `Map` handle exported to JS.

use std::collections::{HashMap, HashSet};

use wasm_bindgen::prelude::*;
use web_sys::WebGl2RenderingContext as Gl;

use maps2_camera::{Camera, CameraPatch, Globeness};
use maps2_render::{
    build_building_bucket, build_fill_bucket, build_label_bucket, build_line_bucket, building_lod,
    normalise_source_levels, plan_residency, register_source_level, road_passes, shading_z_factor,
    target_level, texel_metres, tile_frame, BuildingLod, FillBucket, LabelBucket, LineOptions,
    Pass, RoadLevel, RoadPass, MITER_LIMIT_MAX, View,
};
use maps2_style::{
    background_color, class_alpha, facade_colour, fill_color, road_width_px, Band, Class,
    CASING_EXTRA_PX, ROAD_CASING_COLOR, TUNNEL_ALPHA, TUNNEL_DASH_ON_PX, TUNNEL_DASH_PERIOD_PX,
};
use maps2_text::{layout_line, measure_line, Atlas, Placement, Rejection, ScreenBox};
use maps2_tile::{HeightsRaster, TileView, CLASS_HEIGHTS, HEIGHTS_SIDE};
use maps2_units::{locate, Lonlat, TileId, Zoom};

use crate::gl::{FillProgram, GpuBucket};
use crate::gl_building::{BuildingProgram, GpuBuildingBucket};
use crate::gl_terrain::{GroundMeshGpu, HeightTexture, TerrainProgram, TerrainSettings};
use crate::gl_view::TilePixels;
use crate::input::Input;
use crate::labels::{class_of, frame_candidates, frame_quads, place_frame};
use crate::line_gl::{GpuLineBucket, LineProgram};
use crate::text_gl::{BoxProgram, TextDraw, TextProgram};
use crate::transform::place_tile;

/// Levels the fixture package is cut at; the real pipeline will ship a
/// manifest instead.
const DEFAULT_SOURCE_LEVELS: [u8; 7] = [0, 5, 8, 10, 12, 14, 16];
const LABEL_DRAW_CLASSES: [Class; 9] = [
    Class::Poi,
    Class::Label,
    Class::RoadMotorway,
    Class::RoadTrunk,
    Class::RoadPrimary,
    Class::RoadSecondary,
    Class::RoadResidential,
    Class::RoadService,
    Class::RoadPath,
];

fn tile_paths(ids: &[TileId]) -> String {
    let paths = ids
        .iter()
        .map(|id| format!("\"{}/{}/{}.mt2\"", id.z, id.x, id.y))
        .collect::<Vec<_>>();
    format!("[{}]", paths.join(","))
}

#[wasm_bindgen]
pub struct Map {
    gl: Gl,
    program: FillProgram,
    building_program: BuildingProgram,
    line_program: LineProgram,
    text: TextProgram,
    boxes: BoxProgram,
    atlas: Atlas,
    terrain: TerrainProgram,
    ground: GroundMeshGpu,
    camera: Camera,
    /// The bytes the host handed in, kept so join geometry can be
    /// rebuilt when the miter limit moves — a debug knob, never a frame.
    tiles: HashMap<TileId, Vec<u8>>,
    /// Every tile the host handed in, as CPU meshes.
    cpu: HashMap<TileId, FillBucket>,
    lines: HashMap<TileId, maps2_render::LineBucket>,
    buildings: HashMap<TileId, maps2_render::BuildingBucket>,
    /// Names of every tile the host handed in. Cheap, so they are not
    /// evicted with the GPU buffers — the placement pass filters by
    /// residency instead.
    names: HashMap<TileId, LabelBucket>,
    /// Only what residency admits lives on the GPU.
    gpu: HashMap<TileId, GpuBucket>,
    gpu_lines: HashMap<TileId, GpuLineBucket>,
    gpu_buildings: HashMap<TileId, GpuBuildingBucket>,
    /// Heights follow the same two lives as the meshes: the bytes stay,
    /// the texture comes and goes with residency.
    heights: HashMap<TileId, Vec<u8>>,
    height_textures: HashMap<TileId, HeightTexture>,
    viewport: (f64, f64),
    frame_draw_calls: u32,
    frame_tiles: u32,
    evictions: u32,
    override_band: Option<Band>,
    animate: bool,
    alphas: HashMap<Class, f32>,
    labels: LabelState,
    relief: Relief,
    input: Input,
    line_options: LineOptions,
    casing: bool,
    width_overrides: HashMap<Class, f32>,
    /// The road draw order, built once: a frame walks it, never makes it.
    passes: Vec<RoadPass>,
    /// The package pyramid supplied by the host. Fixture levels make the lab
    /// useful before tiles arrive; real packages add their own levels.
    source_levels: Vec<u8>,
    /// Building mesh detail for the current camera zoom. Checked once a
    /// frame and only rebuilds CPU/GPU buckets on a tier change, not on
    /// every frame — the same one-off-rebuild shape as `rebuild_lines`.
    building_lod: BuildingLod,
}

/// Everything the label pass is steered by, and the last answer it gave.
struct LabelState {
    /// Share of the viewport labels may cover.
    budget: f32,
    halo_em: f32,
    show_boxes: bool,
    /// A type specimen instead of the map's own labels: the one card
    /// that wants to see the face rather than the cartography.
    specimen: Option<String>,
    placement: Placement,
    candidates: usize,
    placement_dirty: bool,
}

impl Default for LabelState {
    fn default() -> Self {
        Self {
            budget: DEFAULT_LABEL_BUDGET,
            halo_em: DEFAULT_HALO_EM,
            show_boxes: false,
            specimen: None,
            placement: Placement::default(),
            candidates: 0,
            placement_dirty: true,
        }
    }
}

/// What the style says about relief right now. Expressiveness governs
/// the sheet, exaggeration the sphere; both are style parameters, not
/// data (VISION: the palette must not rebuild the pyramid).
#[derive(Clone, Copy, Debug)]
struct Relief {
    shaded: bool,
    hypsometric: bool,
    expressiveness: f32,
    exaggeration: f32,
}

impl Default for Relief {
    fn default() -> Self {
        Self { shaded: false, hypsometric: false, expressiveness: 0.5, exaggeration: 40.0 }
    }
}

/// Per-frame step of an animated composition change: a quarter of the
/// remaining distance, settling in ~250 ms at 60 fps.
const ALPHA_STEP: f32 = 0.25;

/// Width of the anti-aliased edge of a ribbon, in screen pixels.
const LINE_FEATHER_PX: f32 = 1.0;
/// How much of the screen labels may take before the frame stops
/// choosing. Eight percent is a quiet map; the density card argues with
/// the number, which is why it is a knob and not a constant in a shader.
const DEFAULT_LABEL_BUDGET: f32 = 0.08;

/// Halo width in em, so it scales with the type it protects.
const DEFAULT_HALO_EM: f32 = 0.07;

/// The halo colour: the quiet canvas itself, so type reads over water,
/// park and buildings alike.
const HALO_COLOUR: [u8; 4] = [0xF8, 0xF7, 0xF4, 0xE6];

/// Sizes the specimen card walks the face through.
const SPECIMEN_SIZES: [f32; 6] = [11.0, 14.0, 18.0, 24.0, 32.0, 44.0];

#[wasm_bindgen]
impl Map {
    /// The canvas is looked up by id; the host owns its size.
    #[wasm_bindgen(constructor)]
    pub fn new(canvas_id: &str) -> Result<Map, JsValue> {
        let document = web_sys::window().ok_or("no window")?.document().ok_or("no document")?;
        let canvas = document
            .get_element_by_id(canvas_id)
            .ok_or_else(|| JsValue::from_str(&format!("no canvas #{canvas_id}")))?
            .dyn_into::<web_sys::HtmlCanvasElement>()?;
        let gl = canvas
            .get_context("webgl2")?
            .ok_or("webgl2 unavailable")?
            .dyn_into::<Gl>()?;
        let program = FillProgram::link(&gl)?;
        let building_program = BuildingProgram::link(&gl)?;
        let line_program = LineProgram::link(&gl)?;
        let terrain = TerrainProgram::link(&gl)?;
        let ground = GroundMeshGpu::upload(&gl)?;
        // Built once, here: a few hundred kilobytes of arithmetic, and
        // never a frame's business.
        let atlas = Atlas::build();
        let text = TextProgram::link(&gl, &atlas)?;
        let boxes = BoxProgram::link(&gl)?;
        gl.enable(Gl::BLEND);
        gl.blend_func(Gl::SRC_ALPHA, Gl::ONE_MINUS_SRC_ALPHA);
        let viewport = (f64::from(canvas.width()), f64::from(canvas.height()));
        Ok(Map {
            gl,
            program,
            building_program,
            line_program,
            text,
            boxes,
            atlas,
            terrain,
            ground,
            camera: Camera::new(Lonlat { lon: -0.3049, lat: 51.5149 }, Zoom::new(12.0)),
            tiles: HashMap::new(),
            cpu: HashMap::new(),
            lines: HashMap::new(),
            buildings: HashMap::new(),
            names: HashMap::new(),
            gpu: HashMap::new(),
            gpu_lines: HashMap::new(),
            gpu_buildings: HashMap::new(),
            heights: HashMap::new(),
            height_textures: HashMap::new(),
            viewport,
            frame_draw_calls: 0,
            frame_tiles: 0,
            evictions: 0,
            override_band: None,
            animate: false,
            alphas: HashMap::new(),
            labels: LabelState::default(),
            relief: Relief::default(),
            input: Input::default(),
            line_options: LineOptions::default(),
            casing: true,
            width_overrides: HashMap::new(),
            passes: road_passes(),
            source_levels: DEFAULT_SOURCE_LEVELS.to_vec(),
            building_lod: building_lod(12.0),
        })
    }

    /// Draw the grey outline under every road, or not — the card's
    /// switch, and the way to see what the casing is actually doing.
    pub fn set_road_casing(&mut self, on: bool) {
        self.casing = on;
    }

    /// How far a corner may reach before it bevels. Join geometry is
    /// decided when the bucket is built, so this rebuilds them.
    pub fn set_miter_limit(&mut self, limit: f64) -> Result<(), JsValue> {
        let limit = limit as f32;
        if !(1.0..=MITER_LIMIT_MAX).contains(&limit) {
            return Err(JsValue::from_str(&format!(
                "miter limit {limit} outside 1..{MITER_LIMIT_MAX}"
            )));
        }
        self.line_options.miter_limit = limit;
        self.rebuild_lines();
        Ok(())
    }

    /// Pin one class's width in screen pixels, instead of the style's
    /// zoom ramp.
    pub fn set_road_width_px(&mut self, class: &str, px: f64) -> Result<(), JsValue> {
        let class = maps2_style::ALL_CLASSES
            .into_iter()
            .find(|c| format!("{c:?}") == class)
            .ok_or_else(|| JsValue::from_str(&format!("unknown class {class}")))?;
        self.width_overrides.insert(class, px as f32);
        Ok(())
    }

    /// Share of the viewport labels may cover, 0..1. The scarce resource
    /// is screen area, not feature count.
    pub fn set_label_budget(&mut self, budget: f64) {
        self.labels.budget = (budget as f32).clamp(0.0, 1.0);
        self.labels.placement_dirty = true;
    }

    /// Halo width in em.
    pub fn set_halo_em(&mut self, halo: f64) {
        self.labels.halo_em = (halo as f32).clamp(0.0, 0.2);
    }

    /// Draw the collision boxes over the map: green placed, red refused.
    pub fn set_collision_boxes(&mut self, on: bool) {
        self.labels.show_boxes = on;
    }

    /// Show a type specimen instead of the map's own labels.
    pub fn set_specimen(&mut self, text: Option<String>) {
        self.labels.specimen = text;
    }

    pub fn set_bearing(&mut self, degrees: f64) -> Result<(), JsValue> {
        let patch = CameraPatch { bearing_deg: Some(degrees), ..CameraPatch::default() };
        self.steer(&patch)
    }

    pub fn set_tilt(&mut self, degrees: f64) -> Result<(), JsValue> {
        let patch = CameraPatch { tilt_deg: Some(degrees), ..CameraPatch::default() };
        self.steer(&patch)
    }

    /// Hillshade on or off. The ground surface is drawn either way —
    /// what changes is whether the light describes its slopes.
    pub fn set_relief(&mut self, shaded: bool) {
        self.relief.shaded = shaded;
    }

    /// Colour the ground by height instead of one flat land colour.
    pub fn set_hypsometric(&mut self, on: bool) {
        self.relief.hypsometric = on;
    }

    /// How loud the shading is allowed to be on the flat sheet, `0..1`.
    pub fn set_relief_expressiveness(&mut self, value: f64) {
        self.relief.expressiveness = value.clamp(0.0, 1.0) as f32;
    }

    /// Vertical exaggeration of the globe's relief, `r = R(1 + h·k·g)`.
    pub fn set_relief_exaggeration(&mut self, value: f64) {
        self.relief.exaggeration = value.clamp(0.0, 400.0) as f32;
    }

    /// Pin the composition to a band regardless of zoom (toggle cards,
    /// debugging), or drop the pin with `None`.
    pub fn set_band_override(&mut self, band: Option<String>) -> Result<(), JsValue> {
        self.override_band = match band {
            None => None,
            Some(name) => Some(
                Band::from_name(&name)
                    .ok_or_else(|| JsValue::from_str(&format!("unknown band {name}")))?,
            ),
        };
        Ok(())
    }

    /// Animated composition changes: alphas move towards their target a
    /// step per frame instead of jumping.
    pub fn set_transition_animated(&mut self, animated: bool) {
        self.animate = animated;
    }

    /// How much of the globe the camera zoom leaves on screen. The
    /// renderer draws the flat sheet only for now; the number is the
    /// camera's truth and the globe card's readout.
    #[must_use]
    pub fn globeness(&self) -> f64 {
        Globeness::at(self.camera.zoom()).value()
    }

    /// Ingest one tile: the CPU mesh is built here, once. The GPU copy
    /// appears when residency admits the tile, and leaves when it stops.
    pub fn load_tile(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
        let view = TileView::parse(bytes).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
        let id = view.header().id;
        register_source_level(&mut self.source_levels, id.z);
        let fills = build_fill_bucket(&view).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
        let buildings = build_building_bucket(&view, self.building_lod).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
        let roads = build_line_bucket(&view, self.line_options)
            .map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
        let names = build_label_bucket(&view).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
        self.cpu.insert(id, fills);
        self.buildings.insert(id, buildings);
        self.lines.insert(id, roads);
        self.names.insert(id, names);
        self.labels.placement_dirty = true;
        self.tiles.insert(id, bytes.to_vec());
        if let Some(raster) = view.raster(CLASS_HEIGHTS) {
            HeightsRaster::parse(raster).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
            self.heights.insert(id, raster.to_vec());
        }
        Ok(())
    }

    /// Remove every CPU and GPU representation of one host-managed tile.
    pub fn unload_tile(&mut self, z: u8, x: u32, y: u32) {
        let id = TileId { z, x, y };
        self.tiles.remove(&id);
        self.cpu.remove(&id);
        self.lines.remove(&id);
        self.buildings.remove(&id);
        self.names.remove(&id);
        self.labels.placement_dirty = true;
        self.heights.remove(&id);
        if let Some(bucket) = self.gpu.remove(&id) { bucket.delete(&self.gl); }
        if let Some(bucket) = self.gpu_lines.remove(&id) { bucket.delete(&self.gl); }
        if let Some(bucket) = self.gpu_buildings.remove(&id) { bucket.delete(&self.gl); }
        if let Some(texture) = self.height_textures.remove(&id) { texture.delete(&self.gl); }
    }

    /// Replaces the fixture pyramid with the levels declared by a package.
    /// The host must set this before asking for tiles from a real package.
    pub fn set_source_levels(&mut self, levels: Vec<u8>) -> Result<(), JsValue> {
        self.source_levels = normalise_source_levels(levels)
            .ok_or_else(|| JsValue::from_str("a package needs at least one source level"))?;
        Ok(())
    }

    /// Tile paths that cover the viewport but have not reached the host yet.
    /// Fetch policy belongs to the host, so this only names the deterministic
    /// demand and does not perform network work during a frame.
    #[must_use]
    pub fn missing_tiles(&self) -> String {
        let available = self.tiles.keys().copied().collect::<HashSet<_>>();
        let plan = plan_residency(&self.camera, self.viewport, &self.source_levels, &available);
        tile_paths(&plan.missing)
    }

    /// Loaded tile paths outside the viewport and one-tile keep margin.
    /// The host owns fetching and CPU memory, so it releases these bytes.
    #[must_use]
    pub fn evictable_tiles(&self) -> String {
        let available = self.tiles.keys().copied().collect::<HashSet<_>>();
        let plan = plan_residency(&self.camera, self.viewport, &self.source_levels, &available);
        tile_paths(&plan.evict)
    }

    pub fn set_zoom(&mut self, zoom: f64) -> Result<(), JsValue> {
        self.steer(&CameraPatch { zoom: Some(zoom), ..CameraPatch::default() })
    }

    pub fn set_centre(&mut self, lon: f64, lat: f64) -> Result<(), JsValue> {
        let centre = Some(Lonlat { lon, lat });
        self.steer(&CameraPatch { centre, ..CameraPatch::default() })
    }

    /// The pointer took hold: a drag begins, and whatever the map was
    /// still doing on its own stops.
    pub fn pointer_down(&mut self, x: f64, y: f64, now_ms: f64) {
        self.input.pointer_down((x, y), now_ms);
    }

    /// # Errors
    ///
    /// Only if the host hands in a coordinate that is not a number.
    pub fn pointer_move(&mut self, x: f64, y: f64, now_ms: f64) -> Result<bool, JsValue> {
        let Some(patch) = self.input.pointer_move(&self.camera, (x, y), now_ms) else {
            return Ok(false);
        };
        self.steer(&patch)?;
        Ok(true)
    }

    pub fn pointer_up(&mut self) {
        self.input.pointer_up();
    }

    /// A wheel notch, or a trackpad pinch — which the browser delivers as
    /// a wheel with `ctrlKey`, so the host passes that flag through.
    ///
    /// # Errors
    ///
    /// Only if the host hands in a delta that is not a number.
    pub fn wheel(&mut self, x: f64, y: f64, delta_y: f64, pinch: bool) -> Result<(), JsValue> {
        let patch = self.input.wheel(&self.camera, self.viewport, (x, y), delta_y, pinch);
        self.steer(&patch)
    }

    pub fn double_click(&mut self, x: f64, y: f64) {
        self.input.double_click(&self.camera, (x, y));
    }

    /// Whether the key was the map's. The host suppresses the browser's
    /// own handling of it only when this says yes.
    ///
    /// # Errors
    ///
    /// Only if the resulting camera is not a finite one.
    pub fn key(&mut self, key: &str) -> Result<bool, JsValue> {
        let Some(patch) = self.input.key(&self.camera, self.viewport, key) else {
            return Ok(false);
        };
        self.steer(&patch)?;
        Ok(true)
    }

    /// One frame of what runs by itself — inertia, the double-click zoom.
    /// Answers whether the camera is still moving, so the host knows to
    /// ask for another frame.
    ///
    /// # Errors
    ///
    /// Only if the host hands in a frame time that is not a number.
    pub fn tick(&mut self, dt_ms: f64) -> Result<bool, JsValue> {
        if let Some(patch) = self.input.tick(&self.camera, self.viewport, dt_ms) {
            self.steer(&patch)?;
        }
        Ok(!self.input.is_idle())
    }

    pub fn render(&mut self) {
        let gl = &self.gl;
        gl.viewport(0, 0, self.viewport.0 as i32, self.viewport.1 as i32);
        let void = background_color();
        gl.clear_color(
            f32::from(void[0]) / 255.0,
            f32::from(void[1]) / 255.0,
            f32::from(void[2]) / 255.0,
            1.0,
        );
        gl.clear(Gl::COLOR_BUFFER_BIT);
        self.frame_draw_calls = 0;
        self.frame_tiles = 0;
        // A specimen is a page of type, not a map: drawing cartography
        // under it would only mean the frame answers to the camera, and
        // the one thing the card exists to show is that it does not.
        if self.labels.specimen.is_some() {
            self.draw_labels(&[]);
            return;
        }
        self.sync_building_lod();
        let draw = self.apply_residency();
        self.draw_ground(&draw);
        self.draw_buildings(&draw);
        self.program.bind(&self.gl);
        self.program.view.set_view(&self.gl, &self.view());
        for id in &draw {
            self.draw_tile(*id);
        }
        // Every fill of the frame is down before the first road: a road
        // must not disappear under the neighbouring tile's land.
        self.draw_roads(&draw);
        self.draw_labels(&draw);
    }

    /// One pixel of the last frame, `"r,g,b,a"`, with the origin at the
    /// top-left like every other screen coordinate here. A test hook:
    /// it reads back from the GPU, so it is something a host asks for
    /// after a frame, never something a frame pays for.
    pub fn sample_pixel(&self, x: u32, y: u32) -> String {
        let mut pixel = [0_u8; 4];
        let bottom_up = (self.viewport.1 as u32).saturating_sub(y + 1) as i32;
        let _ = self.gl.read_pixels_with_opt_u8_array(
            x as i32,
            bottom_up,
            1,
            1,
            Gl::RGBA,
            Gl::UNSIGNED_BYTE,
            Some(&mut pixel),
        );
        format!("{},{},{},{}", pixel[0], pixel[1], pixel[2], pixel[3])
    }

    /// Observable state for readouts and e2e. A string of JSON — the
    /// host parses; nothing here runs unless asked.
    pub fn debug(&self) -> String {
        let zoom = self.camera.zoom();
        let classes = self.resident_classes();
        let composition = self
            .override_band
            .map_or_else(|| format!("{:?}", Band::at(zoom)), |b| format!("{b:?}"));
        let centre = self.camera.centre();
        format!(
            "{{\"zoom\":{:.4},\"centre_lon\":{:.6},\"centre_lat\":{:.6},\"bearing\":{:.2},\"moving\":{},\"band\":\"{:?}\",\"composition\":\"{}\",\"override\":{},\"settled\":{},\"tile_level\":{},\"resident_classes\":{:?},\"tiles_drawn\":{},\"draw_calls\":{},\"resident_gpu\":{},\"evictions\":{},\"tilt\":{:.1},\"label_candidates\":{},\"labels_placed\":{},\"labels_rejected\":{},\"label_occupancy\":{:.4},\"label_budget\":{:.4},{},{}}}",
            zoom.value(),
            centre.lon,
            centre.lat,
            self.camera.bearing_deg(),
            !self.input.is_idle(),
            Band::at(zoom),
            composition,
            self.override_band.is_some(),
            self.settled(),
            self.active_level(),
            classes,
            self.frame_tiles,
            self.frame_draw_calls,
            self.gpu.len(),
            self.evictions,
            self.camera.tilt_deg(),
            self.labels.candidates,
            self.labels.placement.placed.len(),
            self.labels.placement.rejected.len(),
            self.labels.placement.occupancy,
            self.labels.budget,
            self.roads_debug(),
            self.debug_relief(),
        )
    }

    /// The frame's label decisions, one entry per candidate that got as
    /// far as being judged. This is the surface the tests read: boxes,
    /// ranks and reasons, never pixels — visibility is a property of the
    /// frame, so "is X on screen at z14" is not a question with a stable
    /// answer, and asking it would only produce a flaky test.
    #[must_use]
    pub fn label_debug(&self) -> String {
        let placed = self.labels.placement.placed.iter().map(|p| {
            entry_json(p.id, p.rank, p.bounds, "placed", None, &p.text)
        });
        let rejected = self.labels.placement.rejected.iter().map(|r| {
            let (state, by) = match r.reason {
                Rejection::Collision { by } => ("collision", Some(by)),
                Rejection::Budget => ("budget", None),
                Rejection::OffScreen => ("offscreen", None),
                Rejection::Duplicate => ("duplicate", None),
            };
            entry_json(r.id, r.rank, r.bounds, state, by, "")
        });
        let all: Vec<String> = placed.chain(rejected).collect();
        format!("[{}]", all.join(","))
    }
}

fn entry_json(
    id: u128,
    rank: u8,
    bounds: ScreenBox,
    state: &str,
    by: Option<u128>,
    text: &str,
) -> String {
    let class = class_of(id).map_or_else(|| "Unknown".to_string(), |c| format!("{c:?}"));
    format!(
        "{{\"id\":\"{id}\",\"rank\":{rank},\"class\":\"{class}\",\"state\":\"{state}\",\
         \"blocked_by\":{},\"text\":{text:?},\"x\":{:.1},\"y\":{:.1},\"w\":{:.1},\"h\":{:.1}}}",
        by.map_or_else(|| "null".to_string(), |b| format!("\"{b}\"")),
        bounds.x0,
        bounds.y0,
        bounds.x1 - bounds.x0,
        bounds.y1 - bounds.y0,
    )
}

/// A tunnel runs under the ground it is drawn on, so it is drawn
/// interrupted: both its passes take the same dash, and dimming alone
/// would only make a white road paler than the near-white land.
fn set_dash(gl: &Gl, program: &LineProgram, pass: RoadPass) {
    let (period, on) = if pass.level == RoadLevel::Tunnel {
        (TUNNEL_DASH_PERIOD_PX, TUNNEL_DASH_ON_PX)
    } else {
        (0.0, 0.0)
    };
    gl.uniform2f(Some(&program.u_dash), period, on);
}

fn set_colour(gl: &Gl, program: &LineProgram, pass: RoadPass, alpha: f32) {
    let colour =
        if pass.pass == Pass::Casing { ROAD_CASING_COLOR } else { fill_color(pass.class) };
    gl.uniform4f(
        Some(&program.u_color),
        f32::from(colour[0]) / 255.0,
        f32::from(colour[1]) / 255.0,
        f32::from(colour[2]) / 255.0,
        f32::from(colour[3]) / 255.0 * alpha,
    );
}

impl Map {
    /// The one place a patch reaches the camera: refusal keeps every
    /// field as it was and travels out as a JS error.
    fn steer(&mut self, patch: &CameraPatch) -> Result<(), JsValue> {
        self.camera.apply(patch).map_err(|e| JsValue::from_str(&format!("{e:?}")))?;
        self.labels.placement_dirty = true;
        Ok(())
    }

    fn active_level(&self) -> u8 {
        target_level(self.camera.zoom().value(), &self.source_levels)
    }

    fn view(&self) -> View {
        View::of(&self.camera, self.viewport)
    }

    /// Pixel geometry of one tile on the sheet, worked out in `f64`
    /// before anything reaches a `float` uniform.
    fn tile_pixels(&self, id: TileId) -> TilePixels {
        let placement = place_tile(id, &self.camera, self.viewport.0, self.viewport.1);
        TilePixels::of(&placement, self.viewport)
    }

    /// The ground surface under everything else: one tessellated grid
    /// per tile, displaced by relief on the globe and shaded by the
    /// north-west light. It stands in for the land fill, which is why
    /// [`Self::draw_tile`] skips that class.
    fn draw_ground(&mut self, draw: &[TileId]) {
        let globeness = self.globeness() as f32;
        let settings = TerrainSettings {
            shading: f32::from(u8::from(self.relief.shaded)),
            hypsometric: f32::from(u8::from(self.relief.hypsometric)),
            z_factor: shading_z_factor(
                self.relief.expressiveness,
                self.relief.exaggeration,
                globeness,
            ),
            exaggeration: if self.relief.shaded { self.relief.exaggeration } else { 0.0 },
            base_color: fill_color(Class::Land),
        };
        self.terrain.bind(&self.gl, settings);
        self.terrain.view.set_view(&self.gl, &self.view());
        self.ground.bind(&self.gl, self.terrain.attribute());
        for id in draw {
            if !self.gpu.contains_key(id) {
                continue;
            }
            self.terrain.view.set_tile(&self.gl, tile_frame(*id), self.tile_pixels(*id));
            self.terrain.bind_heights(
                &self.gl,
                self.height_textures.get(id),
                texel_metres(*id),
            );
            self.ground.draw(&self.gl);
            self.frame_draw_calls += 1;
        }
    }

    fn draw_buildings(&mut self, draw: &[TileId]) {
        self.building_program.bind(
            &self.gl,
            self.metres_to_pixels(),
            self.camera.tilt_deg(),
        );
        self.building_program.view.set_view(&self.gl, &self.view());
        for id in draw {
            let Some(bucket) = self.gpu_buildings.get(id) else {
                continue;
            };
            self.building_program
                .view
                .set_tile(&self.gl, tile_frame(*id), self.tile_pixels(*id));
            self.building_program
                .bind_heights(&self.gl, self.height_textures.get(id));
            // One draw call per facade material present in this tile's
            // buildings, not one per building: `ranges` is already grouped.
            for &range in bucket.ranges() {
                let colour = facade_colour(range.material);
                self.gl.uniform4f(
                    Some(&self.building_program.color),
                    f32::from(colour[0]) / 255.0,
                    f32::from(colour[1]) / 255.0,
                    f32::from(colour[2]) / 255.0,
                    f32::from(colour[3]) / 255.0,
                );
                bucket.draw_range(&self.gl, &self.building_program, range);
                self.frame_draw_calls += 1;
            }
        }
    }

    fn metres_to_pixels(&self) -> f32 {
        let cos_lat = self.camera.centre().lat.to_radians().cos().max(0.01);
        (self.camera.zoom().world_pixels() / (maps2_units::EARTH_CIRCUMFERENCE_METRES * cos_lat)) as f32
    }

    /// Height in metres under the camera centre, when a resident tile
    /// can answer. The terrain card's readout, and the cheapest proof
    /// that the raster arrived and is addressed correctly.
    fn centre_height_m(&self) -> Option<f32> {
        let point = locate(self.camera.centre(), self.active_level());
        let bytes = self.heights.get(&point.tile)?;
        let raster = HeightsRaster::parse(bytes).ok()?;
        let last = (HEIGHTS_SIDE - 1) as f32;
        let texel = |grid: u16| (f32::from(grid) / 65535.0 * last).round() as i32;
        Some(raster.metres(texel(point.coord.0), texel(point.coord.1)))
    }

    /// The shape the renderer actually drew this frame.
    fn shape(&self) -> &'static str {
        let g = self.globeness();
        if g > 0.999 {
            "globe"
        } else if g < 0.001 {
            "flat"
        } else {
            "blend"
        }
    }

    fn debug_relief(&self) -> String {
        let height = self
            .centre_height_m()
            .map_or_else(|| "null".to_string(), |m| format!("{m:.0}"));
        format!(
            "\"shape\":\"{}\",\"globeness\":{:.3},\"relief\":{},\"hypsometric\":{},\"expressiveness\":{:.2},\"exaggeration\":{:.0},\"height_tiles\":{},\"centre_height_m\":{height}",
            self.shape(),
            self.globeness(),
            self.relief.shaded,
            self.relief.hypsometric,
            self.relief.expressiveness,
            self.relief.exaggeration,
            self.height_textures.len(),
        )
    }

    /// The second application of the zoom rule: promote wanted tiles to
    /// the GPU, delete the buffers of evicted ones.
    fn apply_residency(&mut self) -> Vec<TileId> {
        let resident: HashSet<TileId> = self.gpu.keys().copied().collect();
        let plan = plan_residency(&self.camera, self.viewport, &self.source_levels, &resident);
        for id in &plan.evict {
            if let Some(bucket) = self.gpu.remove(id) {
                bucket.delete(&self.gl);
                self.evictions += 1;
            }
            if let Some(bucket) = self.gpu_lines.remove(id) {
                bucket.delete(&self.gl);
            }
            if let Some(bucket) = self.gpu_buildings.remove(id) {
                bucket.delete(&self.gl);
            }
            if let Some(texture) = self.height_textures.remove(id) {
                texture.delete(&self.gl);
            }
        }
        for id in &plan.missing {
            if let Some(cpu) = self.cpu.get(id)
                && let Ok(gpu) = GpuBucket::upload(&self.gl, cpu)
            {
                self.gpu.insert(*id, gpu);
            }
            if let Some(cpu) = self.lines.get(id)
                && let Ok(gpu) = GpuLineBucket::upload(&self.gl, cpu)
            {
                self.gpu_lines.insert(*id, gpu);
            }
            if let Some(cpu) = self.buildings.get(id)
                && !cpu.indices.is_empty()
                && let Ok(gpu) = GpuBuildingBucket::upload(&self.gl, cpu)
            {
                self.gpu_buildings.insert(*id, gpu);
            }
            if let Some(bytes) = self.heights.get(id)
                && let Ok(texture) = HeightTexture::upload(&self.gl, bytes)
            {
                self.height_textures.insert(*id, texture);
            }
        }
        plan.draw
    }

    /// Rebuilds every line bucket from the bytes still held, and
    /// replaces the GPU copy of each tile that is already resident.
    /// Residency decides from the fill buckets alone, so it would never
    /// notice that the join geometry underneath had changed — dropping
    /// the buffers here and waiting for it would lose the roads.
    fn rebuild_lines(&mut self) {
        for (id, bytes) in &self.tiles {
            let Ok(view) = TileView::parse(bytes) else {
                continue;
            };
            if let Ok(bucket) = build_line_bucket(&view, self.line_options) {
                self.lines.insert(*id, bucket);
            }
        }
        for id in self.gpu_lines.keys().copied().collect::<Vec<_>>() {
            let Some(cpu) = self.lines.get(&id) else {
                continue;
            };
            let Ok(gpu) = GpuLineBucket::upload(&self.gl, cpu) else {
                continue;
            };
            if let Some(previous) = self.gpu_lines.insert(id, gpu) {
                previous.delete(&self.gl);
            }
        }
    }

    /// Checks whether the camera zoom crossed a building-detail tier
    /// boundary and, only then, rebuilds building buckets — the steady
    /// state pays one comparison, not a rebuild.
    fn sync_building_lod(&mut self) {
        let wanted = building_lod(self.camera.zoom().value());
        if wanted != self.building_lod {
            self.building_lod = wanted;
            self.rebuild_buildings();
        }
    }

    /// Rebuilds every building bucket from the bytes still held, and
    /// replaces the GPU copy of each tile that is already resident. Same
    /// shape as `rebuild_lines`: residency does not know the LOD tier
    /// changed, so dropping the buffers here and waiting for residency to
    /// notice would leave stale-detail buildings on screen.
    fn rebuild_buildings(&mut self) {
        for (id, bytes) in &self.tiles {
            let Ok(view) = TileView::parse(bytes) else {
                continue;
            };
            if let Ok(bucket) = build_building_bucket(&view, self.building_lod) {
                self.buildings.insert(*id, bucket);
            }
        }
        for id in self.gpu_buildings.keys().copied().collect::<Vec<_>>() {
            let Some(cpu) = self.buildings.get(&id) else {
                continue;
            };
            if cpu.indices.is_empty() {
                if let Some(previous) = self.gpu_buildings.remove(&id) {
                    previous.delete(&self.gl);
                }
                continue;
            }
            let Ok(gpu) = GpuBuildingBucket::upload(&self.gl, cpu) else {
                continue;
            };
            if let Some(previous) = self.gpu_buildings.insert(id, gpu) {
                previous.delete(&self.gl);
            }
        }
    }

    fn draw_roads(&mut self, draw: &[TileId]) {
        if self.gpu_lines.is_empty() {
            return;
        }
        self.line_program.bind(&self.gl);
        self.gl.uniform2f(
            Some(&self.line_program.u_viewport),
            self.viewport.0 as f32,
            self.viewport.1 as f32,
        );
        self.gl.uniform1f(Some(&self.line_program.u_feather), LINE_FEATHER_PX);
        for pass in self.passes.clone() {
            if pass.pass == Pass::Casing && !self.casing {
                continue;
            }
            for id in draw {
                self.draw_road_pass(*id, pass);
            }
        }
        self.line_program.unbind(&self.gl);
    }

    fn draw_road_pass(&mut self, id: TileId, pass: RoadPass) {
        let Some(bucket) = self.gpu_lines.get(&id) else {
            return;
        };
        let Some(range) = bucket.range(pass.class, pass.level) else {
            return;
        };
        // A tunnel is drawn as its dashed casing and nothing else: a
        // white fill under the near-white land would read as a
        // highlight, which is the opposite of running underground.
        if pass.level == RoadLevel::Tunnel && pass.pass == Pass::Fill {
            return;
        }
        let alpha = class_alpha(pass.class, self.camera.zoom(), self.override_band)
            * if pass.level == RoadLevel::Tunnel { TUNNEL_ALPHA } else { 1.0 };
        if alpha < 0.004 {
            return;
        }
        let placement = place_tile(id, &self.camera, self.viewport.0, self.viewport.1);
        bucket.bind(&self.gl, &self.line_program);
        let gl = &self.gl;
        let program = &self.line_program;
        gl.uniform2f(
            Some(&program.u_translate),
            placement.translate_x as f32,
            placement.translate_y as f32,
        );
        gl.uniform1f(Some(&program.u_scale), placement.scale as f32);
        gl.uniform1f(Some(&program.u_half_width), self.pass_half_width(pass));
        set_dash(gl, program, pass);
        set_colour(gl, program, pass, alpha);
        gl.draw_elements_with_i32(
            Gl::TRIANGLES,
            range.index_count as i32,
            Gl::UNSIGNED_INT,
            (range.first_index * 4) as i32,
        );
        self.frame_draw_calls += 1;
    }

    /// A casing is the fill plus a rim on either side; that is what makes
    /// junctions merge into one outline instead of showing a seam.
    fn pass_half_width(&self, pass: RoadPass) -> f32 {
        let width = self.road_width(pass.class);
        let extra = if pass.pass == Pass::Casing { CASING_EXTRA_PX } else { 0.0 };
        (width / 2.0 + extra).max(0.25)
    }

    fn road_width(&self, class: Class) -> f32 {
        self.width_overrides
            .get(&class)
            .copied()
            .unwrap_or_else(|| road_width_px(class, self.camera.zoom()))
    }

    fn draw_tile(&mut self, id: TileId) {
        let Some(bucket) = self.gpu.get(&id) else { return };
        let gl = &self.gl;
        bucket.bind(gl, self.program.a_pos);
        self.program.view.set_tile(gl, tile_frame(id), self.tile_pixels(id));
        self.frame_tiles += 1;
        let ranges: Vec<maps2_render::ClassRange> = bucket.ranges.clone();
        let passes: Vec<(maps2_render::ClassRange, f32)> =
            ranges.into_iter().map(|r| (r, self.current_alpha(r.class))).collect();
        for (range, alpha) in passes {
            // The ground surface already drew the land, tessellated —
            // a four-corner land rectangle would cut the globe flat.
            if alpha < 0.004 || range.class == Class::Land || range.class == Class::Building {
                continue;
            }
            let gl = &self.gl;
            let colour = fill_color(range.class);
            gl.uniform4f(
                Some(&self.program.u_color),
                f32::from(colour[0]) / 255.0,
                f32::from(colour[1]) / 255.0,
                f32::from(colour[2]) / 255.0,
                f32::from(colour[3]) / 255.0 * alpha,
            );
            gl.draw_elements_with_i32(
                Gl::TRIANGLES,
                range.index_count as i32,
                Gl::UNSIGNED_INT,
                (range.first_index * 4) as i32,
            );
            self.frame_draw_calls += 1;
        }
    }

    /// Advances the class alpha one step towards its target (or jumps
    /// when transitions are instant) and returns the value to draw with.
    fn current_alpha(&mut self, class: Class) -> f32 {
        let target = class_alpha(class, self.camera.zoom(), self.override_band);
        let current = self.alphas.get(&class).copied().unwrap_or(target);
        let next = if self.animate {
            let step = (target - current) * ALPHA_STEP;
            if step.abs() < 0.005 { target } else { current + step }
        } else {
            target
        };
        self.alphas.insert(class, next);
        next
    }

    /// The label pass: candidates from the tiles residency is drawing,
    /// placement over the whole viewport, then one draw call per class.
    fn draw_labels(&mut self, drawn: &[TileId]) {
        let viewport = (self.viewport.0 as f32, self.viewport.1 as f32);
        if let Some(text) = self.labels.specimen.clone() {
            self.draw_specimen(&text, viewport);
            return;
        }
        if self.labels.placement_dirty {
            let buckets: Vec<(TileId, &LabelBucket)> = drawn
                .iter()
                .filter_map(|id| self.names.get(id).map(|b| (*id, b)))
                .collect();
            self.labels.candidates =
                frame_candidates(&buckets, &self.camera, self.viewport, &self.atlas).len();
            self.labels.placement = place_frame(
                &buckets,
                &self.camera,
                self.viewport,
                &self.atlas,
                self.labels.budget,
            );
            self.labels.placement_dirty = false;
        }
        self.draw_collision_boxes(viewport);
        for class in LABEL_DRAW_CLASSES {
            let alpha = self.current_alpha(class);
            if alpha < 0.004 {
                continue;
            }
            let quads = frame_quads(&self.atlas, &self.labels.placement, class);
            self.text.draw(
                &self.gl,
                &TextDraw {
                    quads: &quads,
                    viewport,
                    colour: Self::label_colour(class),
                    halo: HALO_COLOUR,
                    halo_em: self.labels.halo_em,
                    opacity: alpha,
                },
            );
            self.frame_draw_calls += u32::from(!quads.is_empty());
        }
    }

    fn label_colour(class: Class) -> [u8; 4] {
        if class == Class::Poi { fill_color(class) } else { fill_color(Class::Label) }
    }

    fn draw_collision_boxes(&mut self, viewport: (f32, f32)) {
        if !self.labels.show_boxes {
            return;
        }
        let placed: Vec<ScreenBox> =
            self.labels.placement.placed.iter().map(|p| p.bounds).collect();
        let refused: Vec<ScreenBox> = self
            .labels
            .placement
            .rejected
            .iter()
            .filter(|r| matches!(r.reason, Rejection::Collision { .. }))
            .map(|r| r.bounds)
            .collect();
        self.boxes.draw(&self.gl, &refused, viewport, [0xC0, 0x39, 0x2B, 0x38]);
        self.boxes.draw(&self.gl, &placed, viewport, [0x2E, 0x86, 0x4B, 0x40]);
        self.frame_draw_calls += 2;
    }

    /// The face at a ramp of sizes, left aligned down the viewport.
    /// Nothing cartographic happens here — the card wants to look at
    /// the type, not at a map.
    fn draw_specimen(&mut self, text: &str, viewport: (f32, f32)) {
        let mut quads = Vec::new();
        let mut y = 40.0_f32;
        for size in SPECIMEN_SIZES {
            quads.extend(layout_line(&self.atlas, text, 24.0, y, size));
            y += measure_line(&self.atlas, text, size).1 + size * 0.45;
        }
        self.labels.candidates = SPECIMEN_SIZES.len();
        self.text.draw(
            &self.gl,
            &TextDraw {
                quads: &quads,
                viewport,
                colour: fill_color(Class::Label),
                halo: HALO_COLOUR,
                halo_em: self.labels.halo_em,
                opacity: 1.0,
            },
        );
        self.frame_draw_calls += 1;
    }

    fn settled(&self) -> bool {
        self.alphas.iter().all(|(class, alpha)| {
            (class_alpha(*class, self.camera.zoom(), self.override_band) - alpha).abs() < 0.004
        })
    }

    /// Joins of everything currently on the GPU — fixed when the
    /// buckets were built, counted here only because a readout asked.
    fn joins(&self) -> maps2_render::JoinCounts {
        self.gpu_lines.values().fold(maps2_render::JoinCounts::default(), |mut total, bucket| {
            total.miter += bucket.joins.miter;
            total.bevel += bucket.joins.bevel;
            total
        })
    }

    fn roads_debug(&self) -> String {
        let joins = self.joins();
        let widths: Vec<String> = maps2_render::ROAD_ORDER
            .iter()
            .map(|class| format!("\"{class:?}\":{:.2}", self.road_width(*class)))
            .collect();
        format!(
            "\"casing\":{},\"miter_limit\":{:.2},\"joins\":{{\"miter\":{},\"bevel\":{}}},\"road_widths\":{{{}}}",
            self.casing,
            self.line_options.miter_limit,
            joins.miter,
            joins.bevel,
            widths.join(","),
        )
    }

    fn resident_classes(&self) -> Vec<String> {
        let level = self.active_level();
        let mut classes: Vec<Class> = Vec::new();
        for (id, bucket) in &self.gpu {
            if id.z != level {
                continue;
            }
            for range in &bucket.ranges {
                let visible =
                    class_alpha(range.class, self.camera.zoom(), self.override_band) > 0.0;
                if visible && !classes.contains(&range.class) {
                    classes.push(range.class);
                }
            }
        }
        classes.sort_by_key(|c| c.code());
        classes.iter().map(|c| format!("{c:?}")).collect()
    }
}
