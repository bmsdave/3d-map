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

## Current position (2026-08-16)

The project has the beginnings of the London path: pinned Greater London OSM
and two Copernicus DEM descriptors, HTTPS-only atomic source acquisition with
checksum validation, deterministic MT2 v4 encoding with v1/v2/v3 readers, terrain rasters, OSM
building-height fallbacks, and deterministic
tile-border clipping, outer/inner-ring multipolygon relations, named places,
bridge/tunnel road structure flags, and
amenity points. A local z16 London rebuild currently emits 2,128,113 feature
parts in 11,944 terrain-bearing tiles from those validated inputs. MT2 v4
preserves 64-bit OSM source IDs, including node IDs beyond the 32-bit range.
MT2 v3 carries interior rings and MT2 v4 preserves full 64-bit source IDs; the renderer excludes holes from fills. Complex
relation topology and geometry repair remain unfinished.

The CLI can also write an inclusive z12–z16 package range while processing one
level at a time. Ingest omits classes below their established style entry zoom:
a z12 local acceptance build emits 161,638 feature parts in 70 terrain-bearing
tiles (17 MB), rather than buildings and address-level detail that cannot be
rendered at z12. Conservative line simplification then reduces nearly
collinear road vertices: a current local v4 z12 rebuild emits 159,638 parts in
70 terrain-bearing tiles and verifies its aggregate and per-tile hashes.
Area geometry and larger road turns remain unsimplified, so this is not yet
production-quality cartographic generalisation.

Named OSM places now carry deterministic settlement ranks: cities, towns,
villages/suburbs, then local places. Low zooms admit only the appropriate rank,
so text collision prioritizes map-scale names over neighbourhood detail.

Every generated manifest carries per-tile SHA-256 values plus an aggregate
package SHA-256, so independent package builds can compare exact bytes without
committing derived data. The ingest CLI verifies the manifest digest table and
every tile before a package is accepted.

This is **not** a browser-ready London map or a production release. The lab
now exercises a manifest-driven, demand-loading host contract with synthetic
tiles, but does not ship a real-data package. There is still no
generalisation pipeline, global source set, real-data attribution acceptance,
release asset, or production performance and resilience evidence. The package
loader visibly exposes manifest attribution and offers a manual retry after a
transient manifest failure; it has not yet been exercised with a real London
package. It bounds an accepted manifest to 50,000 tiles and each fetched tile
to 4 MiB; connection recovery and real-package browser acceptance remain open.
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
4. **True 3D city rendering.** Bump the MT2 format for an explicit building
   payload (base height, top height, roof type, and material class), retain v1
   decoding for existing fixtures, and render terrain-clamped building walls
   and roofs with LODs. Use OSM `height` when valid, then `building:levels` ×
   3 m, then a documented class default; provenance records the fallback mix.
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
