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

This was **not done here**, and the reason is concrete rather than a
preference: it means rebuilding the committed lab packages, whose pinned
sources — the split water-polygon shapefile and the eight GEBCO quadrants — are
not in the repository, and re-committing 118 MB of tiles and their digests.
`carve` cannot help, because it copies tiles verbatim by design. That is a data
change with an owner and a review, not a patch.

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
