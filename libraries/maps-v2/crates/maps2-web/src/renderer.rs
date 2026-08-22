//! Frame renderer: WebGL state and per-frame timings.
//! Extracted from `map.rs:61` to isolate GPU-bound code from `TileStore`.
//! All `Gl` calls stay on the main thread; CPU decode (Task 4) stays out.

/// Milliseconds spent in each phase of the last frame.
/// Mirrors `map.rs:143` `FrameTimings` so `decode` and residency
/// can report without touching `Gl`.
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameTimings {
    pub total: f64,
    pub residency: f64,
    pub ground: f64,
    pub buildings: f64,
    pub fills: f64,
    pub roads: f64,
    pub labels: f64,
}

impl FrameTimings {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.total == 0.0
    }
}

/// Placeholder for future GL renderer state.
/// Today it only tracks timings; Task 3 wires the type so `Map`
/// can delegate without moving all GL code at once.
#[derive(Debug, Default)]
pub struct FrameRenderer {
    pub timings: FrameTimings,
    pub frame_draw_calls: u32,
    pub frame_tiles: u32,
    pub viewport: (f64, f64),
}

impl FrameRenderer {
    #[must_use]
    pub fn new(viewport: (f64, f64)) -> Self {
        Self {
            viewport,
            ..Default::default()
        }
    }

    pub fn reset_frame(&mut self) {
        self.timings = FrameTimings::default();
        self.frame_draw_calls = 0;
        self.frame_tiles = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_starts_empty() {
        let r = FrameRenderer::new((800.0, 600.0));
        assert!(r.timings.is_empty());
        assert_eq!(r.viewport, (800.0, 600.0));
    }

    #[test]
    fn reset_clears() {
        let mut r = FrameRenderer::new((0.0, 0.0));
        r.timings.total = 5.0;
        r.frame_draw_calls = 3;
        r.reset_frame();
        assert!(r.timings.is_empty());
        assert_eq!(r.frame_draw_calls, 0);
    }
}
