# 3D Maps SDK v2

> A deterministic Rust/Wasm 3D map renderer with a browser laboratory.

**Status:** `v0.1.0-alpha` · a real Trafalgar Square carve in the lab, plus the local open-data pipeline · MIT

This repository is an SDK/demo alpha, not a production map of London or the
world. It focuses on a small, inspectable rendering core: versioned binary
tiles, WebGL2 rendering, interaction, labels, terrain, and globe relief.

Every study in the lab draws real ground. A small MT2 package carved around
**Trafalgar Square** ships in the repository, so `npm run dev` opens on real
OpenStreetMap geometry, real Copernicus and GEBCO relief, and real place names —
no build step, no data to download. See [the lab package](#the-lab-package).

![The globe over real GEBCO and Copernicus relief](applications/maps-v2-lab/e2e/terrain.spec.ts-snapshots/globe-relief-darwin.png)

![Animated Maps SDK showcase](applications/maps-v2-lab/assets/showcase.gif)

## What is included

- Rust crates for units, camera math, MT2 tile parsing, style, rendering, text,
  fixtures, and a WebGL2/Wasm binding.
- A browser lab with isolated visual cards for zoom bands, road joins and
  casing, input, point-label collision, density, terrain, and globe relief —
  all of them on the committed Trafalgar Square carve.
- `maps2-ingest carve`, which cuts a small package out of a built one: whole
  levels where the whole planet is on screen, a tile-radius square around a
  subject below that.
- An **MT2 v5** tile format with v1–v4 readers, deterministic synthetic fixtures, Rust unit
  tests, Playwright visual tests, and an executable p95 frame budget of ≤10 ms. Buildings
  carry base/top height, roof shape, and facade material, with documented fallbacks for
  incomplete OSM tags — see the [tile format spec](libraries/maps-v2/docs/tile-format.md).
- A bounded regional GEBCO/DEM reader that decodes only the raster window a build
  actually needs, never a whole world grid, for ocean terrain ingestion.

## What is not included

No published real-world London/world package (the committed carve is a lab
fixture-by-another-name, not a distribution), `roads-real`, routing, geocoding,
search, production data hosting, mobile host, road-following line labels, POI
icon labels, or full Unicode shaping is included. Named roads render as
upright midpoint labels; curved, repeated, and language-shaped road text remain
future work. Real-world data is a beta goal;
see [the beta plan](libraries/maps-v2/docs/real-data-beta-plan.md) and the
[production roadmap](libraries/maps-v2/docs/production-roadmap.md).

The first ingest foundation can verify, scan, and build z16 vector MT2 tiles
from the pinned Greater London OSM extract without committing it. It includes
roads, building footprints, water, parks, simple outer/inner multipolygon rings, named
places and amenity points. Road bridge/tunnel flags are preserved for the
renderer, and pinned Copernicus height rasters attach to
every generated London z16 tile. It clips cross-tile geometry deterministically,
but does not yet provide browser-ready real-data coverage. See the [pipeline
guide](pipelines/maps-v2-ingest/README.md).

The package manifest is the browser-host contract: it carries an MT2 format
version, sorted relative tile paths, a default view, source attribution, and
SHA-256 digests for every tile. The browser loader verifies those digests before
passing downloaded bytes to the renderer. It rejects manifests with more than
50,000 tiles and individual tile responses larger than 4 MiB before decoding.
After a camera move, it releases tiles outside the visible viewport plus the
renderer’s one-tile keep margin, so package CPU memory does not grow without
bound during exploration. The lab's **Пакет: загрузка по спросу** study exercises demand loading against
the same contract using synthetic tiles and accepts a host-selected manifest
URL for local real-data acceptance. Real derived packages remain external
release assets until their source terms and attribution are approved.

## Try the lab

Requirements: stable Rust, the `wasm32-unknown-unknown` target, `wasm-pack`,
Node.js 22.13+, and Chromium for end-to-end tests.

```sh
git clone https://github.com/bmsdave/3d-map.git
cd 3d-map
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
cd applications/maps-v2-lab
npm ci
npx playwright install chromium
npm run dev
```

Open `http://localhost:5178`. The page **is** the lab: all twenty studies are
mounted live on it, no card to click through first, every one of them looking at
Trafalgar Square. Direct routes still isolate one concern at a time —
`/#/card/roads-micro`, `/#/card/globe-relief` — and `/#/showcase` runs the
animated gallery.

### The lab package

`applications/maps-v2-lab/public/packages/trafalgar` is committed: 559 MT2 tiles,
about 119 MB. Its levels z1–z3 cover the whole planet, so the globe studies show
real coastlines and real GEBCO relief; z4–z16 cover a seven-by-seven square of
tiles around Trafalgar Square, which is what a 720×480 stage needs at 60° of tilt
and any bearing. `packages/trafalgar-city` (125 tiles, 30 MB) carries z12–z16 of
the same ground as a second package, because `globe-real`'s subject is composing
two packages on one map and one package cannot demonstrate that.

The camera is clamped to the ground the package covers: studies with free pan and
a continuous zoom cannot be driven off its edge into an empty frame.

Rebuild either package from a full one with the carve tool:

```sh
cargo run --manifest-path libraries/maps-v2/Cargo.toml --bin maps2-ingest -- \
  carve pipelines/maps-v2-ingest/packages/map-v1 -0.1281 51.5080 \
  applications/maps-v2-lab/public/packages/trafalgar --world 3 --keep 4:16:3
```

Or build one from the pinned sources with
[`plans/trafalgar.toml`](pipelines/maps-v2-ingest/plans/trafalgar.toml), which
needs no prior package. Either way, `maps2-ingest verify-package <dir>` re-checks
every digest, and CI runs it on both committed packages.

The data is © OpenStreetMap contributors (ODbL), Copernicus DEM (© DLR e.V.
2010–2014, © Airbus DS 2014–2018, provided under COPERNICUS by the EU and ESA),
GEBCO 2026, and Natural Earth. Attribution travels in each package manifest and
is shown in the lab's own chrome; see [`NOTICE`](NOTICE).

To inspect a locally hosted London or regional package, open
`/#/card/package-loader`, replace **Manifest URL** with its `manifest.json`
URL, and choose **Загрузить пакет**. The package host must permit browser CORS
requests; the loader verifies every requested tile against the manifest’s
SHA-256 digest before rendering it.

## Demos

The lab's board (`npm run dev`, then `http://localhost:5178`) opens rendering:
a live hero canvas, a **Quick start** panel with the exact SDK call shape every
study below it runs, and all twenty studies already mounted and interactive on
the same page. Filter them by text or by group; nothing is behind a click. The
snippet is real, not pseudocode:

```ts
import { createMap, loadPackCentre } from "./sdk";

const canvas = document.querySelector("canvas")!;
const centre = await loadPackCentre("ealing"); // synthetic fixture; see below for a real package
const map = await createMap(canvas, "ealing");
map.setCentre(centre.lon, centre.lat);
map.setZoom(centre.zoom);
map.render();
```

Because a browser tab caps how many WebGL2 contexts it will keep, the board
runs the six studies nearest the viewport and hands a context back when a study
scrolls away — the counter in the toolbar reports how many are live.

A few studies worth opening on their own page:

| Route | What it shows |
| --- | --- |
| `/#/` | All twenty studies, live and interactive on one board. |
| `/#/showcase` | Twenty animated, live SDK studies in one gallery. |
| `/#/card/buildings3d` | Terrain-clamped 3D buildings, roof shapes, and tilt. |
| `/#/card/roads-micro` | Road classes, joins, casing, and screen-pixel widths at z17. |
| `/#/card/package-loader` | Demand-loaded MT2 packages: point it at a real `manifest.json`. |

### A real local London demo

The synthetic fixtures above need no data download. To see the SDK render
**real** OpenStreetMap and Copernicus terrain data instead, build a local
London package from the pinned sources (see the [pipeline
guide](pipelines/maps-v2-ingest/README.md) for `fetch`/`verify` first):

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build-terrain-range \
  ../../pipelines/maps-v2-ingest/sources/london.toml \
  /path/to/cache/greater-london-260814.osm.pbf 12 16 \
  ../../pipelines/maps-v2-ingest/packages/london-v5 \
  ../../pipelines/maps-v2-ingest/sources/london-dem-n51w001.toml \
  /path/to/cache/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51 \
  ../../pipelines/maps-v2-ingest/sources/london-dem-n51e000.toml \
  /path/to/cache/Copernicus_DSM_COG_10_N51_00_E000_00_DEM.tif 0 51
```

Serve `pipelines/maps-v2-ingest/packages/london-v5/` locally with CORS enabled
(for example `npx http-server pipelines/maps-v2-ingest/packages/london-v5
--cors`), open `/#/card/package-loader` in the running lab, paste that host's
`manifest.json` URL into **Manifest URL**, and choose **Загрузить пакет**. This
is the same manifest-driven demand-loading path the lab's opt-in
`MAPS2_REAL_PACKAGE_ROOT` Playwright test exercises against real terrain,
attribution, and the ≤10 ms p95 frame budget.

### One map, built from every source at once

Composing two packages in the browser works, and `addSourceLevels` is a
real capability — but it is the wrong place to decide which source owns a
piece of ground. By tile time a feature no longer knows where it came
from, so Natural Earth's London and OSM's London both arrive, a kilometre
apart, and the M25 arrives twice: once generalised, once as a run of ways.
The renderer cannot repair that at sixty frames a second.

`build-map` settles it once, at build time, against a plan that says where
each source speaks, over which levels, and how strongly:

```sh
cd pipelines/maps-v2-ingest
../../libraries/maps-v2/target/release/maps2-ingest \
  build-map plans/london-world.toml packages/map-v1
```

Two rules resolve the overlaps ([`conflate.rs`](libraries/maps-v2/crates/maps2-ingest/src/conflate.rs)):

- **Coverage** — inside the bounds of a stronger source that is active at
  this level, weaker sources are silent. This is what stops a generalised
  world road network being drawn underneath a city's own.
- **Identity** — a place a stronger source has already named is not named
  again by a weaker one, even outside its bounds, matched on name within
  25 km. This is what gives one city one label.

Identity is matched for places only. Two renderings of a road share no
vertex and often no midpoint, so matching lines by position would be
guesswork; coverage settles roads honestly instead.

The plan also decides the pyramid. World layers are global, so every level
they claim costs the whole planet — z8 alone is 41,000 tiles and z11 is
millions — so coastline, borders and roads stop at z7. Places reach to z11
because they are points and rank gating admits only cities and towns
there, which is a few thousand tiles rather than millions, and because
that band is where the city extract and the world source genuinely
overlap. The city extract owns z8–z16 over its own ground; the renderer
falls back to a coarser tile everywhere else.

The result is one manifest, 41,255 tiles across a continuous z1–z16, from
fifteen pinned sources. The build reports what it reconciled — on this
plan, four Natural Earth places dropped where OSM covers the same ground,
which is why zooming to London at z10 shows OSM's suburb names and exactly
one "London" instead of two a kilometre apart:

```sh
cd applications/maps-v2-lab
MAPS2_MAP_PACKAGE_ROOT=../../pipelines/maps-v2-ingest/packages/map-v1 \
npx playwright test e2e/map-real.spec.ts
```

Serve it with CORS and open `/#/card/map-real`.

### A real global globe, composed with the city

The city package covers one city. The world package covers everything else:
real coastlines from the OSM community's split water polygons, and real
GEBCO relief from the eight 90°×90° quadrants. Built together they give a
globe you can spin and then zoom all the way into the city without the map
going blank in between.

```sh
cd pipelines/maps-v2-ingest
# One <source.toml> <quadrant.tif> <stride> triple per GEBCO quadrant.
# Stride 4 keeps the whole build under a few GB of RAM and still carries
# more samples than a z7 tile's 256x256 height raster can hold.
NE=cache/global/naturalearth
../../libraries/maps-v2/target/release/maps2-ingest build-world \
  sources/world-water-polygons.toml \
  cache/global/simplified-water-polygons-split-3857/simplified_water_polygons.shp \
  1 7 packages/world-v9 \
  --places     sources/natural-earth-places.toml     $NE/ne_10m_populated_places/ne_10m_populated_places.shp \
  --boundaries sources/natural-earth-boundaries.toml $NE/ne_10m_admin_0_boundary_lines_land/ne_10m_admin_0_boundary_lines_land.shp \
  --roads      sources/natural-earth-roads.toml      $NE/ne_10m_roads/ne_10m_roads.shp \
  sources/gebco-2026-n0-s-90-w-180-e-90.toml  cache/global/gebco_quadrants/gebco_2026_sub_ice_n0.0_s-90.0_w-180.0_e-90.0_geotiff.tif  4 \
  sources/gebco-2026-n0-s-90-w-90-e0.toml     cache/global/gebco_quadrants/gebco_2026_sub_ice_n0.0_s-90.0_w-90.0_e0.0_geotiff.tif     4 \
  sources/gebco-2026-n0-s-90-w0-e90.toml      cache/global/gebco_quadrants/gebco_2026_sub_ice_n0.0_s-90.0_w0.0_e90.0_geotiff.tif      4 \
  sources/gebco-2026-n0-s-90-w90-e180.toml    cache/global/gebco_quadrants/gebco_2026_sub_ice_n0.0_s-90.0_w90.0_e180.0_geotiff.tif    4 \
  sources/gebco-2026-n90-s0-w-180-e-90.toml   cache/global/gebco_quadrants/gebco_2026_sub_ice_n90.0_s0.0_w-180.0_e-90.0_geotiff.tif   4 \
  sources/gebco-2026-n90-s0-w-90-e0.toml      cache/global/gebco_quadrants/gebco_2026_sub_ice_n90.0_s0.0_w-90.0_e0.0_geotiff.tif      4 \
  sources/gebco-2026-n90-s0-w0-e90.toml       cache/global/gebco_quadrants/gebco_2026_sub_ice_n90.0_s0.0_w0.0_e90.0_geotiff.tif       4 \
  sources/gebco-2026-n90-s0-w90-e180.toml     cache/global/gebco_quadrants/gebco_2026_sub_ice_n90.0_s0.0_w90.0_e180.0_geotiff.tif     4
```

The three Natural Earth layers are what make the low zooms a *map* rather
than a relief model: place names, country borders and the trunk road
network. Without them the world package carries only coastlines and a
height raster, and every zoom below the city package draws hill shading
and nothing else. Fetch them into the cache first (public domain, ~13 MB
total, pinned by SHA-256 in the three `sources/natural-earth-*.toml`
descriptors) from `https://naciscdn.org/naturalearth/10m/cultural/`:
`ne_10m_populated_places`, `ne_10m_admin_0_boundary_lines_land` and
`ne_10m_roads`.

That writes 16,132 tiles across z1–z7 (~2.1 GB) and a manifest naming all
twelve sources; `verify-package` checks it, and building it twice from
clean gives byte-identical tile digests. Serve it with CORS alongside the
London package, open `/#/card/globe-real`, and put the two `manifest.json`
URLs in **Мир** and **Город**. The two packages compose on one map:
`addSourceLevels` unions the pyramids rather than replacing them, and
wherever the city package has no coverage the renderer draws the nearest
world tile underneath instead of leaving a hole.

Both packages are also what the opt-in acceptance suite runs against:

```sh
cd applications/maps-v2-lab
MAPS2_WORLD_PACKAGE_ROOT=../../pipelines/maps-v2-ingest/packages/world-v9 \
MAPS2_REAL_PACKAGE_ROOT=../../pipelines/maps-v2-ingest/packages/london-v5 \
npx playwright test
```

## 20 animated studies

Launch the full gallery at `/#/showcase`. Every study is a live WebGL2 canvas
driven by the SDK, with a shared pause/play control — not a video, GIF, or
mockup. The scenes use only the committed deterministic fixture packages.

| # | Study | Focus |
| --- | --- | --- |
| 01 | First light | Globe-scale synthetic relief |
| 02 | Blue hour | Globe-to-city transition |
| 03 | Contour | Terrain perspective |
| 04 | Ribbon | Animated road line |
| 05 | Long shadow | Hillshade and relief |
| 06 | Crossfade | Flat/globe blend |
| 07 | Junction | Road join geometry |
| 08 | Atlas | Collision-managed labels |
| 09 | Orbit | Low-altitude globe arc |
| 10 | Green room | Park and land composition |
| 11 | Switchback | Sharp road turns |
| 12 | Highlands | Relief exaggeration |
| 13 | Northbound | Camera movement |
| 14 | Roundabout | Continuous circular road |
| 15 | Far side | Globe curvature |
| 16 | Density | Label placement under load |
| 17 | Overpass | Bridges and tunnels |
| 18 | Rise | Globe-only terrain displacement |
| 19 | City pulse | Continuous zoom bands |
| 20 | Afterglow | Terrain exit pass |

The older direct-link cards remain available for technical inspection and
regression testing. The showcase is the polished presentation surface; the
cards are the engineering workbench behind it.

## Verify a clean checkout

```sh
cd libraries/maps-v2
cargo test --workspace
cargo clippy -p maps2-units --all-targets -- -D warnings
cd ../../applications/maps-v2-lab
npm run build
npm run test:e2e
```

The lab regenerates its synthetic fixture output locally. It needs no
proprietary data, pipeline cache, or external map package.

## Initial local-SDK scope

This first open-source version is for local development. It includes the SDK,
browser lab, deterministic fixtures, and a reproducible local pipeline for
openly licensed OSM and Copernicus inputs. The application code is MIT.

It deliberately excludes package hosting/distribution, package signing,
rollback/on-call processes, and production service support. Those are later
owner-led release decisions, not requirements for using or contributing to the
SDK locally.

## SDK shape

```rust
use maps2_units::{Lonlat, Zoom};

let centre = Lonlat { lon: -0.3049, lat: 51.5149 };
let zoom = Zoom::new(14.5);
```

The browser host creates a `maps2-web` map for a canvas, loads MT2 tile bytes
once, sends camera/style changes, and calls `render()`. Persistent tile buckets
stay on the GPU between frames. `debug()`, label diagnostics, pixel samples, and
frame measurements are host-requested diagnostics rather than hidden per-frame
work. The lab's [`sdk.ts`](applications/maps-v2-lab/src/sdk.ts) is the complete
working Wasm integration example.

## Architecture

| Area | Responsibility |
| --- | --- |
| `maps2-units`, `maps2-camera` | Coordinate units, camera state, flat/globe projection |
| `maps2-tile` | MT2 v5 writing and validated v1–v4 reading |
| `maps2-style`, `maps2-render` | Class visibility, persistent mesh buckets, road and terrain draws |
| `maps2-text` | Deterministic SDF atlas and collision-managed point labels |
| `maps2-fixtures` | Reproducible synthetic Ealing, road-pathology, and ridge packages |
| `maps2-web` | WebGL2/Wasm browser boundary |
| `maps-v2-lab` | Interactive demo, visual goldens, and browser contracts |

## MT2 tile format

MT2 uses a fixed header, an O(1) section table, integer tile coordinates,
delta/varint vector geometry, building base/top/roof/material data, and
optional height rasters. Version 5 is the current write format and readers
accept versions 1–4: a layout change requires a version bump, fixture
migration, and an intentional golden update. Read the full [MT2
specification](libraries/maps-v2/docs/tile-format.md).

## Supported environment

CI targets Linux, Rust stable, Node 22, Chromium, Wasm, and WebGL2. The lab is
intended for current desktop WebGL2 browsers, but broad browser compatibility is
not yet promised. This alpha is deliberately browser-first; iOS, Android, SSR,
and headless renderer support remain future work.

## Contributing and security

Read [CONTRIBUTING.md](CONTRIBUTING.md) before proposing behavioral or format
changes. Public security reports are not appropriate; see [SECURITY.md](SECURITY.md).
The contribution space follows [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md).

## License

Licensed under [MIT](LICENSE). Copyright 2026 Vadim.
