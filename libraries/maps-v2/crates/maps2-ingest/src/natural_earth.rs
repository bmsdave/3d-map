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

    #[test]
    fn text_trims_and_rejects_empty_or_missing_fields() {
        let mut record = Record::default();
        record.insert("NAME".to_string(), FieldValue::Character(Some("  London  ".to_string())));
        assert_eq!(text(&record, "NAME"), Some("London".to_string()));
        record.insert("EMPTY".to_string(), FieldValue::Character(Some("   ".to_string())));
        assert_eq!(text(&record, "EMPTY"), None);
        assert_eq!(text(&record, "MISSING"), None);
        // Wrong type is not text
        record.insert("NUM".to_string(), FieldValue::Numeric(Some(1.0)));
        assert_eq!(text(&record, "NUM"), None);
    }

    #[test]
    fn number_handles_numeric_float_and_integer_fields() {
        let mut record = Record::default();
        record.insert("N".to_string(), FieldValue::Numeric(Some(3.0)));
        assert_eq!(number(&record, "N"), Some(3.0));
        record.insert("F".to_string(), FieldValue::Float(Some(4.5_f32)));
        assert!(matches!(number(&record, "F"), Some(v) if (v - 4.5).abs() < 1e-6));
        record.insert("I".to_string(), FieldValue::Integer(7));
        assert_eq!(number(&record, "I"), Some(7.0));
        assert_eq!(number(&record, "MISSING"), None);
        record.insert("C".to_string(), FieldValue::Character(Some("hi".to_string())));
        assert_eq!(number(&record, "C"), None);
    }

    #[test]
    fn point_of_extracts_lonlat_for_point_variants() {
        let point = shapefile::Shape::Point(shapefile::Point::new(10.0, 20.0));
        assert_eq!(point_of(&point), Some(Lonlat { lon: 10.0, lat: 20.0 }));
        let point_m = shapefile::Shape::PointM(shapefile::PointM::new(1.0, 2.0, 0.0));
        assert_eq!(point_of(&point_m), Some(Lonlat { lon: 1.0, lat: 2.0 }));
        let point_z = shapefile::Shape::PointZ(shapefile::PointZ::new(3.0, 4.0, 5.0, 6.0));
        assert_eq!(point_of(&point_z), Some(Lonlat { lon: 3.0, lat: 4.0 }));
        let line = shapefile::Shape::Polyline(shapefile::Polyline::new(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(1.0, 1.0),
        ]));
        assert_eq!(point_of(&line), None);
    }

    #[test]
    fn polylines_of_extracts_parts_and_filters_short_segments() {
        let line = shapefile::Shape::Polyline(shapefile::Polyline::new(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(1.0, 1.0),
        ]));
        let parts = polylines_of(&line);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].len(), 2);
        // PolylineZ and PolylineM variants
        let line_m = shapefile::Shape::PolylineM(shapefile::PolylineM::with_parts(vec![vec![
            shapefile::PointM::new(0.0, 0.0, 0.0),
            shapefile::PointM::new(1.0, 1.0, 0.0),
        ]]));
        assert_eq!(polylines_of(&line_m).len(), 1);
        let line_z = shapefile::Shape::PolylineZ(shapefile::PolylineZ::with_parts(vec![vec![
            shapefile::PointZ::new(0.0, 0.0, 0.0, 0.0),
            shapefile::PointZ::new(1.0, 1.0, 0.0, 0.0),
        ]]));
        assert_eq!(polylines_of(&line_z).len(), 1);
        // Non-polyline returns empty
        let point = shapefile::Shape::Point(shapefile::Point::new(0.0, 0.0));
        assert!(polylines_of(&point).is_empty());
    }

    #[test]
    fn resolve_place_labels_reads_point_shapefile_into_label_feature() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("places.shp");
        let builder = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("NAME".try_into().unwrap(), 50)
            .add_numeric_field("SCALERANK".try_into().unwrap(), 10, 0);
        let mut writer = shapefile::Writer::from_path(&path, builder).expect("writer");
        let mut record = shapefile::dbase::Record::default();
        record.insert("NAME".to_string(), FieldValue::Character(Some("London".to_string())));
        record.insert("SCALERANK".to_string(), FieldValue::Numeric(Some(0.0)));
        let shape = shapefile::Point::new(0.0, 51.5);
        writer.write_shape_and_record(&shape, &record).expect("write");
        drop(writer);
        // level 3 admits only city (scalerank 0 => city), so should emit 1 feature
        let features = resolve_place_labels(&path, 3).expect("resolve");
        assert_eq!(features.len(), 1, "city at z3 should survive");
        assert_eq!(features[0].class, maps2_style::Class::Label);
        // Missing NAME is skipped
        let path2 = dir.path().join("places2.shp");
        let builder2 = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("NAME".try_into().unwrap(), 50)
            .add_numeric_field("SCALERANK".try_into().unwrap(), 10, 0);
        let mut writer2 = shapefile::Writer::from_path(&path2, builder2).expect("writer2");
        let mut empty_record = shapefile::dbase::Record::default();
        empty_record.insert("NAME".to_string(), FieldValue::Character(None));
        empty_record.insert("SCALERANK".to_string(), FieldValue::Numeric(Some(0.0)));
        let shape2 = shapefile::Point::new(0.0, 0.0);
        writer2.write_shape_and_record(&shape2, &empty_record).expect("write2");
        drop(writer2);
        let features2 = resolve_place_labels(&path2, 14).expect("resolve2");
        assert!(features2.is_empty(), "empty name should be skipped");
        // Invalid path returns error
        assert!(resolve_place_labels(dir.path().join("missing.shp"), 3).is_err());
    }

    #[test]
    fn resolve_boundary_lines_turns_polyline_shapefile_into_boundary_feature() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("bounds.shp");
        let builder = shapefile::dbase::TableWriterBuilder::new()
            .add_numeric_field("MIN_ZOOM".try_into().unwrap(), 10, 2);
        let mut writer = shapefile::Writer::from_path(&path, builder).expect("writer");
        let mut record = shapefile::dbase::Record::default();
        record.insert("MIN_ZOOM".to_string(), FieldValue::Numeric(Some(2.0)));
        let line = shapefile::Polyline::new(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(1.0, 1.0),
            shapefile::Point::new(2.0, 2.0),
        ]);
        writer.write_shape_and_record(&line, &record).expect("write");
        drop(writer);
        let features = resolve_boundary_lines(&path, 5).expect("resolve");
        assert!(!features.is_empty(), "boundary at MIN_ZOOM 2 should survive z5");
        assert!(features.iter().all(|f| f.class == maps2_style::Class::Boundary));
        // MIN_ZOOM filtering: high MIN_ZOOM filtered at low level
        let path2 = dir.path().join("bounds2.shp");
        let builder2 = shapefile::dbase::TableWriterBuilder::new()
            .add_numeric_field("MIN_ZOOM".try_into().unwrap(), 10, 2);
        let mut writer2 = shapefile::Writer::from_path(&path2, builder2).expect("writer2");
        let mut record2 = shapefile::dbase::Record::default();
        record2.insert("MIN_ZOOM".to_string(), FieldValue::Numeric(Some(10.0)));
        let line2 = shapefile::Polyline::new(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(1.0, 1.0),
        ]);
        writer2.write_shape_and_record(&line2, &record2).expect("write2");
        drop(writer2);
        let filtered = resolve_boundary_lines(&path2, 3).expect("filtered");
        assert!(filtered.is_empty(), "high MIN_ZOOM should be filtered at low zoom");
        assert!(resolve_boundary_lines(dir.path().join("nope.shp"), 3).is_err());
    }

    #[test]
    fn resolve_major_roads_turns_polyline_shapefile_into_road_feature() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("roads.shp");
        let builder = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("type".try_into().unwrap(), 30)
            .add_numeric_field("scalerank".try_into().unwrap(), 10, 0)
            .add_character_field("label".try_into().unwrap(), 30)
            .add_character_field("name".try_into().unwrap(), 30);
        let mut writer = shapefile::Writer::from_path(&path, builder).expect("writer");
        let mut record = shapefile::dbase::Record::default();
        record.insert("type".to_string(), FieldValue::Character(Some("Major Highway".to_string())));
        record.insert("scalerank".to_string(), FieldValue::Numeric(Some(1.0)));
        record.insert("label".to_string(), FieldValue::Character(Some("E31".to_string())));
        record.insert("name".to_string(), FieldValue::Character(Some("31".to_string())));
        let line = shapefile::Polyline::new(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(1.0, 1.0),
        ]);
        writer.write_shape_and_record(&line, &record).expect("write");
        drop(writer);
        let features = resolve_major_roads(&path, 14).expect("resolve");
        assert!(!features.is_empty());
        assert_eq!(features[0].class, maps2_style::Class::RoadMotorway);
        // Unknown type is skipped
        let path2 = dir.path().join("roads2.shp");
        let builder2 = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("type".try_into().unwrap(), 30)
            .add_numeric_field("scalerank".try_into().unwrap(), 10, 0);
        let mut writer2 = shapefile::Writer::from_path(&path2, builder2).expect("writer2");
        let mut record2 = shapefile::dbase::Record::default();
        record2.insert("type".to_string(), FieldValue::Character(Some("Ferry Route".to_string())));
        record2.insert("scalerank".to_string(), FieldValue::Numeric(Some(1.0)));
        let line2 = shapefile::Polyline::new(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(1.0, 1.0),
        ]);
        writer2.write_shape_and_record(&line2, &record2).expect("write2");
        drop(writer2);
        let empty = resolve_major_roads(&path2, 14).expect("empty");
        assert!(empty.is_empty(), "unknown road type should be skipped");
        // scalerank filtering at low zoom
        let path3 = dir.path().join("roads3.shp");
        let builder3 = shapefile::dbase::TableWriterBuilder::new()
            .add_character_field("type".try_into().unwrap(), 30)
            .add_numeric_field("scalerank".try_into().unwrap(), 10, 0);
        let mut writer3 = shapefile::Writer::from_path(&path3, builder3).expect("writer3");
        let mut record3 = shapefile::dbase::Record::default();
        record3.insert("type".to_string(), FieldValue::Character(Some("Major Highway".to_string())));
        record3.insert("scalerank".to_string(), FieldValue::Numeric(Some(12.0)));
        let line3 = shapefile::Polyline::new(vec![
            shapefile::Point::new(0.0, 0.0),
            shapefile::Point::new(1.0, 1.0),
        ]);
        writer3.write_shape_and_record(&line3, &record3).expect("write3");
        drop(writer3);
        let filtered = resolve_major_roads(&path3, 3).expect("filtered");
        assert!(filtered.is_empty(), "high scalerank should be filtered at low zoom");
        assert!(resolve_major_roads(dir.path().join("nope.shp"), 3).is_err());
    }
}
