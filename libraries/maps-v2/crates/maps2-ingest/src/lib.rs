//! Reproducible input validation for map-data builds.

use std::{fmt, io::Read};

use maps2_style::Class;
use osmpbfreader::OsmPbfReader;
use serde::Deserialize;
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
    /// Reading a local source file failed.
    Read(String),
}

impl fmt::Display for SourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDigest => f.write_str("expected a lowercase SHA-256 digest"),
            Self::ChecksumMismatch { source } => write!(f, "checksum mismatch for {source}"),
            Self::Read(error) => write!(f, "cannot read source: {error}"),
        }
    }
}

impl std::error::Error for SourceError {}

/// The supported externally acquired source formats.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub enum SourceKind {
    /// OpenStreetMap Protocolbuffer extract.
    #[serde(rename = "osm-pbf")]
    OsmPbf,
    /// Copernicus Digital Elevation Model raster.
    #[serde(rename = "copernicus-dem")]
    CopernicusDem,
    /// GEBCO global bathymetry raster.
    #[serde(rename = "gebco-grid")]
    GebcoGrid,
}

/// A reproducibly pinned source and its public legal metadata.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceDescriptor {
    /// The source input and expected checksum.
    pub source: Source,
    /// The adapter that can read this source.
    pub kind: SourceKind,
    /// The immutable download location.
    pub url: String,
    /// Upstream source date in ISO 8601 calendar-date form.
    pub source_date: String,
    /// Upstream data licence identifier or name.
    pub licence: String,
    /// Attribution that downstream hosts must display.
    pub attribution: String,
}

/// The source descriptor could not be read safely.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DescriptorError {
    /// The TOML text does not match the descriptor schema.
    Parse(String),
    /// The source digest is not a canonical SHA-256 value.
    InvalidSource(SourceError),
    /// The source URL is not an HTTPS URL.
    InsecureUrl,
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid source descriptor: {error}"),
            Self::InvalidSource(error) => error.fmt(f),
            Self::InsecureUrl => f.write_str("source URL must use HTTPS"),
        }
    }
}

impl std::error::Error for DescriptorError {}

#[derive(Deserialize)]
struct DescriptorDocument {
    source: DescriptorSource,
}

#[derive(Deserialize)]
struct DescriptorSource {
    name: String,
    kind: SourceKind,
    url: String,
    sha256: String,
    source_date: String,
    licence: String,
    attribution: String,
}

/// Parses an immutable, attributed source descriptor.
///
/// # Errors
///
/// Returns [`DescriptorError`] when TOML, checksum, or URL validation fails.
pub fn read_descriptor(toml_text: &str) -> Result<SourceDescriptor, DescriptorError> {
    let document = toml::from_str::<DescriptorDocument>(toml_text)
        .map_err(|error| DescriptorError::Parse(error.to_string()))?;
    let source = document.source;
    if !source.url.starts_with("https://") {
        return Err(DescriptorError::InsecureUrl);
    }
    let source_input = Source::new(source.name, source.sha256).map_err(DescriptorError::InvalidSource)?;
    Ok(SourceDescriptor {
        source: source_input,
        kind: source.kind,
        url: source.url,
        source_date: source.source_date,
        licence: source.licence,
        attribution: source.attribution,
    })
}

/// Checks bytes against their descriptor before ingesting them.
///
/// # Errors
///
/// Returns [`SourceError::ChecksumMismatch`] when the bytes differ from the
/// pinned source digest.
pub fn validate_source(source: &Source, bytes: &[u8]) -> Result<(), SourceError> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    validate_digest(source, &actual)
}

/// Streams bytes through SHA-256 before an ingest build begins.
///
/// # Errors
///
/// Returns [`SourceError::Read`] for input failures or
/// [`SourceError::ChecksumMismatch`] for an unpinned file.
pub fn validate_source_reader(source: &Source, mut reader: impl Read) -> Result<(), SourceError> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 64 * 1024].into_boxed_slice();
    loop {
        let bytes = reader.read(&mut buffer).map_err(|error| SourceError::Read(error.to_string()))?;
        if bytes == 0 {
            break;
        }
        hasher.update(&buffer[..bytes]);
    }
    let actual = format!("{:x}", hasher.finalize());
    validate_digest(source, &actual)
}

fn validate_digest(source: &Source, actual: &str) -> Result<(), SourceError> {
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

/// Counts supported OSM objects while reading a PBF stream once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OsmSummary {
    /// Every decoded OSM object, whether it maps to an MT2 class or not.
    pub objects: u64,
    /// Motorways through paths.
    pub roads: u64,
    /// Building footprint candidates.
    pub buildings: u64,
    /// Water polygons and lines.
    pub water: u64,
    /// Park polygons.
    pub parks: u64,
    /// Named or unnamed POI candidates.
    pub pois: u64,
}

/// The OSM PBF reader rejected an input stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsmError(String);

impl fmt::Display for OsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid OSM PBF: {}", self.0)
    }
}

impl std::error::Error for OsmError {}

/// Streams an OSM PBF and counts classes recognized by the first package build.
///
/// # Errors
///
/// Returns [`OsmError`] when the PBF reader cannot decode the input.
pub fn scan_osm_pbf(input: impl Read) -> Result<OsmSummary, OsmError> {
    let mut reader = OsmPbfReader::new(input);
    let mut summary = OsmSummary::default();
    for object in reader.iter() {
        let object = object.map_err(|error| OsmError(error.to_string()))?;
        summary.objects += 1;
        let tags = object.tags().iter().map(|(key, value)| (key.as_str(), value.as_str())).collect::<Vec<_>>();
        count_class(&mut summary, classify_osm_tags(&tags));
    }
    Ok(summary)
}

fn count_class(summary: &mut OsmSummary, class: Option<Class>) {
    match class {
        Some(Class::RoadMotorway | Class::RoadTrunk | Class::RoadPrimary | Class::RoadSecondary
        | Class::RoadResidential | Class::RoadService | Class::RoadPath) => summary.roads += 1,
        Some(Class::Building) => summary.buildings += 1,
        Some(Class::Water) => summary.water += 1,
        Some(Class::Park) => summary.parks += 1,
        Some(Class::Poi) => summary.pois += 1,
        Some(Class::Land | Class::Label) | None => {}
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
    fn streaming_validation_accepts_the_expected_sha256() {
        let source = Source::new("london.osm.pbf", HELLO_WORLD_SHA256).expect("valid digest");

        assert!(validate_source_reader(&source, std::io::Cursor::new(b"hello world")).is_ok());
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

    #[test]
    fn pbf_scan_rejects_corrupt_input() {
        assert!(scan_osm_pbf(&b"not an osm pbf"[..]).is_err());
    }

    #[test]
    fn source_descriptor_requires_a_pinned_checksum() {
        let descriptor = read_descriptor(
            r#"[source]
name = "london.osm.pbf"
kind = "osm-pbf"
url = "https://example.test/london.osm.pbf"
sha256 = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
source_date = "2026-08-14"
licence = "ODbL-1.0"
attribution = "© OpenStreetMap contributors""#,
        )
        .expect("valid descriptor");

        assert_eq!(descriptor.source.name(), "london.osm.pbf");
        assert_eq!(descriptor.kind, SourceKind::OsmPbf);
    }
}
