use std::{env, fs, fs::File, path::Path, process::ExitCode};

use maps2_ingest::{
    SourceDescriptor, SourceKind, build_tiles, load_copernicus_dem, read_descriptor, resolve_osm_pbf, scan_osm_pbf,
    validate_source_reader, OsmSummary,
};
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
    write_tiles(Path::new(output), &tiles)?;
    write_manifest(Path::new(output), &descriptor, level, features.len(), tiles.len())?;
    println!("{{\"features\":{},\"tiles\":{}}}", features.len(), tiles.len());
    Ok(())
}

fn write_manifest(
    output: &Path,
    descriptor: &SourceDescriptor,
    level: u8,
    feature_count: usize,
    tile_count: usize,
) -> Result<(), String> {
    let bytes = manifest_json(descriptor, level, feature_count, tile_count)?;
    let path = output.join("manifest.json");
    fs::write(&path, bytes).map_err(|error| format!("cannot write {}: {error}", path.display()))
}

fn manifest_json(
    descriptor: &SourceDescriptor,
    level: u8,
    feature_count: usize,
    tile_count: usize,
) -> Result<String, String> {
    serde_json::to_string_pretty(&json!({
        "format": "MT2",
        "format_version": 1,
        "level": level,
        "feature_count": feature_count,
        "tile_count": tile_count,
        "source": {
            "name": descriptor.source.name(),
            "kind": source_kind_name(descriptor.kind),
            "url": descriptor.url,
            "sha256": descriptor.source.sha256(),
            "source_date": descriptor.source_date,
            "licence": descriptor.licence,
            "attribution": descriptor.attribution,
        }
    }))
    .map_err(|error| format!("cannot serialize manifest: {error}"))
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
        "maps2-ingest\n\nusage:\n  maps2-ingest scan <osm.pbf>\n  maps2-ingest verify <source.toml> <input>\n  maps2-ingest build <source.toml> <osm.pbf> <level> <output-dir>\n  maps2-ingest dem-info <dem.tif> <west> <south>"
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

        let manifest = manifest_json(&descriptor, 16, 10, 2).expect("manifest JSON");
        assert!(manifest.contains("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"));
        assert!(manifest.contains("© OpenStreetMap contributors"));
    }
}
