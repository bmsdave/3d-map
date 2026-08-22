# The data: where it comes from, how it is stored, what it weighs

Everything the map draws is real data from four public sources, cut into
tiles and written in one binary format. This document is the anatomy of
that: the sources and their terms, the shape of a tile down to individual
bytes, the rules that decide what appears at which zoom, and what all of
it costs.

Numbers here are marked **measured** or **estimated**. Measured means it
came from the package committed to this repository — 559 tiles around
Trafalgar Square, 117 MB — or from a tool in this repository run against
real input. Estimated means a model, and models about planets are wrong by
factors, not percentages.

---

## 1. Where the data comes from

Four sources, none of them ours, each pinned by a descriptor in
`pipelines/maps-v2-ingest/sources/` that carries its URL, SHA-256, date,
licence and required attribution.

| Source | What it gives | On disk | Licence |
|---|---|---|---|
| **OpenStreetMap** (`planet.osm.pbf`) | Roads, buildings, water bodies, parks, POIs, place names | **88 GB** | ODbL 1.0 |
| **GEBCO 2026** sub-ice topo | Sea-floor bathymetry *and* land elevation, 15 arc-seconds (~460 m) | **7 GB** as eight quadrant GeoTIFFs | free, acknowledgement asked |
| **Copernicus DEM** GLO-30 | Land elevation at 30 m, one file per degree cell | **30 MB per cell** (~1.5 TB globally) | free, attribution required |
| **Natural Earth** | Country boundaries, generalised roads, populated places for low zooms | ~115 MB | public domain |
| **OSM water polygons** | Pre-split, pre-simplified coastlines for low zooms | ~29 MB | ODbL 1.0 |

The descriptor is the contract. `maps2-ingest fetch` downloads to a
`.part` file, checks the hash, and only then renames, so an interrupted
download can never be mistaken for a good one. `verify` does the check
alone when a file is already cached. Every value in the descriptor is
copied into the package manifest, which is why a package can tell you what
it is made of and what you owe the people who made it — see
[DATA-LICENCE.md](DATA-LICENCE.md).

**Two sources overlap on purpose.** Natural Earth and the water polygons
speak for the whole planet but coarsely; OSM speaks for everywhere in
detail. Which one owns a given piece of ground at a given zoom is decided
once, at build time, by `maps2_ingest::conflate` — layers declare a
precedence and a bounding box, and the loser's features are dropped rather
than drawn twice. A city that appears in both Natural Earth's populated
places and OSM's place nodes is matched within 25 km and kept once.

**Measured, from `maps2-ingest scan` on the Greater London extract:**

```
objects     13,163,155
buildings    1,169,021
roads          555,896
POIs           163,891
water            3,424
parks            3,272
```

Approximate planet figures, from general knowledge rather than measured
here: ~10 billion nodes, ~1 billion ways, **~600–700 million buildings**,
**~250–300 million road ways**, ~30–40 million waterways, ~100–150 million
POIs. London is among the most completely mapped places on Earth, so
scaling its density to the globe overestimates by orders of magnitude.

---

## 2. From source to tile

The pipeline is five stages, and the code for each is one place:

1. **Read** — `osmpbfreader` streams the PBF; shapefile and GeoTIFF
   adapters handle the others (`natural_earth.rs`, `world_water.rs`,
   `gebco.rs`, `world_terrain.rs`).
2. **Prepare** — `prepare_features` projects each object to Web Mercator,
   clips it to the tiles it crosses, classifies it from its OSM tags
   (`classify_osm_tags`), and normalises building height, roof shape and
   facade material. One source object becomes one `PreparedFeature` per
   tile it touches.
3. **Conflate** — `conflate(level, layers)` resolves overlapping sources.
4. **Build** — `build_tile` sorts a tile's features by class and id, then
   encodes them. Sorting is what makes a build byte-for-byte reproducible.
5. **Write** — features go through a spool (`maps2_ingest::spool`): shard
   files on disk, drained one at a time, so a build holds one shard's
   features rather than the whole package.

---

## 3. What a tile file looks like

One tile is one file, named for its position:

```
packages/trafalgar/16/32744/21791.mt2
                   ^  ^     ^
                zoom  column row
```

The world is one tile at z0, four at z1, 4.3 billion at z16. A browser
fetches only the tiles on screen at the zoom being shown.

**Header — 20 bytes, little-endian throughout:**

| offset | size | field |
|---|---|---|
| 0 | 4 | magic `MT2\0` |
| 4 | 2 | format version (now 6) |
| 6 | 1 | z |
| 7 | 1 | reserved |
| 8 | 4 | x |
| 12 | 4 | y |
| 16 | 2 | section count |
| 18 | 2 | reserved |

**Section table** — ten bytes per section: class code, offset, length. The
renderer can jump straight to the roads without reading the buildings.
Here is a real tile from the committed package, 179,045 bytes:

| class | section | bytes |
|---|---|---|
| 3 | Building | 21,700 |
| 11 | Poi | 11,842 |
| 10 | RoadPath | 10,038 |
| 8 | RoadResidential | 1,681 |
| 7 | RoadSecondary | 784 |
| 9 | RoadService | 687 |
| 6 | RoadPrimary | 675 |
| 12 | Label | 162 |
| 2 | Park | 158 |
| 1 | Water | 116 |
| 0xFF00 | **heights raster** | **131,072** |

Class codes below `0xFF00` are vector sections; at or above it they are
rasters, whose payload is opaque and owned by the class.

---

## 4. A feature, byte by byte

Inside the Building section above are 319 buildings, one after another.
The first, decoded from the real file:

```
id           15698821                            8 bytes   OpenStreetMap id
flags        0                                   1 byte
rank         3                                   1 byte
base_dm      0                                   2 bytes   height above datum
top_dm       90                                  2 bytes   90 dm = 9 m tall
roof         0 (flat)                            1 byte
material     0 (unknown)                         1 byte
name_len     53                                  2 bytes
name         "Leicester Square Tube Station…"   53 bytes
vertex_count 5                                   2 bytes
first vertex (46425, 13502)                      4 bytes
delta        +1511, -853                         varint
delta        +110, +194                          varint
delta        -1513, +853                         varint
delta        …                                   varint
hole_count   0                                   2 bytes
```

**Coordinates are tile-local.** Each tile has its own grid of
`TILE_EXTENT = 65536` steps per axis, so a point is two `u16` — no
floating point, no global coordinates, no projection maths at draw time.
Only the first vertex of a ring is absolute. Every vertex after it is
stored as the difference from the one before, zigzag-encoded so that small
negative moves stay small, then written as a varint so that a one-byte
delta really costs one byte. A building corner is "+110, +194 from the
last corner".

Polygons carry their holes after the outer ring, each encoded the same
way.

---

## 5. Heights

The other kind of section is a raster, and there is one: heights, class
`0xFF00`.

It is 256×256 `u16` values, row-major, exactly **131,072 bytes**, always.
The value is metres offset by +11,000 so that the Mariana Trench stays
positive: `metres = value − 11000`. The grid includes both tile edges —
sample *i* sits at *i*/255 across the tile — so neighbouring tiles share
their edge samples and a surface built from them has no seam.

Since format v6 there is a second class, `0xFF01`: the same raster,
packed. Three steps, each reversible: predict every sample from its
neighbours with PNG's Paeth predictor over `u16`; split the residuals into
a high-byte plane and a low-byte plane; deflate. **Measured: 3.7×**, which
takes the committed carve from 117 MB to 64 MB. A reader that does not
know the class skips it and draws the tile flat, so older readers degrade
rather than fail.

**Terrain stops before the tiles do.** A raster only says something new
while the DEM underneath it has something left to give:
`terrain_cap_for_metres` computes the deepest level worth writing — z12
for Copernicus GLO-30's 30 m, **z8 for GEBCO's 460 m**. Below the cap a
tile carries no raster and the renderer reads the nearest ancestor that
does, through a window (`maps2_render::HeightWindow`), interpolating
between its samples. For a planet that is the difference between 215 GB of
terrain and 0.6 GB.

---

## 6. What appears at which zoom

Classes enter at bands, and a band has an entry zoom. This is the table
that decides the shape of everything:

| band | from zoom | classes entering |
|---|---|---|
| World | 1 | Land, Water, Label, Boundary |
| Region | 5 | RoadMotorway, RoadTrunk |
| City | 8 | Park, RoadPrimary |
| District | 10 | RoadSecondary |
| Street | 12 | RoadResidential |
| Address | 14 | RoadService, RoadPath, Poi |
| Micro | 16 | **Building** |

Once a class has entered, it is written at that level and every deeper
one. Note where buildings are: **a package built to z14 contains no
buildings at all.** That is a product decision living inside a style
table, and it is worth making deliberately rather than discovering.

---

## 7. What it weighs

**Measured**, by parsing all 778,329 features of the committed carve —
44.3 MB of vector sections, 7,685,232 vertices, 483,391 of them named:

| | share |
|---|---|
| geometry (coordinates, varint deltas) | **49.6%** |
| name text | 15.2% |
| feature ids (8 bytes each) | 14.1% |
| building fields (6 bytes on every feature; 8,087 are buildings) | 10.5% |
| flags, rank, name length, hole count | 10.5% |

Per class, measured:

| class | section bytes | features | each | names |
|---|---|---|---|---|
| Water | 11.51 MB | 58,435 | 197 B | 0.4% |
| RoadResidential | 7.74 MB | 165,016 | 47 B | 27.5% |
| RoadPrimary | 5.79 MB | 135,562 | 43 B | 30.7% |
| RoadPath | 4.59 MB | 129,646 | 35 B | 3.9% |
| Poi | 2.16 MB | 61,596 | 35 B | 15.0% |
| Building | 0.53 MB | 8,087 | 66 B | 8.0% |

Names deserve their own note: 6.73 MB of text, but only **53,181 distinct
strings**, which would be 0.76 MB stored once. **88.8% of name bytes are
repeats** — "High Street" appears 3,166 times, "London Road" 2,761. A road
is split per tile and repeated per level, and each piece carries the whole
name again.

### Compression, measured

| | raw | deflated |
|---|---|---|
| vector sections as they are | 44.3 MB | **22.5 MB (1.97×)** |
| with the empty fields made optional | 38.9 MB | 21.4 MB |
| …and a name table per section | 35.0 MB | 21.3 MB |

This is the most useful measurement in the document. Rewriting the format
to remove the duplicate names and unused building bytes saves 21% raw and
**5% after compression**, because zeros and repeated strings are exactly
what a compressor eats for free. **Compress the sections; do not redesign
the feature header to save space.** There is still a case for a leaner
header — fewer bytes to parse is less decode time and less memory — but
that is a speed argument and needs frame timings, not this table.

---

## 8. What a planet would weigh

**Estimated.** Structure and bytes-per-feature are measured; object counts
are approximate, so read the totals as ±2×. Published planet tilesets land
1.5–2× above this model, which suggests it is on the low side.

Vector, by zoom — cost per level is roughly *flat*, because each object is
written once per level wherever it falls:

| zoom | vector per level |
|---|---|
| z1–4 | 4.0 GB |
| z5–7 | 4.3 GB |
| z8–9 | 5.4 GB |
| z10–11 | 6.3 GB |
| z12–13 | 12.1 GB |
| z14 | 22.6 GB |
| z16 | 75.8 GB |

By class across z1–14: **Water 55.6 GB** — over half, because coastlines
are big polygons that enter at z1 and are re-stored fourteen times —
RoadResidential 17.5 GB, Park 4.8, RoadSecondary 4.5, POI 4.5, the rest
under 4 each.

Terrain behaves completely differently: **4× per level**, because every
tile gets a raster whether anything is there or not. z8: 0.6 GB. z10: 17.8
GB. z12: 215 GB.

| build | raw | served with `Content-Encoding: br` |
|---|---|---|
| world to z7 + terrain z8 | ~5 GB | ~3 GB |
| planet z14, terrain z8, no buildings | ~100 GB | ~50 GB |
| planet z16 with buildings, terrain z12 | ~415 GB | ~210 GB |

---

## 9. Where to look in the code

| | |
|---|---|
| Byte-exact format spec | `libraries/maps-v2/docs/tile-format.md` |
| Writer and reader | `maps2-tile/src/{build,view}.rs` |
| Heights and packing | `maps2-tile/src/heights.rs` |
| Classes and entry bands | `maps2-style/src/lib.rs` |
| Ancestor height reads | `maps2-render/src/height_window.rs` |
| Source descriptors | `pipelines/maps-v2-ingest/sources/` |
| Pipeline stages | `maps2-ingest/src/lib.rs`, `spool.rs`, `conflate.rs` |
| Building a planet | `pipelines/maps-v2-ingest/PLANET.md` |
| Licence obligations | `docs/DATA-LICENCE.md` |
