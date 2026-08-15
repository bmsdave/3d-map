//! The browser face of the SDK: a `Map` handle over a canvas.
//!
//! The host (lab, later applications) fetches tile bytes itself and
//! hands them in once; the map builds a GPU bucket per tile and never
//! rebuilds it for a frame. `debug()` is the observable state the lab's
//! readouts and e2e read — diagnostics on request, never paid per frame.

mod labels;
mod input;
mod transform;

pub use labels::{
    class_of, frame_candidates, frame_quads, label_identity, label_size_px, place_frame,
    LABEL_PADDING_PX,
};
pub use input::{Input, pan_patch, wheel_zoom_step, zoom_about};
pub use transform::{place_tile, TilePlacement};

#[cfg(target_arch = "wasm32")]
mod gl;

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
