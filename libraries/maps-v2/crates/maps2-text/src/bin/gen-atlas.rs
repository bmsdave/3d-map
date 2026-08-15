//! Writes the glyph atlas out for inspection: `atlas.png` plus
//! `atlas.json` with the metrics.
//!
//! The renderer does not read these files — it builds the very same
//! bytes in process, deterministically. They exist so the field can be
//! looked at, diffed and argued about, which is the whole point of an
//! offline generator.

use std::{env, fs, path::PathBuf, process};

use maps2_text::{Atlas, ATLAS_CELL_PX, ATLAS_COLUMNS, CELL_ORIGIN_PX, EM_PX, SDF_RANGE_PX};

fn main() {
    let Some(out) = env::args().nth(1).map(PathBuf::from) else {
        eprintln!("usage: gen-atlas <out-dir>");
        process::exit(2);
    };
    fs::create_dir_all(&out).expect("create atlas dir");
    let atlas = Atlas::build();
    fs::write(out.join("atlas.png"), png_grey(atlas.width, atlas.height, &atlas.pixels))
        .expect("write atlas.png");
    fs::write(out.join("atlas.json"), metrics_json(&atlas)).expect("write atlas.json");
    println!(
        "{} glyphs, {}×{} px → {}",
        atlas.glyphs().len(),
        atlas.width,
        atlas.height,
        out.display(),
    );
}

fn metrics_json(atlas: &Atlas) -> String {
    let glyphs: Vec<String> = atlas
        .glyphs()
        .iter()
        .map(|g| {
            format!(
                "{{\"char\":{:?},\"cell\":{},\"advance\":{:.5}}}",
                g.ch.to_string(),
                g.cell,
                g.advance,
            )
        })
        .collect();
    format!(
        "{{\"width\":{},\"height\":{},\"cell_px\":{},\"columns\":{},\"em_px\":{},\
         \"sdf_range_px\":{},\"cell_origin_px\":[{},{}],\"glyphs\":[{}]}}",
        atlas.width,
        atlas.height,
        ATLAS_CELL_PX,
        ATLAS_COLUMNS,
        EM_PX,
        SDF_RANGE_PX,
        CELL_ORIGIN_PX.0,
        CELL_ORIGIN_PX.1,
        glyphs.join(","),
    )
}

/// An 8-bit greyscale PNG with stored (uncompressed) deflate blocks.
/// A hundred kilobytes on disk buys not depending on a zlib crate for a
/// file nothing ships.
fn png_grey(width: u32, height: u32, pixels: &[u8]) -> Vec<u8> {
    let mut raw = Vec::with_capacity(pixels.len() + height as usize);
    for row in 0..height as usize {
        raw.push(0); // filter: none
        raw.extend_from_slice(&pixels[row * width as usize..(row + 1) * width as usize]);
    }
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 0, 0, 0, 0]); // 8-bit greyscale
    push_chunk(&mut png, b"IHDR", &ihdr);
    push_chunk(&mut png, b"IDAT", &zlib_stored(&raw));
    push_chunk(&mut png, b"IEND", &[]);
    png
}

fn push_chunk(out: &mut Vec<u8>, kind: &[u8; 4], body: &[u8]) {
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(kind);
    out.extend_from_slice(body);
    let mut crc_input = kind.to_vec();
    crc_input.extend_from_slice(body);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn zlib_stored(data: &[u8]) -> Vec<u8> {
    let mut out = vec![0x78, 0x01];
    for (i, block) in data.chunks(65535).enumerate() {
        let last = u8::from((i + 1) * 65535 >= data.len());
        out.push(last);
        out.extend_from_slice(&(block.len() as u16).to_le_bytes());
        out.extend_from_slice(&(!(block.len() as u16)).to_le_bytes());
        out.extend_from_slice(block);
    }
    out.extend_from_slice(&adler32(data).to_be_bytes());
    out
}

fn adler32(data: &[u8]) -> u32 {
    let (mut a, mut b) = (1_u32, 0_u32);
    for byte in data {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFF_u32;
    for byte in data {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = if crc & 1 == 1 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}
