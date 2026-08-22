# Licensing a package built with this SDK

The code in this repository is MIT. The **data** a package carries is not,
and this is the notice to publish beside one.

This is a plain-English summary written by engineers, not lawyers. Read the
licences themselves before publishing commercially.

## The short version

An MT2 package built from these sources is a **derived database** of
OpenStreetMap, so ODbL applies to the package itself — not only to the page
that draws it. That is fine for an open project and fatal to a plan to sell
the tiles as a closed product.

## What each source asks

| Source | Licence | What it requires |
|---|---|---|
| OpenStreetMap | ODbL 1.0 | Attribution, and share-alike on derived databases |
| Copernicus DEM (GLO-30) | Copernicus / ESA terms | Attribution |
| GEBCO grid | Free use | Acknowledgement; **not for navigation** |
| Natural Earth | Public domain | Nothing |

Every one of these travels inside the package. `manifest.json` carries a
`sources` array with each source's name, URL, date, licence and required
attribution string, copied from its descriptor in
`pipelines/maps-v2-ingest/sources/`. A package is therefore self-describing:
whoever receives it can read what it is made of and what they owe.

## The obligation with teeth

ODbL's share-alike clause is the one that constrains what you may do.

The OSM Foundation's guidance distinguishes a **produced work** — a
rendered image of a map, which needs attribution but not share-alike — from
a **derived database**, which needs both. Vector tiles carrying real
geometry are a derived database. MT2 tiles carry real geometry.

So publishing a package means:

1. **Attribution where the map is shown.** "© OpenStreetMap contributors"
   plus the other lines above, visible on the map itself — not buried in a
   colophon. The demo does this in its footer.
2. **The package offered under ODbL**, said out loud beside the download.
3. **Access to the derived database**, which is the package. Publishing it
   satisfies this by itself.

## Attribution text

The line the demo shows, which is the minimum for these four sources:

> © OpenStreetMap contributors (ODbL) · land relief Copernicus DEM (© DLR
> e.V. 2010–2014, © Airbus DS 2014–2018, ESA/EU) · bathymetry GEBCO 2026 ·
> boundaries and places Natural Earth.

## What is not covered here

A package built from *other* sources — a national LiDAR DTM, commercial
imagery, an address database — carries whatever those ask for instead.
Add a descriptor with the correct `licence` and `attribution` fields and
they will travel with the package the same way; the pipeline copies them
into the manifest without knowing what they mean.
