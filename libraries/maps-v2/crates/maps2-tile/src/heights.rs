//! The heights raster — the one raster section format v1 defines.
//!
//! 256×256 `u16` LE, row-major, metres offset by +11000 so GEBCO
//! bathymetry stays positive. The grid is inclusive of both tile edges
//! (sample `i` sits at `i / 255` of the tile), so neighbouring tiles
//! share their edge samples and a surface built from them has no seam.

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
}
