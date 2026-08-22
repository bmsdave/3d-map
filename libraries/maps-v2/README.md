# 3D Maps SDK v2 — v0.1.0-alpha

A deterministic Rust/Wasm SDK for rendering tiled 3D map experiments in a
browser. It is released with a browser lab, not a real-world map dataset.

## Quickstart

Requirements: stable Rust with `wasm32-unknown-unknown`, `wasm-pack`, Node.js
22.13 or newer, and a Chromium browser for the end-to-end suite.

```sh
rustup target add wasm32-unknown-unknown
cargo install wasm-pack
cd applications/maps-v2-lab
npm ci
npx playwright install chromium
npm run dev
```

Open `http://localhost:5178`. Each lab card also has a direct URL such as
`/#/card/roads-micro`.

Run the release checks from a clean checkout:

```sh
cd libraries/maps-v2 && cargo test --workspace
cd ../../applications/maps-v2-lab && npm run build && npm run test:e2e
```

## Architecture

`maps2-units` protects coordinate units; `maps2-camera` owns the camera and
projection; `maps2-tile` reads the MT2 v6 binary format (v1–v6 readable); `maps2-style` decides
class visibility and appearance; `maps2-render` builds persistent render
buckets; `maps2-text` provides deterministic SDF glyphs and collision placement;
`maps2-fixtures` creates committed synthetic packages; and `maps2-web` exposes
the WebGL2/Wasm surface used by the lab.

The browser host loads fixture bytes once, then sends camera and style changes
to the SDK. Rendering keeps resident GPU buffers between frames. Diagnostics
and pixel sampling are requested by the host, never performed in every frame.

## API and tile format

The current browser entry point is `maps2-web`: create a map for a canvas,
load MT2 bytes, set camera/style state, render, and request `debug()` or label
diagnostics when needed. The lab is the executable API example.

MT2 v6 is frozen (v1–v5 remain readable). Its header, sections, vector features, height raster, error
rules, and versioning process are documented in [tile-format.md](docs/tile-format.md).
Changing the layout requires a format-version bump and deliberate fixture and
golden updates.

## Supported environment

CI targets Linux, Rust stable, Node 22, Chromium, Wasm, and WebGL2. The lab is
also intended for current desktop browsers with WebGL2, but browser parity is
not yet a compatibility promise. iOS, Android, SSR/headless rendering, and
production data hosting are outside this alpha.

## Deterministic fixtures and limits

All committed fixtures are synthetic and generated from source in this
workspace; no proprietary or real-world data is needed to build or test. Golden
changes must be intentional and reviewed. The alpha does not include OSM/DEM
ingest, provenance/attribution handling, London or world packages, `roads-real`,
line labels, POI icon labels, full Unicode text shaping, routing, search, or
geocoding. See [release-boundary.md](docs/release-boundary.md) and the separate
[beta plan](docs/real-data-beta-plan.md).

## License

MIT. See the root [LICENSE](../../LICENSE).
