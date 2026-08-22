# Changelog

## Unreleased

- The spool writes deflated blocks. Its records are repetitive by nature —
  the same street name on every part of a road, coordinates differing in
  their low bits — and on a city's worth of streets they compress about five
  to one: scratch disk falls from 1.39x the tiles being built to 0.31x
  (measured, `tests/spool_size.rs`, which now fails if the compression is
  ever lost). Level 1 deflate, because scratch is written once and read once
  minutes later.
- Every shard's last block is written before the first shard is read. Draining
  lazily left the other shards' buffers in memory for the whole pass — a
  thousand shards holding up to a megabyte each is a gigabyte, which is
  exactly what the spool exists to avoid.
- PLANET.md's numbers are marked measured or estimate, one by one, and the
  source table is measured off a real cache: 88 GB of planet PBF (already
  compressed — that *is* the compressed form), 7 GB of GEBCO, 150 MB of
  vectors. It also says what is still uncompressed and worth compressing: MT2
  vector sections, which deflate 2.0x on the committed carve and are the
  largest remaining lever on the size of a published package.

- A build plan for the planet (`plans/planet.toml`), its source descriptor, and
  [PLANET.md](pipelines/maps-v2-ingest/PLANET.md) — the runbook for a build
  measured in machine-days: what it needs, what it costs, how to pin a planet
  file that is republished weekly, how to resume, how to verify a package too
  large to carry per-tile digests, and how to host it. Vector to z14 rather
  than z16 on purpose: below z14 a street is redrawn larger rather than
  described better, and a city that wants more is a carve out of the same
  package.
- The demo takes `?package=<manifest url>`, so pointing it at a hosted package
  is a link rather than a rebuild. Without it, the carve committed beside the
  lab, which is what makes the page work from a clone.
- `docs/DATA-LICENCE.md`: what a published package owes the people whose data
  it is made of. The short of it is that MT2 tiles carry real geometry, which
  makes a package a derived database rather than a produced work, so ODbL's
  share-alike applies to the package itself and not only to the page drawing
  it.

- Tiles are built across cores, in batches of sixty-four. Building a tile is
  the expensive half of ingest — triangulation, line joins, label shaping —
  and worth spreading, but only a batch is ever in the air, because not
  holding tiles is what the spool is for. `std::thread::scope` rather than a
  pool crate: the work is already batched, so a dependency would buy `chunks`
  and `join`. A test builds the same batch over one thread and over
  sixty-four and demands the same bytes, so the machine's core count cannot
  reach the package.
- `build-map` resumes. A level that finished writes `.level-NN.done` beside
  the pyramid with its digests and totals; a later run restores it and moves
  on. A planet is machine-days, and a run that dies at hour nine must not
  begin again at hour zero. A half-built level leaves no record and is simply
  built again, and a record this program cannot read is treated the same way
  — rebuilding is always correct, refusing to start is not.

- Ingest builds through a spool instead of holding the whole package. The
  pipeline took every prepared feature as one slice and handed back every
  tile as one `Vec`: for a carve the simplest thing that works, for a planet
  neither end fits — order 10^9 features in, 10^8 tiles out. Features are
  now written to shard files as they are prepared, chosen by the tile they
  belong to so every part of a tile lands together, and the drain reads one
  shard at a time. What a build holds is one shard's features and one tile's
  bytes; the number of shards is the knob that makes it fit.
- The bytes do not change, and that is a test rather than a hope: `build_tile`
  sorts a tile's features itself and the manifest sorts its tiles, so neither
  the order features arrive in nor the order shards drain in reaches the
  output. `the_spool_builds_the_same_bytes_as_memory` holds the spooled build
  against the in-memory one, and a second test builds the same features over
  one shard and over sixty-four to show the knob cannot change the result.
- `build-map` writes each tile as it is built and keeps only its digest —
  eighty bytes where it used to hold a hundred and fifty thousand. The
  remaining ceiling is honest and worth naming: the digest list is still held
  in memory, which is fine to around 10^7 tiles and needs an external sort
  beyond that.

- A manifest stops naming its tiles once there are more than 50,000 of them.
  A carve is a few hundred and the list earns its place: the client knows
  before asking whether a tile exists, and a digest each catches a corrupt
  commit. A planet to z14 is on the order of 10^8, where the same list is
  gigabytes of JSON before the first frame and the digests alone are six
  gigabytes of hex. Above the threshold the manifest carries the envelope
  instead — which levels exist, and the ground each covers — which every
  package now writes, not just `carve`.
- The client reads whichever it is given. Listed tiles are fetched and
  checked exactly as before; an envelope means computed URLs, and a 404 is
  read as "no tile there" rather than a failed pass. Misses are remembered,
  so empty ocean is asked about once rather than on every camera move, and
  one absent tile no longer throws away the batch it travelled with.
- `verify-package` still checks every byte of a package that lists nothing:
  it hashes the tiles on disk and holds the result against the package hash,
  which moves the check to the build, where the bytes are, instead of onto
  every machine that draws them.

- Terrain stops at z12, and everything below reads it. Copernicus GLO-30
  samples the ground every 30 m; a z12 tile's raster samples every 38 m, so
  z12 is where the source is spent. A z16 tile's raster was the same numbers
  written out sixteen times per axis for another 128 KiB — a naive z0–16
  pyramid of them is terabytes. `TERRAIN_MAX_Z` stops ingest emitting them,
  and a tile below the cap reads the nearest ancestor that has one through a
  window: `maps2_render::HeightWindow` for where to look, `sample_bilinear`
  for how, both under test in plain arithmetic that the shaders mirror.
- One copy of that arithmetic in GLSL, shared by the three shaders that read
  terrain — ground displacement, hillshade, and building bases — so the
  surface, its shading and the buildings standing on it cannot disagree
  about where the ground is. The interpolation is written out because it has
  to be: the raster is an `R16UI` texture and WebGL2 will not filter integer
  textures, so nearest-neighbour on a magnified ancestor came out in
  terraces.
- The walk up is bounded at four levels (`MAX_ANCESTOR_DEPTH`), which is the
  distance the cap creates. Unbounded, it found *a* raster — on a world
  package that was a z3 tile spanning a quarter of the planet at eleven
  kilometres a sample, which shades a street as perfectly flat while
  claiming to be terrain. Beyond the limit there is no terrain, and flat
  ground is the honest answer.
- The ancestor's texture is uploaded and kept for as long as something on
  screen reads it. Residency keeps coarse tiles resident but does not draw
  them, and only drawn tiles were uploaded — so the deep tiles found nothing
  above them; releasing it each frame instead would have been 128 KiB to the
  GPU per frame.

- MT2 v6: heights can ride packed. The raster is 128 KiB whatever ground it
  covers, which is ~60% of a tile and all of the reason a world pyramid of them
  costs terabytes. `heights::pack` predicts each sample from its neighbours
  (Paeth, over `u16`), splits the residuals into a high-byte and a low-byte
  plane, and deflates: 3.7x on the committed London carve, which as a whole
  falls from 117 MB to 64 MB. Lossless — `unpack` returns the bytes that went
  in, and the lab's terrain screenshots are unchanged.
- The packed raster is a new class (`0xFF01`) beside the plain one (`0xFF00`),
  not a new meaning for it. A reader that does not know the class skips the
  section and draws the tile flat, so an older SDK meeting a v6 tile degrades
  instead of failing, and every package built before this one still loads.
- Deflate rather than zstd: with the predictor and the plane split doing the
  structural work, zstd was 6% smaller (3.94x against 3.69x) — not worth a C
  dependency inside the wasm bundle when `miniz_oxide` was already in the lock
  and builds for wasm32 like the rest of the crate.
- `scripts/check.sh` runs what CI used to: workspace tests, clippy, the
  coverage ratchet, package digests, the lab build and the e2e suite, stopping
  at the first failure. This project's Actions are billing-blocked, so the gate
  is a command someone runs. It immediately caught a clippy failure that had
  already landed on `main` (`natural_earth.rs`, a test 55 lines long against a
  40-line limit), now split into the four cases it was actually testing.

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
