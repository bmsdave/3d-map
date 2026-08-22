//! How much scratch disk a build needs, against what it produces.
//!
//! The number decides whether a planet fits: the spool holds a level's
//! features while that level is built. Measured on a city's worth of
//! streets it is 0.31 of the tile bytes, because the records deflate
//! about five to one — the same street name on every part of a road, and
//! coordinates that differ only in their low bits. Uncompressed it was
//! 1.39.
//!
//! The assertion is loose on purpose. It is here to catch the compression
//! being lost, not to pin a ratio that legitimately moves with the shape
//! of the data.
//!
//! `cargo test --release -p maps2-ingest --test spool_size -- --nocapture`
//! prints the numbers.

use std::fs;
use std::path::Path;

use maps2_ingest::{build_tiles_spooled, prepare_features, PreparedFeature, SpoolError};
use maps2_units::{Lonlat, TileId};

/// A city's worth of streets: many short named ways over a small area,
/// which is what a deep level of a real build is made of.
fn city_streets() -> Vec<PreparedFeature> {
    let mut prepared = Vec::new();
    let mut id = 1_u64;
    for row in 0..120 {
        for column in 0..120 {
            let lon = -0.20 + f64::from(column) * 0.0016;
            let lat = 51.44 + f64::from(row) * 0.0011;
            prepared.extend(prepare_features(
                id,
                &[("highway", "residential"), ("name", "Somewhere Street")],
                &[
                    Lonlat { lon, lat },
                    Lonlat { lon: lon + 0.0014, lat: lat + 0.0004 },
                    Lonlat { lon: lon + 0.0015, lat: lat + 0.0010 },
                ],
                14,
            ));
            id += 1;
        }
    }
    prepared
}

fn directory_bytes(dir: &Path) -> u64 {
    fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.metadata().map(|meta| meta.len()).unwrap_or_default())
        .sum()
}

#[test]
fn the_spool_costs_less_disk_than_the_tiles_it_builds() {
    let prepared = city_streets();
    let dir = tempfile::tempdir().expect("scratch");
    let spool_dir = dir.path().join("spool");
    let mut tile_bytes = 0_u64;
    let mut tiles = 0_u64;

    build_tiles_spooled(
        prepared.clone(),
        &[],
        maps2_ingest::TERRAIN_MAX_Z,
        &spool_dir,
        16,
        |_: TileId, bytes: Vec<u8>| -> Result<(), SpoolError> {
            tiles += 1;
            tile_bytes += u64::try_from(bytes.len()).unwrap_or_default();
            Ok(())
        },
    )
    .expect("spooled build");

    // The shards are all written before the first is read, and stay on
    // disk until the build is done, so their size now is the peak.
    let spool_bytes = directory_bytes(&spool_dir);

    // A report, not a computation: megabytes and a ratio, both fine as
    // approximations, which is what this conversion says out loud.
    #[allow(clippy::cast_precision_loss)]
    let megabytes = |bytes: u64| bytes as f64 / 1e6;
    println!("features prepared : {}", prepared.len());
    println!("tiles built       : {tiles}");
    println!("tile bytes        : {:.2} MB", megabytes(tile_bytes));
    println!("spool bytes       : {:.2} MB", megabytes(spool_bytes));
    println!("spool / tiles     : {:.2}x", megabytes(spool_bytes) / megabytes(tile_bytes.max(1)));

    assert!(
        spool_bytes < tile_bytes,
        "scratch is {spool_bytes} bytes against {tile_bytes} of tiles — deflated shards measured \
         0.31 of the tiles, so anything above 1.0 means the blocks are going down raw",
    );
}
