//! Bounded reads out of a large north-up geographic raster.
//!
//! GEBCO ships the world as multi-gigabyte grids; a regional build must never
//! hold one in memory. This adapter turns a declared source extent plus a
//! wanted extent into a pixel window, then decodes only the TIFF chunks that
//! window touches, so cost tracks the region and not the file.

use std::{fs::File, path::Path};

use num_traits::ToPrimitive;
use tiff::decoder::{ChunkType, Decoder};

use crate::{DemError, DemGrid, dem_samples};

/// The largest window this adapter will materialise, in cells. 4 Mi cells is
/// 16 MiB of `f32` — a whole GEBCO sub-grid is two orders of magnitude past it,
/// so a caller that forgot to bound its request fails instead of swapping.
pub const WINDOW_CELL_LIMIT: usize = 4 * 1024 * 1024;

/// A window read out of a larger raster, with the read cost it paid.
#[derive(Clone, Debug, PartialEq)]
pub struct RasterWindow {
    grid: DemGrid,
    chunks_read: u32,
    chunks_total: u32,
}

impl RasterWindow {
    /// The sampled window as a standalone grid over its own pixel-aligned bounds.
    #[must_use]
    pub const fn grid(&self) -> &DemGrid {
        &self.grid
    }

    /// Consumes the window, keeping the grid.
    #[must_use]
    pub fn into_grid(self) -> DemGrid {
        self.grid
    }

    /// How many TIFF chunks were decoded to fill this window.
    #[must_use]
    pub const fn chunks_read(&self) -> u32 {
        self.chunks_read
    }

    /// How many chunks the whole source image holds — the cost of a naive read.
    #[must_use]
    pub const fn chunks_total(&self) -> u32 {
        self.chunks_total
    }
}

/// The pixel rectangle of a source image that a geographic window covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PixelWindow {
    left: u32,
    top: u32,
    width: u32,
    height: u32,
}

/// Reads the part of a GEBCO grid that covers `window`.
///
/// `source_bounds` is the descriptor's pinned west, south, east, north extent
/// of the whole file; `window` is the wanted extent, clipped to it. The
/// returned grid is aligned to source pixel edges, so it is a subset of the
/// source and not a resampling of it.
///
/// # Errors
///
/// Returns [`DemError`] when the TIFF cannot be read, the declared bounds are
/// malformed, the window misses the source, or the window exceeds
/// [`WINDOW_CELL_LIMIT`] cells.
pub fn load_gebco_window(
    path: impl AsRef<Path>,
    source_bounds: [f64; 4],
    window: [f64; 4],
) -> Result<RasterWindow, DemError> {
    let file = File::open(path).map_err(|error| DemError::Read(error.to_string()))?;
    let mut decoder = Decoder::new(file).map_err(|error| DemError::Read(error.to_string()))?;
    let (width, height) =
        decoder.dimensions().map_err(|error| DemError::Read(error.to_string()))?;
    let pixels = pixel_window(source_bounds, window, width, height)?;
    read_window(&mut decoder, source_bounds, pixels, width, height)
}

fn pixel_window(
    source_bounds: [f64; 4],
    window: [f64; 4],
    width: u32,
    height: u32,
) -> Result<PixelWindow, DemError> {
    let [west, south, east, north] = source_bounds;
    if !crate::valid_dem_bounds(west, south, east, north) {
        return Err(DemError::Bounds);
    }
    let [want_west, want_south, want_east, want_north] = window;
    if !crate::valid_dem_bounds(want_west, want_south, want_east, want_north) {
        return Err(DemError::Bounds);
    }
    if width == 0 || height == 0 {
        return Err(DemError::Empty);
    }
    if want_east <= west || want_west >= east || want_north <= south || want_south >= north {
        return Err(DemError::WindowOutside);
    }
    let (left, right) = axis_range(want_west - west, want_east - west, east - west, width);
    let (top, bottom) = axis_range(north - want_north, north - want_south, north - south, height);
    let pixels = PixelWindow { left, top, width: right - left, height: bottom - top };
    let cells = crate::grid_len(pixels.width, pixels.height).ok_or(DemError::SampleCount)?;
    if cells > WINDOW_CELL_LIMIT {
        return Err(DemError::WindowTooLarge(cells));
    }
    Ok(pixels)
}

/// Half-open pixel range covering `[low, high]` offsets, always at least one cell.
fn axis_range(low: f64, high: f64, span: f64, cells: u32) -> (u32, u32) {
    let scale = f64::from(cells) / span;
    let first = floor_index(low * scale, cells);
    let last = floor_index(high * scale, cells);
    (first, last.saturating_add(1).min(cells))
}

fn floor_index(value: f64, cells: u32) -> u32 {
    if !value.is_finite() || value < 0.0 {
        return 0;
    }
    value.floor().clamp(0.0, f64::from(cells.saturating_sub(1))).to_u32().unwrap_or(cells.saturating_sub(1))
}

fn read_window(
    decoder: &mut Decoder<File>,
    source_bounds: [f64; 4],
    pixels: PixelWindow,
    width: u32,
    height: u32,
) -> Result<RasterWindow, DemError> {
    let (chunk_width, chunk_height) = chunk_shape(decoder, width, height)?;
    let chunks_x = width.div_ceil(chunk_width);
    let chunks_y = height.div_ceil(chunk_height);
    let mut samples = vec![0.0_f32; window_len(pixels)?];
    let mut chunks_read = 0;
    for chunk_y in pixels.top / chunk_height..=(pixels.top + pixels.height - 1) / chunk_height {
        for chunk_x in pixels.left / chunk_width..=(pixels.left + pixels.width - 1) / chunk_width {
            let index = chunk_y * chunks_x + chunk_x;
            let (data_width, _) = decoder.chunk_data_dimensions(index);
            let chunk = decoder
                .read_chunk(index)
                .map_err(|error| DemError::Read(error.to_string()))?;
            copy_chunk(
                &dem_samples(chunk)?,
                data_width,
                (chunk_x * chunk_width, chunk_y * chunk_height),
                pixels,
                &mut samples,
            );
            chunks_read += 1;
        }
    }
    let grid = DemGrid::with_bounds(
        window_bounds(source_bounds, pixels, width, height),
        pixels.width,
        pixels.height,
        samples,
    )?;
    Ok(RasterWindow { grid, chunks_read, chunks_total: chunks_x * chunks_y })
}

fn chunk_shape(
    decoder: &mut Decoder<File>,
    width: u32,
    height: u32,
) -> Result<(u32, u32), DemError> {
    let (chunk_width, chunk_height) = decoder.chunk_dimensions();
    let chunk_width = if decoder.get_chunk_type() == ChunkType::Strip { width } else { chunk_width };
    if chunk_width == 0 || chunk_height == 0 || chunk_width > width && chunk_height > height {
        return Err(DemError::Read("raster has no readable chunks".to_string()));
    }
    Ok((chunk_width.min(width).max(1), chunk_height.max(1)))
}

fn window_len(pixels: PixelWindow) -> Result<usize, DemError> {
    crate::grid_len(pixels.width, pixels.height).ok_or(DemError::SampleCount)
}

/// Copies the overlap of one decoded chunk into the window buffer.
fn copy_chunk(
    chunk: &[f32],
    data_width: u32,
    origin: (u32, u32),
    pixels: PixelWindow,
    samples: &mut [f32],
) {
    let (origin_x, origin_y) = origin;
    let stride = usize::try_from(data_width).unwrap_or_default();
    if stride == 0 {
        return;
    }
    let rows = chunk.len() / stride;
    for row in 0..rows {
        let image_y = origin_y + u32::try_from(row).unwrap_or(u32::MAX);
        if image_y < pixels.top || image_y >= pixels.top + pixels.height {
            continue;
        }
        let out_row = usize::try_from(image_y - pixels.top).unwrap_or_default()
            * usize::try_from(pixels.width).unwrap_or_default();
        for column in 0..stride {
            let image_x = origin_x + u32::try_from(column).unwrap_or(u32::MAX);
            if image_x < pixels.left || image_x >= pixels.left + pixels.width {
                continue;
            }
            let out_column = usize::try_from(image_x - pixels.left).unwrap_or_default();
            samples[out_row + out_column] = chunk[row * stride + column];
        }
    }
}

fn window_bounds(
    source_bounds: [f64; 4],
    pixels: PixelWindow,
    width: u32,
    height: u32,
) -> [f64; 4] {
    let [west, south, east, north] = source_bounds;
    let cell_x = (east - west) / f64::from(width);
    let cell_y = (north - south) / f64::from(height);
    [
        west + f64::from(pixels.left) * cell_x,
        north - f64::from(pixels.top + pixels.height) * cell_y,
        west + f64::from(pixels.left + pixels.width) * cell_x,
        north - f64::from(pixels.top) * cell_y,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use tiff::encoder::{TiffEncoder, colortype::Gray32Float};

    const SOURCE_BOUNDS: [f64; 4] = [-1.0, 51.0, 0.0, 52.0];

    /// Writes a `size`×`size` `GeoTIFF` of `f32` values `row * size + col`, split
    /// into strips of `rows_per_strip` rows so the reader must decode several
    /// chunks. This is not a real DEM, only a raster whose cell values are
    /// predictable enough to assert a window copied the right pixels.
    fn write_striped_tiff(path: &Path, size: u32, rows_per_strip: u32) {
        let samples: Vec<f32> = (0..size * size).map(|index| index.to_f32().unwrap_or_default()).collect();
        let file = File::create(path).expect("create tiff");
        let mut encoder = TiffEncoder::new(file).expect("tiff encoder");
        let mut image = encoder.new_image::<Gray32Float>(size, size).expect("new image");
        image.rows_per_strip(rows_per_strip).expect("rows per strip");
        let mut offset = 0usize;
        while image.next_strip_sample_count() > 0 {
            let count = usize::try_from(image.next_strip_sample_count()).unwrap_or_default();
            image.write_strip(&samples[offset..offset + count]).expect("write strip");
            offset += count;
        }
        image.finish().expect("finish image");
    }

    #[test]
    fn a_small_window_reads_fewer_chunks_than_the_whole_raster() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gebco.tif");
        write_striped_tiff(&path, 64, 4);

        // The whole raster is 64 rows in 4-row strips: 16 chunks. A window
        // covering only its first few rows must not touch them all — this is
        // the bounded-memory property the adapter exists for.
        let window = load_gebco_window(&path, SOURCE_BOUNDS, [-1.0, 51.9, 0.0, 52.0])
            .expect("window reads");
        assert_eq!(window.chunks_total(), 16);
        assert!(
            window.chunks_read() < window.chunks_total(),
            "expected a partial read, got {} of {} chunks",
            window.chunks_read(),
            window.chunks_total()
        );
    }

    #[test]
    fn a_window_samples_match_the_source_cells_it_covers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gebco.tif");
        write_striped_tiff(&path, 8, 2);

        // Top-left quadrant: rows 0..4, cols 0..4 of an 8x8 grid whose value
        // at (row, col) is row * 8 + col.
        let window = load_gebco_window(&path, SOURCE_BOUNDS, [-1.0, 51.5, -0.5, 52.0])
            .expect("window reads");
        let grid = window.grid();
        for row in 0..4_u32 {
            for col in 0..4_u32 {
                let lon = -1.0 + (f64::from(col) + 0.5) / 8.0;
                let lat = 52.0 - (f64::from(row) + 0.5) / 8.0;
                let expected = (row * 8 + col).to_f32().unwrap_or_default();
                assert!((grid.sample(lon, lat) - expected).abs() < f32::EPSILON);
            }
        }
    }

    #[test]
    fn a_window_wholly_outside_the_source_is_rejected() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("gebco.tif");
        write_striped_tiff(&path, 4, 1);

        let result = load_gebco_window(&path, SOURCE_BOUNDS, [10.0, 51.0, 11.0, 52.0]);
        assert_eq!(result.unwrap_err(), DemError::WindowOutside);
    }

    #[test]
    fn a_window_past_the_cell_limit_is_rejected_before_any_chunk_is_read() {
        // Pixel geometry alone is enough to trigger the guard: no file needs
        // to exist, proving the limit is checked before any decode happens.
        // 4096^2 == 16,777,216 cells, above WINDOW_CELL_LIMIT.
        let result = pixel_window(SOURCE_BOUNDS, SOURCE_BOUNDS, 4096, 4096);
        assert!(matches!(result, Err(DemError::WindowTooLarge(_))));
    }
}
