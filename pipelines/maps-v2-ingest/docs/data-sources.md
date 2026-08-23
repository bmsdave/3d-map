# Globe data sources

Candidate analytics layers for MT2 packages, grouped by the render path they
exercise. Licence column is the gate: `CC BY` / public domain ship freely,
`ODbL` carries share-alike, `NC` cannot ship in a commercial package at all.

Status: survey only. Nothing here is wired into a plan yet.

## 1. Raster fields — existing `0xFF01` packed path

Value channel instead of heights. No format work; see `heights.rs`.

| Layer | Source | Licence |
|---|---|---|
| Population 100m | https://human-settlement.emergency.copernicus.eu/ (GHS-POP) | CC BY 4.0 |
| Population 100m/1km | https://www.worldpop.org/ | CC BY 4.0 |
| Population, H3 hex | https://www.kontur.io/portfolio/population-dataset/ | ODbL |
| Nightlights | https://eogdata.mines.edu/products/vnl/ | public domain |
| Nightlights (NASA) | https://blackmarble.gsfc.nasa.gov/ | public domain |
| Land cover 10m | https://esa-worldcover.org/ | CC BY 4.0 |
| Land cover, near-real-time | https://dynamicworld.app/ | CC BY 4.0 |
| Forest loss by year | https://storage.googleapis.com/earthenginepartners-hansen/GFC-2024-v1.12/download.html | CC BY 4.0 |
| Vegetation / NDVI | https://lpdaac.usgs.gov/ | public domain |
| Sea ice extent, daily | https://nsidc.org/data/seaice_index | free |
| Precipitation, 30-min | https://gpm.nasa.gov/data/imerg | free |
| Sea surface temperature | https://www.ncei.noaa.gov/products/optimum-interpolation-sst | public domain |
| Climate reanalysis | https://cds.climate.copernicus.eu/ (ERA5) | free, registration |
| Temperature anomaly | https://berkeleyearth.org/data/ | CC BY |
| Air quality (NO2, aerosol) | https://atmosphere.copernicus.eu/ (CAMS) | free |
| Solar potential | https://globalsolaratlas.info/ | CC BY 4.0 |
| Wind potential | https://globalwindatlas.info/ | CC BY 4.0 |
| Elevation 30m | https://dataspace.copernicus.eu/ (GLO-30) | free |
| Soil properties | https://soilgrids.org/ | CC BY 4.0 |
| Bathymetry | https://www.gebco.net/ | free — ALREADY INGESTED |

Best first pick: ESA WorldCover (makes the globe look finished), then Hansen
forest loss (per-year stack gives a real time slider from static rasters).

## 2. Point layers — existing places/POI path

| Layer | Source | Licence |
|---|---|---|
| **Earthquakes, live** | https://earthquake.usgs.gov/earthquakes/feed/v1.0/geojson.php | public domain |
| Earthquake catalogue | https://earthquake.usgs.gov/earthquakes/search/ | public domain |
| Volcanoes | https://volcano.si.edu/ | free |
| Active fires | https://firms.modaps.eosdis.nasa.gov/ | free, API key |
| Power plants | https://github.com/wri/global-power-plant-database | CC BY 4.0 |
| Airports | https://ourairports.com/data/ | public domain |
| Languages | https://glottolog.org/ | CC BY |
| Cell towers | https://opencellid.org/ | CC BY-SA, free key |
| Geotagged articles | https://query.wikidata.org/ | CC0 |
| Places / POIs | https://overturemaps.org/ | ODbL / CDLA |

Power plant DB is unmaintained since v1.3.0 (2021) — 34,936 plants, 167
countries, fuel type + capacity. Fine as a static layer, just stale.

Highest value per hour on this whole page: USGS earthquakes. No key, no
ingest step, live GeoJSON, magnitude maps to radius.

## 3. Great-circle arcs — NEW render path

The signature 3D-globe visual; nothing in the engine draws this yet.

| Layer | Source | Licence |
|---|---|---|
| Submarine cables | https://www.submarinecablemap.com/ | **CC BY-NC-SA 3.0 — demo only** |
| Flight routes | https://openflights.org/data.php | free |
| Airports (endpoints) | https://ourairports.com/data/ | public domain |
| Trade flows | https://comtradeplus.un.org/ | free tier |
| Migration stock | https://www.un.org/development/desa/pd/content/international-migrant-stock | free |
| Remittances | https://www.worldbank.org/en/topic/migrationremittancesdiasporaissues/brief/migration-remittances-data | free |
| Shipping density | https://datacatalog.worldbank.org/ (search "shipping traffic density") | CC BY |
| Internet topology | https://www.caida.org/ , https://www.peeringdb.com/ | free |
| Live flights | https://opensky-network.org/ | free, non-commercial |

Submarine cables is the layer everyone reaches for first and the one you
cannot ship. Flight routes is the commercially safe equivalent.

## 4. Choropleth — admin polygons + sidecar values

Geometry:

| Layer | Source | Licence |
|---|---|---|
| Admin 0/1, low zoom | https://www.naturalearthdata.com/ | public domain — ALREADY INGESTED |
| Admin 0-4, global | https://www.geoboundaries.org/ | CC BY 4.0 |
| Admin boundaries | https://gadm.org/ | **non-commercial — avoid** |

Values:

| Layer | Source | Licence |
|---|---|---|
| ~1500 indicators | https://data.worldbank.org/ , https://api.worldbank.org/v2/country/all/indicator/NY.GDP.PCAP.CD?format=json | CC BY 4.0 |
| Subnational NUTS-2/3 | https://ec.europa.eu/eurostat/data/database | free reuse |
| Macro / PPP | https://data.imf.org/ , https://data-explorer.oecd.org/ | free |
| Health | https://www.who.int/data/gho | free |
| Labour | https://ilostat.ilo.org/data/ | free |
| Food / agriculture | https://www.fao.org/faostat/ | CC BY 4.0 |
| Aggregated everything | https://ourworldindata.org/ | mixed — check upstream |
| HDI | https://hdr.undp.org/data-center | free |
| Corruption index | https://www.transparency.org/en/cpi | CC BY-ND |
| Press freedom | https://rsf.org/en/index | free |
| Happiness | https://worldhappiness.report/data-sharing/ | free |
| Köppen climate zones | https://www.gloh2o.org/koppen/ | CC BY 4.0 |

Visually the weakest option — 200 flat polygons is the least interesting
thing a 3D engine can draw. Good for correctness tests, poor for demos.

## 5. Above the surface — exercises the globe camera

| Layer | Source | Licence |
|---|---|---|
| Satellite orbits (TLE) | https://celestrak.org/NORAD/elements/ | free |
| Hurricane tracks | https://www.ncei.noaa.gov/products/international-best-track-archive | public domain |
| Star catalogue | https://github.com/astronexus/HYG-Database | CC BY-SA |

~10k tracked objects from CelesTrak. Propagating orbits puts geometry above
the sphere, which stresses camera and depth handling in a way no surface
layer does. Strong differentiator for a 3D SDK specifically.

## 6. Other bodies — proves the engine is not Earth-hardcoded

| Layer | Source | Licence |
|---|---|---|
| Moon / Mars DEMs + mosaics | https://astrogeology.usgs.gov/search | public domain |
| Planetary data portal | https://pds-geosciences.wustl.edu/ | public domain |

Same tiling code, same MT2 format, different sphere — a plan file plus a
radius constant. Better SDK demo than any statistic.

## 7. Cost of living — see conversation notes

Numbeo is paid-only ($260+/mo API, scraping prohibited, redistribution
barred even for licensees). Open substitutes, per-city where it matters:

| Layer | Source | Licence |
|---|---|---|
| Price level indices / PPP | https://ec.europa.eu/eurostat/data/database | free reuse |
| FR transaction prices | https://app.dvf.etalab.gouv.fr/ | open |
| UK price paid | https://www.gov.uk/government/statistical-data-sets/price-paid-data-downloads | OGL |
| US fair market rents | https://www.huduser.gov/portal/datasets/fmr.html | public domain |
| US census / ACS | https://www.census.gov/data/developers.html | public domain |
| Big Mac index | https://github.com/TheEconomist/big-mac-data | attribution |
| ES/IT/PT listings API | https://developers.idealista.com/ | free tier |

If scraping prices directly: precedent is the Billion Prices Project
(http://www.thebillionpricesproject.com/) and ONS now uses web-scraped
prices in the UK CPI. Extraction hook is schema.org JSON-LD on product
pages (https://schema.org/Product) — `offers.price`, `priceCurrency`,
often `gtin13`. GTIN is the cross-country join key. FX from
https://www.ecb.europa.eu/stats/eurofxref/eurofxref-daily.xml (free daily).
Licensed alternatives to scraping: https://webservices.amazon.com/paapi5/documentation/
and https://developers.google.com/merchant/api

## Licence traps

- **CC BY-NC-SA** — blocks commercial shipping. Catches submarine cables,
  Ookla speed tiles (https://github.com/teamookla/ookla-open-data).
- **Non-commercial** — GADM, OpenSky.
- **ODbL** — share-alike propagates into derived tiles. Kontur, Overture.
  Already accepted for OSM, but a new obligation for each new ODbL source.
- **CC BY-ND** — no derivatives; Transparency International CPI. A
  choropleth may count as a derivative. Check before shipping.

## 8. Photorealistic basemap — imagery, 3D, street level

Goal: Google-Maps-grade realism from open data. The leverage is very
unevenly distributed — imagery + terrain + lighting is ~80% of perceived
realism; photogrammetric mesh is the last 20% and is not openly available
at global scale. Plan accordingly.

### 8.1 Satellite imagery — global

| Source | Res | Licence |
|---|---|---|
| Sentinel-2 L2A | 10m, 5-day revisit | free, full open (Copernicus) |
| — via AWS Open Data COGs | | https://registry.opendata.aws/sentinel-2-l2a-cogs/ |
| — via Planetary Computer STAC | | https://planetarycomputer.microsoft.com/ |
| — via Copernicus Data Space | | https://dataspace.copernicus.eu/ |
| Landsat 8/9 | 30m, 50yr archive | public domain (USGS) |
| NASA GIBS ready-made WMTS | 250m daily | public domain — https://nasa-gibs.github.io/gibs-api-docs/ |
| Blue Marble Next Generation | 500m monthly | public domain — https://visibleearth.nasa.gov/collection/1484/blue-marble |
| Sentinel-2 cloudless (EOX) | 10m seamless mosaic | **CC BY-NC-SA — prototype only** |

Blue Marble for z0-6 (tiny, instant, looks right). Sentinel-2 for z7-13,
but you must build the cloudless composite yourself — cloud masking via the
SCL band, per-pixel median over a season, then colour balancing. EOX proves
it works; their licence means you cannot ship theirs.

### 8.2 Aerial imagery — national, 10-25cm, open

Far better than 10m where available. Roughly 20 countries publish open
aerial ortho at sub-metre resolution:

| Country | Source | Res |
|---|---|---|
| France | https://geoservices.ign.fr/ (BD ORTHO) | 20cm |
| Switzerland | https://www.swisstopo.admin.ch/ (SWISSIMAGE) | 10cm |
| Netherlands | https://www.pdok.nl/ | 8cm |
| USA | https://registry.opendata.aws/naip/ (NAIP) | 60cm |
| UK | https://www.data.gov.uk/ (OS / EA) | 25cm |
| Spain | https://pnoa.ign.es/ | 25cm |
| Denmark | https://dataforsyningen.dk/ | 12cm |
| Norway | https://www.norgeibilder.no/ | 10cm |
| Estonia | https://geoportaal.maaamet.ee/ | 20cm |
| Japan | https://maps.gsi.go.jp/development/ichiran.html | varies |
| Australia | https://elevation.fsdf.org.au/ | varies |

### 8.3 3D buildings

| Source | Coverage | Licence |
|---|---|---|
| OSM | best attributes (height, levels, roof:shape) | ODbL |
| Overture buildings | 2.3B+, merged + heights | ODbL / CDLA |
| Microsoft Building Footprints | ~1.4B global | ODbL — https://github.com/microsoft/GlobalMLBuildingFootprints |
| Google Open Buildings | Africa, S/SE Asia, LatAm | CC BY 4.0 — https://sites.research.google/gr/open-buildings/ |
| **Germany LoD2 CityGML** | every building, statewide, real roof shapes | open data |
| **3D BAG** (Netherlands) | LoD1.2/1.3/2.2 every building | https://3dbag.nl/ , CC BY 4.0 |
| City LoD2 models | Vienna, Zurich, Helsinki, NYC, Singapore | open, varies |

LoD2 (prismatic walls + true roof geometry) is the realistic target and is
free across much of Europe. Photogrammetric mesh — what Google actually
renders — has no open global equivalent. Google's Photorealistic 3D Tiles
API bars caching, so it cannot back an SDK.

### 8.4 Terrain

| Source | Res | Licence |
|---|---|---|
| Copernicus DEM GLO-30 | 30m global | free |
| National LiDAR DEM | 0.5-1m | open (NL AHN, UK EA, FR LiDAR HD, US 3DEP, CH, DK, EE) |
| GEBCO bathymetry | sea floor | free — ALREADY INGESTED |

### 8.5 Vegetation — underrated realism lever

| Source | What | Licence |
|---|---|---|
| Meta / WRI canopy height | 1m global tree height | CC BY 4.0 |
| ESA WorldCover | 10m land cover classes | CC BY 4.0 |
| OSM | individual trees, forest polygons | ODbL |
| City tree inventories | species + trunk position | open, per-city |

### 8.6 Street level

| Source | Scale | Licence |
|---|---|---|
| Mapillary | ~2B images, API | CC BY-SA — https://www.mapillary.com/developer |
| KartaView | crowdsourced | CC BY-SA — https://kartaview.org/ |

### 8.7 What this costs in engine work

Data is the easy half. Required changes:

- **New tile kind for imagery.** Current raster path is single-channel
  uint16 (`0xFF00` = 256x256x2). Colour needs 3-channel compressed —
  add `0xFF02` carrying **KTX2 / Basis Universal**, not raw JPEG, so it
  transcodes to native GPU formats instead of eating VRAM as RGBA.
- Normal maps derived from the DEM (cheap, large visual payoff).
- Shadow mapping + screen-space AO — this is what actually reads as "3D".
- Atmospheric scattering + time-of-day lighting on the globe.
- PBR materials for water, glass, roofs.

Perceived realism comes mostly from lighting, not from data resolution.

### 8.8 Size budget

Tiles = 4^z. Rough, at ~20KB/tile JPEG-class:

| Depth | Tiles | Size |
|---|---|---|
| z0-8 | ~87k | ~2 GB |
| z0-10 | ~1.4M | ~28 GB |
| z0-12 | ~22M | ~450 GB |
| z0-14 | ~358M | ~7 TB |

Global to z10-11 plus hero cities to z16-18 fits the same budget shape as
`plans/world-compact.toml`. Global z14 imagery is not a 100GB project.
