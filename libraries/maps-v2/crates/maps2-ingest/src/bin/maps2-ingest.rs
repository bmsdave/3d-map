use std::{env, fs, fs::File, process::ExitCode};

use maps2_ingest::{read_descriptor, scan_osm_pbf, validate_source_reader, OsmSummary};

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
        _ => Err("usage: maps2-ingest scan <osm.pbf>".to_string()),
    }
}

fn verify(descriptor_path: &str, input_path: &str) -> Result<(), String> {
    let toml_text = fs::read_to_string(descriptor_path)
        .map_err(|error| format!("cannot read {descriptor_path}: {error}"))?;
    let descriptor = read_descriptor(&toml_text).map_err(|error| error.to_string())?;
    let input = File::open(input_path).map_err(|error| format!("cannot open {input_path}: {error}"))?;
    validate_source_reader(&descriptor.source, input).map_err(|error| error.to_string())?;
    println!("verified {}", descriptor.source.name());
    Ok(())
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
    println!("maps2-ingest\n\nusage:\n  maps2-ingest scan <osm.pbf>\n  maps2-ingest verify <source.toml> <input>");
}
