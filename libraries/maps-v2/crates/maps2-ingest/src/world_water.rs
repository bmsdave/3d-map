//! Real-world ocean coverage for the low-zoom globe.
//!
//! Parsing planet-scale OSM data just to derive a coastline is not
//! necessary: the OSM community publishes a pre-simplified, world-wide
//! water-polygon extract for exactly this purpose (every major renderer
//! uses it for the low-zoom ocean layer). This module reads that
//! shapefile and prepares its rings as `Class::Water` MT2 features, the
//! same way [`crate::resolve_osm_pbf`] prepares OSM ways.

use std::{fmt, path::Path};

use maps2_units::Lonlat;

use crate::{prepare_polygon_with_holes, PreparedFeature};

const EARTH_RADIUS_M: f64 = 6_378_137.0;

#[derive(Debug)]
pub enum WaterPolygonsError {
    Read(String),
}

impl fmt::Display for WaterPolygonsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(message) => write!(f, "cannot read water polygons: {message}"),
        }
    }
}

impl std::error::Error for WaterPolygonsError {}

/// Reads a world water-polygon shapefile (EPSG:3857, as published at
/// <https://osmdata.openstreetmap.de/data/water-polygons.html>) and
/// prepares its rings as `Class::Water` features at one MT2 zoom level.
///
/// # Errors
///
/// Returns [`WaterPolygonsError`] when the shapefile cannot be read.
pub fn resolve_water_polygons(
    path: impl AsRef<Path>, level: u8,
) -> Result<Vec<PreparedFeature>, WaterPolygonsError> {
    let mut reader = shapefile::Reader::from_path(path.as_ref())
        .map_err(|error| WaterPolygonsError::Read(error.to_string()))?;
    let mut features = Vec::new();
    let mut id = 0_u64;
    for result in reader.iter_shapes_and_records() {
        let (shape, _) = result.map_err(|error| WaterPolygonsError::Read(error.to_string()))?;
        let shapefile::Shape::Polygon(polygon) = shape else { continue };
        let (outers, inners): (Vec<_>, Vec<_>) =
            polygon.rings().iter().partition(|ring| matches!(ring, shapefile::PolygonRing::Outer(_)));
        let holes = inners.iter().map(|ring| project_ring(ring.points())).collect::<Vec<_>>();
        let hole_slices = holes.iter().map(Vec::as_slice).collect::<Vec<_>>();
        for outer in &outers {
            id += 1;
            let outer_lonlat = project_ring(outer.points());
            features.extend(prepare_polygon_with_holes(
                id,
                &[("natural", "water")],
                &outer_lonlat,
                &hole_slices,
                level,
            ));
        }
    }
    Ok(features)
}

fn project_ring(points: &[shapefile::Point]) -> Vec<Lonlat> {
    points.iter().map(|point| inverse_web_mercator(point.x, point.y)).collect()
}

/// EPSG:3857 metres back to WGS84 degrees, the inverse of the forward
/// projection every slippy-map tile scheme uses.
fn inverse_web_mercator(x: f64, y: f64) -> Lonlat {
    let lon = x / EARTH_RADIUS_M * 180.0 / std::f64::consts::PI;
    let lat = (2.0 * (y / EARTH_RADIUS_M).exp().atan() - std::f64::consts::FRAC_PI_2) * 180.0
        / std::f64::consts::PI;
    Lonlat { lon, lat }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_origin_maps_to_null_island() {
        let Lonlat { lon, lat } = inverse_web_mercator(0.0, 0.0);
        assert!(lon.abs() < 1e-9, "lon = {lon}");
        assert!(lat.abs() < 1e-9, "lat = {lat}");
    }

    #[test]
    fn a_known_web_mercator_point_recovers_its_lonlat() {
        // London, roughly: -0.1278, 51.5074 forward-projected to EPSG:3857
        // (a value cross-checked against the standard formula, not just
        // round-tripped through this same code).
        let Lonlat { lon, lat } = inverse_web_mercator(-14226.0, 6_711_666.0);
        assert!((lon - (-0.1278)).abs() < 0.01, "lon = {lon}");
        assert!((lat - 51.5074).abs() < 0.01, "lat = {lat}");
    }
}
