# Building the planet

A runbook for `plans/planet.toml`: the whole world, vector to z14, terrain
to z12. It is machine-days of work against roughly a hundred gigabytes of
source, so most of this document is about the decisions that make it fit
rather than the commands, which are three.

Numbers below marked **measured** come from this repository — the London
carve, its rasters, its packages. The rest are estimates, and say so.

## What comes out

| | |
|---|---|
| Vector | z1–z7 generalised world, z8–z14 OpenStreetMap |
| Terrain | z0–z12, packed (`TERRAIN_MAX_Z`; deeper tiles read an ancestor) |
| Size | **~250–400 GB** (estimate; terrain ~80–100 GB of it) |
| Tiles | order 10⁷ |
| Manifest | envelope, not an enumeration — past 50,000 tiles the list is dropped for per-level bounds |

The size estimate is the weakest number here. It comes from published
planet pyramids at this depth plus the measured packing ratio, not from a
build. Expect to be wrong by a factor approaching two in either direction,
and watch the disk.

## What it needs

- **Disk: 1 TB free.** Source ~100 GB, output ~400 GB, spool scratch in
  between. The spool writes every prepared feature to disk once per level.
- **Memory: 32 GB is enough, 64 GB is comfortable.** The build holds one
  shard's features and a batch of tiles, not the package — see
  `maps2_ingest::spool`. What still scales with the package is the digest
  list: about eighty bytes a tile, so ~1 GB at 10⁷ tiles.
- **Cores: as many as you have.** Tiles are built in batches across
  threads; triangulation and label shaping are the cost.
- **Time: estimate 1–3 machine-days.** Not measured. The London carve is
  minutes; the planet is four orders of magnitude more ground.
- **Money: $20–100** of spot instance, or a week of a spare desktop.

Process the DEM **in the region the data lives in**. Copernicus GLO-30 is
hundreds of gigabytes; pulling it across a domestic connection costs more
than the compute does.

## Steps

### 1. Pin the sources

`sources/planet.toml` ships with a placeholder hash, because a planet file
is republished weekly and this repository cannot pin a file it has never
seen. Take the SHA-256 that OpenStreetMap publishes beside the download,
put it and the file's date into the descriptor, then let the tool check
it:

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- fetch ../../pipelines/maps-v2-ingest/sources/planet.toml /cache/planet-latest.osm.pbf
```

`fetch` downloads to a `.part` file, verifies the hash, and only then
renames — so an interrupted download can never be mistaken for a good one.
If the file is already on disk, `verify` does the check alone.

The world layers and GEBCO quadrants are pinned already and fetch the same
way. Copernicus DEM tiles are one file per degree cell; fetch the cells
you want relief for.

### 2. Build

```sh
cd libraries/maps-v2
cargo run --release -p maps2-ingest -- build-map \
    ../../pipelines/maps-v2-ingest/plans/planet.toml \
    /packages/planet
```

Release, not debug: the difference is not marginal.

It prints a line per level. **The build resumes** — each finished level
leaves `.level-NN.done` beside the pyramid, and a rerun restores it and
moves on, so a machine that dies at hour nine restarts at hour nine. A
level that did not finish left no record and is simply built again.

To watch it: `du -sh /packages/planet` and the level lines. To stop it:
kill it. To continue: run the same command.

### 3. Verify

```sh
cargo run --release -p maps2-ingest -- verify-package /packages/planet
```

For a package this size the manifest carries no per-tile digests, so this
walks the directory, hashes every tile, and holds the result against the
package hash. It reads the whole package — hours, disk-bound. Do it once,
here, rather than asking every viewer's browser to do it forever.

### 4. Host

Object storage with a CDN in front. Cloudflare R2 is the obvious choice
because egress is free; S3 with CloudFront works and bills per gigabyte
served.

```sh
rclone copy /packages/planet r2:maps2-planet/planet --transfers 32 --checkers 32
```

Serve the tiles **pre-compressed**. The vector sections are not compressed
in the format — only heights are — so `Content-Encoding: br` is worth
roughly another 1.6–2× on the wire for nothing. Set a long `Cache-Control`
on tiles: a tile at a path is immutable until the package is rebuilt.

Then point the demo at it. No code change:

```
https://<the demo>/demo/?package=https://tiles.example/planet/manifest.json
```

The demo reads `?package=`; without it, it opens the carve committed to
this repository.

### 5. Licence the data you publish

The package is a derived database of OpenStreetMap, so **ODbL applies to
the tiles themselves**, not only to the page that draws them. Publishing
it means:

- The attribution the manifest already carries, shown where the map is —
  the demo does this in its footer.
- The package offered under ODbL, with a note beside it saying so.
- Access to the derived data, which is the package.

GEBCO asks for acknowledgement and says its grid is not for navigation.
Copernicus DEM asks for its attribution line. Natural Earth asks for
nothing. All four are in `manifest.sources` with their licence and
attribution, copied from the descriptors — which is why the descriptors
carry them.

`docs/DATA-LICENCE.md` is the notice to publish beside a package.

## What this will not give you

Street-level 3D that looks right up close. Copernicus GLO-30 samples the
ground every 30 m: at z12 that is spent, which is why terrain stops there
and deeper tiles read an ancestor. Kerbs, embankments and correctly seated
building foundations need a LiDAR DTM at about 1 m, which exists for some
countries and is a different dataset, a different licence, and a different
build.
