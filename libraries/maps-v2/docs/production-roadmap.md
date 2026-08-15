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
