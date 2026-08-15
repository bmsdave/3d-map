# Non-blocking plan: real-data beta

This is a planning document, not alpha scope. Derived packages must remain
untracked by default.

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
