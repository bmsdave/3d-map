//! Type on the map: the glyph atlas, screen-space layout, and which of
//! the labels asking for space actually get it.
//!
//! Three things live here and nothing else. The **atlas** is a signed
//! distance field, so one texture stays sharp at every size instead of
//! one bitmap per size. **Layout** turns a string into quads in screen
//! pixels — never in world coordinates: text does not rotate with the
//! map and stands upright under tilt, which only holds if the quads are
//! built after projection, from a screen anchor. **Placement** is the
//! selection stage: the maximum set of non-overlapping labels is
//! NP-hard, so this is the production heuristic instead — rank order,
//! a screen grid, first fit wins.
//!
//! Nothing here knows about tiles or GL. It takes screen anchors and
//! returns screen geometry; `maps2-render` extracts the candidates from
//! tiles and `maps2-web` feeds the quads to WebGL.

mod atlas;
mod collision;
mod font;
mod layout;

pub use atlas::{
    Atlas, GlyphMetrics, ATLAS_CELL_PX, ATLAS_COLUMNS, CELL_ORIGIN_PX, EM_PX, SDF_RANGE_PX,
};
pub use collision::{
    place, Candidate, PlacedLabel, Placement, RejectedLabel, Rejection, ScreenBox, GRID_CELL_PX,
    LABEL_MARGIN_PX,
};
pub use font::{ASCENDER_EM, CAP_HEIGHT_EM, CHARSET, DESCENDER_EM, X_HEIGHT_EM};
pub use layout::{layout_line, measure_line, GlyphQuad, LINE_HEIGHT_EM};
