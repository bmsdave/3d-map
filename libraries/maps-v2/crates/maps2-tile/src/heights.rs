//! The heights raster, plain and packed.
//!
//! 256×256 `u16` LE, row-major, metres offset by +11000 so GEBCO
//! bathymetry stays positive. The grid is inclusive of both tile edges
//! (sample `i` sits at `i / 255` of the tile), so neighbouring tiles
//! share their edge samples and a surface built from them has no seam.
//!
//! That raster is 128 KiB whatever it holds, which is most of a tile and
//! all of the reason a world pyramid of them costs terabytes. [`pack`]
//! is the same raster made small — losslessly, so what comes back out of
//! [`unpack`] is byte-identical to what went in, and every reading path
//! below still sees a plain [`HeightsRaster`].

use crate::TileError;
use num_traits::ToPrimitive;

/// Samples per side of the raster.
pub const HEIGHTS_SIDE: usize = 256;

/// Bytes of one heights section.
pub const HEIGHTS_BYTES: usize = HEIGHTS_SIDE * HEIGHTS_SIDE * 2;

/// Metres added before storing, so the deepest ocean stays positive.
pub const HEIGHT_OFFSET_M: f32 = 11_000.0;

/// Metres → wire value, clamped to the representable band.
#[must_use]
pub fn encode_height(metres: f32) -> u16 {
    let raw = (metres + HEIGHT_OFFSET_M).round();
    raw.clamp(0.0, 65535.0).to_u16().unwrap_or_default()
}

/// Wire value → metres.
#[must_use]
pub fn decode_height(raw: u16) -> f32 {
    f32::from(raw) - HEIGHT_OFFSET_M
}

/// A view over one heights section; borrows, copies nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeightsRaster<'a> {
    bytes: &'a [u8],
}

impl<'a> HeightsRaster<'a> {
    /// # Errors
    ///
    /// [`TileError::Truncated`] when the section is not exactly
    /// [`HEIGHTS_BYTES`] long — a short raster is corruption, not a
    /// smaller tile.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, TileError> {
        if bytes.len() == HEIGHTS_BYTES {
            Ok(Self { bytes })
        } else {
            Err(TileError::Truncated)
        }
    }

    /// Height in metres at a sample, coordinates clamped to the raster.
    #[must_use]
    pub fn metres(&self, x: i32, y: i32) -> f32 {
        let x = usize::try_from(x.max(0)).unwrap_or_default().min(HEIGHTS_SIDE - 1);
        let y = usize::try_from(y.max(0)).unwrap_or_default().min(HEIGHTS_SIDE - 1);
        let at = (y * HEIGHTS_SIDE + x) * 2;
        decode_height(u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]]))
    }

    #[must_use]
    pub fn bytes(&self) -> &'a [u8] {
        self.bytes
    }
}


/// Deflate level. Nine costs the ingest a little and every reader
/// nothing: a tile is compressed once, in a pipeline measured in hours,
/// and decompressed on every machine that ever draws it.
const PACK_LEVEL: u8 = 9;

/// Raster is smooth, so a sample is close to its neighbours: a predictor
/// turns "1483, 1484, 1486" into "1483, 1, 2" and hands the compressor
/// something with far less to say. This is the Paeth predictor PNG uses,
/// over `u16` samples rather than bytes.
fn predict(left: u16, above: u16, above_left: u16) -> u16 {
    let (a, b, c) = (i32::from(left), i32::from(above), i32::from(above_left));
    let p = a + b - c;
    let (pa, pb, pc) = ((p - a).abs(), (p - b).abs(), (p - c).abs());
    if pa <= pb && pa <= pc {
        left
    } else if pb <= pc {
        above
    } else {
        above_left
    }
}

/// The neighbours of a sample, with the raster's outside read as zero —
/// the same rule on both sides of the codec, which is the only thing
/// that has to be true about it.
fn neighbours(samples: &[u16], x: usize, y: usize) -> (u16, u16, u16) {
    let at = |x: usize, y: usize| samples[y * HEIGHTS_SIDE + x];
    let left = if x > 0 { at(x - 1, y) } else { 0 };
    let above = if y > 0 { at(x, y - 1) } else { 0 };
    let above_left = if x > 0 && y > 0 { at(x - 1, y - 1) } else { 0 };
    (left, above, above_left)
}

/// One raster, made small.
///
/// Three steps, each measured on the committed London carve: predict
/// (3.0×), split the residuals into a high-byte plane and a low-byte
/// plane so the compressor sees two runs of similar bytes instead of one
/// alternating run (3.7×), then deflate (3.7× all told, against 3.9× for
/// zstd — six percent that is not worth a C dependency inside wasm).
///
/// # Errors
///
/// [`TileError::Truncated`] when `raster` is not exactly
/// [`HEIGHTS_BYTES`] long.
pub fn pack(raster: &[u8]) -> Result<Vec<u8>, TileError> {
    if raster.len() != HEIGHTS_BYTES {
        return Err(TileError::Truncated);
    }
    let samples: Vec<u16> = raster
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    let mut high = Vec::with_capacity(HEIGHTS_SIDE * HEIGHTS_SIDE);
    let mut low = Vec::with_capacity(HEIGHTS_SIDE * HEIGHTS_SIDE);
    for y in 0..HEIGHTS_SIDE {
        for x in 0..HEIGHTS_SIDE {
            let (left, above, above_left) = neighbours(&samples, x, y);
            let residual =
                samples[y * HEIGHTS_SIDE + x].wrapping_sub(predict(left, above, above_left));
            high.push((residual >> 8) as u8);
            low.push((residual & 0xFF) as u8);
        }
    }
    high.append(&mut low);
    Ok(miniz_oxide::deflate::compress_to_vec(&high, PACK_LEVEL))
}

/// One packed raster, back to the 128 KiB the readers expect.
///
/// # Errors
///
/// [`TileError::BadRaster`] when the payload does not inflate to exactly
/// one raster's worth of residuals — a short or corrupt section is
/// corruption, not a smaller tile.
pub fn unpack(packed: &[u8]) -> Result<Vec<u8>, TileError> {
    let planes = miniz_oxide::inflate::decompress_to_vec_with_limit(packed, HEIGHTS_BYTES)
        .map_err(|_| TileError::BadRaster)?;
    if planes.len() != HEIGHTS_BYTES {
        return Err(TileError::BadRaster);
    }
    let (high, low) = planes.split_at(HEIGHTS_SIDE * HEIGHTS_SIDE);
    let mut samples = vec![0_u16; HEIGHTS_SIDE * HEIGHTS_SIDE];
    let mut out = vec![0_u8; HEIGHTS_BYTES];
    for y in 0..HEIGHTS_SIDE {
        for x in 0..HEIGHTS_SIDE {
            let at = y * HEIGHTS_SIDE + x;
            let residual = (u16::from(high[at]) << 8) | u16::from(low[at]);
            let (left, above, above_left) = neighbours(&samples, x, y);
            let sample = residual.wrapping_add(predict(left, above, above_left));
            samples[at] = sample;
            out[at * 2..at * 2 + 2].copy_from_slice(&sample.to_le_bytes());
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON, "{actual} != {expected}");
    }

    #[test]
    fn the_offset_puts_sea_level_in_the_middle_and_keeps_trenches_positive() {
        assert_eq!(encode_height(0.0), 11_000);
        assert_close(decode_height(11_000), 0.0);
        assert_close(decode_height(encode_height(8848.0)), 8848.0);
        // Challenger Deep, the reason for the offset.
        assert_close(decode_height(encode_height(-10_935.0)), -10_935.0);
        // Out of band saturates instead of wrapping into a mountain.
        assert_eq!(encode_height(-20_000.0), 0);
        assert_eq!(encode_height(90_000.0), 65_535);
    }

    #[test]
    fn a_raster_of_the_wrong_length_is_an_error() {
        assert_eq!(HeightsRaster::parse(&[]), Err(TileError::Truncated));
        assert_eq!(
            HeightsRaster::parse(&vec![0_u8; HEIGHTS_BYTES - 1]),
            Err(TileError::Truncated),
        );
        assert!(HeightsRaster::parse(&vec![0_u8; HEIGHTS_BYTES]).is_ok());
    }

    #[test]
    fn samples_are_row_major_and_edges_clamp() {
        let mut bytes = vec![0_u8; HEIGHTS_BYTES];
        let put = |bytes: &mut Vec<u8>, x: usize, y: usize, metres: f32| {
            let at = (y * HEIGHTS_SIDE + x) * 2;
            bytes[at..at + 2].copy_from_slice(&encode_height(metres).to_le_bytes());
        };
        put(&mut bytes, 1, 0, 100.0);
        put(&mut bytes, 0, 1, 200.0);
        put(&mut bytes, 255, 255, 300.0);
        let raster = HeightsRaster::parse(&bytes).expect("full length");
        assert_close(raster.metres(1, 0), 100.0);
        assert_close(raster.metres(0, 1), 200.0);
        assert_close(raster.metres(255, 255), 300.0);
        // Off the edge reads the edge, never panics — the hillshade
        // gradient asks for neighbours of border samples every frame.
        assert_close(raster.metres(-1, -1), raster.metres(0, 0));
        assert_close(raster.metres(999, 999), raster.metres(255, 255));
    }

    /// A raster shaped like real ground: smooth, with a ridge across it.
    fn ridge_raster() -> Vec<u8> {
        let mut bytes = vec![0_u8; HEIGHTS_BYTES];
        for y in 0..HEIGHTS_SIDE {
            for x in 0..HEIGHTS_SIDE {
                #[allow(clippy::cast_precision_loss)]
                let across = (x as f32 - 128.0) / 128.0;
                #[allow(clippy::cast_precision_loss)]
                let along = y as f32 / 255.0;
                let metres = 900.0 * (1.0 - across * across) + 40.0 * along;
                let at = (y * HEIGHTS_SIDE + x) * 2;
                bytes[at..at + 2].copy_from_slice(&encode_height(metres).to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn packing_a_raster_and_unpacking_it_returns_the_same_bytes() {
        for raster in [ridge_raster(), vec![0_u8; HEIGHTS_BYTES], vec![0xAB_u8; HEIGHTS_BYTES]] {
            let packed = pack(&raster).expect("packs");
            assert_eq!(unpack(&packed).expect("unpacks"), raster);
        }
    }

    /// Every value the wire can hold, including the wrap the predictor's
    /// residuals rely on: a raster that alternates between the deepest
    /// trench and the highest mountain is not smooth, and must still come
    /// back exactly.
    #[test]
    fn the_round_trip_survives_values_the_predictor_cannot_help_with() {
        let mut bytes = vec![0_u8; HEIGHTS_BYTES];
        for (index, pair) in bytes.chunks_exact_mut(2).enumerate() {
            let value = if index % 2 == 0 { u16::MIN } else { u16::MAX };
            pair.copy_from_slice(&value.to_le_bytes());
        }
        let packed = pack(&bytes).expect("packs");
        assert_eq!(unpack(&packed).expect("unpacks"), bytes);
    }

    /// The whole point, stated as a test: ground-shaped data gets much
    /// smaller. The floor is deliberately loose — this guards against the
    /// codec silently becoming a no-op, not against a percent of drift.
    #[test]
    fn ground_shaped_raster_packs_to_a_fraction_of_its_size() {
        let packed = pack(&ridge_raster()).expect("packs");
        assert!(
            packed.len() * 4 < HEIGHTS_BYTES,
            "packed {} of {HEIGHTS_BYTES} bytes",
            packed.len(),
        );
    }

    #[test]
    fn a_raster_of_the_wrong_length_cannot_be_packed() {
        assert_eq!(pack(&[]), Err(TileError::Truncated));
        assert_eq!(pack(&vec![0_u8; HEIGHTS_BYTES + 1]), Err(TileError::Truncated));
    }

    #[test]
    fn a_payload_that_is_not_a_whole_raster_is_an_error_not_a_panic() {
        assert_eq!(unpack(&[]), Err(TileError::BadRaster));
        assert_eq!(unpack(&[0xFF, 0x00, 0x13]), Err(TileError::BadRaster));
        // Valid deflate, wrong length: half a raster is corruption.
        let half = miniz_oxide::deflate::compress_to_vec(&vec![0_u8; HEIGHTS_BYTES / 2], PACK_LEVEL);
        assert_eq!(unpack(&half), Err(TileError::BadRaster));
    }
}
