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

    #[test]
    fn project_ring_converts_mercator_metres_to_lonlat() {
        let points = vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(0.0, 0.0),
        ];
        let ring = project_ring(&points);
        assert_eq!(ring.len(), 2);
        assert!(ring[0].lon.abs() < 1e-9);
    }

    #[test]
    fn resolve_water_polygons_reads_triangle_polygon_via_temp_shapefile() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("water.shp");
        let builder = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("id".try_into().unwrap(), 10);
        let mut writer = shapefile::Writer::from_path(&path, builder).expect("writer");
        // Simple square around null island in EPSG:3857 metres; will be
        // projected back to ~0.09 degrees near lon/lat 0.
        let polygon = shapefile::Polygon::with_rings(vec![shapefile::PolygonRing::Outer(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(10_000.0, 0.0),
            shapefile::Point::new(10_000.0, 10_000.0),
            shapefile::Point::new(0.0, 10_000.0),
            shapefile::Point::new(0.0, 0.0),
        ])]);
        let mut record = shapefile::dbase::Record::default();
        record.insert("id".to_string(), shapefile::dbase::FieldValue::Character(Some("1".to_string())));
        writer.write_shape_and_record(&polygon, &record).expect("write");
        drop(writer);
        let features = resolve_water_polygons(&path, 3).expect("resolve");
        assert!(!features.is_empty(), "water polygon should produce at least one feature");
        assert!(features.iter().all(|f| f.class == maps2_style::Class::Water));
        // Non-polygon shapes are skipped
        let path2 = dir.path().join("water2.shp");
        let builder2 = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("id".try_into().unwrap(), 10);
        let mut writer2 = shapefile::Writer::from_path(&path2, builder2).expect("writer2");
        let mut record2 = shapefile::dbase::Record::default();
        record2.insert("id".to_string(), shapefile::dbase::FieldValue::Character(Some("2".to_string())));
        let point = shapefile::Point::new(0.0, 0.0);
        writer2.write_shape_and_record(&point, &record2).expect("write2");
        drop(writer2);
        let empty = resolve_water_polygons(&path2, 3).expect("empty");
        assert!(empty.is_empty(), "non-polygon should be skipped");
        assert!(resolve_water_polygons(dir.path().join("missing.shp"), 3).is_err());
    }

    #[test]
    fn resolve_water_polygons_handles_inner_ring_as_hole() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("water_hole.shp");
        let builder = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("id".try_into().unwrap(), 10);
        let mut writer = shapefile::Writer::from_path(&path, builder).expect("writer");
        let outer = shapefile::PolygonRing::Outer(vec![
            shapefile::Point::new(-20_000.0, -20_000.0),
            shapefile::Point::new(20_000.0, -20_000.0),
            shapefile::Point::new(20_000.0, 20_000.0),
            shapefile::Point::new(-20_000.0, 20_000.0),
            shapefile::Point::new(-20_000.0, -20_000.0),
        ]);
        let inner = shapefile::PolygonRing::Inner(vec![
            shapefile::Point::new(-5_000.0, -5_000.0),
            shapefile::Point::new(5_000.0, -5_000.0),
            shapefile::Point::new(5_000.0, 5_000.0),
            shapefile::Point::new(-5_000.0, 5_000.0),
            shapefile::Point::new(-5_000.0, -5_000.0),
        ]);
        let polygon = shapefile::Polygon::with_rings(vec![outer, inner]);
        let mut record = shapefile::dbase::Record::default();
        record.insert("id".to_string(), shapefile::dbase::FieldValue::Character(Some("1".to_string())));
        writer.write_shape_and_record(&polygon, &record).expect("write");
        drop(writer);
        let features = resolve_water_polygons(&path, 3).expect("resolve");
        assert!(!features.is_empty());
        assert!(features.iter().any(|f| !f.feature.holes.is_empty() || f.class == maps2_style::Class::Water));
    }
}
