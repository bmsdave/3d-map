# Changelog

## Unreleased

- A public demo page at `/demo/`, built from the lab and published to GitHub
  Pages by `.github/workflows/pages.yml` on every push to `main`. One package,
  one full-window canvas and the same SDK entry points the studies use: four
  viewpoints fly the camera from a shaded globe down to Trafalgar Square, and
  the panel reports the shape, the tile level, the resident tiles and the cost
  of the last frame. `BASE_PATH` tells the build where the site is rooted,
  because a project Pages site is served from `/<repo>/` and the wasm bundle,
  the stylesheet and the tile manifest are all fetched by URL.
- `Map::set_viewport` — the renderer read the canvas size once, at construction,
  which is all a fixed study canvas ever needed. A map filling a window is not
  fixed: every frame after a resize was planned, projected and drawn for the
  size the page opened at.
- The demo sizes its canvas in CSS pixels rather than device pixels. The
  renderer measures road widths and type in `ScreenPx`, and a `ScreenPx` is a
  canvas pixel, so a 2x backing store draws a perfectly correct map at half its
  intended physical size. Taking a device pixel ratio properly is an SDK
  change, not a demo one.

- Low-zoom water is simplified again, by snapping rather than thinning.
  `simplify_area_ring` skipped Douglas-Peucker entirely for water at z≤7 — the
  whole range the world package is built at — because thinning decides which of
  a ring's points survive from that ring's own neighbours, so two grid-split
  ocean pieces kept different subsets of the edge they shared and pulled apart.
  Snapping asks only where a point is, so a shared edge survives shared.
  `WATER_SNAP_STEP` is one level-independent constant where the thinning path
  needs a per-level formula: a tile is drawn at about the same pixel size at
  every zoom, so 1/1024 of a tile is half a pixel at z1 and at z7 alike.
- Snapping can fold a bay narrower than the lattice onto a line, leaving a ring
  that walks out along itself and back. `earcutr` handed one of those as a
  *hole* does not fail — it does not return. `fold_out` unwinds the creases, and
  a ring that still revisits its own points keeps its original geometry rather
  than being drawn wrong.
- The committed lab packages are rebuilt from those sources. `1/0/0.mt2` fell
  from 1,971 KB to 897 KB and its fill bucket from 755 ms to 97 ms, with total
  water area within 0.05% of before. Worst blocking call across the lab: 836.9 ms
  to 180.2 ms. Every visual golden is unmoved.
- Buildings are no longer triangulated into the fill bucket as well as their own.
  The flat copy was uploaded to the GPU and then skipped at draw time, because
  the 3D bucket had already drawn it. `resident_classes` now asks the building
  bucket where buildings are, instead of reading it out of the fills.
- `load_tile` takes its bytes instead of borrowing and copying them a second
  time, and a tile's height raster is a range into the bytes already held rather
  than a duplicate 128 KB per terrain tile.
- `vite.config.ts` honours `PORT`, defaulting to 5178, so a second copy of the
  lab can run beside the first.

- Added [`docs/architecture.md`](libraries/maps-v2/docs/architecture.md): what
  each crate owns, the life of one tile from OSM extract to pixel, where the
  frame's time goes, and a **Known gaps** section that records what is
  deliberately not built yet — tilt is stored but never projected, text is
  unshaped, roof shape is a bounding-box guess. Linked from the README.
- The performance trace now records *what* a span was working on, not only how
  long it took: `decode` spans carry the tile path and the reporter prints it,
  so a failing line reads `802.2ms decode 1/0/0.mt2` instead of leaving the
  address to be guessed. `frameMeasurement` also resets the trace before
  measuring, so a `frame` row's phase breakdown describes its own window rather
  than the whole session.
- Buildings triangulate through `earcutr` like fills do; the hand-written ear
  clipper, whose ear test rescanned every remaining vertex, is gone.
- `build_line_bucket` decodes each road section once and sorts features into
  tunnel/ground/bridge storeys, instead of walking the section once per storey.
  Eight walks per tile rather than twenty-four.
- Label collision no longer grows with the square of the frame: the duplicate
  check is a `HashSet` and the repeat check a map from text to the places that
  name already stands. `labels-collision` worst block fell from 98 ms to 30 ms.
- Measured, and written down: the remaining ~800 ms `decode` failures are one
  thing — the z1 world tiles carry ~856,000 coastline vertices, and triangulating
  them is 780 ms of the 786 ms that `load_tile` spends. Decoding those vertices
  costs 6 ms. See
  [`e2e/perf/FINDINGS.md`](applications/maps-v2-lab/e2e/perf/FINDINGS.md) for the
  measurements, what does not fix it, and what would.
- The lab's front page is now the lab. Twenty studies mount live on it —
  hero canvas, SDK snippet, and every study interactive without a click —
  filtered by text or group instead of navigated to. A WebGL2 context budget
  keeps the six studies nearest the viewport running and hands the rest back,
  so one page can hold twenty renderers without the browser dropping the ones
  at the top. `/#/card/<id>` still mounts a study alone, and the showcase reel
  now fits a whole reel above the fold.
- MT2 bumped to v5: building features carry a facade `material` byte
  (`Unknown`/`Brick`/`Concrete`/`Stone`/`Glass`/`Metal`/`Wood`). Versions 1–4
  remain readable; a v2–v4 tile decodes as `MaterialClass::Unknown`. Fixture
  golden hashes changed knowingly — see `docs/tile-format.md`.
- `maps2-ingest` now maps real OSM tags into the building payload instead of
  a flat default: `roof:shape` → roof form, `building:material`/
  `building:facade:material`/`wall` → material, `min_height`/
  `building:min_level` → base height, each with a documented fallback.
- Added a bounded GEBCO window reader (`maps2-ingest::load_gebco_window`):
  decodes only the TIFF chunks a requested region overlaps, capped at 4 Mi
  cells, so regional builds never load a multi-gigabyte world grid. New
  `gebco-window` CLI subcommand and a pinned (placeholder-checksum) London
  GEBCO descriptor.
- `maps2-render` now builds buildings at one of three LOD tiers
  (`Footprint`/`Simplified`/`Full`) keyed by camera zoom, shapes gabled/hipped
  roofs at `Full`, and groups building meshes into per-material draw ranges;
  `maps2-style::facade_colour` maps `MaterialClass` to a palette colour.
- The lab's index page now opens with a copy-pasteable **Quick start** SDK
  snippet, and the README gained a **Demos** section with a real local-London
  build-and-load walkthrough. Fixed the manifest loader's `format_version`
  check, which only accepted 2–4 and would have rejected real v5 packages.
- Fixed a real multipolygon bug: a relation listing the same outer member way
  twice emitted that ring's geometry twice. Caught against the real Greater
  London extract — the fix removed 11 duplicate feature parts. A full z12–z16
  London rebuild from the pinned real inputs now produces 4,017,061 feature
  parts across 16,246 terrain-bearing tiles, reproducibly
  (`c6e61742d63afd68a40bc07a331a358d9d5b16f16e022ea291eaf193c6ce3f28` across
  two independent clean builds), and passes the local real-package Chromium
  acceptance test under MT2 v5.

## 0.1.0-alpha — 2026-08-15

- Initial public alpha: deterministic MT2 v1 tiles, synthetic fixtures,
  flat/globe rendering, roads, point labels, terrain, and browser lab.
- Includes Rust, browser, visual, coverage, and frame-budget release checks.
- Does not include real-world data ingest or packages; see the beta plan.
