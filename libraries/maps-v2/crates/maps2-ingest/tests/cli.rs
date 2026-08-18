use std::{fs, path::Path, process::Command};

use maps2_ingest::{SourceKind, read_descriptor};

#[test]
fn help_describes_the_pbf_scan_command() {
    let output = Command::new(env!("CARGO_BIN_EXE_maps2-ingest"))
        .arg("--help")
        .output()
        .expect("run ingest command");

    assert!(output.status.success());
    let help = String::from_utf8_lossy(&output.stdout);
    assert!(help.contains("scan <osm.pbf>"));
    assert!(help.contains("verify <source.toml> <input>"));
    assert!(help.contains("build <source.toml> <osm.pbf> <level> <output-dir>"));
    assert!(help.contains("dem-info <dem.tif> <west> <south>"));
    assert!(help.contains("build-terrain <osm-source.toml> <osm.pbf> <dem-source.toml> <dem.tif> <west> <south> <level> <output-dir>"));
    assert!(help.contains("build-terrain-many <osm-source.toml> <osm.pbf> <level> <output-dir> <dem-source.toml> <dem.tif> <west> <south>..."));
    assert!(help.contains("gebco-window <source.toml> <grid.tif> <west> <south> <east> <north>"));
}

/// Every descriptor checked into the pipeline's `sources/` directory must
/// parse under the current schema, so a format change is caught here rather
/// than the first time someone runs a build against a stale descriptor.
#[test]
fn every_committed_source_descriptor_parses() {
    let sources_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../pipelines/maps-v2-ingest/sources");
    let mut checked = 0;
    for entry in fs::read_dir(&sources_dir).expect("read sources dir") {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("toml") {
            continue;
        }
        let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
        read_descriptor(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()));
        checked += 1;
    }
    assert!(checked > 0, "expected at least one committed descriptor in {}", sources_dir.display());
}

#[test]
fn the_gebco_descriptor_declares_the_gebco_grid_adapter() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../pipelines/maps-v2-ingest/sources/gebco-2025-n90-s0-w-90-e0.toml");
    let text = fs::read_to_string(&path).unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let descriptor = read_descriptor(&text).expect("gebco descriptor parses");
    assert_eq!(descriptor.kind, SourceKind::GebcoGrid);
    let [west, south, east, north] = descriptor.bounds;
    assert!((west - -90.0).abs() < f64::EPSILON);
    assert!((south - 0.0).abs() < f64::EPSILON);
    assert!((east - 0.0).abs() < f64::EPSILON);
    assert!((north - 90.0).abs() < f64::EPSILON);
}
