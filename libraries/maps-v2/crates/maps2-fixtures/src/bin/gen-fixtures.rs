//! Writes the fixture packages as files the lab serves statically:
//! `<out>/<pack>/<z>/<x>/<y>.mt2`, with `manifest.json` listing the
//! tiles and `centre.json` saying where a camera has to sit to see
//! them — so the lab never repeats Mercator in TypeScript.

use std::{env, fs, path::Path, path::PathBuf, process};

use maps2_fixtures::{
    ealing_tiles, ridge_tiles, roads_centre, roads_tiles, EALING, RIDGE_DETAIL_LEVEL, ROADS_ZOOM,
};
use maps2_units::{Lonlat, TileId};
use sha2::{Digest, Sha256};

fn main() {
    let Some(out) = env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen-fixtures <out-dir>");
        process::exit(2);
    };
    write_pack(&out.join("ealing"), &ealing_tiles(), EALING, 12.0);
    write_pack(&out.join("roads"), &roads_tiles(), roads_centre(), ROADS_ZOOM);
    write_pack(&out.join("ridge"), &ridge_tiles(), EALING, f64::from(RIDGE_DETAIL_LEVEL));
}

fn write_pack(out: &Path, tiles: &[(TileId, Vec<u8>)], centre: Lonlat, zoom: f64) {
    for (id, bytes) in tiles {
        let dir = out.join(id.z.to_string()).join(id.x.to_string());
        fs::create_dir_all(&dir).expect("create fixture dir");
        fs::write(dir.join(format!("{}.mt2", id.y)), bytes).expect("write tile");
    }
    let manifest: Vec<String> =
        tiles.iter().map(|(id, _)| format!("{}/{}/{}", id.z, id.x, id.y)).collect();
    fs::write(out.join("manifest.json"), format!("{manifest:?}")).expect("write manifest");
    fs::write(out.join("package-manifest.json"), package_manifest(tiles, centre, zoom))
        .expect("write package manifest");
    fs::write(
        out.join("centre.json"),
        format!(
            "{{\"lon\":{},\"lat\":{},\"zoom\":{zoom}}}",
            centre.lon, centre.lat
        ),
    )
    .expect("write centre");
    println!("{} tiles → {}", tiles.len(), out.display());
}

fn package_manifest(tiles: &[(TileId, Vec<u8>)], centre: Lonlat, zoom: f64) -> String {
    let mut paths = tiles
        .iter()
        .map(|(id, _)| format!("\"{}/{}/{}.mt2\"", id.z, id.x, id.y))
        .collect::<Vec<_>>();
    paths.sort();
    let mut levels = tiles.iter().map(|(id, _)| id.z).collect::<Vec<_>>();
    levels.sort_unstable();
    levels.dedup();
    let mut digests = tiles
        .iter()
        .map(|(id, bytes)| (format!("{}/{}/{}.mt2", id.z, id.x, id.y), format!("{:x}", Sha256::digest(bytes))))
        .collect::<Vec<_>>();
    digests.sort_by(|left, right| left.0.cmp(&right.0));
    let digests = digests
        .iter()
        .map(|(path, digest)| format!("{path:?}:{digest:?}"))
        .collect::<Vec<_>>();
    format!(
        "{{\"format\":\"MT2\",\"format_version\":{},\"levels\":{:?},\"tiles\":[{}],\"tile_digests\":{{{}}},\"view\":{{\"lon\":{},\"lat\":{},\"zoom\":{zoom}}},\"sources\":[{{\"name\":\"Synthetic fixture\",\"attribution\":\"Synthetic fixture\",\"licence\":\"MIT\"}}]}}",
        maps2_tile::FORMAT_VERSION,
        levels,
        paths.join(","),
        digests.join(","),
        centre.lon,
        centre.lat,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_manifest_lists_relative_tiles_and_a_default_view() {
        let tiles = [(TileId { z: 16, x: 32736, y: 21791 }, Vec::new())];

        let manifest = package_manifest(&tiles, EALING, 16.0);

        assert!(manifest.contains("\"format\":\"MT2\""));
        assert!(manifest.contains("\"tiles\":[\"16/32736/21791.mt2\"]"));
        assert!(manifest.contains("\"tile_digests\""));
        assert!(manifest.contains("\"zoom\":16"));
    }
}
