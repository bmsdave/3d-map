use std::{collections::BTreeMap, env, fs, fs::File, path::{Path, PathBuf}, process::{Command, ExitCode}};

use maps2_ingest::{
    DemGrid, LayerClaim, PreparedFeature, SourceDescriptor, SourceKind, SourceLayer, build_tiles,
    build_tiles_with_terrains, claimed_levels, conflate,
    load_copernicus_dem, load_gebco_quadrant_decimated, load_gebco_window, read_descriptor,
    resolve_boundary_lines, resolve_major_roads, resolve_osm_pbf, resolve_place_labels,
    resolve_water_polygons, scan_osm_pbf, stitch_world_quadrants, validate_source,
    validate_source_reader, OsmSummary,
};
use maps2_units::{locate, to_lonlat, Lonlat, TileCoord, TileId, TilePoint};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("maps2-ingest: {message}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<(), String> {
    match args {
        [command] if command == "--help" || command == "-h" => {
            print_help();
            Ok(())
        }
        [command, path] if command == "scan" => scan(path),
        [command, descriptor, input] if command == "verify" => verify(descriptor, input),
        [command, package] if command == "verify-package" => verify_package(package),
        [command, descriptor, output] if command == "fetch" => fetch(descriptor, output),
        [command, descriptor, input, level, output] if command == "build" => {
            build(descriptor, input, level, output)
        }
        [command, args @ ..] if command == "build-terrain-many" => build_terrain_many(args),
        [command, args @ ..] if command == "build-terrain-range" => build_terrain_range(args),
        [command, plan, output] if command == "build-map" => build_map(plan, output),
        [command, package, lon, lat, output, options @ ..] if command == "carve" => {
            carve(package, lon, lat, output, options)
        }
        [command, descriptor, shapefile, minimum, maximum, output, terrain @ ..] if command == "build-world" => {
            build_world(descriptor, shapefile, minimum, maximum, output, terrain)
        }
        [command, osm_descriptor, osm_input, dem_descriptor, dem_input, west, south, level, output]
            if command == "build-terrain" => build_terrain(&TerrainBuildArgs {
                osm_descriptor,
                osm_input,
                dem_descriptor,
                dem_input,
                west,
                south,
                level,
                output,
            }),
        [command, path, west, south] if command == "dem-info" => dem_info(path, west, south),
        [command, descriptor, input, west, south, east, north] if command == "gebco-window" => {
            gebco_window(descriptor, input, west, south, east, north)
        }
        _ => Err("usage: maps2-ingest scan <osm.pbf>".to_string()),
    }
}

fn dem_info(path: &str, west: &str, south: &str) -> Result<(), String> {
    let west = west.parse::<f64>().map_err(|error| format!("invalid west {west}: {error}"))?;
    let south = south.parse::<f64>().map_err(|error| format!("invalid south {south}: {error}"))?;
    let grid = load_copernicus_dem(path, west, south).map_err(|error| error.to_string())?;
    println!("{{\"south_west_height_m\":{}}}", grid.sample(west, south));
    Ok(())
}

fn build(descriptor_path: &str, input_path: &str, level: &str, output: &str) -> Result<(), String> {
    let descriptor = load_descriptor(descriptor_path)?;
    if descriptor.kind != SourceKind::OsmPbf {
        return Err("build accepts only an osm-pbf source descriptor".to_string());
    }
    let input = File::open(input_path).map_err(|error| format!("cannot open {input_path}: {error}"))?;
    validate_source_reader(&descriptor.source, input).map_err(|error| error.to_string())?;
    let level = level.parse::<u8>().map_err(|error| format!("invalid level {level}: {error}"))?;
    let features = resolve_osm_pbf(input_path, level).map_err(|error| error.to_string())?;
    let tiles = build_tiles(&features).map_err(|error| format!("cannot encode MT2: {error:?}"))?;
    let digests = tile_digests(&tiles);
    write_tiles(Path::new(output), &tiles)?;
    write_manifest(Path::new(output), &[&descriptor], &[level], features.len(), &digests, 0)?;
    println!("{{\"features\":{},\"tiles\":{}}}", features.len(), tiles.len());
    Ok(())
}

struct TerrainBuildArgs<'a> {
    osm_descriptor: &'a str,
    osm_input: &'a str,
    dem_descriptor: &'a str,
    dem_input: &'a str,
    west: &'a str,
    south: &'a str,
    level: &'a str,
    output: &'a str,
}

fn build_terrain(args: &TerrainBuildArgs<'_>) -> Result<(), String> {
    let osm = load_kind(args.osm_descriptor, SourceKind::OsmPbf)?;
    validate_input(&osm, args.osm_input)?;
    let terrain = load_terrain_input(args.dem_descriptor, args.dem_input, args.west, args.south)?;
    let level = parse_level(args.level)?;
    write_terrain_package(&osm, args.osm_input, level, Path::new(args.output), &[terrain])
}

fn build_terrain_many(args: &[String]) -> Result<(), String> {
    let [osm_descriptor, osm_input, level, output, terrain @ ..] = args else {
        return Err("build-terrain-many requires an OSM source and one or more DEM inputs".to_string());
    };
    let osm = load_kind(osm_descriptor, SourceKind::OsmPbf)?;
    validate_input(&osm, osm_input)?;
    let level = parse_level(level)?;
    let terrain = parse_terrain_inputs(terrain)?;
    write_terrain_package(&osm, osm_input, level, Path::new(output), &terrain)
}

fn build_terrain_range(args: &[String]) -> Result<(), String> {
    let [osm_descriptor, osm_input, minimum, maximum, output, terrain @ ..] = args else {
        return Err("build-terrain-range requires an OSM source, an inclusive zoom range, and DEM inputs".to_string());
    };
    let osm = load_kind(osm_descriptor, SourceKind::OsmPbf)?;
    validate_input(&osm, osm_input)?;
    let levels = parse_levels(minimum, maximum)?;
    let terrain = parse_terrain_inputs(terrain)?;
    write_terrain_levels(&osm, osm_input, &levels, Path::new(output), &terrain)
}

struct TerrainInput {
    descriptor: SourceDescriptor,
    grid: maps2_ingest::DemGrid,
}

fn parse_terrain_inputs(args: &[String]) -> Result<Vec<TerrainInput>, String> {
    if args.is_empty() || !args.len().is_multiple_of(4) {
        return Err("each terrain input needs <dem-source.toml> <dem.tif> <west> <south>".to_string());
    }
    args.chunks_exact(4)
        .map(|input| load_terrain_input(&input[0], &input[1], &input[2], &input[3]))
        .collect()
}

fn load_terrain_input(
    descriptor_path: &str,
    input_path: &str,
    west: &str,
    south: &str,
) -> Result<TerrainInput, String> {
    let descriptor = load_kind(descriptor_path, SourceKind::CopernicusDem)?;
    validate_input(&descriptor, input_path)?;
    let grid = load_terrain(input_path, west, south)?;
    Ok(TerrainInput { descriptor, grid })
}

fn write_terrain_package(
    osm: &SourceDescriptor,
    osm_input: &str,
    level: u8,
    output: &Path,
    terrain: &[TerrainInput],
) -> Result<(), String> {
    write_terrain_levels(osm, osm_input, &[level], output, terrain)
}

fn write_terrain_levels(
    osm: &SourceDescriptor,
    osm_input: &str,
    levels: &[u8],
    output: &Path,
    terrain: &[TerrainInput],
) -> Result<(), String> {
    let grids = terrain.iter().map(|input| input.grid.clone()).collect::<Vec<_>>();
    let mut feature_count = 0;
    let mut height_tile_count = 0;
    let mut digests = Vec::new();
    for level in levels {
        let features = resolve_osm_pbf(osm_input, *level).map_err(|error| error.to_string())?;
        let tiles = build_tiles_with_terrains(&features, &grids).map_err(|error| format!("cannot encode MT2: {error:?}"))?;
        height_tile_count += tiles.iter().filter(|(tile, _)| grids.iter().any(|grid| grid.covers_tile(*tile))).count();
        feature_count += features.len();
        digests.extend(tile_digests(&tiles));
        write_tiles(output, &tiles)?;
    }
    let mut sources = vec![osm];
    sources.extend(terrain.iter().map(|input| &input.descriptor));
    write_manifest(output, &sources, levels, feature_count, &digests, height_tile_count)?;
    println!("{{\"features\":{feature_count},\"tiles\":{},\"height_tiles\":{height_tile_count}}}", digests.len());
    Ok(())
}

/// Builds a low-zoom world package from the OSM community's pre-simplified
/// water-polygon shapefile: real global ocean coverage without parsing
/// planet-scale OSM data, which the low-zoom globe band never needed
/// vector detail for in the first place — see `world_water` for why.
///
/// `terrain` is zero or more `<gebco-source.toml> <gebco.tif> <stride>`
/// triples, each a whole GEBCO quadrant decimated by `stride` — see
/// `world_terrain` for why a bounded window is the wrong tool at this
/// scale. A world tile gets a height raster when one quadrant's bounds
/// fully cover it (the same `covers_tile` rule regional builds already
/// use for their DEM tiles); a z0/z1 tile is wider than any single
/// quadrant, so when the quadrants tile a regular rectangle (the whole
/// globe, given all eight) they are also stitched into one coarser
/// whole-world grid and appended as a fallback — checked only after
/// every individual quadrant, so the more precise per-quadrant grid
/// still wins wherever one covers a tile.
fn build_world(
    descriptor_path: &str, shapefile_path: &str, minimum: &str, maximum: &str, output: &str,
    terrain: &[String],
) -> Result<(), String> {
    let descriptor = load_kind(descriptor_path, SourceKind::WaterPolygons)?;
    validate_input(&descriptor, shapefile_path)?;
    let levels = parse_levels(minimum, maximum)?;
    let layers = parse_world_layers(terrain)?;
    let mut grids = layers.terrain.iter().map(|input| input.grid.clone()).collect::<Vec<_>>();
    if let Ok(world) = stitch_world_quadrants(&grids) {
        grids.push(world);
    }
    let output = Path::new(output);
    let mut feature_count = 0;
    let mut height_tile_count = 0;
    let mut digests = Vec::new();
    for level in &levels {
        // Coastline first, then the furniture that makes it a map rather
        // than a relief model: borders, roads, and the place names that
        // are the only thing readable at a world zoom.
        let mut features = resolve_water_polygons(shapefile_path, *level).map_err(|error| error.to_string())?;
        for layer in layers.vector_layers() {
            features.extend(layer.resolve(*level)?);
        }
        let tiles = build_tiles_with_terrains(&features, &grids)
            .map_err(|error| format!("cannot encode MT2: {error:?}"))?;
        height_tile_count += tiles.iter().filter(|(tile, _)| grids.iter().any(|grid| grid.covers_tile(*tile))).count();
        feature_count += features.len();
        digests.extend(tile_digests(&tiles));
        write_tiles(output, &tiles)?;
    }
    let mut sources = vec![&descriptor];
    sources.extend(layers.terrain.iter().map(|input| &input.descriptor));
    sources.extend(layers.vector_layers().iter().map(|layer| &layer.descriptor));
    write_manifest(output, &sources, &levels, feature_count, &digests, height_tile_count)?;
    println!("{{\"features\":{feature_count},\"tiles\":{},\"height_tiles\":{height_tile_count}}}", digests.len());
    Ok(())
}

struct WorldTerrainInput {
    descriptor: SourceDescriptor,
    grid: DemGrid,
}

/// One Natural Earth shapefile layer: which resolver reads it, and the
/// descriptor that pins the bytes it read.
struct WorldVectorLayer {
    descriptor: SourceDescriptor,
    path: String,
    kind: SourceKind,
}

impl WorldVectorLayer {
    fn resolve(&self, level: u8) -> Result<Vec<PreparedFeature>, String> {
        let features = match self.kind {
            SourceKind::NaturalEarthPlaces => resolve_place_labels(&self.path, level),
            SourceKind::NaturalEarthBoundaries => resolve_boundary_lines(&self.path, level),
            _ => resolve_major_roads(&self.path, level),
        };
        features.map_err(|error| error.to_string())
    }
}

/// Everything `build-world` layers onto the coastline, however the
/// command line happened to order it.
struct WorldLayers {
    terrain: Vec<WorldTerrainInput>,
    places: Option<WorldVectorLayer>,
    boundaries: Option<WorldVectorLayer>,
    roads: Option<WorldVectorLayer>,
}

impl WorldLayers {
    fn vector_layers(&self) -> Vec<&WorldVectorLayer> {
        [self.boundaries.as_ref(), self.roads.as_ref(), self.places.as_ref()]
            .into_iter()
            .flatten()
            .collect()
    }
}

/// The terrain triples stay positional, as they were; the Natural Earth
/// layers are flagged, because they are optional and a package built
/// without them is still a valid globe.
fn parse_world_layers(args: &[String]) -> Result<WorldLayers, String> {
    let mut terrain = Vec::new();
    let (mut places, mut boundaries, mut roads) = (None, None, None);
    let mut index = 0;
    while index < args.len() {
        let kind = match args[index].as_str() {
            "--places" => SourceKind::NaturalEarthPlaces,
            "--boundaries" => SourceKind::NaturalEarthBoundaries,
            "--roads" => SourceKind::NaturalEarthRoads,
            _ => {
                terrain.push(args[index].clone());
                index += 1;
                continue;
            }
        };
        let flag = &args[index];
        let (Some(descriptor_path), Some(shapefile)) = (args.get(index + 1), args.get(index + 2))
        else {
            return Err(format!("{flag} needs <source.toml> <shapefile.shp>"));
        };
        let descriptor = load_kind(descriptor_path, kind)?;
        validate_input(&descriptor, shapefile)?;
        let layer = WorldVectorLayer { descriptor, path: shapefile.clone(), kind };
        match kind {
            SourceKind::NaturalEarthPlaces => places = Some(layer),
            SourceKind::NaturalEarthBoundaries => boundaries = Some(layer),
            _ => roads = Some(layer),
        }
        index += 3;
    }
    Ok(WorldLayers { terrain: parse_world_terrain_inputs(&terrain)?, places, boundaries, roads })
}

fn parse_world_terrain_inputs(args: &[String]) -> Result<Vec<WorldTerrainInput>, String> {
    if args.is_empty() {
        return Ok(Vec::new());
    }
    if !args.len().is_multiple_of(3) {
        return Err("each world terrain input needs <gebco-source.toml> <gebco.tif> <stride>".to_string());
    }
    args.chunks_exact(3).map(|input| load_world_terrain_input(&input[0], &input[1], &input[2])).collect()
}

fn load_world_terrain_input(
    descriptor_path: &str, input_path: &str, stride: &str,
) -> Result<WorldTerrainInput, String> {
    let descriptor = load_kind(descriptor_path, SourceKind::GebcoGrid)?;
    validate_input(&descriptor, input_path)?;
    let stride = stride.parse::<u32>().map_err(|error| format!("invalid stride {stride}: {error}"))?;
    let grid = load_gebco_quadrant_decimated(input_path, descriptor.bounds, stride)
        .map_err(|error| error.to_string())?;
    Ok(WorldTerrainInput { descriptor, grid })
}

fn load_kind(path: &str, kind: SourceKind) -> Result<SourceDescriptor, String> {
    let descriptor = load_descriptor(path)?;
    (descriptor.kind == kind).then_some(descriptor).ok_or_else(|| format!("{path} has the wrong source kind"))
}

fn validate_input(descriptor: &SourceDescriptor, path: &str) -> Result<(), String> {
    let input = File::open(path).map_err(|error| format!("cannot open {path}: {error}"))?;
    validate_source_reader(&descriptor.source, input).map_err(|error| error.to_string())
}

fn load_terrain(path: &str, west: &str, south: &str) -> Result<maps2_ingest::DemGrid, String> {
    let west = parse_coordinate("west", west)?;
    let south = parse_coordinate("south", south)?;
    load_copernicus_dem(path, west, south).map_err(|error| error.to_string())
}

fn parse_coordinate(name: &str, value: &str) -> Result<f64, String> {
    value.parse::<f64>().map_err(|error| format!("invalid {name} {value}: {error}"))
}

fn parse_level(value: &str) -> Result<u8, String> {
    value.parse::<u8>().map_err(|error| format!("invalid level {value}: {error}"))
}

fn parse_levels(minimum: &str, maximum: &str) -> Result<Vec<u8>, String> {
    let start = parse_level(minimum)?;
    let end = parse_level(maximum)?;
    (start <= end)
        .then(|| (start..=end).collect())
        .ok_or_else(|| format!("minimum zoom {start} exceeds maximum zoom {end}"))
}

fn write_manifest(
    output: &Path,
    descriptors: &[&SourceDescriptor],
    levels: &[u8],
    feature_count: usize,
    tile_digests: &[TileDigest],
    height_tile_count: usize,
) -> Result<(), String> {
    let bytes = manifest_json(descriptors, levels, feature_count, tile_digests, height_tile_count)?;
    let path = output.join("manifest.json");
    fs::write(&path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn manifest_json(
    descriptors: &[&SourceDescriptor],
    levels: &[u8],
    feature_count: usize,
    tile_digests: &[TileDigest],
    height_tile_count: usize,
) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "format": "MT2",
        "format_version": maps2_tile::FORMAT_VERSION,
        "levels": levels,
        "feature_count": feature_count,
        "tile_count": tile_digests.len(),
        "tiles": tile_paths(tile_digests),
        "tile_digests": digest_map(tile_digests),
        "package_sha256": package_sha256(tile_digests),
        "view": package_view(tile_digests),
        "height_tile_count": height_tile_count,
        "sources": descriptors.iter().map(|descriptor| json!({
            "name": descriptor.source.name(),
            "kind": source_kind_name(descriptor.kind),
            "url": descriptor.url,
            "sha256": descriptor.source.sha256(),
            "source_date": descriptor.source_date,
            "licence": descriptor.licence,
            "attribution": descriptor.attribution,
            "bounds": descriptor.bounds,
            "adapter_version": descriptor.adapter_version,
        })).collect::<Vec<_>>(),
    }))
    .map_err(|error| format!("cannot serialize manifest: {error}"))
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct TileDigest {
    id: TileId,
    sha256: String,
}

fn tile_digests(tiles: &[(TileId, Vec<u8>)]) -> Vec<TileDigest> {
    tiles.iter().map(|(id, bytes)| TileDigest { id: *id, sha256: sha256(bytes) }).collect()
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn tile_paths(digests: &[TileDigest]) -> Vec<String> {
    let mut ids = digests.iter().map(|digest| digest.id).collect::<Vec<_>>();
    ids.sort_by_key(|id| (id.z, id.x, id.y));
    ids.into_iter().map(tile_path).collect()
}

fn digest_map(digests: &[TileDigest]) -> BTreeMap<String, String> {
    digests.iter().map(|digest| (tile_path(digest.id), digest.sha256.clone())).collect()
}

fn package_sha256(digests: &[TileDigest]) -> String {
    package_sha256_map(&digest_map(digests))
}

fn package_sha256_map(digests: &BTreeMap<String, String>) -> String {
    let mut hasher = Sha256::new();
    for (path, digest) in digests {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update(digest.as_bytes());
        hasher.update(*b"\n");
    }
    format!("{:x}", hasher.finalize())
}

fn tile_path(id: TileId) -> String {
    format!("{}/{}/{}.mt2", id.z, id.x, id.y)
}

fn package_view(digests: &[TileDigest]) -> Option<serde_json::Value> {
    let level = digests.iter().map(|digest| digest.id.z).min()?;
    let first = digests.iter().find(|digest| digest.id.z == level)?.id;
    let (min_x, max_x, min_y, max_y) = digests.iter().filter(|digest| digest.id.z == level).fold(
        (first.x, first.x, first.y, first.y),
        |(min_x, max_x, min_y, max_y), digest| {
            let tile = digest.id;
            (min_x.min(tile.x), max_x.max(tile.x), min_y.min(tile.y), max_y.max(tile.y))
        },
    );
    let centre = to_lonlat(TilePoint {
        tile: TileId { z: level, x: min_x + (max_x - min_x) / 2, y: min_y + (max_y - min_y) / 2 },
        coord: TileCoord(u16::MAX / 2, u16::MAX / 2),
    });
    Some(json!({ "lon": centre.lon, "lat": centre.lat, "zoom": level }))
}

const fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::OsmPbf => "osm-pbf",
        SourceKind::CopernicusDem => "copernicus-dem",
        SourceKind::GebcoGrid => "gebco-grid",
        SourceKind::WaterPolygons => "water-polygons",
        SourceKind::NaturalEarthPlaces => "natural-earth-places",
        SourceKind::NaturalEarthBoundaries => "natural-earth-boundaries",
        SourceKind::NaturalEarthRoads => "natural-earth-roads",
    }
}

fn write_tiles(output: &Path, tiles: &[(maps2_units::TileId, Vec<u8>)]) -> Result<(), String> {
    for (id, bytes) in tiles {
        let parent = output.join(id.z.to_string()).join(id.x.to_string());
        fs::create_dir_all(&parent).map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
        let path = parent.join(format!("{}.mt2", id.y));
        fs::write(&path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    }
    Ok(())
}

fn verify(descriptor_path: &str, input_path: &str) -> Result<(), String> {
    let descriptor = load_descriptor(descriptor_path)?;
    let input = File::open(input_path).map_err(|error| format!("cannot open {input_path}: {error}"))?;
    validate_source_reader(&descriptor.source, input).map_err(|error| error.to_string())?;
    println!("verified {}", descriptor.source.name());
    Ok(())
}

#[derive(Deserialize)]
struct PackageIntegrity {
    tile_digests: BTreeMap<String, String>,
    package_sha256: String,
}

fn verify_package(path: &str) -> Result<(), String> {
    let root = Path::new(path);
    let manifest = fs::read_to_string(root.join("manifest.json"))
        .map_err(|error| format!("cannot read package manifest: {error}"))?;
    let integrity = serde_json::from_str::<PackageIntegrity>(&manifest)
        .map_err(|error| format!("invalid package manifest: {error}"))?;
    verify_package_contents(root, &integrity.tile_digests, &integrity.package_sha256)?;
    println!("verified package {}", root.display());
    Ok(())
}

fn verify_package_contents(
    root: &Path,
    digests: &BTreeMap<String, String>,
    package_hash: &str,
) -> Result<(), String> {
    if package_sha256_map(digests) != package_hash {
        return Err("package hash does not match tile digest manifest".to_string());
    }
    for (path, expected) in digests {
        let tile = package_tile_path(root, path)?;
        let bytes = fs::read(&tile).map_err(|error| format!("cannot read {}: {error}", tile.display()))?;
        if sha256(&bytes) != *expected {
            return Err(format!("tile hash does not match {path}"));
        }
    }
    Ok(())
}

fn package_tile_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if path.is_absolute() || path.components().any(|component| !matches!(component, std::path::Component::Normal(_))) {
        return Err(format!("invalid package tile path {relative}"));
    }
    Ok(root.join(path))
}

fn fetch(descriptor_path: &str, output: &str) -> Result<(), String> {
    let descriptor = load_descriptor(descriptor_path)?;
    let output = Path::new(output);
    let partial = partial_path(output)?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    if output.exists() || partial.exists() {
        return Err(format!("refusing to overwrite {} or {}", output.display(), partial.display()));
    }
    let status = Command::new("curl")
        .args(fetch_arguments(&descriptor.url, &partial)?)
        .status()
        .map_err(|error| format!("cannot start curl: {error}"))?;
    if !status.success() {
        return Err(format!("download failed; partial file remains at {}", partial.display()));
    }
    validate_input(&descriptor, partial.to_str().ok_or_else(|| "download path is not UTF-8".to_string())?)?;
    fs::rename(&partial, output).map_err(|error| format!("cannot finalize {}: {error}", output.display()))?;
    println!("fetched and verified {}", output.display());
    Ok(())
}

fn partial_path(output: &Path) -> Result<PathBuf, String> {
    let name = output.file_name().ok_or_else(|| "download output needs a filename".to_string())?;
    Ok(output.with_file_name(format!("{}.part", name.to_string_lossy())))
}

fn fetch_arguments(url: &str, output: &Path) -> Result<Vec<String>, String> {
    if !url.starts_with("https://") {
        return Err("source URL must use HTTPS".to_string());
    }
    Ok(vec![
        "--fail".to_string(), "--location".to_string(), "--proto".to_string(), "=https".to_string(),
        "--output".to_string(), output.display().to_string(), url.to_string(),
    ])
}

fn load_descriptor(path: &str) -> Result<maps2_ingest::SourceDescriptor, String> {
    let toml_text = fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    read_descriptor(&toml_text).map_err(|error| error.to_string())
}

/// Reads only the part of a pinned GEBCO/DEM source that covers the given
/// window, printing its cost so a caller can see the bounded read actually
/// stayed bounded.
fn gebco_window(descriptor: &str, input: &str, west: &str, south: &str, east: &str, north: &str) -> Result<(), String> {
    let descriptor = load_descriptor(descriptor)?;
    let bytes = fs::read(input).map_err(|error| format!("cannot read {input}: {error}"))?;
    validate_source(&descriptor.source, &bytes).map_err(|error| error.to_string())?;
    let window = [
        parse_degrees("west", west)?,
        parse_degrees("south", south)?,
        parse_degrees("east", east)?,
        parse_degrees("north", north)?,
    ];
    let result = load_gebco_window(input, descriptor.bounds, window).map_err(|error| error.to_string())?;
    let grid = result.grid();
    let corner_lon = window[0];
    let corner_lat = window[1];
    println!(
        "{{\"chunks_read\":{},\"chunks_total\":{},\"corner_sample_m\":{}}}",
        result.chunks_read(),
        result.chunks_total(),
        grid.sample(corner_lon, corner_lat)
    );
    Ok(())
}

fn parse_degrees(name: &str, value: &str) -> Result<f64, String> {
    value.parse::<f64>().map_err(|error| format!("invalid {name} {value}: {error}"))
}

fn scan(path: &str) -> Result<(), String> {
    let input = File::open(path).map_err(|error| format!("cannot open {path}: {error}"))?;
    let summary = scan_osm_pbf(input).map_err(|error| error.to_string())?;
    println!("{}", summary_json(summary));
    Ok(())
}

fn summary_json(summary: OsmSummary) -> String {
    format!(
        "{{\"objects\":{},\"roads\":{},\"buildings\":{},\"water\":{},\"parks\":{},\"pois\":{}}}",
        summary.objects, summary.roads, summary.buildings, summary.water, summary.parks, summary.pois
    )
}

fn print_help() {
    println!(
        "maps2-ingest\n\nusage:\n  maps2-ingest scan <osm.pbf>\n  maps2-ingest verify <source.toml> <input>\n  maps2-ingest verify-package <package-dir>\n  maps2-ingest fetch <source.toml> <output>\n  maps2-ingest build <source.toml> <osm.pbf> <level> <output-dir>\n  maps2-ingest build-terrain <osm-source.toml> <osm.pbf> <dem-source.toml> <dem.tif> <west> <south> <level> <output-dir>\n  maps2-ingest build-terrain-many <osm-source.toml> <osm.pbf> <level> <output-dir> <dem-source.toml> <dem.tif> <west> <south>...\n  maps2-ingest build-terrain-range <osm-source.toml> <osm.pbf> <min-level> <max-level> <output-dir> <dem-source.toml> <dem.tif> <west> <south>...\n  maps2-ingest dem-info <dem.tif> <west> <south>\n  maps2-ingest gebco-window <source.toml> <grid.tif> <west> <south> <east> <north>\n  maps2-ingest build-world <water-source.toml> <water.shp> <min-level> <max-level> <output-dir> [<gebco-source.toml> <gebco.tif> <stride>]...\n  maps2-ingest build-map <plan.toml> <output-dir>\n  maps2-ingest carve <package-dir> <lon> <lat> <output-dir> [--world <level>] [--keep <min>:<max>:<radius>]..."
    );
}

/// A carve of an existing package: the same tiles, fewer of them.
///
/// A built world package is gigabytes because it answers everywhere. A
/// lab study asks about one place, so it needs the whole planet only at
/// the levels where the whole planet is on screen, and a square around
/// its subject below that. Carving copies tiles verbatim — nothing is
/// re-encoded, so the digests are still the digests of the bytes the
/// build produced — and rewrites the manifest around the subset.
struct CarveRule {
    levels: (u8, u8),
    radius: u32,
}

fn carve(package: &str, lon: &str, lat: &str, output: &str, options: &[String]) -> Result<(), String> {
    let centre = Lonlat {
        lon: lon.parse::<f64>().map_err(|error| format!("invalid lon {lon}: {error}"))?,
        lat: lat.parse::<f64>().map_err(|error| format!("invalid lat {lat}: {error}"))?,
    };
    let (world, rules) = parse_carve_options(options)?;
    let root = Path::new(package);
    let manifest = fs::read_to_string(root.join("manifest.json"))
        .map_err(|error| format!("cannot read package manifest: {error}"))?;
    let manifest = serde_json::from_str::<serde_json::Value>(&manifest)
        .map_err(|error| format!("invalid package manifest: {error}"))?;
    let available = manifest
        .get("tile_digests")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| "package manifest has no tile_digests".to_string())?;

    let wanted = carve_selection(centre, world, &rules, available);
    if wanted.is_empty() {
        return Err("carve selected no tiles: check the centre and the level rules".to_string());
    }

    let carved = copy_carved_tiles(root, Path::new(output), &wanted)?;
    write_carved_manifest(Path::new(output), &manifest, centre, &carved)?;
    println!(
        "{{\"features\":{},\"tiles\":{},\"height_tiles\":{},\"levels\":[{},{}]}}",
        carved.features,
        carved.digests.len(),
        carved.height_tiles,
        carved.levels.first().copied().unwrap_or_default(),
        carved.levels.last().copied().unwrap_or_default(),
    );
    Ok(())
}

/// What a carve copied: the bytes are the source package's, so the
/// digests are too — only the counts have to be recomputed, because a
/// subset holds a different number of features than the whole.
struct CarvedTiles {
    digests: Vec<TileDigest>,
    levels: Vec<u8>,
    features: usize,
    height_tiles: usize,
}

fn copy_carved_tiles(root: &Path, output: &Path, wanted: &[TileId]) -> Result<CarvedTiles, String> {
    let mut digests = Vec::new();
    let mut features = 0_usize;
    let mut height_tiles = 0_usize;
    for id in wanted {
        let relative = tile_path(*id);
        let source = package_tile_path(root, &relative)?;
        let bytes = fs::read(&source)
            .map_err(|error| format!("cannot read {}: {error}", source.display()))?;
        let tile = maps2_tile::TileView::parse(&bytes)
            .map_err(|error| format!("cannot parse {relative}: {error:?}"))?;
        features += tile
            .classes()
            .filter(|class| *class < maps2_tile::RASTER_CLASS_BASE)
            .filter_map(|class| tile.section(class))
            .map(|section| section.features().count())
            .sum::<usize>();
        if tile.raster(maps2_tile::CLASS_HEIGHTS).is_some() {
            height_tiles += 1;
        }
        digests.push(TileDigest { id: *id, sha256: sha256(&bytes) });
        write_tiles(output, &[(*id, bytes)])?;
    }
    let levels = carved_levels(&digests);
    Ok(CarvedTiles { digests, levels, features, height_tiles })
}

fn parse_carve_options(options: &[String]) -> Result<(Option<u8>, Vec<CarveRule>), String> {
    let mut world = None;
    let mut rules = Vec::new();
    let mut rest = options.iter();
    while let Some(flag) = rest.next() {
        let value = rest.next().ok_or_else(|| format!("{flag} needs a value"))?;
        match flag.as_str() {
            "--world" => world = Some(parse_level(value)?),
            "--keep" => rules.push(parse_carve_rule(value)?),
            other => return Err(format!("unknown carve option {other}")),
        }
    }
    if world.is_none() && rules.is_empty() {
        return Err("carve needs at least one --world or --keep".to_string());
    }
    Ok((world, rules))
}

/// `<min>:<max>:<radius>` — the levels this rule speaks for, and how
/// many tiles either side of the subject it keeps at each of them.
fn parse_carve_rule(value: &str) -> Result<CarveRule, String> {
    let parts = value.split(':').collect::<Vec<_>>();
    let [minimum, maximum, radius] = parts.as_slice() else {
        return Err(format!("invalid --keep {value}: want <min>:<max>:<radius>"));
    };
    let levels = (parse_level(minimum)?, parse_level(maximum)?);
    if levels.0 > levels.1 {
        return Err(format!("invalid --keep {value}: minimum level exceeds maximum"));
    }
    Ok(CarveRule {
        levels,
        radius: radius.parse::<u32>().map_err(|error| format!("invalid radius {radius}: {error}"))?,
    })
}

/// Every tile the rules ask for that the package actually has. A rule
/// asking beyond a package's coverage is not an error: the point of a
/// radius is that it may run off the edge of what was built.
fn carve_selection(
    centre: Lonlat,
    world: Option<u8>,
    rules: &[CarveRule],
    available: &serde_json::Map<String, serde_json::Value>,
) -> Vec<TileId> {
    let mut selected = available
        .keys()
        .filter_map(|path| parse_tile_path(path))
        .filter(|id| {
            world.is_some_and(|level| id.z <= level) || rules.iter().any(|rule| keeps(rule, centre, *id))
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|id| (id.z, id.x, id.y));
    selected
}

fn keeps(rule: &CarveRule, centre: Lonlat, id: TileId) -> bool {
    if id.z < rule.levels.0 || id.z > rule.levels.1 {
        return false;
    }
    let subject = locate(centre, id.z).tile;
    id.x.abs_diff(subject.x) <= rule.radius && id.y.abs_diff(subject.y) <= rule.radius
}

fn parse_tile_path(path: &str) -> Option<TileId> {
    let (z, rest) = path.split_once('/')?;
    let (x, y) = rest.split_once('/')?;
    Some(TileId {
        z: z.parse().ok()?,
        x: x.parse().ok()?,
        y: y.strip_suffix(".mt2")?.parse().ok()?,
    })
}

fn carved_levels(digests: &[TileDigest]) -> Vec<u8> {
    let mut levels = digests.iter().map(|digest| digest.id.z).collect::<Vec<_>>();
    levels.sort_unstable();
    levels.dedup();
    levels
}

/// The carved manifest keeps the source package's provenance verbatim —
/// a subset of tiles is still made of the same sources, and dropping
/// their licence and attribution is the one edit a carve must not make.
fn write_carved_manifest(
    output: &Path,
    source: &serde_json::Value,
    centre: Lonlat,
    carved: &CarvedTiles,
) -> Result<(), String> {
    let deepest = carved.levels.last().copied().unwrap_or_default();
    let value = json!({
        "format": "MT2",
        "format_version": source.get("format_version").cloned().unwrap_or_else(|| json!(maps2_tile::FORMAT_VERSION)),
        "levels": carved.levels,
        "feature_count": carved.features,
        "tile_count": carved.digests.len(),
        "tiles": tile_paths(&carved.digests),
        "tile_digests": digest_map(&carved.digests),
        "package_sha256": package_sha256(&carved.digests),
        // The subject the carve was centred on, not the middle of the
        // coarsest level: a city carve opens on its city.
        "view": { "lon": centre.lon, "lat": centre.lat, "zoom": deepest },
        // What the carve actually covers, per level, so a host can keep
        // a camera on ground the package can answer for. Computed here
        // because Mercator lives in maps2-units and nowhere else.
        "bounds": carved_bounds(&carved.digests),
        "height_tile_count": carved.height_tiles,
        "carved_from": source.get("package_sha256").cloned().unwrap_or(serde_json::Value::Null),
        "sources": source.get("sources").cloned().unwrap_or_else(|| json!([])),
    });
    let bytes = serde_json::to_string_pretty(&value)
        .map_err(|error| format!("cannot serialize manifest: {error}"))?;
    let path = output.join("manifest.json");
    fs::write(&path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn carved_bounds(digests: &[TileDigest]) -> BTreeMap<String, serde_json::Value> {
    let mut bounds = BTreeMap::new();
    for level in carved_levels(digests) {
        let ids = digests.iter().map(|digest| digest.id).filter(|id| id.z == level);
        let Some(first) = ids.clone().next() else { continue };
        let (min_x, max_x, min_y, max_y) = ids.fold(
            (first.x, first.x, first.y, first.y),
            |(min_x, max_x, min_y, max_y), id| {
                (min_x.min(id.x), max_x.max(id.x), min_y.min(id.y), max_y.max(id.y))
            },
        );
        // y grows southwards in tile space, so the north-west corner is
        // (min_x, min_y) and the south-east one is past (max_x, max_y).
        let north_west = to_lonlat(TilePoint {
            tile: TileId { z: level, x: min_x, y: min_y },
            coord: TileCoord(0, 0),
        });
        let south_east = to_lonlat(TilePoint {
            tile: TileId { z: level, x: max_x, y: max_y },
            coord: TileCoord(u16::MAX, u16::MAX),
        });
        bounds.insert(
            level.to_string(),
            json!({
                "west": north_west.lon,
                "north": north_west.lat,
                "east": south_east.lon,
                "south": south_east.lat,
            }),
        );
    }
    bounds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_carve_keeps_whole_world_levels_and_a_square_below_them() {
        let available = ["1/0/0.mt2", "2/1/1.mt2", "2/0/0.mt2", "8/127/85.mt2", "8/120/85.mt2"]
            .into_iter()
            .map(|path| (path.to_string(), json!("")))
            .collect::<serde_json::Map<_, _>>();
        let trafalgar = Lonlat { lon: -0.1281, lat: 51.508 };
        let rules = vec![CarveRule { levels: (8, 16), radius: 1 }];

        let selected = carve_selection(trafalgar, Some(2), &rules, &available);

        let paths = selected.iter().copied().map(tile_path).collect::<Vec<_>>();
        // Both z2 tiles: a world level is kept whole, wherever it is.
        assert!(paths.contains(&"2/0/0.mt2".to_string()));
        assert!(paths.contains(&"2/1/1.mt2".to_string()));
        // The z8 tile under the subject, but not the one seven tiles west.
        assert!(paths.contains(&"8/127/85.mt2".to_string()));
        assert!(!paths.contains(&"8/120/85.mt2".to_string()));
    }

    #[test]
    fn a_rule_reaching_past_the_package_is_not_an_error() {
        let available = [("8/127/85.mt2".to_string(), json!(""))].into_iter().collect();
        let rules = vec![CarveRule { levels: (8, 16), radius: 3 }];

        let selected =
            carve_selection(Lonlat { lon: -0.1281, lat: 51.508 }, None, &rules, &available);

        // Forty-nine tiles asked for, one of them built: the radius is a
        // request, not a claim about what exists.
        assert_eq!(selected.len(), 1);
    }

    #[test]
    fn carved_bounds_enclose_every_tile_of_their_level() {
        let digests = ["12/2045/1361.mt2", "12/2047/1363.mt2"]
            .into_iter()
            .map(|path| TileDigest { id: parse_tile_path(path).expect("path"), sha256: String::new() })
            .collect::<Vec<_>>();

        let bounds = carved_bounds(&digests);
        let box_ = bounds.get("12").expect("z12 bounds");
        let west = box_["west"].as_f64().expect("west");
        let east = box_["east"].as_f64().expect("east");
        let south = box_["south"].as_f64().expect("south");
        let north = box_["north"].as_f64().expect("north");

        assert!(west < east && south < north, "a box runs west to east and south to north");
        for digest in &digests {
            let centre = to_lonlat(TilePoint {
                tile: digest.id,
                coord: TileCoord(u16::MAX / 2, u16::MAX / 2),
            });
            assert!(centre.lon > west && centre.lon < east, "{} inside", tile_path(digest.id));
            assert!(centre.lat > south && centre.lat < north, "{} inside", tile_path(digest.id));
        }
    }

    #[test]
    fn a_carved_manifest_keeps_the_source_provenance() {
        let source = json!({
            "package_sha256": "abc",
            "format_version": 5,
            "sources": [{ "name": "greater-london.osm.pbf", "licence": "ODbL-1.0" }],
        });
        let carved = CarvedTiles {
            digests: vec![TileDigest {
                id: parse_tile_path("16/32744/21792.mt2").expect("path"),
                sha256: "f".repeat(64),
            }],
            levels: vec![16],
            features: 7,
            height_tiles: 1,
        };
        let output = std::env::temp_dir().join("maps2-carve-manifest-test");
        fs::create_dir_all(&output).expect("temp dir");

        write_carved_manifest(&output, &source, Lonlat { lon: -0.1281, lat: 51.508 }, &carved)
            .expect("manifest");

        let written: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(output.join("manifest.json")).expect("read"))
                .expect("json");
        assert_eq!(written["sources"], source["sources"], "licence and attribution survive");
        assert_eq!(written["carved_from"], json!("abc"));
        assert_eq!(written["view"]["zoom"], json!(16), "a carve opens on its subject");
        assert_eq!(written["feature_count"], json!(7));
        assert!(written["bounds"]["16"].is_object());
        fs::remove_dir_all(&output).ok();
    }

    #[test]
    fn manifest_carries_source_hash_and_attribution() {
        let descriptor = read_descriptor(
            r#"[source]
name = "london.osm.pbf"
kind = "osm-pbf"
url = "https://example.test/london.osm.pbf"
sha256 = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
source_date = "2026-08-14"
licence = "ODbL-1.0"
attribution = "© OpenStreetMap contributors"
bounds = [-0.6, 51.2, 0.4, 51.8]
adapter_version = "osm-v1""#,
        )
        .expect("descriptor");

        let mut tiles = vec![
            (TileId { z: 16, x: 32737, y: 21791 }, Vec::new()),
            (TileId { z: 16, x: 32736, y: 21791 }, Vec::new()),
        ];
        let digests = tile_digests(&tiles);
        let manifest = manifest_json(&[&descriptor], &[16], 10, &digests, 0).expect("manifest JSON");
        assert!(manifest.contains("\"format_version\": 5"));
        assert!(manifest.contains("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"));
        assert!(manifest.contains("© OpenStreetMap contributors"));

        let value = serde_json::from_str::<serde_json::Value>(&manifest).expect("manifest JSON");
        assert_eq!(value["tiles"], serde_json::json!(["16/32736/21791.mt2", "16/32737/21791.mt2"]));
        assert_eq!(value["levels"], serde_json::json!([16]));
        assert_eq!(value["view"]["zoom"], 16);
        assert_eq!(value["sources"][0]["bounds"], serde_json::json!([-0.6, 51.2, 0.4, 51.8]));
        assert_eq!(value["sources"][0]["adapter_version"], "osm-v1");
        assert_eq!(value["tile_digests"].as_object().expect("tile digests").len(), 2);
        assert_eq!(value["package_sha256"].as_str().expect("package hash").len(), 64);

        let first = package_sha256(&digests);
        tiles[0].1.push(1);
        assert_ne!(first, package_sha256(&tile_digests(&tiles)));
    }

    #[test]
    fn level_range_is_inclusive_and_ascending() {
        assert_eq!(parse_levels("12", "16").expect("valid range"), vec![12, 13, 14, 15, 16]);
        assert!(parse_levels("16", "12").is_err());
    }

    #[test]
    fn range_manifest_view_uses_the_lowest_package_level() {
        let digests = [
            TileDigest { id: TileId { z: 12, x: 2046, y: 1362 }, sha256: String::new() },
            TileDigest { id: TileId { z: 12, x: 2047, y: 1363 }, sha256: String::new() },
            TileDigest { id: TileId { z: 16, x: 32737, y: 21791 }, sha256: String::new() },
        ];

        let view = package_view(&digests).expect("package view");

        assert_eq!(view["zoom"], 12);
        assert!(view["lon"].as_f64().expect("longitude").abs() < 1.0);
        assert!(view["lat"].as_f64().expect("latitude") > 51.0);
    }

    #[test]
    fn source_download_uses_https_and_leaves_a_partial_file() {
        let output = Path::new("/tmp/london.osm.pbf.part");

        let arguments = fetch_arguments("https://example.test/london.osm.pbf", output).expect("HTTPS URL");

        assert_eq!(arguments, vec!["--fail", "--location", "--proto", "=https", "--output", "/tmp/london.osm.pbf.part", "https://example.test/london.osm.pbf"]);
        assert!(fetch_arguments("http://example.test/london.osm.pbf", output).is_err());
    }

    #[test]
    fn package_integrity_rejects_changed_tile_bytes() {
        let root = tempfile::tempdir().expect("temporary package");
        let path = "12/2047/1365.mt2";
        let tile = root.path().join(path);
        fs::create_dir_all(tile.parent().expect("tile parent")).expect("create tile parent");
        fs::write(&tile, b"original").expect("write tile");
        let digests = BTreeMap::from([(path.to_string(), sha256(b"original"))]);
        let package_hash = package_sha256_map(&digests);

        assert!(verify_package_contents(root.path(), &digests, &package_hash).is_ok());

        fs::write(tile, b"changed").expect("change tile");
        assert!(verify_package_contents(root.path(), &digests, &package_hash).is_err());
    }
}

/// One layer of a build plan: a source, the ground and levels it speaks
/// for, and how strongly.
#[derive(Debug, Deserialize)]
struct PlanLayer {
    descriptor: String,
    input: String,
    precedence: u8,
    levels: [u8; 2],
    /// Defaults to the descriptor's own bounds; set it to narrow a
    /// source to the ground it is actually good for.
    bounds: Option<[f64; 4]>,
}

/// A terrain raster: the same inputs the other commands take, named
/// rather than positional because a plan lists many.
#[derive(Debug, Deserialize)]
struct PlanTerrain {
    descriptor: String,
    input: String,
    /// GEBCO quadrants decimate by this; Copernicus tiles do not use it.
    stride: Option<u32>,
    west: Option<f64>,
    south: Option<f64>,
}

#[derive(Debug, Deserialize)]
struct BuildPlan {
    #[serde(default)]
    layer: Vec<PlanLayer>,
    #[serde(default)]
    terrain: Vec<PlanTerrain>,
}

struct LoadedLayer {
    descriptor: SourceDescriptor,
    input: String,
    claim: LayerClaim,
}

impl LoadedLayer {
    fn resolve(&self, level: u8) -> Result<Vec<PreparedFeature>, String> {
        if !self.claim.covers_level(level) {
            return Ok(Vec::new());
        }
        match self.descriptor.kind {
            SourceKind::WaterPolygons => {
                resolve_water_polygons(&self.input, level).map_err(|e| e.to_string())
            }
            SourceKind::NaturalEarthPlaces => {
                resolve_place_labels(&self.input, level).map_err(|e| e.to_string())
            }
            SourceKind::NaturalEarthBoundaries => {
                resolve_boundary_lines(&self.input, level).map_err(|e| e.to_string())
            }
            SourceKind::NaturalEarthRoads => {
                resolve_major_roads(&self.input, level).map_err(|e| e.to_string())
            }
            SourceKind::OsmPbf => resolve_osm_pbf(&self.input, level).map_err(|e| format!("{e:?}")),
            kind => Err(format!("{} is not a vector layer", source_kind_name(kind))),
        }
    }
}

/// Builds one package from many sources, reconciling their overlaps at
/// build time — see `maps2_ingest::conflate`. This is the command the
/// two-package composition became once it was clear that merging tiles
/// in the browser could not settle which source owned a given piece of
/// ground.
fn build_map(plan_path: &str, output: &str) -> Result<(), String> {
    let text = fs::read_to_string(plan_path)
        .map_err(|error| format!("cannot read {plan_path}: {error}"))?;
    let plan: BuildPlan =
        toml::from_str(&text).map_err(|error| format!("cannot parse {plan_path}: {error}"))?;
    let layers = load_plan_layers(&plan)?;
    let terrains = load_plan_terrains(&plan)?;
    let mut grids = terrains.iter().map(|input| input.grid.clone()).collect::<Vec<_>>();
    if let Ok(world) = stitch_world_quadrants(&grids) {
        grids.push(world);
    }
    let levels = claimed_levels(&layers.iter().map(|l| l.claim).collect::<Vec<_>>());
    if levels.is_empty() {
        return Err("a build plan needs at least one layer".to_string());
    }

    let output = Path::new(output);
    let mut totals = MapBuildTotals::default();
    let mut digests = Vec::new();
    for level in &levels {
        write_conflated_level(*level, &layers, &grids, output, &mut totals, &mut digests)?;
    }
    let (feature_count, height_tile_count) = (totals.features, totals.height_tiles);
    let (covered, matched) = (totals.covered, totals.matched);

    let mut descriptors: Vec<&SourceDescriptor> = layers.iter().map(|l| &l.descriptor).collect();
    descriptors.extend(terrains.iter().map(|t| &t.descriptor));
    write_manifest(output, &descriptors, &levels, feature_count, &digests, height_tile_count)?;
    println!(
        "{{\"features\":{feature_count},\"tiles\":{},\"height_tiles\":{height_tile_count},\"dropped_covered\":{covered},\"dropped_matched\":{matched}}}",
        digests.len()
    );
    Ok(())
}

#[derive(Default)]
struct MapBuildTotals {
    features: usize,
    height_tiles: usize,
    covered: usize,
    matched: usize,
}

/// Resolves every layer at one level, reconciles them, and writes the
/// tiles that come out.
fn write_conflated_level(
    level: u8,
    layers: &[LoadedLayer],
    grids: &[DemGrid],
    output: &Path,
    totals: &mut MapBuildTotals,
    digests: &mut Vec<TileDigest>,
) -> Result<(), String> {
    let mut sources = Vec::new();
    for layer in layers {
        sources.push(SourceLayer { claim: layer.claim, features: layer.resolve(level)? });
    }
    let (features, report) = conflate(level, sources);
    totals.covered += report.covered;
    totals.matched += report.matched;
    totals.features += features.len();
    let tiles = build_tiles_with_terrains(&features, grids)
        .map_err(|error| format!("cannot encode MT2: {error:?}"))?;
    totals.height_tiles +=
        tiles.iter().filter(|(tile, _)| grids.iter().any(|grid| grid.covers_tile(*tile))).count();
    digests.extend(tile_digests(&tiles));
    write_tiles(output, &tiles)
}

fn load_plan_layers(plan: &BuildPlan) -> Result<Vec<LoadedLayer>, String> {
    plan.layer
        .iter()
        .map(|layer| {
            let descriptor = load_descriptor(&layer.descriptor)?;
            validate_input(&descriptor, &layer.input)?;
            let claim = LayerClaim {
                precedence: layer.precedence,
                bounds: layer.bounds.unwrap_or(descriptor.bounds),
                min_level: layer.levels[0],
                max_level: layer.levels[1],
            };
            Ok(LoadedLayer { descriptor, input: layer.input.clone(), claim })
        })
        .collect()
}

fn load_plan_terrains(plan: &BuildPlan) -> Result<Vec<TerrainInput>, String> {
    plan.terrain
        .iter()
        .map(|terrain| {
            let descriptor = load_descriptor(&terrain.descriptor)?;
            validate_input(&descriptor, &terrain.input)?;
            let grid = match (terrain.stride, terrain.west, terrain.south) {
                (Some(stride), _, _) => {
                    load_gebco_quadrant_decimated(&terrain.input, descriptor.bounds, stride)
                        .map_err(|error| error.to_string())?
                }
                (None, Some(west), Some(south)) => {
                    load_copernicus_dem(&terrain.input, west, south).map_err(|e| e.to_string())?
                }
                _ => {
                    return Err(format!(
                        "terrain {} needs either a stride or a west/south corner",
                        terrain.input
                    ))
                }
            };
            Ok(TerrainInput { descriptor, grid })
        })
        .collect()
}
