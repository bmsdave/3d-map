# Architecture TL;DR — for agents

Full: `architecture.md`. Build-time vs frame-time split `architecture.md:20`.

## Flow

```
OSM/Copernicus/GEBCO → maps2-ingest (conflate→clip→simplify→encode→digest) → MT2 package (tiles+manifest.json)
browser host sdk.ts (fetch→verify→load_tile) → maps2-web Map → maps2-tile(parse) + maps2-render(meshes) + maps2-text(collision)
```

## Key invariants

* `load_tile` builds all CPU buckets once; first frame uploads to GPU as STATIC_DRAW. Per-frame work is style eval + label reprojection only. `architecture.md:76`
* Style (widths, colors, material palette) lives in `maps2-style`, not in tile. Road stored as centreline, expanded in shader (48km bug if baked) `tile-format.md:25`.
* Camera is `CameraPatch` atomic — one invalid field rejects whole patch `maps2-camera/src/lib.rs:119`. Globeness smoothstep 3.5-4.5 `lib.rs:35`.
* Labels: rank+id sorted, 64px grid greedy, budget cutoff `maps2-text/collision.rs:1`.
* Tile budget: no single main-thread span >10ms, p95 render <10ms `e2e/perf/FINDINGS.md`.

## Where expensive work lives

Load-time (`load_tile`) is synchronous and costly — do not move work into it. Frame-time is cheap by construction. Network time excluded from perf budget.

## Crate owners (same as AGENTS.md)

maps2-units: units, maps2-camera: camera, maps2-tile: MT2, maps2-style: bands, maps2-render: residency/meshes, maps2-text: SDF/collision, maps2-web: wasm, maps2-ingest: pipeline, maps2-fixtures: golden determinism `lib.rs:382`.

## Gaps (honest)

Tilt stored but not projected except in building shader. Text is LTR latin only. Roof is bbox guess. No bearing/tilt gestures. Perf suite not in CI.
