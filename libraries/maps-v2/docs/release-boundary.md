# v0.1.0-alpha release boundary

## Included

- Rust workspace crates for units, camera, MT2 v4 tile writing with v1/v2/v3 reading, style, fixture
  generation, rendering, text placement, and the WebGL2/Wasm boundary.
- MT2 v4 preserves 64-bit OSM feature IDs; label diagnostics expose their
  128-bit class-plus-feature identities as strings so browsers never lose precision.
- A deterministic synthetic fixture set and browser lab cards for zoom bands,
  roads, point-label collision, density, input, globe transition, terrain, and
  globe relief.
- Rust tests, Playwright visual/interaction checks, and an on-demand p95
  rendering-card frame-budget check of 10 ms or less.

## Excluded

This is not a map of London or the world. The local Stage-8 foundation can
fetch HTTPS OSM/DEM inputs with checksum validation and build untracked Greater
London MT2 packages at one level or an inclusive level range, carrying source
provenance and attribution in the manifest. MT2 v3 supports simple
outer/inner multipolygon rings, but this alpha does not provide a published
London/world package, cartographic generalisation, full relation topology
repair, or a `roads-real` browser surface. Line labels and
POI icon-plus-text labels are planned but not implemented. Text is a
deterministic Latin fixture atlas, not a general Unicode shaping system.

The alpha also excludes production availability, mobile host support, search,
routing, geocoding, analytics layers, and a data-hosting service.

## Compatibility and release policy

MT2 v4 is the current alpha write format; readers continue to accept v1/v2/v3. The browser API and fixture content remain alpha APIs and may
change before beta. Goldens are deterministic fixtures: update them only with a
documented rendering reason. Real-world data support is explicitly a beta
milestone; its delivery plan is [real-data-beta-plan.md](real-data-beta-plan.md).

This source is licensed under MIT. The alpha's API and data limitations
remain unchanged by that license choice.
