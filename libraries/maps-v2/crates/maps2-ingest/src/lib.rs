//! Reproducible input validation for map-data builds.

use std::fmt;

use maps2_style::Class;
use sha2::{Digest, Sha256};

/// A named pipeline input pinned to its SHA-256 digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Source {
    name: String,
    expected_sha256: String,
}

impl Source {
    /// Creates a source descriptor from a lowercase SHA-256 digest.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::InvalidDigest`] when the digest is not canonical.
    pub fn new(name: impl Into<String>, expected_sha256: impl Into<String>) -> Result<Self, SourceError> {
        let expected_sha256 = expected_sha256.into();
        if !is_sha256(&expected_sha256) {
            return Err(SourceError::InvalidDigest);
        }
        Ok(Self { name: name.into(), expected_sha256 })
    }

    /// The descriptor's stable human-readable name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

/// Input validation failed before a data build began.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceError {
    /// The manifest did not contain a lowercase SHA-256 digest.
    InvalidDigest,
    /// The downloaded bytes do not match the manifest.
    ChecksumMismatch { source: String },
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDigest => f.write_str("expected a lowercase SHA-256 digest"),
            Self::ChecksumMismatch { source } => write!(f, "checksum mismatch for {source}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// Checks bytes against their descriptor before ingesting them.
///
/// # Errors
///
/// Returns [`SourceError::ChecksumMismatch`] when the bytes differ from the
/// pinned source digest.
pub fn validate_source(source: &Source, bytes: &[u8]) -> Result<(), SourceError> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual == source.expected_sha256 {
        Ok(())
    } else {
        Err(SourceError::ChecksumMismatch { source: source.name.clone() })
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Default extrusion height when OSM has neither usable height nor levels.
pub const DEFAULT_BUILDING_HEIGHT_M: f32 = 9.0;

/// The source of a normalized building extrusion height.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BuildingHeight {
    /// OSM supplied a usable metric `height` tag.
    Explicit(f32),
    /// OSM supplied `building:levels`, normalized at three metres per level.
    Levels(f32),
    /// Neither OSM tag was usable.
    Default(f32),
}

/// Normalizes the building-height tags used by the first real-data renderer.
#[must_use]
pub fn building_height_m(tags: &[(&str, &str)]) -> BuildingHeight {
    if let Some(metres) = tag(tags, "height").and_then(parse_metres) {
        return BuildingHeight::Explicit(metres);
    }
    if let Some(levels) = tag(tags, "building:levels").and_then(parse_positive) {
        return BuildingHeight::Levels(levels * 3.0);
    }
    BuildingHeight::Default(DEFAULT_BUILDING_HEIGHT_M)
}

fn tag<'a>(tags: &'a [(&str, &str)], key: &str) -> Option<&'a str> {
    tags.iter().find_map(|(candidate, value)| (*candidate == key).then_some(*value))
}

fn parse_metres(value: &str) -> Option<f32> {
    parse_positive(value.trim().strip_suffix('m').unwrap_or(value).trim())
}

fn parse_positive(value: &str) -> Option<f32> {
    value.parse::<f32>().ok().filter(|number| number.is_finite() && *number > 0.0)
}

/// Maps the supported OSM feature tags to their MT2 class.
#[must_use]
pub fn classify_osm_tags(tags: &[(&str, &str)]) -> Option<Class> {
    let highway = tag(tags, "highway");
    road_class(highway)
        .or_else(|| tag(tags, "building").filter(|value| *value != "no").map(|_| Class::Building))
        .or_else(|| tag(tags, "natural").filter(|value| *value == "water").map(|_| Class::Water))
        .or_else(|| tag(tags, "leisure").filter(|value| *value == "park").map(|_| Class::Park))
        .or_else(|| tag(tags, "amenity").map(|_| Class::Poi))
}

fn road_class(highway: Option<&str>) -> Option<Class> {
    match highway? {
        "motorway" => Some(Class::RoadMotorway),
        "trunk" => Some(Class::RoadTrunk),
        "primary" => Some(Class::RoadPrimary),
        "secondary" | "tertiary" => Some(Class::RoadSecondary),
        "residential" | "living_street" | "unclassified" => Some(Class::RoadResidential),
        "service" => Some(Class::RoadService),
        "footway" | "path" | "cycleway" => Some(Class::RoadPath),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HELLO_WORLD_SHA256: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    #[test]
    fn source_validation_accepts_the_expected_sha256() {
        let source = Source::new("london.osm.pbf", HELLO_WORLD_SHA256).expect("valid digest");

        assert!(validate_source(&source, b"hello world").is_ok());
    }

    #[test]
    fn source_validation_rejects_changed_bytes() {
        let source = Source::new("london.osm.pbf", HELLO_WORLD_SHA256).expect("valid digest");

        assert_eq!(
            validate_source(&source, b"hello world!"),
            Err(SourceError::ChecksumMismatch { source: "london.osm.pbf".to_string() })
        );
    }

    #[test]
    fn source_rejects_a_noncanonical_digest() {
        assert_eq!(Source::new("source", "ABC"), Err(SourceError::InvalidDigest));
    }

    #[test]
    fn building_height_prefers_a_valid_height_tag_then_levels_then_default() {
        assert_eq!(building_height_m(&[("height", "42 m")]), BuildingHeight::Explicit(42.0));
        assert_eq!(building_height_m(&[("building:levels", "8")]), BuildingHeight::Levels(24.0));
        assert_eq!(building_height_m(&[("height", "unknown")]), BuildingHeight::Default(9.0));
    }

    #[test]
    fn osm_tags_map_to_the_stable_tile_classes() {
        assert_eq!(classify_osm_tags(&[("highway", "primary")]), Some(Class::RoadPrimary));
        assert_eq!(classify_osm_tags(&[("building", "yes")]), Some(Class::Building));
        assert_eq!(classify_osm_tags(&[("natural", "water")]), Some(Class::Water));
        assert_eq!(classify_osm_tags(&[("amenity", "cafe")]), Some(Class::Poi));
        assert_eq!(classify_osm_tags(&[("highway", "footway")]), Some(Class::RoadPath));
    }
}
