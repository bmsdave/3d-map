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
tile; border clipping, lower zoom generalization, DEM sampling, package
manifests, and browser loading are deliberately still incomplete.
