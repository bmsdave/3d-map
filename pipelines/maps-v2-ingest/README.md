# Maps v2 ingest

The pipeline keeps source data, caches, and generated packages out of Git.
Only versioned source descriptors and code belong in this repository.
Source acquisition requires `curl` with HTTPS support.

## Verify Greater London input

Fetch the pinned source to an untracked cache, then run the PBF scanner from a
clean checkout. `fetch` accepts HTTPS descriptors only, downloads to a sibling
`.part` file, verifies SHA-256 before renaming, and never overwrites either
file:

```sh
cd libraries/maps-v2
cargo run -p maps2-ingest -- fetch ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/cache/greater-london-260814.osm.pbf
cargo run -p maps2-ingest -- scan /path/to/cache/greater-london-260814.osm.pbf
```

`verify` remains available when a source is already cached. `scan` streams the
PBF and reports the candidate feature counts that the next geometry and tile
stages must account for. The descriptor carries its upstream URL, date,
licence, and required attribution; downstream packages copy those values into
their own manifest and host-facing attribution surface.

## Build the first London vector tiles

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/greater-london-260814.osm.pbf 16 ../../pipelines/maps-v2-ingest/packages/london-z16
```

The command verifies the pinned input before two-pass way/node resolution,
then writes deterministic `z/x/y.mt2` files. The output directory is ignored
by Git. Cross-tile geometry is clipped before encoding. Each build writes
`manifest.json` beside the tiles with MT2 version, source URL/date/hash,
licence, attribution, declared bounds, adapter version, levels, feature/tile
counts, a SHA-256 value for every tile, and an aggregate package SHA-256.
Hashes let an independent build
compare exact package contents without committing derived data.

Validate a completed package before handing it to a browser host or release
process:

```sh
cd libraries/maps-v2
cargo run -p maps2-ingest -- verify-package ../../pipelines/maps-v2-ingest/packages/london-z12-z16
```

The verifier rejects changed tile bytes, a modified digest table, and unsafe
tile paths in the manifest.

For a browser package that can begin at city zoom and load detail on demand,
build an inclusive level range. Levels are resolved and written one at a time,
so a package build does not retain the whole range in memory:

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build-terrain-range ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/greater-london-260814.osm.pbf 12 16 ../../pipelines/maps-v2-ingest/packages/london-z12-z16 ../../pipelines/maps-v2-ingest/sources/london-dem-n51w001.toml /path/to/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51 ../../pipelines/maps-v2-ingest/sources/london-dem-n51e000.toml /path/to/Copernicus_DSM_COG_10_N51_00_E000_00_DEM.tif 0 51
```

This range applies deterministic Douglas–Peucker simplification to roads and a
conservative per-tile pass to area geometry. The latter preserves tile-edge
vertices but is not global topology-aware generalisation. It establishes the
package-loader path, not production-quality cartographic generalisation.
Classes below their style entry zoom are omitted (for example, buildings do not
enter before z16), which keeps low-zoom packages aligned with the renderer's
composition policy.

## Terrain input

`sources/london-dem-n51w001.toml` and `sources/london-dem-n51e000.toml` pin
the public Copernicus GLO-30 COGs covering Greater London. Validate decoding
without storing them in the repository:

```sh
cd libraries/maps-v2
cargo run -p maps2-ingest -- fetch ../../pipelines/maps-v2-ingest/sources/london-dem-n51w001.toml /path/to/cache/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif
cargo run -p maps2-ingest -- dem-info /path/to/cache/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51
```

The decoder accepts the signed and floating elevation rasters used by
Copernicus COGs. Build a terrain-bearing package with both independently
verified cells:

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build-terrain-many ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/greater-london-260814.osm.pbf 16 ../../pipelines/maps-v2-ingest/packages/london-z16-terrain ../../pipelines/maps-v2-ingest/sources/london-dem-n51w001.toml /path/to/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51 ../../pipelines/maps-v2-ingest/sources/london-dem-n51e000.toml /path/to/Copernicus_DSM_COG_10_N51_00_E000_00_DEM.tif 0 51
```

The resulting manifest records all sources and counts tiles carrying an MT2
height raster. The two pinned cells cover every current z16 Greater London
vector tile. Copernicus GLO-30 is a surface model, so it is not yet appropriate
to describe the output as ground-true terrain.

## Bounded GEBCO ingestion

GEBCO ships global bathymetry as multi-gigabyte 90°×90° `GeoTIFF` grids. A
regional build must never decode a whole grid into memory to sample one city's
worth of ocean cells, so `maps2-ingest` reads a geographic window directly out
of the source file: it maps the window to a pixel rectangle, then decodes only
the TIFF strips or tiles that rectangle overlaps. Cost tracks the requested
region, not the file on disk — a window's cell count is capped at
`WINDOW_CELL_LIMIT` (4 Mi cells, 16 MiB of `f32`), so a caller that forgot to
bound its request fails fast instead of silently paging in gigabytes.

`sources/gebco-2025-n90-s0-w-90-e0.toml` pins the north-west 90°×90° sub-grid
that covers London. Its `sha256` is a deliberate all-zero placeholder — this
repository has not downloaded the file — so `fetch`/`verify` fail loudly
against it until an operator pulls the pinned GEBCO_2025 release from the
[official grid distribution](https://www.gebco.net/data-products-gridded-bathymetry-data/gebco2025-grid)
and records the real digest.

Inspect a window once a real grid is cached locally:

```sh
cd libraries/maps-v2
cargo run -p maps2-ingest -- gebco-window ../../pipelines/maps-v2-ingest/sources/gebco-2025-n90-s0-w-90-e0.toml /path/to/cache/gebco_2025_n90.0_s0.0_w-90.0_e0.0.tif -1 51 0 52
```

The command prints how many TIFF chunks it read against how many the whole
grid holds, plus a corner sample, so a bounded read is a directly observable
property rather than an assumption.
