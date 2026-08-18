# Changelog

## Unreleased

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

## 0.1.0-alpha — 2026-08-15

- Initial public alpha: deterministic MT2 v1 tiles, synthetic fixtures,
  flat/globe rendering, roads, point labels, terrain, and browser lab.
- Includes Rust, browser, visual, coverage, and frame-budget release checks.
- Does not include real-world data ingest or packages; see the beta plan.
