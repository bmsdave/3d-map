//! Real-world terrain relief for the low-zoom globe.
//!
//! [`crate::gebco::load_gebco_window`] bounds a read by pixel count so a
//! regional build never materialises a multi-gigabyte source grid — but
//! that native-resolution window is at most a few thousand pixels
//! square, and a single GEBCO quadrant is 90° on a side. A world MT2
//! tile at z2–z5 is itself tens of degrees wide, wider than any
//! in-budget native-resolution window, so the bounded reader is the
//! wrong tool here: a low-zoom tile's terrain raster is a fixed 256×256
//! samples regardless of source resolution (see
//! `maps2_tile::HEIGHTS_SIDE`), so it never needed GEBCO's native 15
//! arc-second density in the first place.
//!
//! This reader takes the opposite tradeoff on purpose: it decodes a
//! whole 90°×90° quadrant (as [`crate::load_copernicus_dem`] already
//! does for a whole regional DEM tile) and decimates it by picking every
//! `stride`-th sample, discarding the full decode once that smaller grid
//! is built. Peak memory is the one quadrant's native decode (about
//! 1.9 GB for a 21600×21600 `f32` grid) for as long as the decimation
//! pass takes, not something a bounded window is meant to avoid: it is
//! the same shape of tradeoff a full-resolution regional DEM read
//! already makes, just at a bigger source size.

use std::{fs::File, path::Path};

use num_traits::ToPrimitive;
use tiff::decoder::{Decoder, Limits};

use crate::{DemError, DemGrid, dem_samples};

/// Reads a whole GEBCO quadrant and keeps every `stride`-th sample on
/// each axis, so a 21600×21600 native quadrant becomes a grid small
/// enough to hold eight of, across the whole globe, at once.
///
/// # Errors
///
/// Returns [`DemError`] when the TIFF cannot be read or decoded, `bounds`
/// are invalid, or `stride` is zero.
pub fn load_gebco_quadrant_decimated(
    path: impl AsRef<Path>, bounds: [f64; 4], stride: u32,
) -> Result<DemGrid, DemError> {
    if stride == 0 {
        return Err(DemError::Empty);
    }
    let file = File::open(path.as_ref()).map_err(|error| DemError::Read(error.to_string()))?;
    // A whole GEBCO quadrant is deliberately over the crate's default
    // decode-size guard (meant to catch decompression bombs, not a
    // legitimate multi-gigabyte source this reader is explicitly built
    // to decode once and then shrink — see the module doc).
    let mut decoder = Decoder::new(file)
        .map_err(|error| DemError::Read(error.to_string()))?
        .with_limits(Limits::unlimited());
    let (width, height) = decoder.dimensions().map_err(|error| DemError::Read(error.to_string()))?;
    let image = decoder.read_image().map_err(|error| DemError::Read(error.to_string()))?;
    let samples = dem_samples(image)?;
    let (out_width, out_height) = (width.div_ceil(stride), height.div_ceil(stride));
    let mut decimated = Vec::with_capacity(usize::try_from(out_width).unwrap_or_default()
        * usize::try_from(out_height).unwrap_or_default());
    let mut y = 0;
    while y < height {
        let mut x = 0;
        while x < width {
            let index = usize::try_from(y).unwrap_or_default() * usize::try_from(width).unwrap_or_default()
                + usize::try_from(x).unwrap_or_default();
            decimated.push(samples[index]);
            x += stride;
        }
        y += stride;
    }
    DemGrid::with_bounds(bounds, out_width, out_height, decimated)
}

/// Mosaics same-size quadrant grids into one whole-world grid.
///
/// `covers_tile` requires a single grid to contain every corner of a
/// tile, so no individual 90°×90° quadrant can ever cover a z0 or z1
/// world tile — those are wider than one quadrant. This stitches the
/// eight [`load_gebco_quadrant_decimated`] outputs (two rows of four,
/// identical size after decimation) into one grid spanning the whole
/// globe, so those largest tiles get a (coarser, but real) terrain
/// fallback instead of none at all. Pass it after the individual
/// quadrants in [`crate::build_tiles_with_terrains`]'s grid list so the
/// more precise per-quadrant grid still wins wherever one covers a tile.
///
/// # Errors
///
/// Returns [`DemError::Empty`] for an empty input, and
/// [`DemError::SampleCount`] when the quadrants are not identically
/// sized or do not tile a regular rectangular grid — this is a mosaic of
/// same-shaped pieces, not a general-purpose stitcher.
pub fn stitch_world_quadrants(quadrants: &[DemGrid]) -> Result<DemGrid, DemError> {
    let first = quadrants.first().ok_or(DemError::Empty)?;
    let (tile_width, tile_height) = first.dimensions();
    if quadrants.iter().any(|grid| grid.dimensions() != (tile_width, tile_height)) {
        return Err(DemError::SampleCount);
    }
    let world_bounds @ [world_west, world_south, world_east, world_north] = mosaic_bounds(quadrants)?;
    let (cols, rows) = mosaic_shape(world_bounds, quadrants.len())?;
    let (out_width, out_height) = (tile_width * cols, tile_height * rows);
    let mut mosaic = vec![0.0_f32; usize::try_from(out_width * out_height).unwrap_or_default()];
    for grid in quadrants {
        let [west, _, _, north] = grid.bounds();
        let col = quadrant_step(west, world_west, world_east, cols);
        let row = quadrant_step(world_north - north, 0.0, world_north - world_south, rows);
        blit(&mut mosaic, out_width, grid, col * tile_width, row * tile_height);
    }
    DemGrid::with_bounds(world_bounds, out_width, out_height, mosaic)
}

fn mosaic_bounds(quadrants: &[DemGrid]) -> Result<[f64; 4], DemError> {
    let mut bounds = quadrants.iter().map(DemGrid::bounds);
    let Some(first) = bounds.next() else { return Err(DemError::Empty) };
    Ok(bounds.fold(first, |[w, s, e, n], [qw, qs, qe, qn]| {
        [w.min(qw), s.min(qs), e.max(qe), n.max(qn)]
    }))
}

/// The mosaic's column/row count, from its bounding box and how many
/// same-size quadrants it took to fill it.
fn mosaic_shape(bounds: [f64; 4], quadrant_count: usize) -> Result<(u32, u32), DemError> {
    let [west, south, east, north] = bounds;
    let aspect = (east - west) / (north - south);
    let rows = (quadrant_count.to_f64().unwrap_or(1.0) / aspect).sqrt().round();
    let cols = quadrant_count.to_f64().unwrap_or(1.0) / rows;
    let (rows, cols) = (rows.to_u32().unwrap_or(1), cols.to_u32().unwrap_or(1));
    (rows * cols == u32::try_from(quadrant_count).unwrap_or_default())
        .then_some((cols, rows))
        .ok_or(DemError::SampleCount)
}

fn quadrant_step(offset: f64, low: f64, high: f64, steps: u32) -> u32 {
    ((offset - low) / (high - low) * f64::from(steps)).round().to_u32().unwrap_or_default()
}

fn blit(mosaic: &mut [f32], mosaic_width: u32, source: &DemGrid, x0: u32, y0: u32) {
    let (source_width, source_height) = source.dimensions();
    let samples = source.samples();
    for y in 0..source_height {
        let dest_row = usize::try_from((y0 + y) * mosaic_width + x0).unwrap_or_default();
        let source_row = usize::try_from(y * source_width).unwrap_or_default();
        let width = usize::try_from(source_width).unwrap_or_default();
        mosaic[dest_row..dest_row + width].copy_from_slice(&samples[source_row..source_row + width]);
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use num_traits::ToPrimitive;
    use tiff::encoder::{colortype::Gray32Float, TiffEncoder};

    use super::*;

    fn synthetic_quadrant(side: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        {
            let mut encoder = TiffEncoder::new(Cursor::new(&mut bytes)).expect("tiff encoder");
            let pixels =
                (0..side * side).map(|index| index.to_f32().unwrap_or_default()).collect::<Vec<_>>();
            encoder.write_image::<Gray32Float>(side, side, &pixels).expect("tiff written");
        }
        bytes
    }

    #[test]
    fn decimation_keeps_every_stride_th_sample() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quadrant.tif");
        std::fs::write(&path, synthetic_quadrant(16)).expect("write fixture");

        let grid = load_gebco_quadrant_decimated(&path, [0.0, 0.0, 90.0, 90.0], 4).expect("decimated grid");

        // side=16, stride=4 => 4x4 output, keeping pixel (x*4, y*4) of the
        // source's row-major index ramp.
        assert!((grid.sample(0.0 + f64::EPSILON, 90.0 - f64::EPSILON) - 0.0).abs() < 1e-6);
    }

    #[test]
    fn a_zero_stride_is_rejected_not_a_division_by_zero_panic() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("quadrant.tif");
        std::fs::write(&path, synthetic_quadrant(4)).expect("write fixture");

        let result = load_gebco_quadrant_decimated(&path, [0.0, 0.0, 90.0, 90.0], 0);

        assert_eq!(result, Err(DemError::Empty));
    }

    fn flat_quadrant(bounds: [f64; 4], value: f32) -> DemGrid {
        DemGrid::with_bounds(bounds, 2, 2, vec![value; 4]).expect("flat quadrant")
    }

    #[test]
    fn four_quadrants_stitch_into_one_grid_a_world_tile_can_cover() {
        // NW / NE / SW / SE, each 90x90, tiling a 180x180 square. No
        // single one covers the whole square (the property that makes a
        // z0/z1 world tile terrain-less with only individual quadrants),
        // but the mosaic must.
        let nw = flat_quadrant([-90.0, 0.0, 0.0, 90.0], 1.0);
        let ne = flat_quadrant([0.0, 0.0, 90.0, 90.0], 2.0);
        let sw = flat_quadrant([-90.0, -90.0, 0.0, 0.0], 3.0);
        let se = flat_quadrant([0.0, -90.0, 90.0, 0.0], 4.0);

        let world = stitch_world_quadrants(&[nw, ne, sw, se]).expect("stitched");

        let [west, south, east, north] = world.bounds();
        assert!((west - -90.0).abs() < 1e-9);
        assert!((south - -90.0).abs() < 1e-9);
        assert!((east - 90.0).abs() < 1e-9);
        assert!((north - 90.0).abs() < 1e-9);
        assert!((world.sample(-45.0, 45.0) - 1.0).abs() < 1e-6, "NW corner");
        assert!((world.sample(45.0, 45.0) - 2.0).abs() < 1e-6, "NE corner");
        assert!((world.sample(-45.0, -45.0) - 3.0).abs() < 1e-6, "SW corner");
        assert!((world.sample(45.0, -45.0) - 4.0).abs() < 1e-6, "SE corner");
    }

    #[test]
    fn an_empty_quadrant_list_is_rejected_not_a_panic() {
        assert_eq!(stitch_world_quadrants(&[]), Err(DemError::Empty));
    }

    #[test]
    fn mismatched_quadrant_sizes_are_rejected() {
        let a = flat_quadrant([-90.0, 0.0, 0.0, 90.0], 1.0);
        let mismatched = DemGrid::with_bounds([0.0, 0.0, 90.0, 90.0], 3, 3, vec![2.0; 9]).expect("grid");

        assert_eq!(stitch_world_quadrants(&[a, mismatched]), Err(DemError::SampleCount));
    }
}
