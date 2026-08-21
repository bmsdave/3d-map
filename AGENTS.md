# AGENTS.md — 3D Maps SDK v2

Fast map for AI agents. Read this first; read full docs only on demand.

## Token budget for agents

Bootstrap: `AGENTS.md` + ONE of `architecture.tldr.md:1` / `tile-format.en.tldr.md:1` / grep result. Never read `README.md:1` (20k), `architecture.md:1` (28k), `sdk.ts:1` (28k) fully at bootstrap. Controller <2k tok, subagent <4k. For long loops / multiple tasks: use `maps2-loop` skill — delegate per-crate/card work to subagents, controller keeps summaries only.

## Stack

Rust `libraries/maps-v2` (9 crates) + lab `applications/maps-v2-lab` (TS+Wasm). Split `architecture.md:20`.

## Crate map

| Crate | Path | Owns | Key file |
|---|---|---|---|
| maps2-units | `crates/maps2-units/src/lib.rs:1` | Metres/ScreenPx, TileId/TileCoord, mercator | `lib.rs:18` TILE_EXTENT=65536 |
| maps2-camera | `crates/maps2-camera/src/lib.rs:54` | Camera, patch validation, flat↔globe | `lib.rs:17` Globeness at 3.5-4.5 |
| maps2-tile | `crates/maps2-tile/src/lib.rs:48` | MT2 v5 writer, v1-5 reader | `lib.rs:16` header layout |
| maps2-style | `crates/maps2-style/src/lib.rs:1` | Class 0-12, bands, widths, palette | `lib.rs:1` class enum |
| maps2-render | `crates/maps2-render/src/lib.rs:1` | Residency, buckets, meshes | `residency.rs:1` select_tiles |
| maps2-text | `crates/maps2-text/src/lib.rs:1` | SDF atlas, collision | `collision.rs:1` 64px grid |
| maps2-web | `crates/maps2-web/src/lib.rs:1` | Wasm boundary, Map handle | `map.rs:1` load_tile/render |
| maps2-ingest | `crates/maps2-ingest/src/lib.rs:41` | Pipeline, conflate, manifests | `conflate.rs:29` 25km match |
| maps2-fixtures | `crates/maps2-fixtures/src/lib.rs:1` | Synthetic Ealing/ridge/roads | `lib.rs:382` golden hash |

## Lab map

| Area | Path | Note |
|---|---|---|
| SDK | `src/sdk.ts:441` | `createMap` `sdk.ts:292` |
| Cards | `src/cards/types.ts:1` | `CardSpec` |
| Router | `src/main.ts:1` | `#/` board |
| Showcase | `src/showcase.ts` | 20 studies |
| Perf | `src/perfTrace.ts` | 10ms `e2e/perf/FINDINGS.md` |

## Commands

```sh
cd libraries/maps-v2 && cargo test --workspace
cd libraries/maps-v2 && cargo clippy -p maps2-units --all-targets -- -D warnings
cd applications/maps-v2-lab && npm ci && npm run build:sdk && npm run typecheck && npm run build && npm run dev
cd libraries/maps-v2 && cargo run -p maps2-ingest -- verify-package <dir>
```

## MT2 v5 quick ref

Header LE 20+10*count `lib.rs:16` + raster `0xFF00` 131072B `heights.rs:1` — full `tile-format.en.tldr.md:1`. Vector `docs/tile-format.md:60` id/flags/rank/base_dm/top_dm/roof/material/name/deltas. Never panics `lib.rs:74`.

## Conflation

`conflate(level,layers)` `conflate.rs:1` coverage+identity 25km `conflate.rs:29` Label/Poi only.

## Manifest

`manifest.json` `bin/maps2-ingest.rs:395` MT2 v5 sorted tiles, digests. Limit 50000, 4MiB `sdk.ts:194`.

## Where to edit

| Task | Edit | Verify |
|---|---|---|
| Road width/join | `maps2-style/lib.rs` + `maps2-render/line.rs:1` | `e2e/roads.spec.ts` |
| Building 3D | `maps2-render/building.rs` + `maps2-web/gl_building.rs` | `e2e/buildings.spec.ts` |
| Labels | `maps2-text/collision.rs` + `maps2-render/labels.rs` | `e2e/labels.spec.ts` |
| Camera/globe | `maps2-camera/lib.rs:35` Globeness + `maps2-render/globe.rs` | `e2e/terrain.spec.ts` |
| Input | `maps2-web/input.rs:1` | `e2e/input.spec.ts` |
| Tile format | `maps2-tile/lib.rs:48` + `docs/tile-format.md` bump version + golden `maps2-fixtures/lib.rs:382` | `cargo test` |
| Ingest | `maps2-ingest/conflate.rs` `gebco.rs` | `verify-package` |
| New card | `src/cards/<id>.ts` + `src/cards/index.ts` | `npm run build` |

## Agent rules

1. Use `grep` before `read`. Use `glob` with specific pattern, never `**/*`.
2. Never read `target/`, `node_modules/`, `public/packages/`, `public/fixtures/`, `dist/`, `*.mt2`, `e2e/*snapshots/`.
3. Prefer `read` with `offset/limit`. Keep diffs minimal (CONTRIBUTING.md:5).
4. Always emit `file_path:line` refs. Run verification before claiming success.
5. For rendering changes: intentional golden update + visual check required.
6. Use `sdk.ts:292` loader for real packages, `sdk.ts:204` loadFixtureTiles for synthetic.
