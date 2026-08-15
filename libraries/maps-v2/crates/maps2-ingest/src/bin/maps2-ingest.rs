use std::{env, fs, fs::File, path::Path, process::ExitCode};

use maps2_ingest::{
    SourceDescriptor, SourceKind, build_tiles, build_tiles_with_terrains, load_copernicus_dem, read_descriptor,
    resolve_osm_pbf, scan_osm_pbf, validate_source_reader, OsmSummary,
};
use maps2_units::{TileCoord, TileId, TilePoint, to_lonlat};
use serde_json::json;

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
        [command, descriptor, input, level, output] if command == "build" => {
            build(descriptor, input, level, output)
        }
        [command, args @ ..] if command == "build-terrain-many" => build_terrain_many(args),
        [command, args @ ..] if command == "build-terrain-range" => build_terrain_range(args),
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
    let ids = tile_ids(&tiles);
    write_tiles(Path::new(output), &tiles)?;
    write_manifest(Path::new(output), &[&descriptor], &[level], features.len(), &ids, 0)?;
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
    let mut ids = Vec::new();
    for level in levels {
        let features = resolve_osm_pbf(osm_input, *level).map_err(|error| error.to_string())?;
        let tiles = build_tiles_with_terrains(&features, &grids).map_err(|error| format!("cannot encode MT2: {error:?}"))?;
        height_tile_count += tiles.iter().filter(|(tile, _)| grids.iter().any(|grid| grid.covers_tile(*tile))).count();
        feature_count += features.len();
        ids.extend(tile_ids(&tiles));
        write_tiles(output, &tiles)?;
    }
    let mut sources = vec![osm];
    sources.extend(terrain.iter().map(|input| &input.descriptor));
    write_manifest(output, &sources, levels, feature_count, &ids, height_tile_count)?;
    println!("{{\"features\":{feature_count},\"tiles\":{},\"height_tiles\":{height_tile_count}}}", ids.len());
    Ok(())
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
    tile_ids: &[TileId],
    height_tile_count: usize,
) -> Result<(), String> {
    let bytes = manifest_json(descriptors, levels, feature_count, tile_ids, height_tile_count)?;
    let path = output.join("manifest.json");
    fs::write(&path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn manifest_json(
    descriptors: &[&SourceDescriptor],
    levels: &[u8],
    feature_count: usize,
    tile_ids: &[TileId],
    height_tile_count: usize,
) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "format": "MT2",
        "format_version": maps2_tile::FORMAT_VERSION,
        "levels": levels,
        "feature_count": feature_count,
        "tile_count": tile_ids.len(),
        "tiles": tile_paths(tile_ids),
        "view": package_view(tile_ids),
        "height_tile_count": height_tile_count,
        "sources": descriptors.iter().map(|descriptor| json!({
            "name": descriptor.source.name(),
            "kind": source_kind_name(descriptor.kind),
            "url": descriptor.url,
            "sha256": descriptor.source.sha256(),
            "source_date": descriptor.source_date,
            "licence": descriptor.licence,
            "attribution": descriptor.attribution,
        })).collect::<Vec<_>>(),
    }))
    .map_err(|error| format!("cannot serialize manifest: {error}"))
}

fn tile_ids(tiles: &[(TileId, Vec<u8>)]) -> Vec<TileId> {
    tiles.iter().map(|(id, _)| *id).collect()
}

fn tile_paths(tile_ids: &[TileId]) -> Vec<String> {
    let mut ids = tile_ids.to_vec();
    ids.sort_by_key(|id| (id.z, id.x, id.y));
    ids.into_iter().map(|id| format!("{}/{}/{}.mt2", id.z, id.x, id.y)).collect()
}

fn package_view(tile_ids: &[TileId]) -> Option<serde_json::Value> {
    let first = *tile_ids.first()?;
    let (min_x, max_x, min_y, max_y) = tile_ids.iter().fold(
        (first.x, first.x, first.y, first.y),
        |(min_x, max_x, min_y, max_y), tile| {
            (min_x.min(tile.x), max_x.max(tile.x), min_y.min(tile.y), max_y.max(tile.y))
        },
    );
    let centre = to_lonlat(TilePoint {
        tile: TileId { z: first.z, x: min_x + (max_x - min_x) / 2, y: min_y + (max_y - min_y) / 2 },
        coord: TileCoord(u16::MAX / 2, u16::MAX / 2),
    });
    Some(json!({ "lon": centre.lon, "lat": centre.lat, "zoom": first.z }))
}

const fn source_kind_name(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::OsmPbf => "osm-pbf",
        SourceKind::CopernicusDem => "copernicus-dem",
        SourceKind::GebcoGrid => "gebco-grid",
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

fn load_descriptor(path: &str) -> Result<maps2_ingest::SourceDescriptor, String> {
    let toml_text = fs::read_to_string(path).map_err(|error| format!("cannot read {path}: {error}"))?;
    read_descriptor(&toml_text).map_err(|error| error.to_string())
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
        "maps2-ingest\n\nusage:\n  maps2-ingest scan <osm.pbf>\n  maps2-ingest verify <source.toml> <input>\n  maps2-ingest build <source.toml> <osm.pbf> <level> <output-dir>\n  maps2-ingest build-terrain <osm-source.toml> <osm.pbf> <dem-source.toml> <dem.tif> <west> <south> <level> <output-dir>\n  maps2-ingest build-terrain-many <osm-source.toml> <osm.pbf> <level> <output-dir> <dem-source.toml> <dem.tif> <west> <south>...\n  maps2-ingest build-terrain-range <osm-source.toml> <osm.pbf> <min-level> <max-level> <output-dir> <dem-source.toml> <dem.tif> <west> <south>...\n  maps2-ingest dem-info <dem.tif> <west> <south>"
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
attribution = "© OpenStreetMap contributors""#,
        )
        .expect("descriptor");

        let tiles = vec![
            (TileId { z: 16, x: 32737, y: 21791 }, Vec::new()),
            (TileId { z: 16, x: 32736, y: 21791 }, Vec::new()),
        ];
        let manifest = manifest_json(&[&descriptor], &[16], 10, &tile_ids(&tiles), 0).expect("manifest JSON");
        assert!(manifest.contains("\"format_version\": 2"));
        assert!(manifest.contains("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"));
        assert!(manifest.contains("© OpenStreetMap contributors"));

        let value = serde_json::from_str::<serde_json::Value>(&manifest).expect("manifest JSON");
        assert_eq!(value["tiles"], serde_json::json!(["16/32736/21791.mt2", "16/32737/21791.mt2"]));
        assert_eq!(value["levels"], serde_json::json!([16]));
        assert_eq!(value["view"]["zoom"], 16);
    }

    #[test]
    fn level_range_is_inclusive_and_ascending() {
        assert_eq!(parse_levels("12", "16").expect("valid range"), vec![12, 13, 14, 15, 16]);
        assert!(parse_levels("16", "12").is_err());
    }
}
