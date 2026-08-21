//! The map's furniture at world zooms: place names, country borders and
//! trunk roads.
//!
//! The water polygons and the GEBCO grid give the globe its shape, but a
//! shape is not yet a map — without a label or a border there is nothing
//! to read, only relief. Planet-scale OSM would carry all of it and is
//! far too large to parse for the handful of features a z3 tile can
//! show; Natural Earth publishes exactly that generalised subset, in the
//! same shapefile form [`crate::resolve_water_polygons`] already reads.
//!
//! Each resolver turns a shapefile record into the OSM-style tags the
//! rest of the pipeline already understands, so classification, band
//! eligibility and label ranking stay in one place
//! ([`crate::classify_osm_tags`]) instead of growing a second dialect.

use std::{fmt, path::Path};

use maps2_units::Lonlat;
use shapefile::dbase::FieldValue;

use crate::{prepare_features, PreparedFeature};

#[derive(Debug)]
pub enum NaturalEarthError {
    Read(String),
}

impl fmt::Display for NaturalEarthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(message) => write!(f, "cannot read Natural Earth shapefile: {message}"),
        }
    }
}

impl std::error::Error for NaturalEarthError {}

type Record = shapefile::dbase::Record;

fn text(record: &Record, field: &str) -> Option<String> {
    match record.get(field) {
        Some(FieldValue::Character(Some(value))) => {
            let trimmed = value.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_string())
        }
        _ => None,
    }
}

/// Natural Earth writes its ranks as DBF numerics, which the reader
/// hands back as either `Numeric` or `Float` depending on the column.
fn number(record: &Record, field: &str) -> Option<f64> {
    match record.get(field) {
        Some(FieldValue::Numeric(value)) => *value,
        Some(FieldValue::Float(value)) => value.map(f64::from),
        Some(FieldValue::Integer(value)) => Some(f64::from(*value)),
        _ => None,
    }
}

/// Natural Earth ships WGS84 degrees, unlike the water polygons, which
/// are EPSG:3857 metres — no reprojection needed here.
fn point_of(shape: &shapefile::Shape) -> Option<Lonlat> {
    match shape {
        shapefile::Shape::Point(point) => Some(Lonlat { lon: point.x, lat: point.y }),
        shapefile::Shape::PointM(point) => Some(Lonlat { lon: point.x, lat: point.y }),
        shapefile::Shape::PointZ(point) => Some(Lonlat { lon: point.x, lat: point.y }),
        _ => None,
    }
}

fn polylines_of(shape: &shapefile::Shape) -> Vec<Vec<Lonlat>> {
    let parts = match shape {
        shapefile::Shape::Polyline(line) => line.parts().clone(),
        shapefile::Shape::PolylineM(line) => {
            line.parts().iter().map(|part| part.iter().map(|p| shapefile::Point::new(p.x, p.y)).collect()).collect()
        }
        shapefile::Shape::PolylineZ(line) => {
            line.parts().iter().map(|part| part.iter().map(|p| shapefile::Point::new(p.x, p.y)).collect()).collect()
        }
        _ => return Vec::new(),
    };
    parts
        .into_iter()
        .map(|part| part.iter().map(|point| Lonlat { lon: point.x, lat: point.y }).collect::<Vec<_>>())
        .filter(|part: &Vec<Lonlat>| part.len() >= 2)
        .collect()
}

/// The `place` value a populated place is given, which is what decides
/// the zoom it survives to (see `label_rank_limit`). Natural Earth's
/// SCALERANK is already "how early should this appear", counting up from
/// 0 for the handful of cities a world view can hold, so it maps
/// straight onto the same idea rather than being re-derived from
/// population.
fn place_kind(scalerank: f64) -> &'static str {
    if scalerank <= 1.0 {
        "city"
    } else if scalerank <= 4.0 {
        "town"
    } else if scalerank <= 7.0 {
        "village"
    } else {
        "hamlet"
    }
}

/// Reads `ne_10m_populated_places` and prepares its points as
/// `Class::Label` features for one MT2 zoom level.
///
/// # Errors
///
/// Returns [`NaturalEarthError`] when the shapefile cannot be read.
pub fn resolve_place_labels(
    path: impl AsRef<Path>, level: u8,
) -> Result<Vec<PreparedFeature>, NaturalEarthError> {
    let mut reader = shapefile::Reader::from_path(path.as_ref())
        .map_err(|error| NaturalEarthError::Read(error.to_string()))?;
    let mut features = Vec::new();
    let mut id = 0_u64;
    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result.map_err(|error| NaturalEarthError::Read(error.to_string()))?;
        let (Some(point), Some(name)) = (point_of(&shape), text(&record, "NAME")) else {
            continue;
        };
        let kind = place_kind(number(&record, "SCALERANK").unwrap_or(10.0));
        id += 1;
        features.extend(prepare_features(id, &[("place", kind), ("name", &name)], &[point], level));
    }
    Ok(features)
}

/// Reads `ne_10m_admin_0_boundary_lines_land` and prepares its lines as
/// `Class::Boundary` features for one MT2 zoom level.
///
/// # Errors
///
/// Returns [`NaturalEarthError`] when the shapefile cannot be read.
pub fn resolve_boundary_lines(
    path: impl AsRef<Path>, level: u8,
) -> Result<Vec<PreparedFeature>, NaturalEarthError> {
    let mut reader = shapefile::Reader::from_path(path.as_ref())
        .map_err(|error| NaturalEarthError::Read(error.to_string()))?;
    let mut features = Vec::new();
    let mut id = 0_u64;
    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result.map_err(|error| NaturalEarthError::Read(error.to_string()))?;
        // MIN_ZOOM is Natural Earth's own judgement of when a border is
        // worth drawing; honouring it keeps disputed and minor lines out
        // of a world view without a second opinion here.
        if number(&record, "MIN_ZOOM").unwrap_or(0.0) > f64::from(level) + 1.0 {
            continue;
        }
        for part in polylines_of(&shape) {
            id += 1;
            features.extend(prepare_features(
                id,
                &[("boundary", "administrative"), ("admin_level", "2")],
                &part,
                level,
            ));
        }
    }
    Ok(features)
}

/// Natural Earth's road `type` values, mapped onto the OSM `highway`
/// values the classifier already knows. Anything else is skipped rather
/// than guessed into a class it might not belong to.
fn highway_kind(kind: &str) -> Option<&'static str> {
    match kind {
        "Major Highway" => Some("motorway"),
        "Secondary Highway" => Some("trunk"),
        "Road" | "Beltway" | "Bypass" => Some("primary"),
        _ => None,
    }
}

/// Reads `ne_10m_roads` and prepares its lines as road-class features
/// for one MT2 zoom level.
///
/// # Errors
///
/// Returns [`NaturalEarthError`] when the shapefile cannot be read.
pub fn resolve_major_roads(
    path: impl AsRef<Path>, level: u8,
) -> Result<Vec<PreparedFeature>, NaturalEarthError> {
    let mut reader = shapefile::Reader::from_path(path.as_ref())
        .map_err(|error| NaturalEarthError::Read(error.to_string()))?;
    let mut features = Vec::new();
    let mut id = 0_u64;
    for result in reader.iter_shapes_and_records() {
        let (shape, record) = result.map_err(|error| NaturalEarthError::Read(error.to_string()))?;
        let Some(highway) = text(&record, "type").as_deref().and_then(highway_kind) else {
            continue;
        };
        if number(&record, "scalerank").unwrap_or(12.0) > f64::from(level) + 2.0 {
            continue;
        }
        // `name` is the bare route number — "31", "30" — which on a map
        // is just a number floating over a line. `label` is the same
        // route written the way it is signed ("E31", "M25"), which is
        // what a reader is looking for.
        let name = text(&record, "label")
            .or_else(|| text(&record, "name"))
            .unwrap_or_default();
        for part in polylines_of(&shape) {
            id += 1;
            features.extend(prepare_features(
                id,
                &[("highway", highway), ("name", name.as_str())],
                &part,
                level,
            ));
        }
    }
    Ok(features)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalerank_decides_how_early_a_place_is_allowed_to_appear() {
        // The gate this feeds is `label_rank_limit`, which admits only
        // rank 0 ("city") below z6 — so only these survive a world view.
        assert_eq!(place_kind(0.0), "city");
        assert_eq!(place_kind(1.0), "city");
        assert_eq!(place_kind(2.0), "town");
        assert_eq!(place_kind(5.0), "village");
        assert_eq!(place_kind(9.0), "hamlet");
    }

    #[test]
    fn only_the_road_types_with_an_osm_equivalent_are_carried_over() {
        assert_eq!(highway_kind("Major Highway"), Some("motorway"));
        assert_eq!(highway_kind("Secondary Highway"), Some("trunk"));
        assert_eq!(highway_kind("Bypass"), Some("primary"));
        assert_eq!(highway_kind("Ferry Route"), None);
        assert_eq!(highway_kind("Track"), None);
    }
}
