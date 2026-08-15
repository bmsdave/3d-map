//! Reproducible input validation for map-data builds.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs::File,
    io::Read,
    path::Path,
};

use maps2_style::Class;
use maps2_tile::{FeatureDraft, TileBuilder, TileError};
use maps2_units::{Lonlat, TileId, locate};
use osmpbfreader::{NodeId, OsmObj, OsmPbfReader};
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

/// The OSM PBF reader or resolver rejected an input stream.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OsmError {
    /// The PBF file could not be read or decoded.
    Read(String),
    /// An OSM way ID cannot fit the MT2 v1 feature-ID field.
    WayIdOutOfRange(i64),
    /// A classified way refers to a node missing from the PBF stream.
    MissingNode { way_id: u32, node_id: i64 },
}

impl fmt::Display for OsmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(error) => write!(f, "invalid OSM PBF: {error}"),
            Self::WayIdOutOfRange(id) => write!(f, "OSM way ID {id} exceeds MT2 v1"),
            Self::MissingNode { way_id, node_id } => {
                write!(f, "OSM way {way_id} references missing node {node_id}")
            }
        }
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
        let object = object.map_err(|error| OsmError::Read(error.to_string()))?;
        summary.objects += 1;
        let tags = object.tags().iter().map(|(key, value)| (key.as_str(), value.as_str())).collect::<Vec<_>>();
        count_class(&mut summary, classify_osm_tags(&tags));
    }
    Ok(summary)
}

#[derive(Clone, Debug)]
struct RawWay {
    id: u32,
    tags: Vec<(String, String)>,
    nodes: Vec<NodeId>,
}

/// Resolves classified OSM ways to MT2-ready geometry using two PBF passes.
///
/// # Errors
///
/// Returns [`OsmError`] for unreadable PBF input, missing nodes, or an OSM ID
/// that cannot be represented by MT2 v1.
pub fn resolve_osm_pbf(path: impl AsRef<Path>, level: u8) -> Result<Vec<PreparedFeature>, OsmError> {
    let path = path.as_ref();
    let ways = read_classified_ways(path)?;
    let nodes = read_referenced_nodes(path, &referenced_nodes(&ways))?;
    prepare_ways(&ways, &nodes, level)
}

fn read_classified_ways(path: &Path) -> Result<Vec<RawWay>, OsmError> {
    let input = File::open(path).map_err(|error| OsmError::Read(error.to_string()))?;
    let mut reader = OsmPbfReader::new(input);
    let mut ways = Vec::new();
    for object in reader.iter() {
        let OsmObj::Way(way) = object.map_err(|error| OsmError::Read(error.to_string()))? else {
            continue;
        };
        let tags = owned_tags(&way.tags);
        if classify_osm_tags(&tag_refs(&tags)).is_none() {
            continue;
        }
        let id = u32::try_from(way.id.0).map_err(|_| OsmError::WayIdOutOfRange(way.id.0))?;
        ways.push(RawWay { id, tags, nodes: way.nodes });
    }
    Ok(ways)
}

fn referenced_nodes(ways: &[RawWay]) -> HashSet<NodeId> {
    ways.iter().flat_map(|way| way.nodes.iter().copied()).collect()
}

fn read_referenced_nodes(path: &Path, wanted: &HashSet<NodeId>) -> Result<HashMap<NodeId, Lonlat>, OsmError> {
    let input = File::open(path).map_err(|error| OsmError::Read(error.to_string()))?;
    let mut reader = OsmPbfReader::new(input);
    let mut nodes = HashMap::with_capacity(wanted.len());
    for object in reader.iter() {
        let OsmObj::Node(node) = object.map_err(|error| OsmError::Read(error.to_string()))? else {
            continue;
        };
        if wanted.contains(&node.id) {
            nodes.insert(node.id, Lonlat { lon: node.lon(), lat: node.lat() });
        }
    }
    Ok(nodes)
}

fn prepare_ways(ways: &[RawWay], nodes: &HashMap<NodeId, Lonlat>, level: u8) -> Result<Vec<PreparedFeature>, OsmError> {
    ways.iter().map(|way| prepare_way(way, nodes, level)).collect::<Result<Vec<_>, _>>()
        .map(|features| features.into_iter().flatten().collect())
}

fn prepare_way(way: &RawWay, nodes: &HashMap<NodeId, Lonlat>, level: u8) -> Result<Option<PreparedFeature>, OsmError> {
    let vertices = way
        .nodes
        .iter()
        .map(|node| nodes.get(node).copied().ok_or(OsmError::MissingNode { way_id: way.id, node_id: node.0 }))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(prepare_feature(way.id, &tag_refs(&way.tags), &vertices, level))
}

fn owned_tags(tags: &osmpbfreader::Tags) -> Vec<(String, String)> {
    tags.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

fn tag_refs(tags: &[(String, String)]) -> Vec<(&str, &str)> {
    tags.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect()
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

/// A classified OSM feature expressed on the MT2 coordinate grid.
#[derive(Clone, Debug, PartialEq)]
pub struct PreparedFeature {
    /// The single tile that owns all feature vertices.
    pub tile: TileId,
    /// The MT2 section class.
    pub class: Class,
    /// The MT2 feature payload.
    pub feature: FeatureDraft,
    /// The normalized height source, only for buildings.
    pub building_height: Option<BuildingHeight>,
}

/// Converts a classified OSM geometry that fits one tile to its MT2 form.
///
/// Returns `None` for unsupported tags, empty geometry, or geometry that
/// crosses a tile boundary. Boundary clipping is deliberately performed by
/// the package tiler, not by this coordinate adapter.
#[must_use]
pub fn prepare_feature(
    id: u32,
    tags: &[(&str, &str)],
    vertices: &[Lonlat],
    level: u8,
) -> Option<PreparedFeature> {
    let class = classify_osm_tags(tags)?;
    let points = vertices.iter().copied().map(|point| locate(point, level)).collect::<Vec<_>>();
    let tile = points.first()?.tile;
    if points.iter().any(|point| point.tile != tile) {
        return None;
    }
    let feature = FeatureDraft {
        id,
        flags: 0,
        rank: 0,
        name: tag(tags, "name").unwrap_or_default().to_string(),
        vertices: points.into_iter().map(|point| point.coord).collect(),
    };
    let building_height = (class == Class::Building).then(|| building_height_m(tags));
    Some(PreparedFeature { tile, class, feature, building_height })
}

/// Builds deterministic MT2 tile bytes from single-tile prepared features.
///
/// # Errors
///
/// Returns [`TileError`] when the MT2 v1 size limits are exceeded.
pub fn build_tiles(features: &[PreparedFeature]) -> Result<Vec<(TileId, Vec<u8>)>, TileError> {
    let mut grouped = HashMap::<TileId, Vec<&PreparedFeature>>::new();
    for feature in features {
        grouped.entry(feature.tile).or_default().push(feature);
    }
    let mut ids = grouped.keys().copied().collect::<Vec<_>>();
    ids.sort_by_key(|id| (id.z, id.x, id.y));
    ids.into_iter().map(|id| build_tile(id, &grouped[&id])).collect()
}

fn build_tile(id: TileId, features: &[&PreparedFeature]) -> Result<(TileId, Vec<u8>), TileError> {
    let mut features = features.to_vec();
    features.sort_by_key(|feature| (feature.class.code(), feature.feature.id));
    let mut builder = TileBuilder::new(id);
    for feature in features {
        builder.push(feature.class.code(), feature.feature.clone());
    }
    Ok((id, builder.build()?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::TileView;

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

    #[test]
    fn geometry_adapter_keeps_a_building_in_its_mt2_tile() {
        let vertices = [
            Lonlat { lon: -0.1278, lat: 51.5074 },
            Lonlat { lon: -0.1277, lat: 51.5074 },
            Lonlat { lon: -0.1277, lat: 51.5075 },
            Lonlat { lon: -0.1278, lat: 51.5074 },
        ];
        let feature = prepare_feature(17, &[("building", "yes"), ("height", "42")], &vertices, 16)
            .expect("a small building fits one tile");

        assert_eq!(feature.class, Class::Building);
        assert_eq!(feature.feature.vertices.len(), vertices.len());
        assert_eq!(feature.building_height, Some(BuildingHeight::Explicit(42.0)));
    }

    #[test]
    fn package_writer_groups_prepared_features_into_deterministic_mt2_tiles() {
        let vertices = [
            Lonlat { lon: -0.1278, lat: 51.5074 },
            Lonlat { lon: -0.1277, lat: 51.5074 },
            Lonlat { lon: -0.1278, lat: 51.5074 },
        ];
        let feature = prepare_feature(17, &[("building", "yes")], &vertices, 16).expect("one tile");

        let tiles = build_tiles(&[feature]).expect("tile package");

        assert_eq!(tiles.len(), 1);
        let tile = TileView::parse(&tiles[0].1).expect("valid MT2");
        assert!(tile.section(Class::Building.code()).is_some());
    }

    #[test]
    fn resolver_rejects_a_corrupt_pbf_file() {
        let file = tempfile::NamedTempFile::new().expect("temporary PBF");
        std::fs::write(file.path(), b"not an osm pbf").expect("write corrupt bytes");

        assert!(resolve_osm_pbf(file.path(), 16).is_err());
    }
}
