# Production roadmap

This roadmap turns the SDK into an embeddable, self-hostable 3D map. It does
not add a hosted tile service, user accounts, routing, search, or geocoding.
The first acceptance package is **Greater London**; the same reproducible
pipeline then produces the global package.

## Product contract

- The public code is MIT. Source data keeps its own terms: every package ships
  a machine-readable provenance manifest and the browser host exposes its
  required attribution.
- The SDK takes a versioned MT2 package from a host-controlled URL or local
  filesystem. It never embeds a data provider token and does not require a
  proprietary cache to build or test.
- The initial sources are OSM for vector data, Copernicus DEM for land terrain,
  and GEBCO for global bathymetry. A source adapter boundary permits a future
  licensed provider without changing MT2 or the renderer.
- Derived packages, downloaded source data, and pipeline caches are ignored by
  Git. Small synthetic fixtures remain the only committed map content.

## Current position (2026-08-18)

The project has the beginnings of the London path: pinned Greater London OSM
and two Copernicus DEM descriptors, HTTPS-only atomic source acquisition with
checksum validation, deterministic MT2 v5 encoding with v1–v4 readers, terrain rasters, OSM
building-height/roof/material/base-height fallbacks, and deterministic
tile-border clipping, outer/inner-ring multipolygon relations whose member
ways are not emitted a second time, named places, bridge/tunnel road structure
flags, and
amenity points. Relations whose outer member lists the same way id twice — a
real OSM data-quality issue — no longer double-emit that ring's geometry; on
the real London extract this fix removed 11 duplicate feature parts. A local
z12–z16 London rebuild from real inputs now emits 4,017,061 feature parts in
16,246 terrain-bearing tiles (2.2 GB); two independent clean builds produced
the identical package digest
`c6e61742d63afd68a40bc07a331a358d9d5b16f16e022ea291eaf193c6ce3f28`, and that
real package passed the local Chromium acceptance test (demand loading,
attribution, terrain, tilt, ≤10 ms p95). MT2 v5
preserves 64-bit OSM source IDs, including node IDs beyond the 32-bit range,
and adds a facade material byte to the building payload.
MT2 v3 carries interior rings and MT2 v4 preserves full 64-bit source IDs; the renderer excludes holes from fills. Complex
relation topology and geometry repair remain unfinished.

Road and polygon geometries crossing ±180° are split at the world seam before
tile clipping, preventing short features from producing world-spanning packages.
Complex relation topology remains a global-build requirement.

The CLI can also write an inclusive z12–z16 package range while processing one
level at a time. Ingest omits classes below their established style entry zoom:
a z12 local acceptance build emits 161,638 feature parts in 70 terrain-bearing
tiles (17 MB), rather than buildings and address-level detail that cannot be
rendered at z12. Deterministic Douglas–Peucker simplification reduces road
geometry while retaining each segment's farthest deviation. A conservative
per-tile area pass removes nearly collinear interior vertices while preserving
every tile-edge vertex, so adjacent tiles remain exact. Two independent local
v4 z12 rebuilds emitted 159,726 parts in 70 terrain-bearing tiles and produced
identical aggregate and per-tile hashes (`db15c97fd6983f2577e8ed4b997e9f11952e118bdcf00ee66bbcd29a5d41849a`);
the same determinism now holds at v5 across the full z12–z16 range (see above).
The area pass does not yet perform global topology-aware generalisation, so
this is not yet production-quality cartographic generalisation.

Named OSM places now carry deterministic settlement ranks: cities, towns,
villages/suburbs, then local places. Low zooms admit only the appropriate rank,
so text collision prioritizes map-scale names over neighbourhood detail.

Every generated manifest carries per-tile SHA-256 values plus an aggregate
package SHA-256, so independent package builds can compare exact bytes without
committing derived data. The ingest CLI verifies the manifest digest table and
every tile before a package is accepted.

This is **not** a browser-ready London map or a production release. The lab
now exercises a manifest-driven, demand-loading host contract with synthetic
tiles, but does not ship a real-data package. There is still no global
topology-aware generalisation, global source set, real-data attribution
acceptance, release asset, or production performance and resilience evidence.
The package loader visibly exposes manifest attribution and offers a manual
retry after a transient manifest failure plus one automatic retry for a
network, HTTP 429, or 5xx tile failure. A local, untracked z12/z16 London
package has been loaded through that host with all requested hashes verified
and attribution visible. An opt-in Chromium acceptance test exercises that
local package's demand loading, attribution, terrain, tilt, and ≤10 ms p95
frame budget; it is not a CI or release-asset gate. The host bounds an accepted manifest to 50,000 tiles
and each fetched tile to 4 MiB; it recreates a package map after a WebGL
context-loss event. Multi-request recovery and release asset validation remain
open.
Strict workspace Clippy now passes without
suppressions; the remaining blockers are real-data quality and release
operations rather than the Rust lint gate.

## Milestones

1. **Reliable SDK baseline.** Make strict workspace Clippy and the existing
   Rust/Wasm/browser checks green; publish platform support, package
   compatibility, API stability, security policy, and a reproducible release
   checklist. Keep the ≤10 ms p95 frame budget on representative scenes.
2. **Versioned ingest core.** Add `maps2-ingest`, a CLI with `fetch`,
   `validate`, `build-london`, and `build-world` commands. Source descriptors
   pin URL, version/date, SHA-256, licence, attribution, bounds, and adapter
   version. A build consumes only validated local inputs and produces a
   content-addressed package manifest, tiles, statistics, and notices.
3. **Greater London package.** Read an OSM PBF extract and Copernicus DEM;
   validate coordinates, geometry, topology, source checksum, feature counts,
   and tile completeness. Classify land, water, parks, roads, POIs, labels, and
   building footprints; simplify by zoom and produce deterministic MT2 tiles.
4. **True 3D city rendering.** Done for the base payload and renderer: MT2 v5
   carries base height, top height, roof type, and material class; v1–v4
   remain readable. The renderer draws terrain-clamped walls with LOD tiers
   and shapes gabled/hipped roofs (a bounding-box ridge approximation, not a
   straight-skeleton solver). OSM `height` is used when valid, then
   `building:levels` × 3 m, then a documented class default; `roof:shape`,
   `building:material`/`facade:material`/`wall`, and `min_height`/
   `building:min_level` map with documented fallbacks. Remaining: a richer
   roof solver for non-rectangular footprints, and provenance reporting the
   fallback mix per package.
5. **Production browser SDK.** Expose a stable package loader, visible data
   attribution, abortable fetches, retry/error states, resource bounds, context
   loss recovery, accessibility-friendly controls, and labels for roads and
   POIs. The London lab becomes a real-data visual acceptance suite rather than
   a demo claim.
6. **World build and release operations.** Assemble global OSM and terrain
   inputs by deterministic region shards, stitch and validate border tiles,
   generate a low-zoom globe package plus higher zoom regional packages, and
   publish checksums and source notices. CI tests fixtures only; a scheduled
   release workflow builds data externally and attaches packages as release
   assets, never commits them.

## Delivery sequence and exit criteria

| Phase | Deliverable | Exit criterion |
| --- | --- | --- |
| London ingest hardening | Geometry repair, multipolygon/relation support, zoom generalisation, antimeridian tests, and bounded resource use | Two clean rebuilds have identical manifest and tile hashes; border and topology tests pass. |
| London visual beta | A package loader, source notices, visible attribution, real roads/buildings/terrain/labels, and intentional browser goldens | Supported-browser acceptance covers loading, error/retry, context loss, labels, tilt, and a p95 frame time of ≤10 ms. |
| London release operations | Signed package manifest, size/memory budgets, release checklist, rollback procedure, and ownership | An independent clean environment rebuilds and validates the candidate package without proprietary inputs. |
| Global data build | Region-sharded OSM/DEM/GEBCO acquisition, low-zoom globe, regional high-zoom packages, and border stitching | Complete shard inventory, deterministic rerun, aggregate attribution, and global z0–z5 coverage validation. |
| Production SDK | Stable compatibility policy, versioned package loading/migration, accessibility, observability, security response and support matrix | Performance, recovery, upgrade, security, and rollback exercises are signed off by the owner. |

## Quality gates

- Every parser, normalizer, simplifier, tile encoder, and building-height
  fallback has a focused failing test before its implementation. Fixtures cover
  malformed inputs, source checksum mismatch, antimeridian/border continuity,
  missing DEM, invalid OSM height tags, and deterministic byte-for-byte output.
- CI runs Rust tests, strict Clippy, coverage ratchet, lab build/typecheck, and
  Chromium visual/interaction tests from a clean clone. Separate signed data
  build jobs verify source hashes and package manifests.
- London acceptance asserts real roads, terrain, buildings, road labels, POI
  labels, attribution, intentional visual goldens, p95 ≤10 ms on the supported
  desktop WebGL2 baseline, bounded memory, and no WebGL context errors.
- Global acceptance asserts world z0–z5 terrain coverage, complete shard and
  border manifests, deterministic reruns, attribution aggregation, and no
  checked-in derived data.

## Release sequence

Release an explicitly experimental real-data beta only after the London gates
are green. Declare a production SDK release only after global package
reproducibility, compatibility policy, security response process, performance
matrix, rollbackable package versioning, and on-call ownership are documented
and exercised.
