//! Tile storage: CPU buckets and retained bytes.
//! Extracted from `map.rs:74` to isolate residency-independent state.
//! `TileStore` owns everything the host handed in; `Map` only decides what
//! residency admits to the GPU. Heights are kept here as well so `decode`
//! (Task 4) can live off the main thread.

use std::collections::HashMap;
use std::ops::Range;

use maps2_render::{BuildingBucket, FillBucket, LabelBucket, LineBucket, BuildingLod};
use maps2_units::TileId;

/// Where a tile's height raster lives.
#[derive(Debug)]
pub enum HeightSource {
    /// Window into `tiles` bytes (128 KiB) — zero-copy for plain `0xFF00`.
    Plain(Range<usize>),
    /// Inflated raster for packed `0xFF01` — owned after `unpack`.
    Unpacked(Box<[u8]>),
}

/// CPU-side tile store: retained bytes + decoded buckets.
/// No `Gl` here — this is `Send` and can be built off the main thread.
#[derive(Debug)]
pub struct TileStore {
    /// Retained bytes the host handed in (`map.rs:74`).
    pub tiles: HashMap<TileId, Vec<u8>>,
    pub cpu: HashMap<TileId, FillBucket>,
    pub lines: HashMap<TileId, LineBucket>,
    pub buildings: HashMap<TileId, BuildingBucket>,
    pub names: HashMap<TileId, LabelBucket>,
    pub heights: HashMap<TileId, HeightSource>,
    pub source_levels: Vec<u8>,
    pub building_lod: BuildingLod,
}

impl Default for TileStore {
    fn default() -> Self {
        Self {
            tiles: HashMap::new(),
            cpu: HashMap::new(),
            lines: HashMap::new(),
            buildings: HashMap::new(),
            names: HashMap::new(),
            heights: HashMap::new(),
            source_levels: Vec::new(),
            building_lod: BuildingLod::Footprint,
        }
    }
}

impl TileStore {
    #[must_use]
    pub fn new(source_levels: Vec<u8>) -> Self {
        Self {
            source_levels,
            building_lod: BuildingLod::Footprint,
            ..Default::default()
        }
    }

    #[must_use]
    pub fn contains(&self, id: &TileId) -> bool {
        self.tiles.contains_key(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.tiles.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.tiles.is_empty()
    }

    pub fn remove(&mut self, id: &TileId) -> Option<Vec<u8>> {
        self.cpu.remove(id);
        self.lines.remove(id);
        self.buildings.remove(id);
        self.names.remove(id);
        self.heights.remove(id);
        self.tiles.remove(id)
    }

    #[must_use]
    pub fn available_ids(&self) -> std::collections::HashSet<TileId> {
        self.tiles.keys().copied().collect()
    }
}

/// Format tile ids as JSON array `"z/x/y.mt2"` — `map.rs:52`.
#[must_use]
pub fn tile_paths(ids: &[TileId]) -> String {
    let paths = ids
        .iter()
        .map(|id| format!("\"{}/{}/{}.mt2\"", id.z, id.x, id.y))
        .collect::<Vec<_>>();
    format!("[{}]", paths.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_units::TileId;

    #[test]
    fn tile_store_holds_tiles_without_gl() {
        let mut s = TileStore::new(vec![0, 5, 8]);
        let id = TileId { z: 12, x: 0, y: 0 };
        s.tiles.insert(id, vec![1, 2, 3]);
        assert!(s.contains(&id));
        assert_eq!(s.len(), 1);
        assert_eq!(s.available_ids().len(), 1);
    }

    #[test]
    fn tile_paths_formats() {
        let ids = [TileId { z: 0, x: 0, y: 0 }];
        assert_eq!(tile_paths(&ids), "[\"0/0/0.mt2\"]");
    }

    #[test]
    fn remove_cleans_buckets() {
        let mut s = TileStore::new(vec![]);
        let id = TileId { z: 1, x: 0, y: 0 };
        s.tiles.insert(id, vec![]);
        s.remove(&id);
        assert!(s.is_empty());
    }
}
