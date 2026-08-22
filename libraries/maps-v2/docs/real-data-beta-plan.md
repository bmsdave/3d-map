# Non-blocking plan: real-data beta

This is a planning document, not alpha scope. Derived packages must remain
untracked by default.

## Verified starting point

Greater London already has a pinned OSM extract and two pinned Copernicus
inputs, deterministic MT2 v6 output, per-tile/package checksums, and a local
Chromium acceptance test. On 2026-08-16, two clean z12 terrain builds produced
159,726 feature parts in 70 tiles with the same package digest:
`db15c97fd6983f2577e8ed4b997e9f11952e118bdcf00ee66bbcd29a5d41849a`.
That test also proves demand loading, visible attribution, terrain, tilt, and
a p95 frame time of 10 ms or less. None of those generated inputs or outputs
are tracked by Git.

On 2026-08-18, with MT2 bumped to v5, a full local z12–z16 London build ran
from the same three validated inputs. The first pass surfaced a real
correctness bug: a relation listing the same outer member way twice emitted
that ring's geometry twice. Fixed by deduping member way ids before stitching
(`relation_rings`), which removed exactly 11 duplicate feature parts from the
real London extract — a concrete case, not a synthetic worry. After the fix,
two clean `build-terrain-range` runs each produced 4,017,061 feature parts
across 16,246 terrain-bearing tiles (2.2 GB) with the identical package digest
`c6e61742d63afd68a40bc07a331a358d9d5b16f16e022ea291eaf193c6ce3f28`. The real
package loaded through `MAPS2_REAL_PACKAGE_ROOT` and passed the Chromium
acceptance test — demand loading, attribution, terrain, tilt, and a p95 frame
time of 10 ms or less — this time exercising v5 building payloads on real OSM
data rather than the older v4 test package. A byte-level sample of real
building features confirmed the v5 material/roof pipeline decodes real tag
diversity rather than only the fallback: most buildings correctly fall back to
`Flat`/`Unknown` (OSM rarely tags `roof:shape` or `building:material`), and the
buildings that do carry those tags decoded `Gabled`, `Hipped`, `Other`,
`Brick`, `Stone`, and `Wood` correctly.

1. Create `pipelines/maps-v2-ingest` with isolated readers for version-pinned
   OSM PBF and DEM inputs, plus unit tests for tag mapping and height handling.
2. Add reproducible source acquisition: checksummed source manifests, immutable
   source URLs/releases, documented refresh commands, and cache locations kept
   out of Git.
3. Record provenance, attribution, licences, processing version, and source
   date in every output package manifest. Review OSM/ODbL and each DEM source's
   redistribution terms before publishing derived data.
4. Build a London package first, then a low-zoom world package. Validate format
   version, coverage, deterministic regeneration, package size, and visual
   quality against committed small test fixtures rather than committing derived
   city/world payloads.
5. Add a `roads-real` lab card that validates class mapping, generalisation,
   bridges/tunnels, source attribution, and intentional visual goldens.
6. Publish only source code, manifests, and small synthetic fixtures by default.
   Make data-package release an explicit owner-approved action after licence and
   attribution review.

## Remaining beta gates

| Gate | Evidence required before it can close |
| --- | --- |
| London topology | Geometry repair and topology-aware area generalisation; fixtures for incomplete/overlapping relations and shared boundaries. Duplicate multipolygon member ways are now deduped before stitching (a real bug, caught against the actual London extract — see above); nested holes, unclosed member chains, and topology-aware simplification that keeps adjacent-feature shared edges in sync remain open. |
| London visual quality | A `roads-real` acceptance surface with intentional goldens for real roads, buildings, labels, water, parks, and terrain. |
| Global inputs | A checked-in shard inventory for OSM, Copernicus land DEM, and GEBCO bathymetry. Each descriptor must pin version, URL, SHA-256, licence, attribution, bounds, and adapter version. GEBCO now has a bounded-window reader (`maps2-ingest::load_gebco_window`) that decodes only the TIFF chunks a requested region overlaps, capped at `WINDOW_CELL_LIMIT` cells, and one descriptor (`gebco-2025-n90-s0-w-90-e0.toml`) pinning the sub-grid covering London — its SHA-256 is still a placeholder pending an actual download. The OSM/Copernicus half of the inventory and the world-tiling check remain open. |
| World assembly | Region-sharded low-zoom build, seam/border validation, aggregated source notices, and two clean identical runs across z0–z5. |
| Data release | Owner approval of source attribution/redistribution review, package signing, hosting, rollback, and support ownership. |

For ocean terrain, freeze one annual GEBCO release only after downloading it
from the official grid distribution and recording its SHA-256. GEBCO publishes
global 15-arc-second terrain as eight 90° × 90° tiles and permits copying,
adapting, publishing, distributing, and commercial use with attribution. Its
annual cadence means a bare “latest” URL is explicitly not an acceptable source
descriptor. See [GEBCO’s grid and terms](https://www.gebco.net/data-products-gridded-bathymetry-data/gebco2025-grid).
