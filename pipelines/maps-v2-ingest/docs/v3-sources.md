# v3 source plan — globe

Decision doc. `data-sources.md` is the survey of everything available;
this is the shortlist we commit to and the zoom band each source owns.

## Design premise

A globe is not a Mercator map with a curve. **It spends most of its life at
z0-8** — that is the default view, the one every session starts in and
returns to. In a flat map z0-6 is a throwaway overview; here it is the
product. So low-zoom quality gets first-class budget, and the cheap
public-domain sources (Natural Earth, Blue Marble) punch far above their
size.

Second premise: **transcode, don't parse.** v2 parsed OSM PBF and sharded
by hand. Planetiler ingests the planet in hours and Overture publishes
normalized GeoParquet monthly. v3 ingest should be a transcoder into MT2,
not a re-implementation of feature extraction.

## Visual target: F4Map geometry + real imagery

Reference: https://demo.f4map.com/ for the 3D vector baseline, plus
photographic ground and roofs on top of it.

Critical property — **F4Map uses no satellite imagery at all.** It is a
pure styled vector map. Every bit of its realism comes from OSM tags plus
rendering: extruded footprints, real roof geometry, per-building colour and
material, trees, and shadows.

Consequences for this plan:

1. **F4Map alone needs no imagery** — its realism is entirely tags plus
   rendering. That sets the geometry baseline. Imagery is then added on
   top as a separate track, scoped by the resolution table below.
2. **Building attribute richness becomes the top priority**, not raster
   resolution.
3. MT2 already carries `roof` and `material` per building. The format was
   designed for this target; the gap is data and renderer, not format.

### The reference implementations

| Project | What | Licence |
|---|---|---|
| https://wiki.openstreetmap.org/wiki/Simple_3D_Buildings | S3DB — the tag schema F4Map renders | spec |
| https://github.com/tordanik/OSM2World | OSM -> 3D meshes, best open roof-shape implementation | LGPL |
| https://github.com/OSMBuildings/OSMBuildings | open web renderer, same target | AGPL/commercial |

F4Map itself is **not** open source — free to view, nothing to reuse.
OSM2World is the one to read for roof geometry.

### The actual constraint: tag coverage

Measured from taginfo, 2026-08-22, against 705,492,482 OSM buildings:

| Tag | Count | Coverage |
|---|---|---|
| `building` | 705,492,482 | 100% |
| `building:levels` | 42,243,002 | **6.0%** |
| `height` | 26,775,446 | **3.8%** |
| `building:part` | 4,635,536 | 0.66% |
| `roof:shape` | 9,589,935 | **1.4%** |
| `roof:colour` | 2,571,765 | 0.36% |
| `building:material` | 2,546,169 | 0.36% |
| `roof:material` | 2,299,399 | 0.33% |
| `building:colour` | 1,481,694 | 0.21% |

`height` and `building:levels` overlap, so roughly **8-9% of buildings
have any height signal and ~1.4% have a roof shape.**

That is the whole problem in one table. The F4Map look exists only where
mappers did 3D tagging — parts of Germany, Czechia, Poland, and scattered
cities. Everywhere else that renderer falls back to default-height boxes.

So the v3 quality ceiling is not set by the engine or the format. It is set
by how the remaining ~91% gets filled:

- **Default heuristics** — height by `building` type and by region. Cheap,
  and what every renderer does today.
- **LoD2 ground truth** where open — Germany statewide, 3D BAG (NL),
  PLATEAU (JP), swisstopo. Real roof geometry, no inference.
- **Inference from imagery** — height from shadow length + sun angle, roof
  shape from nadir imagery. This is the verifiable half of the ML idea:
  hold out Switzerland/Denmark LiDAR and report height MAE and roof-type
  F1. Facade synthesis is not needed for this target and can be dropped.

Priority order follows directly: heuristics first (covers everything
immediately), LoD2 second (makes hero cities excellent), inference third.

## Zoom band allocation

| z | Vector | Heights | Colour |
|---|---|---|---|
| 0-4 | Natural Earth 110m/50m | GLO-30 + GEBCO, heavily downsampled | Blue Marble NG |
| 5-7 | Natural Earth 10m | GLO-30 + GEBCO | Blue Marble NG |
| 8-13 | Overture | GLO-30 + GEBCO | Sentinel-2 composite |
| 14-16 | Overture | national LiDAR DEM where open | national aerial ortho |
| 17+ | Overture + OSM direct | national LiDAR DEM | national aerial ortho |

Precedence follows v2's convention: generalised sources at low precedence,
detailed sources override inside their bounds.

## Committed sources

### Vector geometry

| Band | Source | Licence | Why |
|---|---|---|---|
| z0-7 | https://www.naturalearthdata.com/ | **public domain** | Already generalised. This IS the low-zoom map; do not derive it from OSM |
| z8+ | https://docs.overturemaps.org/ | ODbL / CDLA — **per theme, verify** | Buildings, transportation, places, divisions, addresses. GeoParquet, monthly |
| fallback | https://www.openstreetmap.org/ | ODbL | Only where Overture lags |
| QA snapshot | https://daylightmap.org/ | ODbL | Vandalism-free pinned OSM if Overture is not used |

Themes needed: `divisions` (admin), `transportation` (roads), `buildings`,
`places` (POI/labels), `base` (land, water, landuse).

### Water

| Band | Source | Licence |
|---|---|---|
| z0-7 | Natural Earth ocean + lakes | public domain |
| z8+ | OSM water polygons (osmdata.openstreetmap.de) | ODbL — ALREADY IN v2 |

### Heights

| Source | Coverage | Licence |
|---|---|---|
| Copernicus DEM GLO-30 | global land, 30m, to 85degN | free — https://dataspace.copernicus.eu/ |
| GEBCO 2026 | ocean floor | free — ALREADY IN v2 |
| National LiDAR DEM | 0.5-1m, ~20 countries | open, per-country |

GLO-30 over SRTM: cleaner voids, better high-latitude coverage, and the
globe shows the poles. Merge land+bathymetry into one height field as v2
already does.

### Resolution budget

Web Mercator is `156543 / 2^z` m/px:

| z | m/px | Matching source |
|---|---|---|
| 13 | 19.1 | Sentinel-2 (10m) |
| 14 | 9.6 | Sentinel-2 limit |
| 16 | 2.4 | aerial required |
| 18 | 0.60 | national ortho (25cm) |
| 19 | 0.30 | national ortho limit |

Sentinel-2 covers photographic ground **globally to ~z14**. Below that,
sub-metre national ortho, available open in ~20 countries.

Order of work, by value per unit of effort:

1. **Ground ortho from national programmes.** Already cloud-free, colour
   balanced and tiled — reproject and retile, no compositing. Ship this
   before the global Sentinel-2 mosaic.
2. **Roof textures sampled from the same ortho.** Nearly free once (1)
   exists, and the biggest single step from F4Map toward Google Maps 3D.
3. **Global Sentinel-2 composite** for z7-14 outside ortho countries.
   This is where the cloud-masking pipeline finally earns its cost.
4. **Facades: procedural, not photographic.** `building:levels` drives
   window rows, `building:material` selects from a ~50-texture regional
   library. Mapillary photogrammetry is a research project, not a feature.

### Two registration hazards

- **Building lean.** Standard ortho corrects terrain only, so tall
  buildings lean outward and their roofs sit off their footprints.
  True-ortho fixes it but needs a DSM and is not published everywhere.
  Verify per country before committing.
- **Roof smear.** Ground ortho already contains roof pixels; draping it
  and then extruding stretches those pixels down the walls. Mask
  footprints out of the ground texture, apply roofs separately.

Misregistration between ortho and footprints is the most visible artifact
in this class of map. Design for it now, not at z18.

### Colour / imagery sources

| Band | Source | Res | Licence |
|---|---|---|---|
| z0-6 | Blue Marble Next Generation | 500m | **public domain** |
| z7-13 | Sentinel-2 L2A composite | 10m | free, full open |
| z14+ | national aerial ortho | 10-25cm | open, per-country |

Sentinel-2 access via AWS COGs (https://registry.opendata.aws/sentinel-2-l2a-cogs/)
or Planetary Computer STAC. The cloudless composite must be built in-house:
SCL band cloud mask, per-pixel seasonal median, colour balance. EOX's
ready-made mosaic is CC BY-NC-SA and **cannot ship**.

Blue Marble first — it is public domain, ~500m, tiny, and covers the band
where the globe actually lives. Ship that before touching Sentinel-2.

### Buildings

| Tier | Source | Licence |
|---|---|---|
| global LoD1 | Overture buildings (2.3B, heights where known) | ODbL / CDLA |
| hero LoD2 | Germany state LoD2, 3D BAG (NL), PLATEAU (JP), swisstopo | open, per-country |

CityJSON is the ingest format for LoD2, not CityGML — same model, far
lighter parser.

### Labels

| Band | Source | Licence |
|---|---|---|
| z0-7 | Natural Earth populated places | public domain |
| z8+ | Overture places | ODbL / CDLA |

Natural Earth places carry population + scale rank, which maps directly to
label priority in `maps2-text/collision.rs`. Do not re-derive this.

## Licence gate

Everything above is commercially shippable. Obligations:

- **ODbL** (Overture OSM-derived, OSM water, Daylight) — attribution plus
  share-alike on derived data. Already accepted in v2.
- **Public domain** (Natural Earth, Blue Marble) — no obligation. This is
  why they own the low-zoom bands.
- **Copernicus/Sentinel** — free full open, attribution requested.
- **Per-country aerial and LoD2** — verify individually before shipping.

Explicitly excluded, do not use:

- EOX Sentinel-2 cloudless — CC BY-NC-SA
- GADM — non-commercial
- Google Photorealistic 3D Tiles — caching prohibited
- Submarine cable map — CC BY-NC-SA
- Ookla open data — CC BY-NC-SA

## Rough size budget

Order of magnitude, MT2-compacted:

| Component | Depth | Size |
|---|---|---|
| Vector, global | z0-14 | 30-60 GB |
| Heights, global | z0-11 | 10-20 GB |
| Colour, global | z0-11 | ~28 GB |
| Hero cities | z15-18 | budget per city |

Lands in the same ~100GB envelope as `plans/world-compact.toml`. Global
colour at z14 is ~7TB and is explicitly out of scope.

## Open decisions

1. **Overture vs own OSM pipeline.** Recommendation: Overture. Removes the
   PBF parser and sharding from `maps2-ingest`, inherits their conflation.
   Cost: monthly release cadence, and per-theme licence verification.
2. **Imagery scope.** Settled: `0xFF02` tile kind is required. Blue Marble
   z0-6, national ortho z15-19 for covered countries, global Sentinel-2
   composite deferred until after ortho and roof texturing ship.
3. **LoD2 in v3 or v3.1.** Given 1.4% roof:shape coverage, LoD2 from open
   national sources is the single biggest visual upgrade available and
   probably belongs in v3 rather than v3.1.

## First steps

1. Pin Natural Earth + GLO-30 + GEBCO + Blue Marble — all public
   domain/free, no licence risk, covers z0-7 completely.
2. Prototype the Overture GeoParquet -> MT2 transcoder on one metro,
   compare package size against the v2 PBF pipeline on the same area.
3. Decide open question 2 on the strength of the Blue Marble result.
