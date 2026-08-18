use std::{collections::BTreeMap, env, fs, fs::File, path::{Path, PathBuf}, process::{Command, ExitCode}};

use maps2_ingest::{
    DemGrid, SourceDescriptor, SourceKind, build_tiles, build_tiles_with_terrains, load_copernicus_dem,
    load_gebco_quadrant_decimated, load_gebco_window, read_descriptor, resolve_osm_pbf, resolve_water_polygons,
    scan_osm_pbf, stitch_world_quadrants, validate_source, validate_source_reader, OsmSummary,
};
use maps2_units::{TileCoord, TileId, TilePoint, to_lonlat};
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
    let terrain = parse_world_terrain_inputs(terrain)?;
    let mut grids = terrain.iter().map(|input| input.grid.clone()).collect::<Vec<_>>();
    if let Ok(world) = stitch_world_quadrants(&grids) {
        grids.push(world);
    }
    let output = Path::new(output);
    let mut feature_count = 0;
    let mut height_tile_count = 0;
    let mut digests = Vec::new();
    for level in &levels {
        let features = resolve_water_polygons(shapefile_path, *level).map_err(|error| error.to_string())?;
        let tiles = build_tiles_with_terrains(&features, &grids)
            .map_err(|error| format!("cannot encode MT2: {error:?}"))?;
        height_tile_count += tiles.iter().filter(|(tile, _)| grids.iter().any(|grid| grid.covers_tile(*tile))).count();
        feature_count += features.len();
        digests.extend(tile_digests(&tiles));
        write_tiles(output, &tiles)?;
    }
    let mut sources = vec![&descriptor];
    sources.extend(terrain.iter().map(|input| &input.descriptor));
    write_manifest(output, &sources, &levels, feature_count, &digests, height_tile_count)?;
    println!("{{\"features\":{feature_count},\"tiles\":{},\"height_tiles\":{height_tile_count}}}", digests.len());
    Ok(())
}

struct WorldTerrainInput {
    descriptor: SourceDescriptor,
    grid: DemGrid,
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
        "maps2-ingest\n\nusage:\n  maps2-ingest scan <osm.pbf>\n  maps2-ingest verify <source.toml> <input>\n  maps2-ingest verify-package <package-dir>\n  maps2-ingest fetch <source.toml> <output>\n  maps2-ingest build <source.toml> <osm.pbf> <level> <output-dir>\n  maps2-ingest build-terrain <osm-source.toml> <osm.pbf> <dem-source.toml> <dem.tif> <west> <south> <level> <output-dir>\n  maps2-ingest build-terrain-many <osm-source.toml> <osm.pbf> <level> <output-dir> <dem-source.toml> <dem.tif> <west> <south>...\n  maps2-ingest build-terrain-range <osm-source.toml> <osm.pbf> <min-level> <max-level> <output-dir> <dem-source.toml> <dem.tif> <west> <south>...\n  maps2-ingest dem-info <dem.tif> <west> <south>\n  maps2-ingest gebco-window <source.toml> <grid.tif> <west> <south> <east> <north>\n  maps2-ingest build-world <water-source.toml> <water.shp> <min-level> <max-level> <output-dir> [<gebco-source.toml> <gebco.tif> <stride>]..."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

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
