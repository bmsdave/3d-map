# Architecture

How the 3D Maps SDK is put together: what each crate owns, how a tile travels
from an OpenStreetMap extract to a pixel, and where the frame's time goes.

This document describes the code as it stands. Where it records a decision, it
records why — most of the constants here exist because something specific went
wrong once. For the byte layout of a tile see
[tile-format.md](tile-format.md); for what the project does and does not claim
to be yet, see [production-roadmap.md](production-roadmap.md).

Paths below are relative to `libraries/maps-v2/` unless they start with
`applications/`; line numbers are as of the commit that introduced this file.

## The shape of it

Nine Rust crates and one browser application. The dividing line that matters is
not Rust/TypeScript — it is *build time* versus *frame time*.

```
build time                          frame time
──────────────────────────────      ────────────────────────────────────────
OSM extract, Copernicus DEM,        ┌──────────────────────────────────┐
GEBCO, Natural Earth                │ browser host (maps-v2-lab/sdk.ts)│
        │                           │  fetch · verify digest · demand  │
        ▼                           └───────────────┬──────────────────┘
   maps2-ingest                                     │ load_tile(bytes)
   conflate → clip → simplify                       ▼
   → encode → digest              ┌─────────────────────────────────────┐
        │                         │ maps2-web  (the wasm boundary)      │
        ▼                         │  Map: camera, buckets, GL context   │
   MT2 package                    └──┬───────────┬──────────┬───────────┘
   tiles + manifest.json    ────▶     │           │          │
                              maps2-tile   maps2-render  maps2-text
                              (parse)      (meshes,      (atlas,
                                   │        residency)    collision)
                                   └──────┬────┴──────────┘
                                          │
                              maps2-units · maps2-camera · maps2-style
                              (coordinates, projection, what is visible)
```

`maps2-fixtures` sits outside this flow: it generates the deterministic
synthetic packages the tests and offline studies run on.

| Crate | Owns |
|---|---|
| `maps2-units` | Coordinate and unit vocabulary; the mercator conversions |
| `maps2-camera` | Camera state, validation and clamping, flat↔globe projection |
| `maps2-tile` | MT2 v5 writing, validated v1–v5 reading |
| `maps2-style` | Class visibility bands, colours, screen-pixel road widths |
| `maps2-render` | Residency planning and the per-tile mesh buckets |
| `maps2-text` | SDF atlas, glyph layout, label collision |
| `maps2-web` | The WebGL2/wasm browser boundary; input; label projection |
| `maps2-ingest` | The build-time pipeline and its CLI |
| `maps2-fixtures` | Reproducible synthetic packages |

## The life of one tile

1. **Ingest** reads a pinned, checksummed source, classifies its features,
   clips them to a tile, simplifies the geometry for the level, and encodes an
   MT2 tile. Its SHA-256 goes into the package manifest.
2. **The host** plans residency, fetches the tile over HTTP, verifies its
   digest against the manifest, and hands the bytes to `load_tile`.
3. **`load_tile`** parses the tile once and builds every CPU bucket it will
   need — fills, buildings, roads, label anchors — plus the heights raster.
4. **The first frame that draws it** uploads those buckets to the GPU once, as
   static buffers, and a 256×256 height texture.
5. **Every frame after that** the tile is a handful of draw calls against
   buffers that are already there. Style is evaluated per frame from the live
   camera zoom; labels are projected, shaped on first use, and placed against
   the whole viewport.
6. **When the camera leaves**, residency marks the tile evictable, the host
   unloads it, and both CPU buckets and GPU buffers go.

Steps 1 and 3 are where the expensive work lives. Steps 4–5 were made cheap
deliberately, and the comments in the code name the regressions that taught
each lesson.

## Data and build time

### `maps2-units` — the unit vocabulary

`maps2-units` exists to make the wrong unit unrepresentable. Its module doc states the point directly: `Metres` and `ScreenPx` are distinct newtypes, and the only way between them is an explicit function that demands latitude and zoom, so "a metre became a pixel" cannot compile — the doc notes that v1 grew a 48 km wide road from exactly that mistake (`crates/maps2-units/src/lib.rs:1-8`).

Zoom is a continuous `f64`, not an integer level: `Zoom::new` accepts any finite value and `world_pixels()` computes `256 · 2^z` without rounding, so `z14.37` is a legal camera state, not a rounding error (`crates/maps2-units/src/lib.rs:15,30-51`). Tile addressing and ground position are kept apart as different types: `TileId { z, x, y }` names a tile in the global Web Mercator pyramid, `TileCoord(u16, u16)` is a point on the intra-tile grid, and `TilePoint { tile, coord }` combines the two into an exactly-addressed point (`crates/maps2-units/src/lib.rs:54-69`). That intra-tile grid is `TILE_EXTENT = 65536` steps per tile — chosen because it keeps the step under 10 cm at z16, where an `f32` normalised to world bounds would step 2.4 m and buildings would visibly shiver (`crates/maps2-units/src/lib.rs:18-21`).

`locate` and `to_lonlat` convert between a geographic point and a `TilePoint` at a given zoom level, going through a shared `mercator_normalised` helper that clamps latitude to `MAX_LATITUDE_DEG` before applying the standard Web Mercator formula (`crates/maps2-units/src/lib.rs:96-104,134-165`). `MAX_LATITUDE_DEG = 85.051_128_779_806_59°` is where Web Mercator's square world ends (`crates/maps2-units/src/lib.rs:27`), and every geographic input is clamped to it before conversion.

`world_position_px`/`lonlat_at_world_px` place a point in continuous world-pixel space at a given zoom — a *position*, never a size (`crates/maps2-units/src/lib.rs:176-196`). Sizes stay typed: `tile_grid_step_metres` reports the ground size of one grid step at a level and latitude, and `metres_to_screen_px` is described in its own doc comment as "the only bridge between ground and screen: it cannot be crossed without stating latitude and zoom" (`crates/maps2-units/src/lib.rs:167-203`).

### `maps2-camera` — camera state and projection

`Camera` holds `centre: Lonlat`, `zoom: Zoom`, `bearing_deg: f64`, `tilt_deg: f64` (`crates/maps2-camera/src/lib.rs:54-59`). It is never mutated field-by-field; every change goes through `CameraPatch`, a set of four `Option` fields applied atomically by `Camera::apply`. The doc comment is explicit: "a patch with one invalid field changes no field at all" (`crates/maps2-camera/src/lib.rs:1-8`). `apply` first calls `validate`, which rejects the whole patch with `PatchError::NonFinite` if *any* supplied field is NaN or infinite, before touching `self` at all (`crates/maps2-camera/src/lib.rs:119-147`).

Once a patch passes validation, each field is clamped inside `Camera::apply`, not by callers: centre longitude wraps into `[-180, 180)` and latitude clamps to `±MAX_LATITUDE_DEG` (`clamp_centre`, `crates/maps2-camera/src/lib.rs:149-154`); zoom clamps to `[0.0, MAX_ZOOM]` with `MAX_ZOOM = 22.0` (`crates/maps2-camera/src/lib.rs:21,156-158`); bearing wraps via `rem_euclid(360.0)`; tilt clamps to `[0.0, MAX_TILT_DEG]` with `MAX_TILT_DEG = 60.0` (`crates/maps2-camera/src/lib.rs:22,130-132`). The doc comment for `apply` states the rule generally: "clamps live inside, not in hosts" (`crates/maps2-camera/src/lib.rs:119-122`).

The world has two shapes, blended rather than switched. `Globeness::at(zoom)` maps zoom to a `0..1` value via a smoothstep (`t*t*(3-2t)`) over the band `GLOBE_FULL_BELOW = 3.5` to `GLOBE_GONE_ABOVE = 4.5`: fully a globe below 3.5, fully a flat sheet above 4.5, smoothly mixed in between (`crates/maps2-camera/src/lib.rs:17-19,35-52`). `project` computes both a `flat_offset` (Mercator sheet, with `cos(latitude)` compensation that itself fades in with globeness so the two shapes agree on local scale through the transition) and a `globe_offset` (orthographic projection, radius chosen so its centre-screen scale matches the compensated sheet), and linearly blends the two by `g` before rotating by bearing (`crates/maps2-camera/src/lib.rs:164-225`).

`unproject` is the inverse. Where the globe is fully gone, `sheet_inverse` inverts the Mercator sheet in closed form and is exact; where any globe blend remains, that closed-form answer is only the first guess, and Newton's method walks the residual against `project` itself for up to `INVERSE_STEPS = 6` iterations or until the residual is under `INVERSE_TOLERANCE_PX = 1e-6` px, using a numeric Jacobian built from finite differences of size `DERIVATIVE_STEP_DEG = 1e-6°` (`crates/maps2-camera/src/lib.rs:27-31,229-275`). The doc comment on `unproject` explains why: "a blend of two projections has no closed form... Newton walks the residual out against `project` itself — the inverse cannot drift away from the forward map that way" (`crates/maps2-camera/src/lib.rs:213-219`).

Tilt today is stored and clamped, nothing more. The crate's module doc says so explicitly: "Tilt is stored but not yet applied by `project`; it becomes real with the renderer stage" (`crates/maps2-camera/src/lib.rs:7-8`). What tilt *does* affect is one shader — see the GL layer below.

### The MT2 v5 tile format

MT2 is documented byte-for-byte in [tile-format.md](tile-format.md) (in Russian; summarised here) and implemented in `crates/maps2-tile/src/`. The format was frozen at implementation-plan step 3.4 on 11 August 2026; version 2 was added for real buildings, and version 5 added facade material and widened the feature identifier to 64 bits on 18 August 2026. From that point, any layout change is a new header version and a deliberate fixture migration, never an in-place edit, because bytes travel to clients and into caches (`docs/tile-format.md:1-8`; mirrored in `crates/maps2-tile/src/lib.rs:11-14`).

The design choices and why: binary, not JSON, because JSON parsing cost 54% of a frame in v1; sections are found by an O(1) offset table so a reader is a zero-copy view over the caller's buffer and corruption in one section doesn't block reading another; geometry uses integer coordinates on the 65536-step tile grid for the same sub-10cm-at-z16 reason as `maps2-units`; adjacent vertices are encoded as varint deltas since they tend to be close together; and a road is stored as a centreline, not a polygon — width lives in style and is expanded in the shader, because a width baked in metres (v1) produced a 48 km-wide road once the camera zoomed out to see the whole Earth (`docs/tile-format.md:10-20`).

**Header** (all little-endian): offset 0, 4 bytes, magic `4D 54 32 00` ("MT2\0"); offset 4, 2 bytes, `version` (= 5); offset 6, 1 byte, `z`; offset 7, 1 byte reserved; offset 8, 4 bytes, `x`; offset 12, 4 bytes, `y`; offset 16, 2 bytes, `section_count`; offset 18, 2 bytes reserved; offset 20 onward, the section table, 10 bytes per entry (`docs/tile-format.md:22-35`). This matches the crate's own layout diagram (`crates/maps2-tile/src/lib.rs:16-29`) and the constants `MAGIC = b"MT2\0"` and `FORMAT_VERSION = 5` (`crates/maps2-tile/src/lib.rs:48-49`).

**Section table.** Each entry is `class: u16, offset: u32, len: u32` (10 bytes); `offset` is relative to the payload base, `20 + 10 · section_count` (`docs/tile-format.md:36-38`). A class code below `0xFF00` is a vector section (features); `0xFF00` and above is raster, an opaque payload whose meaning belongs to the class — matching `RASTER_CLASS_BASE: ClassCode = 0xFF00` in the crate (`crates/maps2-tile/src/lib.rs:58-64`). Class codes 0–13 (land, water, park, building, roads 4–10 motorway→path, POI, labels, and administrative boundaries) are defined by `maps2-style` (`crates/maps2-style/src/lib.rs:24-42`); the one raster class currently defined is `0xFF00` heights (`docs/tile-format.md:40-46`). `Class::Boundary` goes through the line pipeline like a road — it is a line — but is styled as a dashed rule with no casing.

**Vector section payload**: `feature_count: u16`, followed by that many features, each: `id: u64` (widened from `u32` in v4), `flags: u8` (style-owned bits: one-way, bridge, tunnel), `rank: u8` (selection importance, 0 highest), `base_dm: u16` and `top_dm: u16` (building base/top in decimetres above datum, zero for non-buildings), `roof: u8` (0 flat, 1 gabled, 2 hipped, 3 other), `material: u8` (0 unknown … 6 wood, v5 only), `name_len: u16` and `name` (UTF-8, non-empty only for labels/POI), `vertex_count: u16`, a first vertex `(x: u16, y: u16)`, then `vertex_count − 1` zigzag-varint `(dx, dy)` deltas (`docs/tile-format.md:48-64`; the same layout is documented in `crates/maps2-tile/src/lib.rs:19-25` and the field struct at `crates/maps2-tile/src/lib.rs:93-101`). `rank` feeds selection: a zoom band only admits a class, and admitted point features then compete for screen space by rank and collision (`docs/tile-format.md:66-68`).

Building fields degrade gracefully across versions: v1 tiles have none of `base_dm`/`top_dm`/`roof`/`material` (no 3D payload at all); v2–v4 carry `base_dm`/`top_dm`/`roof` but not `material`, and a v5 reader fills `MaterialClass::Unknown` rather than failing (`docs/tile-format.md:70-73`; `MaterialClass::from_wire` at `crates/maps2-tile/src/lib.rs:150-166`). `material` is explicitly styled, not geometry: it drives facade colour and never forces a rebuild on palette changes (`docs/tile-format.md:75-77`, `crates/maps2-tile/src/lib.rs:106-112`). Its value comes from OSM tags with documented fallbacks (`maps2-ingest::building_material`, `building_roof`, `building_base_height_dm`): `roof` from `roof:shape` (`gabled`→Gabled, `hipped`/`pyramidal`→Hipped, any other declared value→Other, absent→Flat); `material` from `building:material`, then `building:facade:material`, then `wall`, first match wins, unrecognised or absent→Unknown; `base_dm` from `min_height` metres, then `building:min_level` × 3 m/level, both absent→0. If the computed base is not below the top (a corrupt tag combination), ingest resets the base to 0 rather than dropping the building (`docs/tile-format.md:79-90`).

Zigzag encoding is `enc(v) = (v << 1) ^ (v >> 31)`; varint is 7 bits per byte with a continuation high bit, and more than 5 bytes is `BadVarint` (`docs/tile-format.md:92-93`).

**Raster (heights) sections.** Class `0xFF00` is 256×256 `u16` LE values, row-major, 131,072 bytes total; the stored value is metres offset by +11000 so GEBCO bathymetry stays positive (`metres = value − 11000`) (`docs/tile-format.md:95-97`). The crate implements this in `heights.rs`: `HEIGHTS_SIDE = 256`, `HEIGHTS_BYTES = 256*256*2`, `HEIGHT_OFFSET_M = 11_000.0`, with `encode_height`/`decode_height` doing the round-trip and `HeightsRaster::parse` rejecting any section that isn't exactly `HEIGHTS_BYTES` long as `TileError::Truncated` (`crates/maps2-tile/src/heights.rs:1-49`). The grid is inclusive of both tile edges, so neighbouring tiles share edge samples and a surface built from them has no seam (`crates/maps2-tile/src/heights.rs:1-4`).

**Version history**, per `docs/tile-format.md:99-107` and the crate's version constants (`crates/maps2-tile/src/lib.rs:48-56`):

| Version | Added |
|---|---|
| 1 (`LEGACY_FORMAT_VERSION`) | Base format: `id: u32`, no buildings, no interior rings. |
| 2 | `base_dm`/`top_dm`/`roof` for buildings. |
| 3 (`HOLES_FORMAT_VERSION`) | Interior rings (holes) on polygons. |
| 4 (`WIDE_ID_FORMAT_VERSION`) | `id` widened to `u64` (OSM IDs exceed `u32`). |
| 5 (`MATERIAL_FORMAT_VERSION`, `FORMAT_VERSION`) | `material: u8` on buildings; reads back `Unknown` for v2–v4 tiles. |

`PREVIOUS_FORMAT_VERSION = 4` names the prior frozen version explicitly (`crates/maps2-tile/src/lib.rs:50`).

Reading a corrupted tile always yields `Err`, never a panic: `TooShort`, `BadMagic`, `UnsupportedVersion(v)`, `SectionOutOfBounds`, `Truncated`, `BadVarint`, `DeltaOutOfRange`, `BadText`, `BadBuilding`, `TooLarge`, `EmptyGeometry` (`docs/tile-format.md:109-112`; `crates/maps2-tile/src/lib.rs:74-88`).

**What the format deliberately excludes.** Colour, widths, draw order, and lane counts are all style, not format, and live in `maps2-style` — a palette change never repacks the tile pyramid, and the doc calls out that this applies equally to `material`: the wire code `1..=6` is fixed by the format, but what it renders *as* is a style decision (`docs/tile-format.md:114-117`). This is the same reasoning that keeps road width out of the format entirely (centreline plus style-time expansion) after v1's 48 km road bug (`docs/tile-format.md:18-20`).

### `maps2-ingest` — the build-time pipeline

`maps2-ingest` (`crates/maps2-ingest/`, not the `pipelines/` directory, which holds plans, source descriptors and built packages) turns external geo sources into MT2 packages. Every input is a **pinned, checksummed `Source`**: `Source::new(name, expected_sha256)` rejects any digest that isn't a canonical lowercase SHA-256 (`crates/maps2-ingest/src/lib.rs:41-70,261-262`). A `SourceDescriptor` wraps a `Source` with its public metadata — `kind`, `url`, `source_date`, `licence`, `attribution`, `bounds: [f64;4]`, `adapter_version` (`crates/maps2-ingest/src/lib.rs:124-141`) — parsed from a TOML file by `read_descriptor`, which additionally rejects non-HTTPS URLs (`DescriptorError::InsecureUrl`) and malformed bounds (`crates/maps2-ingest/src/lib.rs:192-215`). `validate_source`/`validate_source_reader` re-hash the actual bytes (streaming, in 64 KiB chunks, for the reader form) and compare against the pinned digest before any build proceeds (`crates/maps2-ingest/src/lib.rs:228-253`). Seven `SourceKind`s are supported: `osm-pbf`, `copernicus-dem`, `gebco-grid`, `water-polygons`, `natural-earth-places`, `natural-earth-boundaries`, `natural-earth-roads` (`crates/maps2-ingest/src/lib.rs:96-121`).

**CLI subcommands**, from `src/bin/maps2-ingest.rs`:

| Subcommand | What it does |
|---|---|
| `scan <osm.pbf>` | Summarises an OSM extract without building tiles (`:634-666`). |
| `verify <source.toml> <input>` | Hashes a local input against its pinned descriptor digest (`:510-515`). |
| `verify-package <package-dir>` | Recomputes `package_sha256` from the manifest's `tile_digests`, then re-hashes every tile file on disk against its recorded digest (`:524-559`). |
| `fetch <source.toml> <output>` | Downloads via `curl` (HTTPS only, `--fail --location --proto =https`) to a `.part` file, refusing to overwrite an existing output or partial, then validates the checksum before the atomic rename (`:561-605`). |
| `build <source.toml> <osm.pbf> <level> <output-dir>` | Builds one zoom level's tiles from a single OSM source and writes tiles plus manifest (`:75-88`). |
| `build-terrain` | One OSM source plus one DEM source and a west/south corner → one level's tiles with terrain attached (`:96-113`). |
| `build-terrain-many` | Like `build-terrain` but accepts several `<dem-source.toml> <dem.tif> <west> <south>` quadruples for one level (`:112-121`). |
| `build-terrain-range` | Like `build-terrain-many` but over an inclusive `<min-level>..<max-level>` range (`:123-131`). |
| `build-world` | Builds a low-zoom global package from the pre-simplified OSM water-polygon shapefile plus zero or more decimated GEBCO terrain quadrants, optionally stitched into one coarse world grid as a z0/z1 fallback (`:181-212`). |
| `build-map <plan.toml> <output-dir>` | Builds a multi-source map from a TOML build plan: loads every declared layer and terrain input, conflates each claimed level, writes tiles and one manifest (`:1139-1170`). |
| `carve <package-dir> <lon> <lat> <output-dir> [--world <level>] [--keep <min>:<max>:<radius>]…` | Copies an existing package's tiles verbatim into a smaller package around a point — world coverage at low levels plus a radius at higher ones — and rewrites the manifest; because tiles are copied, not re-encoded, digests are unchanged (`:654-699`). |
| `dem-info <dem.tif> <west> <south>` | Prints the DEM sample at one corner, for spot-checking a source (`:66-72`). |
| `gebco-window <source.toml> <grid.tif> <west> <south> <east> <north>` | Reads only the bounded window from a GEBCO/DEM source and reports how many TIFF chunks were actually decoded vs. the source total (`:607-631`). |

**Conflation rules** (`src/conflate.rs`). `conflate(level, layers)` applies two rules in order, documented in the module doc: **coverage** — inside the bounds of an *active, strictly stronger* source at that level, a weaker source's features are dropped, which is what stops a world road network being drawn under a city's own; and **identity** — a place a stronger source already named is not named again by a weaker one even outside its bounds, which stops one city carrying two labels a kilometre apart (`crates/maps2-ingest/src/conflate.rs:1-15,82-90`). `PLACE_MATCH_METRES = 25_000.0` is the matching radius, deliberately generous because two sources can disagree by kilometres about where a city "is" — historic centre versus built-up-area centroid (`crates/maps2-ingest/src/conflate.rs:29-33`). Identity matching is asked only of point classes (`Class::Label`, `Class::Poi`, via `names_a_place`) — the doc explains why: "a road is a line whose two renderings share no vertex and often no midpoint, so matching it by position would be guesswork; coverage already settles roads, and pretending otherwise would drop real geometry on a coincidence" (`crates/maps2-ingest/src/conflate.rs:91-95,160-164`). Only *strictly* stronger layers count toward coverage and identity for a given precedence tier — peers of equal precedence describe different global things (coastline, borders, roads, places) and must not silence each other (`crates/maps2-ingest/src/conflate.rs:97-104`).

**Two opposite GEBCO readers**, and why both exist. `gebco.rs`'s `load_gebco_window` does a *bounded* read: it turns a wanted geographic extent into a pixel window and decodes only the TIFF chunks that window touches, so cost tracks the requested region, not the multi-gigabyte source file. `WINDOW_CELL_LIMIT = 4 * 1024 * 1024` (4 Mi cells, 16 MiB of `f32`) caps what this reader will ever materialise, so a caller that forgot to bound its request fails loudly instead of swapping (`crates/maps2-ingest/src/gebco.rs:1-18`). `world_terrain.rs`'s `load_gebco_quadrant_decimated` does the opposite on purpose: it decodes an *entire* 90°×90° quadrant and keeps every `stride`-th sample, discarding the full decode afterward. Its module doc explains the reasoning: a low-zoom world tile's terrain raster is a fixed 256×256 samples regardless of source resolution, and a world tile at z2–z5 is itself wider than any in-budget bounded window could cover, so the bounded reader is "the wrong tool here" — peak memory is one quadrant's native decode (about 1.9 GB for a 21600×21600 `f32` grid) for the duration of the decimation pass (`crates/maps2-ingest/src/world_terrain.rs:1-38`).

### What a package is

A build writes a `manifest.json` alongside the tile tree, built by `manifest_json` (`crates/maps2-ingest/src/bin/maps2-ingest.rs:395-424`). It records: `"format": "MT2"` and `"format_version"` (from `maps2_tile::FORMAT_VERSION`, currently 5); the built `levels`; `feature_count` and `tile_count`; `tiles` — the sorted list of relative tile paths, `{z}/{x}/{y}.mt2`, produced by `tile_paths`, which sorts tile ids by `(z, x, y)` before formatting them (`:441-482`); `tile_digests`, a map from each tile path to its SHA-256; `package_sha256`, a single digest over the whole sorted digest map, computed by hashing each `(path, "\0", digest, "\n")` in map order (`:454-461` — a `BTreeMap` iterates in sorted key order, so this is deterministic); a `view` — the default centre and zoom, computed as the geographic centre of the tiles at the lowest built level (`:466-478`); and per-source `attribution`, `licence`, `url`, `sha256`, `source_date`, `bounds`, `adapter_version` for every descriptor that contributed (`:404-420`).

Determinism is the point of hashing everything this way: building twice from a clean checkout should produce byte-identical tiles and therefore identical digests. `maps2-fixtures` enforces this directly with golden-hash tests over its synthetic packages — for example `the_package_bytes_are_golden`, which FNV-1a-hashes every tile's id and bytes and asserts the result against a hard-coded `GOLDEN_FNV1A` constant, with the comment "The package must be bit-for-bit stable: any format or content change shows up as a hash change and is made knowingly" (`crates/maps2-fixtures/src/lib.rs:382-412`; the same pattern recurs in `crates/maps2-fixtures/src/roads.rs:201` and `crates/maps2-fixtures/src/ridge.rs:304`). `verify-package` performs the corresponding check against a built package on disk, and CI runs it on both committed lab packages before building the lab.

## Where the time goes

The lab measures itself against two budgets, both 10 ms: no single stretch of
main-thread work may be longer, and p95 `render()` on a still camera must stay
under it. Which of the two a piece of work answers to is the useful question to
ask of any change here.

**Frame time is cheap, and stays cheap by construction.** Style is evaluated
per frame, but it is arithmetic over a handful of classes. Meshes are not
rebuilt per frame — the crate doc says "a new frame is never a reason to
rebuild". GPU buffers are uploaded once, at residency transition, as
`STATIC_DRAW`. The residency plan is computed once per frame and cached, after a
bug where one drag built nine plans per pointer event. The one thing genuinely
rebuilt every frame is the glyph vertex buffer, because label placement is
itself a per-frame decision.

**Load time is where the cost concentrates**, and deliberately so: `load_tile`
turns one tile's bytes into every mesh and label anchor it will ever need, in
one synchronous call. That is the right trade for a map that then draws
sixty times a second from what it built — but it means a single tile can hold
the thread, and the host's decode slicing yields *between* tiles, not inside
one. Anything moved into `load_tile` is paid at the worst possible moment;
anything moved out of it, like label shaping, stops being a frame's problem
too. The measured record of this is in
[`applications/maps-v2-lab/e2e/perf/FINDINGS.md`](../../../applications/maps-v2-lab/e2e/perf/FINDINGS.md).

**Network time is not the map's time.** `fetch`, `digest` and the wall clock of
a whole load pass are traced but excluded from the responsiveness budget, so a
slow connection is never reported as a slow renderer.

## Known gaps

Recorded here so the document is honest about its own subject, not as a backlog.

- **Tilt is not a camera.** `maps2-camera` stores and clamps `tilt_deg` but
  `project` ignores it; the only place tilt reaches the screen is the building
  vertex shader, which leans wall geometry by `sin(tilt)`. The ground, roads and
  labels are not tilted. The crate doc says so plainly.
- **Text is not shaped.** `layout_line` advances one character at a time, left
  to right — the crate calls this "recorded debt against the world map, honest
  for Latin names". No bidi, no complex-script shaping, no road-following or
  repeated line labels; a named road gets one upright label at its midpoint.
- **Roof shape is a bounding-box guess**, exact for rectangles and a reasonable
  approximation otherwise — not a straight-skeleton solve.
- **No gesture sets bearing or tilt.** `Input` handles pan, wheel/pinch zoom,
  double-click, arrows and `+`/`-`. Rotation and tilt are host API calls only.
- **Two docs have drifted from the code.** `release-boundary.md` still describes
  MT2 v4 as the current write format and `implementation-plan.md` still says the
  format is frozen at v1; the code has been at v5 since 18 August 2026, as
  `tile-format.md` and `production-roadmap.md` both record correctly.
- **The performance suite does not run in CI**, for the reason `PERF.md` gives.
  `PERF_GATE=regressions` exists and is unused.
