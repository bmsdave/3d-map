# Maps v2 ingest

The pipeline keeps source data, caches, and generated packages out of Git.
Only versioned source descriptors and code belong in this repository.

## Verify Greater London input

Download the URL from `sources/london.toml` to an untracked directory, then
run the verifier and PBF scanner from a clean checkout:

```sh
cd libraries/maps-v2
cargo run -p maps2-ingest -- verify ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/greater-london-260814.osm.pbf
cargo run -p maps2-ingest -- scan /path/to/greater-london-260814.osm.pbf
```

`verify` streams SHA-256 and rejects a changed file. `scan` streams the PBF and
reports the candidate feature counts that the next geometry and tile stages
must account for. The descriptor carries its upstream URL, date, licence, and
required attribution; downstream packages will copy those values into their
own manifest and host-facing attribution surface.

## Build the first London vector tiles

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/greater-london-260814.osm.pbf 16 ../../pipelines/maps-v2-ingest/packages/london-z16
```

The command verifies the pinned input before two-pass way/node resolution,
then writes deterministic `z/x/y.mt2` files. The output directory is ignored
by Git. Cross-tile geometry is clipped before encoding. Each build writes
`manifest.json` beside the tiles with MT2 version, source URL/date/hash,
licence, attribution, levels, and feature/tile counts.

For a browser package that can begin at city zoom and load detail on demand,
build an inclusive level range. Levels are resolved and written one at a time,
so a package build does not retain the whole range in memory:

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build-terrain-range ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/greater-london-260814.osm.pbf 12 16 ../../pipelines/maps-v2-ingest/packages/london-z12-z16 ../../pipelines/maps-v2-ingest/sources/london-dem-n51w001.toml /path/to/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51 ../../pipelines/maps-v2-ingest/sources/london-dem-n51e000.toml /path/to/Copernicus_DSM_COG_10_N51_00_E000_00_DEM.tif 0 51
```

This range contains unsimplified source geometry at every level. It establishes
the package-loader path, not production-quality cartographic generalisation.

## Terrain input

`sources/london-dem-n51w001.toml` and `sources/london-dem-n51e000.toml` pin
the public Copernicus GLO-30 COGs covering Greater London. Validate decoding
without storing them in the repository:

```sh
cd libraries/maps-v2
cargo run -p maps2-ingest -- dem-info /path/to/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51
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
