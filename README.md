# 3D Maps SDK v2

> A deterministic Rust/Wasm 3D map renderer with a browser laboratory.

**Status:** `v0.1.0-alpha` · synthetic fixtures only · Apache-2.0

This repository is an SDK/demo alpha, not a production map of London or the
world. It focuses on a small, inspectable rendering core: versioned binary
tiles, deterministic fixture generation, WebGL2 rendering, interaction, labels,
terrain, and globe relief.

![Synthetic globe-relief fixture](applications/maps-v2-lab/e2e/terrain.spec.ts-snapshots/globe-relief-darwin.png)

## What is included

- Rust crates for units, camera math, MT2 tile parsing, style, rendering, text,
  fixtures, and a WebGL2/Wasm binding.
- A browser lab with isolated visual cards for zoom bands, road joins and
  casing, input, point-label collision, density, terrain, and globe relief.
- A frozen **MT2 v1** tile format, deterministic synthetic fixtures, Rust unit
  tests, Playwright visual tests, and an executable p95 frame budget of ≤10 ms.

## What is not included

No OSM/DEM ingestion, real-world London/world package, `roads-real`, routing,
geocoding, search, production data hosting, mobile host, line labels, POI icon
labels, or full Unicode shaping is included. Real-world data is a beta goal;
see [the beta plan](libraries/maps-v2/docs/real-data-beta-plan.md).

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

Open `http://localhost:5178`. Direct routes make each renderer concern easy to
inspect, for example `/#/showcase`, `/#/card/roads-micro`, and
`/#/card/globe-relief`. The showcase contains twenty animated, live SDK studies.

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

## Current alpha blocker

The release tests and browser suite pass, but strict workspace-wide Clippy is
not green yet: `maps2-camera`, `maps2-style`, `maps2-tile`, and `maps2-text`
still have an inherited pedantic-lint cleanup backlog. The CI job keeps that
gate visible rather than suppressing it. `maps2-units`, the original reported
lint failure, is clean.

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
| `maps2-tile` | MT2 v1 parsing with validated zero-copy views |
| `maps2-style`, `maps2-render` | Class visibility, persistent mesh buckets, road and terrain draws |
| `maps2-text` | Deterministic SDF atlas and collision-managed point labels |
| `maps2-fixtures` | Reproducible synthetic Ealing, road-pathology, and ridge packages |
| `maps2-web` | WebGL2/Wasm browser boundary |
| `maps-v2-lab` | Interactive demo, visual goldens, and browser contracts |

## MT2 tile format

MT2 uses a fixed header, an O(1) section table, integer tile coordinates,
delta/varint vector geometry, and optional height rasters. Format version 1 is
frozen: a layout change requires a version bump, fixture migration, and an
intentional golden update. Read the full [MT2 v1 specification](libraries/maps-v2/docs/tile-format.md).

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

Licensed under [Apache-2.0](LICENSE). Copyright 2026 Vadim.
