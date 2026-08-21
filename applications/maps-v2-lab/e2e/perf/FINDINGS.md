# Performance findings

What `npm run test:perf` measured, why the numbers were what they were, and
what changed. Dated and specific: this is a report on runs, not a standing
claim about the SDK.

The harness itself, its phases and how to read them are documented in
[PERF.md](../../PERF.md). This file is the reading of one set of results.

## The budget

Two numbers, both in [`harness.ts`](harness.ts):

| constant | value | what it asserts |
|---|---|---|
| `BLOCK_BUDGET_MS` | 10 ms | no single stretch of main-thread work is longer |
| `FRAME_BUDGET_MS` | 10 ms | p95 `render()` on a still camera |

Only phases that genuinely hold the thread count against the first one —
`render`, `debug`, `demand`, `decode`, `clamp`, `labels`
(`BLOCKING_PHASES`, [`../../src/perfTrace.ts`](../../src/perfTrace.ts)).
`fetch` and `pass` are wall time waiting on the network, `digest` goes to
`crypto.subtle`, and `blocked` is already a sum; charging any of them to the
map would mean calling a slow connection a slow renderer.

## Before: the run of 2026-08-21

83 rows, of which **9 missed the budget**.

| study / scenario | worst block | phase |
|---|---|---|
| `board/open-study` | 836.9 ms | `decode` |
| `board/scroll` | 836.7 ms | `decode` |
| `showcase/animation` | 833.4 ms | `decode` |
| `zoom-bands/primary` | 727.9 ms | `decode` |
| `input-flat/secondary` | 725.5 ms | `decode` |
| `globe-real/primary` | 340.5 ms | `decode` |
| `globe-transition/primary` | 214.5 ms | `decode` |
| `map-real/primary` | 32.6 ms | `decode` |
| `labels-collision/primary` | 98.0 ms | `render` |

Two things stand out, and they point in the same direction.

**The map draws fine.** p95 `render()` on a still camera is under a
millisecond in most studies and never failed its budget. Nothing here is a
shading, geometry-volume or draw-call problem.

**Eight of the nine failures are one call.** `decode` is
`map.loadTile(tile)` — a single synchronous crossing into wasm that turns
one tile's bytes into every mesh and label anchor it will ever need
(`sdk.ts`). The host already spreads decoding over slices of
`DECODE_SLICE_MS = 6`, but a slice boundary sits *between* tiles: it cannot
break up one call that takes 830 ms on its own.

Resident memory was never the problem: 80 tiles / 31.6 MB at the worst
(`globe-real/primary`), and the `repeat` scenario's no-growth invariant —
repeating the same motion on a warm map must not keep taking more —
passed everywhere.

## Why decode cost that much

Four things in `load_tile`, in rough order of how much they hurt.

**1. Buildings were triangulated with a naive ear clipper.**
`maps2-render/src/triangulate.rs` carried two triangulators: a hand-written
ear-clipping loop whose ear test rescans every remaining vertex per
candidate, and a hole-aware one already routed through `earcutr`. Fills used
the fast one. Buildings used the slow one — for every footprint, every roof
cap, at every LOD tier, on every tile. A rectangle is cheap; a real London
footprint with a courtyard is not, and there are hundreds per z16 tile.

**2. Each road section was walked three times.**
`build_line_bucket` iterated a class's features once per `RoadLevel`,
filtering inside the loop for tunnel, then ground, then bridge. With eight
road classes that is 24 full section walks per tile, and because a feature's
geometry is a varint stream, *walking* it is most of the cost — two of every
three walks decoded a road in full only to answer "not this storey".

**3. Geometry is decoded twice per feature.**
`FeaturesIter::decode_one` calls `skip_geometry` to find where a feature's
vertex data ends — which fully varint-decodes every delta and discards it —
and then `vertices()` decodes the same bytes again to produce coordinates.
This is systemic, affecting every bucket builder. It is the price of the
crate's zero-copy reading contract, and it has not been touched.

**4. `load_tile` builds everything, for every tile, up front.**
Fills, buildings, roads and label anchors are four independent full walks of
the tile, plus two retained byte copies. Text shaping is already deferred
out of this path — a comment records the 300 ms regression that taught that
lesson — but the mesh work is not.

The single non-decode failure has its own cause. **Label collision had two
linear scans that grew with the frame**: `Frame::consider` checked
`seen.contains(&id)` against a growing `Vec`, and `repeats_nearby` scanned
every name already placed, comparing `String`s. A crowded micro frame offers
a thousand candidates, so both scans grew with the square of the frame.

## What changed

Four changes landed, and one measurement tool that turned out to matter more
than any of them.

**The worst block now carries an address.** `traced()` takes an optional
detail, `decode` spans carry the tile path, and the reporter prints it. A
failure line went from

```
NEW   board  scroll  836.7ms decode  decode ×153 (11359.2ms)
```

to

```
KNOWN board  scroll  802.2ms decode 1/0/0.mt2  decode ×144 (11242.8ms)
```

That one word is what turned this investigation from guesswork into a
measurement.

**Buildings now triangulate through `earcutr`** — the naive ear clipper is
gone, and both entry points in `triangulate.rs` share one call.

**Each road section is decoded once** and sorted into tunnel/ground/bridge
storeys, rather than walked once per storey. Eight walks per tile instead of
twenty-four.

**Label collision's duplicate and repeat checks are hash lookups.** `seen` is a
`HashSet`, `seen_text` a map from text to the places that name already stands.
Neither grows with the square of the frame any more.

**The harness resets its trace before `measureFrames`.** Before this, every
`frame` row in `last-run.json` reported a phase breakdown for the whole session,
which is why forty passing rows advertised an 800 ms decode that did not happen
in their measurement window.

## After: the run of 2026-08-21, same machine

| study / scenario | before | after | phase | tile |
|---|---|---|---|---|
| `board/open-study` | 836.9 ms | 808.2 ms | `decode` | `1/0/0.mt2` |
| `board/scroll` | 836.7 ms | 802.2 ms | `decode` | `1/0/0.mt2` |
| `showcase/animation` | 833.4 ms | 807.7 ms | `decode` | `1/0/0.mt2` |
| `zoom-bands/primary` | 727.9 ms | 711.1 ms | `decode` | `1/1/0.mt2` |
| `input-flat/secondary` | 725.5 ms | 721.8 ms | `decode` | `1/1/0.mt2` |
| `globe-real/primary` | 340.5 ms | 339.9 ms | `decode` | `4/8/4.mt2` |
| `globe-transition/primary` | 214.5 ms | 212.5 ms | `decode` | `3/2/2.mt2` |
| `map-real/primary` | 32.6 ms | 31.0 ms | `decode` | `9/255/170.mt2` |
| **`labels-collision/primary`** | **98.0 ms** | **30.3 ms** | `render` | — |

Nine rows failed before, nine fail now. No previously-passing row regressed;
the worst passing measurement is `input-flat/repeat` at 9.5 ms.

**Read this honestly.** One change worked: de-quadratifying label collision cut
that failure by 3.2×, and it is still over budget. The other two Rust changes
are better code that barely moved this workload — a few percent — because the
work they removed is not the work that was costing the time. They will matter on
a building-dense city tile; they did not matter here, and saying otherwise would
be inventing a result.

## What the 800 ms actually is

With the address in hand, the question became answerable directly. Timing each
bucket builder natively, in release, on the worst tile:

```
1/0/0.mt2  1971 KB  parse 25µs  walk 6.0ms (856,343 verts, 33,857 rings)
           fills 786ms (2,489,583 indices)
           buildings 0.2µs (0)  roads 0.4ms  labels 2µs
```

Everything except fills is noise. And within fills, decoding is not the cost
either: walking every one of those 856,343 vertices out of the varint stream
takes **6 ms**. The other **780 ms is triangulation**.

Per ring, the distribution is a long tail rather than one monster: 4,951 polygon
features, the worst a 4,191-vertex ring costing **51 ms by itself**, the top ten
rings accounting for 32% of the total. A 4,191-vertex ring taking 51 ms is
earcut in its quadratic worst case, which detailed coastline reliably provokes.

This has a plain reading. **`1/0/0.mt2` is a z1 tile — a quarter of the
planet — and it carries 856,000 coastline vertices.** Drawn at a few hundred
pixels across, that is roughly two orders of magnitude more detail than the
tile can possibly show. The renderer is being asked to triangulate geometry
nobody will ever see a pixel of.

### What does not fix it

- **Decoding the geometry once instead of twice** (the double varint decode in
  `maps2-tile`) would save at most 6 ms of 780. Measured, and dropped: it would
  have traded the crate's zero-copy contract for under 1% of the problem.
- **Sub-pixel simplification at decode time.** A tile is drawn at about the same
  pixel size at every zoom, so a tolerance in tile units is a tolerance in
  pixels. Thinning every ring to a quarter-pixel tolerance before triangulating
  cuts 772 ms to 279 ms and keeps 47% of vertices — a real 2.8×, and still 28×
  over budget. It also gets *worse* at coarser tolerances (373 ms at 64 units),
  because thinning a coastline into near-degenerate rings gives earcut more
  trouble, not less. Not landed: it is a renderer papering over a build-time
  problem, at a cost in rendered geometry, for a result that still fails.
- **Slicing decode further.** The host already yields every 6 ms, but the slice
  boundary is between tiles. Slicing inside one tile means a half-triangulated
  world on screen, and the same 780 ms of work.

### What would

**Simplify the world layers much harder at z1–z3, at build time.** This is where
the decision belongs — the same argument the README already makes for
conflation: by tile time a feature no longer knows what it is for, and the
renderer cannot repair at sixty frames a second what the build got wrong. A z1
coastline needs hundreds of vertices per ring, not thousands. It would cut
triangulation super-linearly, and shrink a 2 MB tile at the same time.

This was not done in that round. It means rebuilding the committed lab
packages and re-committing 118 MB of tiles and their digests; `carve` cannot
help, because it copies tiles verbatim by design.

**Correction.** That round recorded the pinned sources as unavailable. They are
not in the *repository* — `pipelines/maps-v2-ingest/cache/` is gitignored — but
they are on the machine this was measured on, all 99 GB of them. The rebuild was
possible all along. See the next section.

**Or move decode off the main thread.** A worker holding its own wasm instance
would make an 800 ms tile a slow tile rather than a frozen page. That is an
architectural change to the host contract, and a larger piece of work than
anything in this document.

## Still open

- `labels-collision/primary` at 30.3 ms in `render`, down from 98 ms but over
  budget. Not yet traced to a specific cause inside the frame.
- Whether crossing a building LOD tier is expensive. `render()` calls
  `sync_building_lod`, which on a tier crossing rebuilds and re-uploads the
  building bucket of *every* held tile in one frame. It is a plausible spike and
  no failing row is attributable to it, so it has not been changed — making it
  incremental would risk the visual goldens for a cost nothing has yet measured.
- The perf suite still does not run in CI, for the reason `PERF.md` gives. The
  `PERF_GATE=regressions` machinery exists and is unused; a nightly or
  manually-triggered job is the obvious answer if the flake rate is acceptable.

## What was not measured

One machine, one run each, macOS, single Playwright worker. No CI numbers. The
committed `baseline.json` records the post-change figures, so the next run
compares against these, not against the worse ones above.

---

# Round two: snapping the world's coastline

## What the 780 ms actually was

The address said `1/0/0.mt2`, so the next question was answerable by reading the
ingest. The tolerance was not too fine. There was no tolerance at all:

```rust
// maps2-ingest/src/lib.rs — before
const WATER_TOPOLOGY_SAFE_MAX_LEVEL: u8 = 7;
let water_would_split_at_a_shared_edge =
    class == Class::Water && level <= WATER_TOPOLOGY_SAFE_MAX_LEVEL;
if class == Class::Building || water_would_split_at_a_shared_edge
    || level >= 16 || points.len() < 4 { return points; }
```

Water at z≤7 skipped simplification entirely, and the world package is built at
exactly z1–z7 — so **no simplification ever ran on water anywhere it was built.**

The bypass was not an oversight. The water dataset ships pre-split into a grid,
so one tile carries several polygons that share a cut edge, and Douglas–Peucker
decides which of a ring's points survive by looking at *that ring's own
neighbours*. Two rings that shared an edge kept different subsets of it and
pulled apart — the pale wedges over the North Sea that commit `b393e11` fixed by
turning simplification off.

## Snap, don't thin

Snapping to a lattice has the property Douglas–Peucker lacks: it asks only where
a point is, so the same position lands on the same lattice point whichever ring
is asking, and a shared edge survives shared. That is the bypass's requirement,
met rather than avoided.

It is also a simpler rule than the one it replaces. A tile is drawn at roughly
the same pixel size at every zoom, so a lattice in tile fractions is a lattice in
pixels — `WATER_SNAP_STEP = 1/1024` is half a pixel on a 512-pixel tile at z1 and
at z7 alike, where `generalisation_tolerance` needs a per-level formula. At z1
whole runs of coastline land in one pixel and collapse; at z7 there is almost
nothing to drop. One constant, and each level keeps what it can show.

## The part that was wrong

The plan predicted the win from vertex count alone. The first snapped tile was
six times smaller and **hung a release build for minutes.** Dissecting the
feature it stopped on:

```
outer 28 verts, 1 hole
(65087,44031) (65023,44031) (64959,44031) (65023,44031) (65023,43967) (65023,44031) …
hole: (65215,43967) (65151,43967) (65215,43967) (65151,43967) …
```

Snapping had folded a bay narrower than the lattice onto a line, and what was
left walked out along itself and straight back. The outer ring alone triangulates
in 5 µs. Handed the same shape as a *hole*, `earcutr` does not fail — bridging a
hole into an outer ring assumes the hole encloses something, and it does not
return.

Two things fix it, and both are in `snap_ring`:

- **`fold_out`** — a stack pass where a point equal to the one before it is a
  duplicate and a point equal to the one two back means the path doubled back,
  so the step out is unwound. It removes the creases.
- **A guard.** If the folded ring still returns to a point it has already
  visited, snapping has made it stranger rather than smaller, and the original
  ring is kept. That ring stays expensive; drawing it correctly is worth more
  than drawing it cheaply. The lattice is halved and retried up to four times
  before giving up, because a ring that folds at half a pixel often sits happily
  at an eighth of one.

## Measured, on `1/0/0.mt2`

| | before | after |
|---|---|---|
| fill bucket build | **755 ms** | **97 ms** |
| tile size | 1,971 KB | 819 KB |
| vertices | 856,343 | 361,190 |
| rings | 33,857 | 4,876 |
| features | 4,951 | 3,715 |
| water area | 69.263% of tile | 69.229% |

**7.8× on the work that dominated everything**, and the coastline is provably
still where it was: total water area moved by 0.05% relative while the tile
carries 2.4× fewer vertices.

## Measured, in the lab

Same machine, single worker, against the rebuilt packages. "First" is the
original run of 2026-08-21, "round one" the earcut/collision changes, "now" this
one.

| study / scenario | first | round one | now | total |
|---|---|---|---|---|
| `board/open-study` | 836.9 ms | 808.2 ms | **113.0 ms** | 7.4× |
| `board/scroll` | 836.7 ms | 802.2 ms | **180.2 ms** | 4.6× |
| `showcase/animation` | 833.4 ms | 807.7 ms | **110.7 ms** | 7.5× |
| `zoom-bands/primary` | 727.9 ms | 711.1 ms | **177.0 ms** | 4.1× |
| `input-flat/secondary` | 725.5 ms | 721.8 ms | **53.4 ms** | 13.6× |
| `globe-real/primary` | 340.5 ms | 339.9 ms | **178.6 ms** | 1.9× |
| `globe-transition/primary` | 214.5 ms | 212.5 ms | **48.6 ms** | 4.4× |
| `labels-collision/primary` | 98.0 ms | 30.3 ms | **29.7 ms** | 3.3× |
| `map-real/primary` | 32.6 ms | 31.0 ms | **30.6 ms** | 1.1× |

Peak resident memory fell with it: 80 tiles / 31.6 MB at the worst, now 55 tiles
/ 23.4 MB. Every visual golden is unmoved, including `globe-relief` and
`terrain-shade` — the two that would show an ocean sliver if snapping had opened
one.

**The bottleneck moved.** The worst block used to name `1/0/0.mt2` in almost
every study; now the expensive tile is `5/17/9.mt2`. At z5 a tile covers a
thirty-second of the planet, so the same coastline is thirty-two times denser in
tile fractions and much less of it lands inside one lattice cell. The rule is
doing what it should — each level keeps what it can show — and z5 is simply the
level where that is still a lot.

`map-real` barely moved, which is right: it is a city package, its water comes
from OSM rather than the split world dataset, and none of this touched it.

## What is still true

**Nine rows still miss the budget, the same nine.** The worst is 180 ms against
10 ms. Getting the rest means proper snap rounding, which is a published
algorithm and a real piece of work, not a constant to tune. The guard is
deliberately conservative — on the z1 tile 361,190 vertices survive against an
ideal of roughly 13,000 — so most of what remains is rings snapping could not
safely touch at any lattice.

Two other things landed in the same round, neither of them a fix for the above:

- **The building fill is gone.** `Class::Building` was in `FILL_ORDER`, so every
  footprint was triangulated into the fill bucket and uploaded to the GPU — and
  then skipped at draw time, because the 3D building bucket had already drawn it.
  The geometry was dead work; the *ranges* were not. `resident_classes` read them
  to answer "which classes are on screen" for the band studies' readout, so
  removing them silently dropped Building from it — caught by `cards.spec.ts`,
  not by reading the draw path. It now asks the building bucket, which is the
  question it meant. `Class::Land` is in the same position and was left alone;
  removing it would rewrite two contract tests for no measured gain.
- **Two copies per tile removed.** `load_tile` now takes the bytes rather than
  borrowing and copying them a second time, and the heights raster is a range
  into bytes already held instead of a duplicate 128 KB per terrain tile.

## Declined, on the measurements

- **Decode-once in `maps2-tile`**: 6 ms of 786, and it would replace a `Copy`,
  zero-copy `FeatureView` with a shared scratch buffer touching all four bucket
  builders.
- **Building LOD as a shader uniform**: needs a per-vertex role tag and bigger
  building vertices to delete a rebuild no measurement blames.
- **The Web Worker**: it makes an expensive tile a slow tile rather than a frozen
  page, but it does not make it cheaper. Worth revisiting now that the number it
  would be hiding is 97 ms rather than 800.
