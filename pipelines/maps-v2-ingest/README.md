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

This is deliberately not a package build yet. The next stages resolve way
geometry, normalize/simplify it per zoom, sample DEM terrain, and write MT2
tiles plus a content-addressed package manifest.
