# Skill: ingest-build

Work on the ingest pipeline (OSM → MT2 package).

Steps:
1. Read `crates/maps2-ingest/src/lib.rs:41` (Source pinned SHA256) + task-specific file: `conflate.rs:1` for dedup, `gebco.rs:1` for bounded read, `world_terrain.rs:1` for global read.
2. For conflation: 25km place match `conflate.rs:29`, coverage vs identity rules `conflate.rs:82`.
3. Commands (from `libraries/maps-v2`):
   - `cargo run -p maps2-ingest -- scan <pbf>`
   - `cargo run -p maps2-ingest -- verify <source.toml> <file>`
   - `cargo run -p maps2-ingest -- verify-package <package-dir>` — always run after build.
   - `cargo run -p maps2-ingest -- build-terrain-range ...` for local London.
4. Do not read full `lib.rs:2248` — grep for subcommand. Do not commit `pipelines/maps-v2-ingest/packages/` or `cache/`.
