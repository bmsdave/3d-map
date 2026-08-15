use std::process::Command;

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
}
