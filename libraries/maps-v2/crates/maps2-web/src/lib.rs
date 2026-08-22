mod labels;
mod input;
mod transform;
mod tile_store;
mod renderer;

pub use labels::{
    class_of, frame_candidates, frame_quads, label_identity, label_size_px, place_frame,
    LABEL_PADDING_PX,
};
pub use input::{Input, pan_patch, wheel_zoom_step, zoom_about};
pub use transform::{place_tile, TilePlacement};
pub use tile_store::{tile_paths, HeightSource, TileStore};
pub use renderer::{FrameRenderer, FrameTimings};

#[cfg(target_arch = "wasm32")]
mod gl;

#[cfg(target_arch = "wasm32")]
mod gl_building;

#[cfg(target_arch = "wasm32")]
mod line_gl;

#[cfg(target_arch = "wasm32")]
mod gl_terrain;

#[cfg(target_arch = "wasm32")]
mod gl_view;

#[cfg(target_arch = "wasm32")]
mod map;

#[cfg(target_arch = "wasm32")]
mod text_gl;

#[cfg(target_arch = "wasm32")]
pub use map::Map;
