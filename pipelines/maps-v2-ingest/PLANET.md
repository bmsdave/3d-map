# Building the planet

A runbook for `plans/planet.toml`: the whole world, vector to z14, terrain
to z12. It is machine-days of work against ninety-five gigabytes of
source, so most of this document is about the decisions that make it fit
rather than the commands, which are three.

Numbers below marked **measured** come from this repository — the London
carve, its rasters, its packages. The rest are estimates, and say so.

## What comes out

| | |
|---|---|
| Vector | z1–z7 generalised world, z8–z14 OpenStreetMap |
| Terrain | z0–z8 from GEBCO, packed (`terrain_metres`; deeper tiles read an ancestor) |
| Size | **~150–250 GB** (estimate) — vector nearly all of it, terrain about 1 GB |
| Tiles | order 10⁷ |
| Manifest | envelope, not an enumeration — past 50,000 tiles the list is dropped for per-level bounds |

The size estimate is the weakest number here. It comes from published
planet pyramids at this depth plus the measured packing ratio, not from a
build. Expect to be wrong by a factor approaching two in either direction,
and watch the disk.

## What it needs

Sizes below are measured off this machine's cache unless marked estimate.

**Source: ~95 GB, and already compressed.** There is no win waiting here:

| | |
|---|---|
| `planet-latest.osm.pbf` | **88 GB** — protobuf with zlib-compressed blobs; this *is* the compressed form |
| GEBCO quadrants | **7 GB** as GeoTIFF, from a 4.1 GB archive |
| Natural Earth, water polygons | **~150 MB** |
| Copernicus DEM | 30 MB per degree cell; global GLO-30 is **~1.5 TB**, which is why the plan does not ask for it |

GEBCO's sub-ice topo grid covers land as well as sea floor, at 15 arc-sec —
about 450 m. That is enough terrain for the levels the cap keeps (z0–z12
sample the ground no finer than 38 m, and below z9 nothing finer than
GEBCO is visible anyway). Copernicus is what you add for a *region* whose
relief you want sharper; fetch those cells for the ground you care about
rather than for the planet.

**Disk: 600 GB free is enough, 1 TB is comfortable.**

- Source ~95 GB, if it is not already sitting there.
- Output **~150–250 GB (estimate)**, and served roughly half that with
  `Content-Encoding: br`. The weakest number in this document: it comes
  from published planet pyramids at this depth, not from a build. Terrain
  is about 1 GB of it once the cap follows GEBCO's actual resolution.
- Scratch **~0.3× of the level being built (measured)**. The spool writes
  each level's features down and deletes them when that level is done, and
  its records deflate about five to one, so this is tens of gigabytes
  rather than hundreds. See `tests/spool_size.rs`.

**Memory: 32 GB is enough, 64 GB is comfortable.** A build holds one
shard's features and a batch of tiles, not the package. What still grows
with the package is the digest list, about eighty bytes a tile — a
gigabyte at 10⁷ tiles.

**Cores: as many as you have.** Tiles are built in batches across threads;
triangulation and label shaping are the cost.

**Time: 1–3 machine-days (estimate).** Not measured. The London carve is
minutes and the planet is four orders of magnitude more ground.

**Money: $20–100** of spot instance, or a week of a spare desktop.

### Where the bytes actually go

Measured by parsing every feature of the committed carve — 778,329
features, 44.3 MB of vector sections:

| | share |
|---|---|
| geometry (coordinates, already varint deltas) | 49.6% |
| name text | 15.2% |
| feature ids (8 bytes each, every feature, every level) | 14.1% |
| building fields (6 bytes each — on 778k features, 8k are buildings) | 10.5% |
| flags, rank, name length, hole count | 10.5% |

A third of it is per-feature header on features that mostly have nothing
to put there. It looks like an obvious target. It is not, and this is the
measurement that says so:

| | raw | deflated |
|---|---|---|
| as it is today | 44.3 MB | **22.5 MB** |
| make the building and hole fields optional | 38.9 MB | 21.4 MB |
| …and put one name table per section | 35.0 MB | **21.3 MB** |

Rewriting the format saves 21% raw and **5% after compression**, because
those bytes are mostly zeros and repeated names, which is exactly what a
compressor eats for free. So:

- **Compress the vector sections.** 1.97× measured — half the vector half
  of the package, for no format change at all. Either store them deflated
  behind a section flag, or store the tiles pre-compressed and serve
  `Content-Encoding: br`, which the hosting step below does.
- **Do not redesign the feature header for size.** There is a case for it
  — fewer bytes to parse is less decode time and less memory per tile —
  but it is a speed argument, not a size one, and it should be made with
  frame timings rather than with this table.

### The two levers that actually move a planet

1. **Terrain depth, against the DEM's real resolution.** Terrain z0–12 is
   about 214 GB. From GEBCO — 15 arc-seconds, about 460 m — everything
   below z8 is interpolation the renderer does for free, and z0–8 is about
   **1 GB**. `terrain_metres` in the plan sets this; the planet plan
   declares 460 and gets z8. Deepen it only where a finer DEM covers the
   ground.
2. **Vector depth.** Each level has four times the tiles of the one above,
   so the deepest level is roughly three quarters of the whole pyramid.
   z14 → z13 is about four times smaller. This plan stops at z14 because
   below it a street is drawn larger rather than described better.

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

Serve the tiles **pre-compressed**. Only the heights are compressed inside
the format, and the vector sections deflate 2.0× on the committed carve
(measured), so `Content-Encoding: br` roughly halves the vector half of
the package on the wire for nothing. Set a long `Cache-Control`
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
