# Non-blocking plan: real-data beta

This is a planning document, not alpha scope. Derived packages must remain
untracked by default.

## Verified starting point

Greater London already has a pinned OSM extract and two pinned Copernicus
inputs, deterministic MT2 v4 output, per-tile/package checksums, and a local
Chromium acceptance test. On 2026-08-16, two clean z12 terrain builds produced
159,726 feature parts in 70 tiles with the same package digest:
`db15c97fd6983f2577e8ed4b997e9f11952e118bdcf00ee66bbcd29a5d41849a`.
That test also proves demand loading, visible attribution, terrain, tilt, and
a p95 frame time of 10 ms or less. None of those generated inputs or outputs
are tracked by Git.

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
| London topology | Geometry repair and topology-aware area generalisation; fixtures for incomplete/overlapping relations and shared boundaries. |
| London visual quality | A `roads-real` acceptance surface with intentional goldens for real roads, buildings, labels, water, parks, and terrain. |
| Global inputs | A checked-in shard inventory for OSM, Copernicus land DEM, and GEBCO bathymetry. Each descriptor must pin version, URL, SHA-256, licence, attribution, bounds, and adapter version. |
| World assembly | Region-sharded low-zoom build, seam/border validation, aggregated source notices, and two clean identical runs across z0–z5. |
| Data release | Owner approval of source attribution/redistribution review, package signing, hosting, rollback, and support ownership. |
