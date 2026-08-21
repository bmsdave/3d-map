# Changelog

## Unreleased

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
