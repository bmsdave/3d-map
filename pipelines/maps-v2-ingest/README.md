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
by Git. It currently emits only geometry that is entirely contained by a z16
tile; border clipping, lower zoom generalization, and package browser loading
are deliberately still incomplete. Each build writes `manifest.json` beside
the tiles with MT2 version, zoom, source URL/date/hash, licence, attribution,
and feature/tile counts.

## Terrain input

`sources/london-dem-n51w001.toml` pins the public Copernicus GLO-30 COG that
covers the western London degree cell. Validate decoding without storing it in
the repository:

```sh
cd libraries/maps-v2
cargo run -p maps2-ingest -- dem-info /path/to/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51
```

The decoder accepts the signed and floating elevation rasters used by
Copernicus COGs. Build a terrain-bearing package with both independently
verified descriptors:

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build-terrain ../../pipelines/maps-v2-ingest/sources/london.toml /path/to/greater-london-260814.osm.pbf ../../pipelines/maps-v2-ingest/sources/london-dem-n51w001.toml /path/to/Copernicus_DSM_COG_10_N51_00_W001_00_DEM.tif -1 51 16 ../../pipelines/maps-v2-ingest/packages/london-z16-terrain
```

The resulting manifest records both sources and counts tiles carrying an MT2
height raster. This descriptor covers only the western `N51/W001` degree cell,
so terrain coverage is intentionally partial until the adjacent source cells
are pinned and built. Copernicus GLO-30 is a surface model, so it is not yet
appropriate to describe the output as ground-true terrain.
