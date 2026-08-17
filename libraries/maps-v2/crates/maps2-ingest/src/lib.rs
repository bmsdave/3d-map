//! Reproducible input validation for map-data builds.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    fs::File,
    io::Read,
    path::Path,
};

use maps2_style::{Class, FLAG_BRIDGE, FLAG_TUNNEL, entry_band};
use maps2_tile::{
    CLASS_HEIGHTS, BuildingDraft, FeatureDraft, TileBuilder, TileError, HEIGHTS_BYTES, HEIGHTS_SIDE,
    encode_height,
};
use maps2_units::{Lonlat, TileCoord, TileId, TilePoint, Zoom, locate, to_lonlat, world_position_px};
use num_traits::ToPrimitive;
use osmpbfreader::{NodeId, OsmId, OsmObj, OsmPbfReader, Relation, WayId};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tiff::decoder::{Decoder, DecodingResult};

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

    /// The lowercase SHA-256 digest required for the source bytes.
    #[must_use]
    pub fn sha256(&self) -> &str {
        &self.expected_sha256
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
#[derive(Clone, Debug, PartialEq)]
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
    /// Source extent as west, south, east, north longitude/latitude degrees.
    pub bounds: [f64; 4],
    /// Version of the reader/normalizer contract for this input.
    pub adapter_version: String,
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
    /// The declared source extent is malformed.
    InvalidBounds,
}

impl fmt::Display for DescriptorError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(error) => write!(f, "invalid source descriptor: {error}"),
            Self::InvalidSource(error) => error.fmt(f),
            Self::InsecureUrl => f.write_str("source URL must use HTTPS"),
            Self::InvalidBounds => f.write_str("source bounds must be finite west,south,east,north"),
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
    bounds: [f64; 4],
    adapter_version: String,
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
    if !valid_bounds(source.bounds) {
        return Err(DescriptorError::InvalidBounds);
    }
    let source_input = Source::new(source.name, source.sha256).map_err(DescriptorError::InvalidSource)?;
    Ok(SourceDescriptor {
        source: source_input,
        kind: source.kind,
        url: source.url,
        source_date: source.source_date,
        licence: source.licence,
        attribution: source.attribution,
        bounds: source.bounds,
        adapter_version: source.adapter_version,
    })
}

fn valid_bounds([west, south, east, north]: [f64; 4]) -> bool {
    west.is_finite() && south.is_finite() && east.is_finite() && north.is_finite()
        && west < east && south < north && (-180.0..=180.0).contains(&west)
        && (-180.0..=180.0).contains(&east) && (-90.0..=90.0).contains(&south)
        && (-90.0..=90.0).contains(&north)
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

fn osm_flags(tags: &[(&str, &str)]) -> u8 {
    let bridge = u8::from(tag(tags, "bridge").is_some_and(|value| value != "no"));
    let tunnel = u8::from(tag(tags, "tunnel").is_some_and(|value| value != "no"));
    (bridge * FLAG_BRIDGE) | (tunnel * FLAG_TUNNEL)
}

fn osm_rank(tags: &[(&str, &str)]) -> u8 {
    match tag(tags, "place") {
        Some("city") => 0,
        Some("town") => 1,
        Some("village" | "borough" | "suburb") => 2,
        _ => 3,
    }
}

fn is_eligible(class: Class, tags: &[(&str, &str)], level: u8) -> bool {
    f64::from(level) >= entry_band(class).entry_zoom()
        && (class != Class::Label || osm_rank(tags) <= label_rank_limit(level))
}

fn label_rank_limit(level: u8) -> u8 {
    match level {
        0..=5 => 0,
        6..=10 => 1,
        11..=13 => 2,
        _ => 3,
    }
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
        .or_else(|| tag(tags, "place").map(|_| Class::Label))
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
    MissingNode { way_id: u64, node_id: i64 },
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
    id: u64,
    tags: Vec<(String, String)>,
    nodes: Vec<NodeId>,
}

#[derive(Clone, Debug)]
struct RawNode {
    id: u64,
    tags: Vec<(String, String)>,
    point: Lonlat,
}

#[derive(Clone, Debug)]
struct RawRelation {
    id: u64,
    tags: Vec<(String, String)>,
    outer: Vec<WayId>,
    inner: Vec<WayId>,
}

/// Resolves classified OSM ways to MT2-ready geometry using two PBF passes.
///
/// # Errors
///
/// Returns [`OsmError`] for unreadable PBF input, missing nodes, or an OSM ID
/// that cannot be represented by MT2 v1.
pub fn resolve_osm_pbf(path: impl AsRef<Path>, level: u8) -> Result<Vec<PreparedFeature>, OsmError> {
    let path = path.as_ref();
    let relations = read_classified_relations(path)?;
    let ways = read_classified_ways(path, &relations)?;
    let point_features = read_classified_nodes(path)?;
    let nodes = read_referenced_nodes(path, &referenced_nodes(&ways))?;
    prepare_osm_features(&relations, &ways, &point_features, &nodes, level)
}

fn read_classified_relations(path: &Path) -> Result<Vec<RawRelation>, OsmError> {
    let input = File::open(path).map_err(|error| OsmError::Read(error.to_string()))?;
    let mut reader = OsmPbfReader::new(input);
    reader.iter().filter_map(|object| match object {
        Ok(OsmObj::Relation(relation)) => Some(Ok(relation)),
        Ok(_) => None,
        Err(error) => Some(Err(OsmError::Read(error.to_string()))),
    }).filter_map(|relation| match relation {
        Ok(relation) => classified_relation(&relation),
        Err(error) => Some(Err(error)),
    }).collect()
}

fn classified_relation(relation: &Relation) -> Option<Result<RawRelation, OsmError>> {
    let tags = owned_tags(&relation.tags);
    let outer = relation_ways(relation, |role| role == "outer" || role.is_empty());
    let inner = relation_ways(relation, |role| role == "inner");
    classify_osm_tags(&tag_refs(&tags)).filter(|_| !outer.is_empty()).map(|_| {
        u64::try_from(relation.id.0).map(|id| RawRelation { id, tags, outer, inner })
            .map_err(|_| OsmError::WayIdOutOfRange(relation.id.0))
    })
}

fn relation_ways(relation: &Relation, role_matches: impl Fn(&str) -> bool) -> Vec<WayId> {
    relation.refs.iter().filter(|member| role_matches(&member.role))
        .filter_map(|member| match member.member { OsmId::Way(id) => Some(id), _ => None })
        .collect()
}

fn read_classified_ways(path: &Path, relations: &[RawRelation]) -> Result<Vec<RawWay>, OsmError> {
    let input = File::open(path).map_err(|error| OsmError::Read(error.to_string()))?;
    let mut reader = OsmPbfReader::new(input);
    let mut ways = Vec::new();
    let relation_ways = relations.iter().flat_map(|relation| relation.outer.iter().chain(&relation.inner).copied()).collect::<HashSet<_>>();
    for object in reader.iter() {
        let OsmObj::Way(way) = object.map_err(|error| OsmError::Read(error.to_string()))? else {
            continue;
        };
        let tags = owned_tags(&way.tags);
        if classify_osm_tags(&tag_refs(&tags)).is_none() && !relation_ways.contains(&way.id) {
            continue;
        }
        let id = u64::try_from(way.id.0).map_err(|_| OsmError::WayIdOutOfRange(way.id.0))?;
        ways.push(RawWay { id, tags, nodes: way.nodes });
    }
    Ok(ways)
}

fn read_classified_nodes(path: &Path) -> Result<Vec<RawNode>, OsmError> {
    let input = File::open(path).map_err(|error| OsmError::Read(error.to_string()))?;
    let mut reader = OsmPbfReader::new(input);
    let mut nodes = Vec::new();
    for object in reader.iter() {
        let OsmObj::Node(node) = object.map_err(|error| OsmError::Read(error.to_string()))? else {
            continue;
        };
        let tags = owned_tags(&node.tags);
        if !matches!(classify_osm_tags(&tag_refs(&tags)), Some(Class::Poi | Class::Label)) {
            continue;
        }
        let id = feature_id(node.id.0);
        nodes.push(RawNode { id, tags, point: Lonlat { lon: node.lon(), lat: node.lat() } });
    }
    Ok(nodes)
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

fn prepare_ways<'a>(
    ways: impl IntoIterator<Item = &'a RawWay>, nodes: &HashMap<NodeId, Lonlat>, level: u8,
) -> Result<Vec<PreparedFeature>, OsmError> {
    ways.into_iter().map(|way| prepare_way(way, nodes, level)).collect::<Result<Vec<_>, _>>()
        .map(|features| features.into_iter().flatten().collect())
}

fn prepare_osm_features(
    relations: &[RawRelation], ways: &[RawWay], point_features: &[RawNode],
    nodes: &HashMap<NodeId, Lonlat>, level: u8,
) -> Result<Vec<PreparedFeature>, OsmError> {
    let members = relation_member_ids(relations);
    let mut features = prepare_ways(ways.iter().filter(|way| !members.contains(&way.id)), nodes, level)?;
    features.extend(prepare_relations(relations, ways, nodes, level)?);
    features.extend(prepare_nodes(point_features, level));
    Ok(features)
}

fn relation_member_ids(relations: &[RawRelation]) -> HashSet<u64> {
    relations.iter().flat_map(|relation| relation.outer.iter().chain(&relation.inner))
        .filter_map(|id| u64::try_from(id.0).ok()).collect()
}

fn prepare_nodes(nodes: &[RawNode], level: u8) -> Vec<PreparedFeature> {
    nodes.iter().flat_map(|node| {
        prepare_features(node.id, &tag_refs(&node.tags), std::slice::from_ref(&node.point), level)
    }).collect()
}

fn feature_id(source_id: i64) -> u64 {
    u64::try_from(source_id).expect("OSM feature IDs are non-negative")
}

fn prepare_way(way: &RawWay, nodes: &HashMap<NodeId, Lonlat>, level: u8) -> Result<Vec<PreparedFeature>, OsmError> {
    let vertices = way
        .nodes
        .iter()
        .map(|node| nodes.get(node).copied().ok_or(OsmError::MissingNode { way_id: way.id, node_id: node.0 }))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(prepare_features(way.id, &tag_refs(&way.tags), &vertices, level))
}

fn prepare_relations(
    relations: &[RawRelation], ways: &[RawWay], nodes: &HashMap<NodeId, Lonlat>, level: u8,
) -> Result<Vec<PreparedFeature>, OsmError> {
    let index = ways.iter().map(|way| (WayId(i64::try_from(way.id).expect("OSM ID fits i64")), way)).collect::<HashMap<_, _>>();
    relations.iter().map(|relation| {
        let outer = relation_rings(&relation.outer, &index, nodes, relation.id)?;
        let inner = relation_rings(&relation.inner, &index, nodes, relation.id)?;
        let tags = tag_refs(&relation.tags);
        let is_polygon = classify_osm_tags(&tags).is_some_and(is_area);
        Ok(outer.into_iter().flat_map(|ring| {
            let holes = inner.iter().filter(|hole| hole.first().is_some_and(|point| point_in_ring(*point, &ring)))
                .map(Vec::as_slice).collect::<Vec<_>>();
            if is_polygon {
                prepare_polygon_with_holes(relation.id, &tags, &ring, &holes, level)
            } else {
                prepare_features(relation.id, &tags, &ring, level)
            }
        }).collect::<Vec<_>>())
    }).collect::<Result<Vec<_>, _>>().map(|parts| parts.into_iter().flatten().collect())
}

fn relation_rings(
    ids: &[WayId], index: &HashMap<WayId, &RawWay>, nodes: &HashMap<NodeId, Lonlat>, relation_id: u64,
) -> Result<Vec<Vec<Lonlat>>, OsmError> {
    stitch_rings(ids.iter().filter_map(|id| index.get(id).map(|way| way.nodes.clone())).collect())
        .into_iter().map(|ring| ring.into_iter().map(|node| {
            nodes.get(&node).copied().ok_or(OsmError::MissingNode { way_id: relation_id, node_id: node.0 })
        }).collect()).collect()
}

fn point_in_ring(point: Lonlat, ring: &[Lonlat]) -> bool {
    ring.windows(2).fold(false, |inside, edge| {
        let (a, b) = (edge[0], edge[1]);
        let crosses = (a.lat > point.lat) != (b.lat > point.lat)
            && point.lon < (b.lon - a.lon) * (point.lat - a.lat) / (b.lat - a.lat) + a.lon;
        inside ^ crosses
    })
}

/// Stitches unordered OSM member ways into canonical closed rings.
///
/// Incomplete member chains are discarded, so callers never emit invalid
/// polygon geometry into MT2.
#[must_use]
pub fn stitch_rings(mut ways: Vec<Vec<NodeId>>) -> Vec<Vec<NodeId>> {
    let mut rings = Vec::new();
    while let Some(seed) = take_next_way(&mut ways) {
        let mut ring = seed;
        while !is_closed(&ring) && append_matching_way(&mut ring, &mut ways) {}
        if is_closed(&ring) {
            rings.push(canonical_ring(ring));
        }
    }
    rings
}

fn take_next_way(ways: &mut Vec<Vec<NodeId>>) -> Option<Vec<NodeId>> {
    let index = ways.iter().position(|way| way.len() > 1)?;
    Some(ways.remove(index))
}

fn is_closed(nodes: &[NodeId]) -> bool {
    nodes.len() > 3 && nodes.first() == nodes.last()
}

fn append_matching_way(ring: &mut Vec<NodeId>, ways: &mut Vec<Vec<NodeId>>) -> bool {
    let Some(&end) = ring.last() else { return false };
    let Some(index) = ways.iter().position(|way| way.first() == Some(&end) || way.last() == Some(&end)) else {
        return false;
    };
    let mut way = ways.remove(index);
    if way.last() == Some(&end) { way.reverse(); }
    ring.extend(way.into_iter().skip(1));
    true
}

fn canonical_ring(mut ring: Vec<NodeId>) -> Vec<NodeId> {
    ring.pop();
    let start = ring.iter().enumerate().min_by_key(|(_, node)| *node).map_or(0, |(index, _)| index);
    ring.rotate_left(start);
    ring.push(ring[0]);
    ring
}

fn owned_tags(tags: &osmpbfreader::Tags) -> Vec<(String, String)> {
    tags.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

fn tag_refs(tags: &[(String, String)]) -> Vec<(&str, &str)> {
    tags.iter().map(|(key, value)| (key.as_str(), value.as_str())).collect()
}

/// A north-up DEM raster over geographic bounds.
#[derive(Clone, Debug, PartialEq)]
pub struct DemGrid {
    west: f64,
    south: f64,
    east: f64,
    north: f64,
    width: u32,
    height: u32,
    samples: Vec<f32>,
}

/// A DEM grid does not match its declared geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DemError {
    /// The raster has no cells.
    Empty,
    /// The cell count does not equal width times height.
    SampleCount,
    /// The geographic bounds are not finite and strictly ordered.
    Bounds,
    /// The TIFF file could not be read or decoded.
    Read(String),
    /// The TIFF sample type is not an elevation raster this adapter supports.
    SampleType,
}

impl fmt::Display for DemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("DEM grid must have nonzero dimensions"),
            Self::SampleCount => f.write_str("DEM sample count does not match dimensions"),
            Self::Bounds => f.write_str("DEM bounds must be finite and strictly ordered"),
            Self::Read(error) => write!(f, "cannot read DEM: {error}"),
            Self::SampleType => f.write_str("unsupported DEM sample type"),
        }
    }
}

impl std::error::Error for DemError {}

/// Loads a one-degree Copernicus DEM Cloud-Optimized `GeoTIFF`.
///
/// # Errors
///
/// Returns [`DemError`] for unreadable TIFF data, unsupported sample types, or
/// inconsistent raster dimensions.
pub fn load_copernicus_dem(path: impl AsRef<Path>, west: f64, south: f64) -> Result<DemGrid, DemError> {
    let file = File::open(path).map_err(|error| DemError::Read(error.to_string()))?;
    let mut decoder = Decoder::new(file).map_err(|error| DemError::Read(error.to_string()))?;
    let (width, height) = decoder.dimensions().map_err(|error| DemError::Read(error.to_string()))?;
    let image = decoder.read_image().map_err(|error| DemError::Read(error.to_string()))?;
    DemGrid::new(west, south, width, height, dem_samples(image)?)
}

fn dem_samples(image: DecodingResult) -> Result<Vec<f32>, DemError> {
    match image {
        DecodingResult::I16(samples) => Ok(samples.into_iter().map(f32::from).collect()),
        DecodingResult::U16(samples) => Ok(samples.into_iter().map(f32::from).collect()),
        DecodingResult::I32(samples) => Ok(samples.into_iter().map(|sample| sample.to_f32().unwrap_or_default()).collect()),
        DecodingResult::U32(samples) => Ok(samples.into_iter().map(|sample| sample.to_f32().unwrap_or_default()).collect()),
        DecodingResult::F32(samples) => Ok(samples),
        DecodingResult::F64(samples) => Ok(samples.into_iter().map(|sample| sample.to_f32().unwrap_or_default()).collect()),
        _ => Err(DemError::SampleType),
    }
}

impl DemGrid {
    /// Creates a one-degree grid whose west/south edge identifies its bounds.
    ///
    /// # Errors
    ///
    /// Returns [`DemError`] when the dimensions cannot describe `samples`.
    pub fn new(
        west: f64,
        south: f64,
        width: u32,
        height: u32,
        samples: Vec<f32>,
    ) -> Result<Self, DemError> {
        Self::with_bounds([west, south, west + 1.0, south + 1.0], width, height, samples)
    }

    /// Creates a grid over the supplied geographic bounds.
    ///
    /// # Errors
    ///
    /// Returns [`DemError`] when bounds are invalid or dimensions cannot
    /// describe `samples`.
    pub fn with_bounds(bounds: [f64; 4], width: u32, height: u32, samples: Vec<f32>) -> Result<Self, DemError> {
        let [west, south, east, north] = bounds;
        if !valid_dem_bounds(west, south, east, north) {
            return Err(DemError::Bounds);
        }
        if width == 0 || height == 0 {
            return Err(DemError::Empty);
        }
        if grid_len(width, height) != Some(samples.len()) {
            return Err(DemError::SampleCount);
        }
        Ok(Self { west, south, east, north, width, height, samples })
    }

    /// Samples the containing north-up raster cell, clamped to this tile.
    #[must_use]
    pub fn sample(&self, lon: f64, lat: f64) -> f32 {
        let x = cell_index(lon - self.west, self.east - self.west, self.width);
        let y = cell_index(self.north - lat, self.north - self.south, self.height);
        self.samples[y * usize::try_from(self.width).unwrap_or_default() + x]
    }

    /// Whether this source grid covers every edge of `tile`.
    #[must_use]
    pub fn covers_tile(&self, tile: TileId) -> bool {
        let corners = [
            TileCoord(0, 0),
            TileCoord(u16::MAX, 0),
            TileCoord(0, u16::MAX),
            TileCoord(u16::MAX, u16::MAX),
        ];
        corners.into_iter().map(|coord| to_lonlat(TilePoint { tile, coord })).all(|point| {
            (self.west..=self.east).contains(&point.lon)
                && (self.south..=self.north).contains(&point.lat)
        })
    }
}

fn grid_len(width: u32, height: u32) -> Option<usize> {
    usize::try_from(width).ok()?.checked_mul(usize::try_from(height).ok()?)
}

fn valid_dem_bounds(west: f64, south: f64, east: f64, north: f64) -> bool {
    [west, south, east, north].into_iter().all(f64::is_finite) && west < east && south < north
}

fn cell_index(offset: f64, span: f64, cells: u32) -> usize {
    let ratio = (offset / span).clamp(0.0, 1.0 - f64::EPSILON);
    let index = (ratio * f64::from(cells)).floor().to_u32().unwrap_or_default();
    usize::try_from(index).unwrap_or_default()
}

/// Samples a DEM on the edge-aligned grid defined by an MT2 height section.
#[must_use]
pub fn height_raster_for_tile(grid: &DemGrid, tile: TileId) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(HEIGHTS_BYTES);
    for y in 0..HEIGHTS_SIDE {
        for x in 0..HEIGHTS_SIDE {
            let point = to_lonlat(TilePoint { tile, coord: TileCoord(raster_coord(x), raster_coord(y)) });
            bytes.extend_from_slice(&encode_height(grid.sample(point.lon, point.lat)).to_le_bytes());
        }
    }
    bytes
}

fn raster_coord(index: usize) -> u16 {
    let value = index * usize::from(u16::MAX) / (HEIGHTS_SIDE - 1);
    u16::try_from(value).unwrap_or_default()
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
    /// The tile containing this clipped feature part.
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
/// crosses a tile boundary. Use [`prepare_features`] for package ingestion.
#[must_use]
pub fn prepare_feature(
    id: u64,
    tags: &[(&str, &str)],
    vertices: &[Lonlat],
    level: u8,
) -> Option<PreparedFeature> {
    let class = classify_osm_tags(tags)?;
    if !is_eligible(class, tags, level) {
        return None;
    }
    let points = vertices.iter().copied().map(|point| locate(point, level)).collect::<Vec<_>>();
    let tile = points.first()?.tile;
    if points.iter().any(|point| point.tile != tile) {
        return None;
    }
    let feature = FeatureDraft {
        id,
        flags: osm_flags(tags),
        rank: osm_rank(tags),
        name: tag(tags, "name").unwrap_or_default().to_string(),
        vertices: points.into_iter().map(|point| point.coord).collect(),
        holes: Vec::new(),
    };
    let building_height = (class == Class::Building).then(|| building_height_m(tags));
    Some(PreparedFeature { tile, class, feature, building_height })
}

/// Clips a classified OSM way into all MT2 tiles it covers.
///
/// Areas are clipped as closed polygons and roads as line segments. Output
/// order is stable by tile address and then source segment order.
#[must_use]
pub fn prepare_features(
    id: u64,
    tags: &[(&str, &str)],
    vertices: &[Lonlat],
    level: u8,
) -> Vec<PreparedFeature> {
    let Some(class) = classify_osm_tags(tags) else {
        return Vec::new();
    };
    if !is_eligible(class, tags, level) {
        return Vec::new();
    }
    if matches!(class, Class::Poi | Class::Label) && vertices.len() == 1 {
        return prepare_feature(id, tags, vertices, level).into_iter().collect();
    }
    if is_area(class) {
        return prepare_polygon_with_holes(id, tags, vertices, &[], level);
    }
    split_antimeridian(vertices).into_iter().flat_map(|line| {
        prepare_line_features(id, class, tags, &line, level)
    }).collect()
}

fn prepare_line_features(
    id: u64, class: Class, tags: &[(&str, &str)], vertices: &[Lonlat], level: u8,
) -> Vec<PreparedFeature> {
    let points = simplify_road(grid_points(vertices, level), class, level);
    let height = (class == Class::Building).then(|| building_height_m(tags));
    let name = tag(tags, "name").unwrap_or_default();
    let flags = osm_flags(tags);
    let rank = osm_rank(tags);
    let tiles = covered_tiles(&points, level);
    tiles.into_iter().flat_map(|tile| {
        clipped_line_parts(&points, tile).into_iter().filter_map(move |part| {
            prepared_part(PartInput { id, class, tile, points: part, building_height: height, flags, rank, name })
        })
    }).collect()
}

fn split_antimeridian(vertices: &[Lonlat]) -> Vec<Vec<Lonlat>> {
    let Some(&first) = vertices.first() else { return Vec::new(); };
    let mut parts = vec![vec![first]];
    for &next in &vertices[1..] {
        let current = *parts.last().and_then(|part| part.last()).expect("first point exists");
        if (next.lon - current.lon).abs() <= 180.0 {
            parts.last_mut().expect("first part exists").push(next);
            continue;
        }
        let (edge, opposite, adjusted_next) = if current.lon > next.lon {
            (180.0, -180.0, next.lon + 360.0)
        } else {
            (-180.0, 180.0, next.lon - 360.0)
        };
        let ratio = (edge - current.lon) / (adjusted_next - current.lon);
        let lat = current.lat + ratio * (next.lat - current.lat);
        parts.last_mut().expect("first part exists").push(Lonlat { lon: edge, lat });
        parts.push(vec![Lonlat { lon: opposite, lat }, next]);
    }
    parts
}

/// Clips a classified area and its interior rings into every MT2 tile it covers.
#[must_use]
pub fn prepare_polygon_with_holes(
    id: u64, tags: &[(&str, &str)], outer: &[Lonlat], holes: &[&[Lonlat]], level: u8,
) -> Vec<PreparedFeature> {
    let Some(class) = classify_osm_tags(tags) else { return Vec::new(); };
    if !is_area(class) || !is_eligible(class, tags, level) {
        return Vec::new();
    }
    let outer_parts = split_antimeridian_polygon(outer);
    let hole_parts = holes.iter().flat_map(|ring| split_antimeridian_polygon(ring)).collect::<Vec<_>>();
    outer_parts.into_iter().flat_map(|outer| {
        let holes = hole_parts.iter().filter(|hole| {
            hole.first().is_some_and(|point| point_in_ring(*point, &outer))
        }).map(Vec::as_slice).collect::<Vec<_>>();
        prepare_polygon_part(id, class, tags, &outer, &holes, level)
    }).collect()
}

fn prepare_polygon_part(
    id: u64, class: Class, tags: &[(&str, &str)], outer: &[Lonlat], holes: &[&[Lonlat]], level: u8,
) -> Vec<PreparedFeature> {
    let outer_points = grid_points(outer, level);
    let hole_points = holes.iter().map(|ring| grid_points(ring, level)).collect::<Vec<_>>();
    let height = (class == Class::Building).then(|| building_height_m(tags));
    let name = tag(tags, "name").unwrap_or_default();
    let flags = osm_flags(tags);
    let rank = osm_rank(tags);
    covered_tiles(&outer_points, level).into_iter().filter_map(|tile| {
        let mut part = prepared_part(PartInput {
            id,
            class,
            tile,
            points: simplify_area_ring(clip_polygon(&outer_points, tile), class, level, tile),
            building_height: height,
            flags,
            rank,
            name,
        })?;
        part.feature.holes = hole_points.iter().filter_map(|ring| {
            polygon_tile_vertices(simplify_area_ring(clip_polygon(ring, tile), class, level, tile), tile)
        }).collect();
        Some(part)
    }).collect()
}

fn split_antimeridian_polygon(ring: &[Lonlat]) -> Vec<Vec<Lonlat>> {
    let unwrapped = unwrap_longitudes(ring);
    let Some((minimum, maximum)) = longitude_span(&unwrapped) else { return Vec::new(); };
    let seam = 180.0 + 360.0 * ((minimum + 180.0) / 360.0).floor();
    if maximum <= seam {
        return vec![normalise_longitudes(unwrapped, seam - 0.1)];
    }
    [
        normalise_longitudes(clip_longitude(&unwrapped, seam, true), seam - 0.1),
        normalise_longitudes(clip_longitude(&unwrapped, seam, false), seam + 0.1),
    ].into_iter().filter(|part| part.len() >= 4).collect()
}

fn unwrap_longitudes(ring: &[Lonlat]) -> Vec<Lonlat> {
    let Some(&first) = ring.first() else { return Vec::new(); };
    ring.iter().copied().skip(1).fold(vec![first], |mut points, mut point| {
        let previous = points.last().expect("first point remains");
        while point.lon - previous.lon > 180.0 { point.lon -= 360.0; }
        while point.lon - previous.lon < -180.0 { point.lon += 360.0; }
        points.push(point);
        points
    })
}

fn longitude_span(points: &[Lonlat]) -> Option<(f64, f64)> {
    points.iter().map(|point| point.lon).fold(None, |span, longitude| match span {
        Some((minimum, maximum)) => Some((minimum.min(longitude), maximum.max(longitude))),
        None => Some((longitude, longitude)),
    })
}

fn clip_longitude(points: &[Lonlat], boundary: f64, below: bool) -> Vec<Lonlat> {
    let Some(&last) = points.last() else { return Vec::new(); };
    points.iter().copied().fold((Vec::new(), last), |(mut output, previous), current| {
        let previous_inside = (previous.lon <= boundary) == below;
        let current_inside = (current.lon <= boundary) == below;
        match (previous_inside, current_inside) {
            (true, true) => output.push(current),
            (true, false) => output.push(longitude_intersection(previous, current, boundary)),
            (false, true) => { output.push(longitude_intersection(previous, current, boundary)); output.push(current); }
            (false, false) => {}
        }
        (output, current)
    }).0
}

fn longitude_intersection(a: Lonlat, b: Lonlat, longitude: f64) -> Lonlat {
    let ratio = (longitude - a.lon) / (b.lon - a.lon);
    Lonlat { lon: longitude, lat: a.lat + ratio * (b.lat - a.lat) }
}

fn normalise_longitudes(mut points: Vec<Lonlat>, reference: f64) -> Vec<Lonlat> {
    let shift = if reference > 180.0 { -360.0 } else if reference < -180.0 { 360.0 } else { 0.0 };
    for point in &mut points {
        point.lon += shift;
    }
    points
}

#[derive(Clone, Copy)]
struct GridPoint {
    x: f64,
    y: f64,
}

fn grid_points(vertices: &[Lonlat], level: u8) -> Vec<GridPoint> {
    let zoom = Zoom::new(f64::from(level));
    vertices.iter().map(|point| {
        let (x, y) = world_position_px(*point, zoom);
        GridPoint { x: x / 256.0, y: y / 256.0 }
    }).collect()
}

fn simplify_road(points: Vec<GridPoint>, class: Class, level: u8) -> Vec<GridPoint> {
    if class.road_rank().is_none() || level >= 16 || points.len() < 3 {
        return points;
    }
    let tolerance = generalisation_tolerance(level);
    let mut keep = vec![false; points.len()];
    keep[0] = true;
    keep[points.len() - 1] = true;
    keep_significant_points(&points, 0, points.len() - 1, tolerance, &mut keep);
    points
        .into_iter()
        .enumerate()
        .filter_map(|(index, point)| keep[index].then_some(point))
        .collect()
}

fn simplify_area_ring(
    points: Vec<GridPoint>, class: Class, level: u8, tile: TileId,
) -> Vec<GridPoint> {
    if class == Class::Building || level >= 16 || points.len() < 4 {
        return points;
    }
    let tolerance = generalisation_tolerance(level);
    let simplified = (0..points.len()).filter_map(|index| {
        let previous = points[(index + points.len() - 1) % points.len()];
        let point = points[index];
        let next = points[(index + 1) % points.len()];
        (on_tile_edge(point, tile) || point_distance(point, previous, next) > tolerance).then_some(point)
    }).collect::<Vec<_>>();
    if simplified.len() >= 3 { simplified } else { points }
}

fn on_tile_edge(point: GridPoint, tile: TileId) -> bool {
    let left = f64::from(tile.x);
    let top = f64::from(tile.y);
    (point.x - left).abs() < f64::EPSILON
        || (point.x - (left + 1.0)).abs() < f64::EPSILON
        || (point.y - top).abs() < f64::EPSILON
        || (point.y - (top + 1.0)).abs() < f64::EPSILON
}

fn keep_significant_points(
    points: &[GridPoint], start: usize, end: usize, tolerance: f64, keep: &mut [bool],
) {
    let mut farthest: Option<(usize, f64)> = None;
    for index in start + 1..end {
        let distance = point_distance(points[index], points[start], points[end]);
        if farthest.is_none_or(|(_, best)| distance > best) {
            farthest = Some((index, distance));
        }
    }
    let Some((index, distance)) = farthest else {
        return;
    };
    if distance <= tolerance {
        return;
    }
    keep[index] = true;
    keep_significant_points(points, start, index, tolerance, keep);
    keep_significant_points(points, index, end, tolerance, keep);
}

fn generalisation_tolerance(level: u8) -> f64 {
    let scale = 1_u32 << u32::from(16_u8.saturating_sub(level));
    f64::from(scale * 2) / f64::from(u16::MAX)
}

fn point_distance(point: GridPoint, start: GridPoint, end: GridPoint) -> f64 {
    let (dx, dy) = (end.x - start.x, end.y - start.y);
    let length = dx.mul_add(dx, dy * dy);
    if length <= f64::EPSILON {
        return (point.x - start.x).hypot(point.y - start.y);
    }
    let t = ((point.x - start.x).mul_add(dx, (point.y - start.y) * dy) / length).clamp(0.0, 1.0);
    (point.x - start.x - t * dx).hypot(point.y - start.y - t * dy)
}

fn is_area(class: Class) -> bool {
    matches!(class, Class::Building | Class::Water | Class::Park)
}

fn covered_tiles(points: &[GridPoint], level: u8) -> Vec<TileId> {
    let max = (1_u32 << level).saturating_sub(1);
    let Some(first) = points.first() else {
        return Vec::new();
    };
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (first.x, first.x, first.y, first.y);
    for point in points {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }
    let xs = tile_axis(min_x, max_x, max);
    let ys = tile_axis(min_y, max_y, max);
    ys.flat_map(|y| xs.clone().map(move |x| TileId { z: level, x, y })).collect()
}

fn tile_axis(min: f64, max: f64, limit: u32) -> std::ops::RangeInclusive<u32> {
    let start = bounded_tile_index(min, limit);
    let end = bounded_tile_index(max, limit);
    start..=end
}

fn bounded_tile_index(value: f64, limit: u32) -> u32 {
    value.floor().clamp(0.0, f64::from(limit)).to_u32().unwrap_or(limit)
}

fn clipped_line_parts(points: &[GridPoint], tile: TileId) -> Vec<Vec<GridPoint>> {
    points.windows(2).fold(Vec::new(), |mut parts, pair| {
        if let Some((start, end)) = clip_segment(pair[0], pair[1], tile) {
            append_line_part(&mut parts, start, end);
        }
        parts
    })
}

fn append_line_part(parts: &mut Vec<Vec<GridPoint>>, start: GridPoint, end: GridPoint) {
    if same_point(start, end) {
        return;
    }
    if let Some(part) = parts.last_mut().filter(|part| part.last().is_some_and(|last| same_point(*last, start))) {
        part.push(end);
    } else {
        parts.push(vec![start, end]);
    }
}

fn same_point(a: GridPoint, b: GridPoint) -> bool {
    (a.x - b.x).abs() < f64::EPSILON && (a.y - b.y).abs() < f64::EPSILON
}

fn clip_segment(a: GridPoint, b: GridPoint, tile: TileId) -> Option<(GridPoint, GridPoint)> {
    let (mut start, mut end) = (0.0, 1.0);
    let dx = b.x - a.x;
    let dy = b.y - a.y;
    let x = f64::from(tile.x);
    let y = f64::from(tile.y);
    for (p, q) in [(-dx, a.x - x), (dx, x + 1.0 - a.x), (-dy, a.y - y), (dy, y + 1.0 - a.y)] {
        if !clip_interval(p, q, &mut start, &mut end) {
            return None;
        }
    }
    Some((point_on(a, dx, dy, start), point_on(a, dx, dy, end)))
}

fn clip_interval(p: f64, q: f64, start: &mut f64, end: &mut f64) -> bool {
    if p.abs() < f64::EPSILON {
        return q >= 0.0;
    }
    let ratio = q / p;
    if p < 0.0 {
        if ratio > *end { return false; }
        *start = start.max(ratio);
    } else {
        if ratio < *start { return false; }
        *end = end.min(ratio);
    }
    true
}

fn point_on(a: GridPoint, dx: f64, dy: f64, ratio: f64) -> GridPoint {
    GridPoint { x: a.x + dx * ratio, y: a.y + dy * ratio }
}

fn clip_polygon(points: &[GridPoint], tile: TileId) -> Vec<GridPoint> {
    let x = f64::from(tile.x);
    let y = f64::from(tile.y);
    let left = clip_edge(points, |point| point.x >= x, |a, b| vertical_intersection(a, b, x));
    let right = clip_edge(&left, |point| point.x <= x + 1.0, |a, b| vertical_intersection(a, b, x + 1.0));
    let top = clip_edge(&right, |point| point.y >= y, |a, b| horizontal_intersection(a, b, y));
    clip_edge(&top, |point| point.y <= y + 1.0, |a, b| horizontal_intersection(a, b, y + 1.0))
}

fn clip_edge(
    points: &[GridPoint],
    inside: impl Fn(GridPoint) -> bool,
    intersection: impl Fn(GridPoint, GridPoint) -> GridPoint,
) -> Vec<GridPoint> {
    let Some(&last) = points.last() else { return Vec::new(); };
    points.iter().copied().fold((Vec::new(), last), |(mut output, previous), current| {
        match (inside(previous), inside(current)) {
            (true, true) => output.push(current),
            (true, false) => output.push(intersection(previous, current)),
            (false, true) => { output.push(intersection(previous, current)); output.push(current); }
            (false, false) => {}
        }
        (output, current)
    }).0
}

fn vertical_intersection(a: GridPoint, b: GridPoint, x: f64) -> GridPoint {
    point_on(a, b.x - a.x, b.y - a.y, (x - a.x) / (b.x - a.x))
}

fn horizontal_intersection(a: GridPoint, b: GridPoint, y: f64) -> GridPoint {
    point_on(a, b.x - a.x, b.y - a.y, (y - a.y) / (b.y - a.y))
}

struct PartInput<'a> {
    id: u64,
    class: Class,
    tile: TileId,
    points: Vec<GridPoint>,
    building_height: Option<BuildingHeight>,
    flags: u8,
    rank: u8,
    name: &'a str,
}

fn prepared_part(part: PartInput<'_>) -> Option<PreparedFeature> {
    let PartInput { id, class, tile, points, building_height, flags, rank, name } = part;
    let mut vertices = points.into_iter().map(|point| tile_coord(point, tile)).collect::<Vec<_>>();
    vertices.dedup();
    if is_area(class) && vertices.first() != vertices.last() {
        vertices.push(*vertices.first()?);
    }
    let required = if is_area(class) { 4 } else { 2 };
    (vertices.len() >= required).then(|| PreparedFeature {
        tile, class, building_height,
        feature: FeatureDraft { id, flags, rank, name: name.to_string(), vertices, holes: Vec::new() },
    })
}

fn polygon_tile_vertices(points: Vec<GridPoint>, tile: TileId) -> Option<Vec<TileCoord>> {
    let mut vertices = points.into_iter().map(|point| tile_coord(point, tile)).collect::<Vec<_>>();
    vertices.dedup();
    if vertices.first() != vertices.last() {
        vertices.push(*vertices.first()?);
    }
    (vertices.len() >= 4).then_some(vertices)
}

fn tile_coord(point: GridPoint, tile: TileId) -> TileCoord {
    let scale = f64::from(u16::MAX);
    let x = tile_axis_coord(point.x - f64::from(tile.x), scale);
    let y = tile_axis_coord(point.y - f64::from(tile.y), scale);
    TileCoord(x, y)
}

fn tile_axis_coord(value: f64, scale: f64) -> u16 {
    (value * scale).round().clamp(0.0, scale).to_u16().unwrap_or(u16::MAX)
}

/// Builds deterministic MT2 tile bytes from prepared feature parts.
///
/// # Errors
///
/// Returns [`TileError`] when the MT2 v1 size limits are exceeded.
pub fn build_tiles(features: &[PreparedFeature]) -> Result<Vec<(TileId, Vec<u8>)>, TileError> {
    build_tiles_inner(features, &[])
}

/// Builds deterministic MT2 tile bytes, attaching terrain where one grid covers a tile.
///
/// # Errors
///
/// Returns [`TileError`] when the MT2 v1 size limits are exceeded.
pub fn build_tiles_with_terrain(
    features: &[PreparedFeature],
    terrain: &DemGrid,
) -> Result<Vec<(TileId, Vec<u8>)>, TileError> {
    build_tiles_with_terrains(features, std::slice::from_ref(terrain))
}

/// Builds deterministic MT2 tile bytes, attaching the matching terrain grid.
///
/// # Errors
///
/// Returns [`TileError`] when the MT2 v1 size limits are exceeded.
pub fn build_tiles_with_terrains(
    features: &[PreparedFeature],
    terrain: &[DemGrid],
) -> Result<Vec<(TileId, Vec<u8>)>, TileError> {
    build_tiles_inner(features, terrain)
}

fn build_tiles_inner(
    features: &[PreparedFeature],
    terrain: &[DemGrid],
) -> Result<Vec<(TileId, Vec<u8>)>, TileError> {
    let mut grouped = HashMap::<TileId, Vec<&PreparedFeature>>::new();
    for feature in features {
        grouped.entry(feature.tile).or_default().push(feature);
    }
    let mut ids = grouped.keys().copied().collect::<Vec<_>>();
    ids.sort_by_key(|id| (id.z, id.x, id.y));
    ids.into_iter().map(|id| build_tile(id, &grouped[&id], terrain)).collect()
}

fn build_tile(
    id: TileId,
    features: &[&PreparedFeature],
    terrain: &[DemGrid],
) -> Result<(TileId, Vec<u8>), TileError> {
    let mut features = features.to_vec();
    features.sort_by_key(|feature| (feature.class.code(), feature.feature.id));
    let mut builder = TileBuilder::new(id);
    for feature in features {
        push_feature(&mut builder, feature);
    }
    if let Some(grid) = terrain.iter().find(|grid| grid.covers_tile(id)) {
        builder.push_raster(CLASS_HEIGHTS, height_raster_for_tile(grid, id));
    }
    Ok((id, builder.build()?))
}

fn push_feature(builder: &mut TileBuilder, feature: &PreparedFeature) {
    if let Some(height) = feature.building_height {
        builder.push_building(feature.class.code(), feature.feature.clone(), BuildingDraft::flat(0, height_dm(height)));
    } else {
        builder.push(feature.class.code(), feature.feature.clone());
    }
}

fn height_dm(height: BuildingHeight) -> u16 {
    let metres = match height {
        BuildingHeight::Explicit(value) | BuildingHeight::Levels(value) | BuildingHeight::Default(value) => value,
    };
    (metres * 10.0).round().to_u16().unwrap_or(u16::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::{CLASS_HEIGHTS, HeightsRaster, TileView};

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
    fn source_descriptor_requires_bounds_and_adapter_version() {
        let descriptor = r#"
[source]
name = "london.osm.pbf"
kind = "osm-pbf"
url = "https://example.test/london.osm.pbf"
sha256 = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
source_date = "2026-08-14"
licence = "ODbL-1.0"
attribution = "© OpenStreetMap contributors""#;

        assert!(read_descriptor(descriptor).is_err());
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
        assert_eq!(classify_osm_tags(&[("place", "city")]), Some(Class::Label));
        assert_eq!(classify_osm_tags(&[("highway", "footway")]), Some(Class::RoadPath));
    }

    #[test]
    fn osm_road_flags_preserve_bridges_and_tunnels() {
        assert_eq!(osm_flags(&[("bridge", "yes")]), maps2_style::FLAG_BRIDGE);
        assert_eq!(osm_flags(&[("tunnel", "yes")]), maps2_style::FLAG_TUNNEL);
        assert_eq!(osm_flags(&[("bridge", "yes"), ("tunnel", "yes")]), maps2_style::FLAG_BRIDGE | maps2_style::FLAG_TUNNEL);
        assert_eq!(osm_flags(&[("bridge", "no")]), 0);
    }

    #[test]
    fn osm_label_rank_prioritises_larger_settlements() {
        assert_eq!(osm_rank(&[("place", "city")]), 0);
        assert_eq!(osm_rank(&[("place", "town")]), 1);
        assert_eq!(osm_rank(&[("place", "village")]), 2);
        assert_eq!(osm_rank(&[("place", "neighbourhood")]), 3);
    }

    #[test]
    fn geometry_adapter_keeps_a_named_point_feature() {
        let point = Lonlat { lon: -0.1278, lat: 51.5074 };

        let features = prepare_features(19, &[("amenity", "library"), ("name", "City Library")], &[point], 16);

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].class, Class::Poi);
        assert_eq!(features[0].feature.vertices.len(), 1);
        assert_eq!(features[0].feature.name, "City Library");
    }

    #[test]
    fn geometry_adapter_omits_classes_before_their_entry_zoom() {
        let building = [
            Lonlat { lon: -0.1278, lat: 51.5074 },
            Lonlat { lon: -0.1277, lat: 51.5074 },
            Lonlat { lon: -0.1277, lat: 51.5073 },
            Lonlat { lon: -0.1278, lat: 51.5073 },
            Lonlat { lon: -0.1278, lat: 51.5074 },
        ];

        assert!(prepare_features(20, &[("building", "yes")], &building, 12).is_empty());
        assert_eq!(prepare_features(20, &[("building", "yes")], &building, 16).len(), 1);
    }

    #[test]
    fn low_zoom_roads_drop_nearly_collinear_vertices() {
        let tile = locate(Lonlat { lon: -0.1278, lat: 51.5074 }, 12).tile;
        let road = [
            to_lonlat(TilePoint { tile, coord: TileCoord(100, 100) }),
            to_lonlat(TilePoint { tile, coord: TileCoord(300, 101) }),
            to_lonlat(TilePoint { tile, coord: TileCoord(500, 100) }),
        ];

        let features = prepare_features(22, &[("highway", "primary")], &road, 12);

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].feature.vertices.len(), 2);
    }

    #[test]
    fn low_zoom_roads_keep_the_farthest_point_of_a_broad_turn() {
        let tolerance = generalisation_tolerance(12);
        let points = vec![
            GridPoint { x: 0.0, y: 0.0 },
            GridPoint { x: 0.2, y: 2.0 * tolerance },
            GridPoint { x: 0.4, y: 2.0 * tolerance },
            GridPoint { x: 1.0, y: 0.0 },
        ];

        let simplified = simplify_road(points, Class::RoadPrimary, 12);

        assert_eq!(simplified.len(), 3);
        assert!((simplified[1].x - 0.2).abs() < f64::EPSILON);
    }

    #[test]
    fn low_zoom_areas_drop_nearly_collinear_inner_vertices() {
        let tile = locate(Lonlat { lon: -0.1278, lat: 51.5074 }, 12).tile;
        let ring = [
            TileCoord(10_000, 10_000),
            TileCoord(20_000, 10_000),
            TileCoord(30_000, 10_005),
            TileCoord(40_000, 10_000),
            TileCoord(40_000, 40_000),
            TileCoord(10_000, 40_000),
        ]
        .map(|coord| to_lonlat(TilePoint { tile, coord }));

        let features = prepare_features(42, &[("natural", "water")], &ring, 12);

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].feature.vertices.len(), 5);
    }

    #[test]
    fn node_adapter_emits_named_osm_places() {
        let nodes = vec![RawNode {
            id: 20,
            tags: vec![("place".to_string(), "city".to_string()), ("name".to_string(), "London".to_string())],
            point: Lonlat { lon: -0.1278, lat: 51.5074 },
        }];

        let features = prepare_nodes(&nodes, 16);

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].class, Class::Label);
        assert_eq!(features[0].feature.name, "London");
    }

    #[test]
    fn oversized_osm_ids_keep_their_source_identity() {
        let source_id = 4_296_598_207;

        let first = feature_id(source_id);
        let second = feature_id(source_id);

        assert_eq!(first, second);
        assert_eq!(first, source_id.cast_unsigned());
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
attribution = "© OpenStreetMap contributors"
bounds = [-0.6, 51.2, 0.4, 51.8]
adapter_version = "osm-v1""#,
        )
        .expect("valid descriptor");

        assert_eq!(descriptor.source.name(), "london.osm.pbf");
        assert_eq!(descriptor.source.sha256(), HELLO_WORLD_SHA256);
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
    fn geometry_adapter_clips_a_road_across_its_tile_boundary() {
        let tile = locate(Lonlat { lon: -0.1278, lat: 51.5074 }, 16).tile;
        let west = to_lonlat(TilePoint { tile, coord: TileCoord(10, u16::MAX / 2) });
        let middle = to_lonlat(TilePoint { tile, coord: TileCoord(u16::MAX / 2, u16::MAX / 2) });
        let east_tile = TileId { z: tile.z, x: tile.x + 1, y: tile.y };
        let east = to_lonlat(TilePoint { tile: east_tile, coord: TileCoord(u16::MAX - 10, u16::MAX / 2) });

        let features = prepare_features(
            17,
            &[("highway", "primary"), ("name", "Boundary Road")],
            &[west, middle, east],
            16,
        );

        assert_eq!(features.len(), 2);
        assert_eq!(features[0].tile, tile);
        assert_eq!(features[1].tile, east_tile);
        assert_eq!(features[0].feature.vertices.len(), 3);
        assert_eq!(features[1].feature.vertices.len(), 2);
        assert!(features.iter().all(|feature| feature.feature.name == "Boundary Road"));
    }

    #[test]
    fn geometry_adapter_carries_road_structure_flags_into_tiles() {
        let vertices = [
            Lonlat { lon: -0.1278, lat: 51.5074 },
            Lonlat { lon: -0.1277, lat: 51.5074 },
        ];

        let features = prepare_features(21, &[("highway", "primary"), ("bridge", "yes")], &vertices, 16);

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].feature.flags, maps2_style::FLAG_BRIDGE);
    }

    #[test]
    fn geometry_adapter_clips_a_building_across_its_tile_boundary() {
        let tile = locate(Lonlat { lon: -0.1278, lat: 51.5074 }, 16).tile;
        let east = TileId { z: tile.z, x: tile.x + 1, y: tile.y };
        let vertices = [
            to_lonlat(TilePoint { tile, coord: TileCoord(u16::MAX - 10, 10) }),
            to_lonlat(TilePoint { tile: east, coord: TileCoord(10, 10) }),
            to_lonlat(TilePoint { tile: east, coord: TileCoord(10, u16::MAX - 10) }),
            to_lonlat(TilePoint { tile, coord: TileCoord(u16::MAX - 10, u16::MAX - 10) }),
        ];

        let features = prepare_features(18, &[("building", "yes"), ("height", "12")], &vertices, 16);

        assert_eq!(features.len(), 2);
        assert!(features.iter().all(|feature| feature.feature.vertices.first() == feature.feature.vertices.last()));
        assert!(features.iter().all(|feature| feature.building_height == Some(BuildingHeight::Explicit(12.0))));
    }

    #[test]
    fn relation_ring_stitcher_orders_and_reverses_member_ways() {
        let ways = vec![
            vec![NodeId(3), NodeId(4), NodeId(1)],
            vec![NodeId(3), NodeId(2)],
            vec![NodeId(2), NodeId(1)],
        ];

        let rings = stitch_rings(ways);

        assert_eq!(rings, vec![vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4), NodeId(1)]]);
    }

    #[test]
    fn a_tagged_multipolygon_member_is_emitted_only_by_its_relation() {
        let way = RawWay {
            id: 10,
            tags: vec![("natural".to_string(), "water".to_string())],
            nodes: vec![NodeId(1), NodeId(2), NodeId(3), NodeId(4), NodeId(1)],
        };
        let relation = RawRelation {
            id: 20,
            tags: vec![("natural".to_string(), "water".to_string())],
            outer: vec![WayId(10)],
            inner: Vec::new(),
        };
        let nodes = HashMap::from([
            (NodeId(1), Lonlat { lon: -0.1278, lat: 51.5074 }),
            (NodeId(2), Lonlat { lon: -0.1277, lat: 51.5074 }),
            (NodeId(3), Lonlat { lon: -0.1277, lat: 51.5075 }),
            (NodeId(4), Lonlat { lon: -0.1278, lat: 51.5075 }),
        ]);

        let features = prepare_osm_features(&[relation], &[way], &[], &nodes, 16).expect("valid relation");

        assert_eq!(features.len(), 1);
        assert_eq!(features[0].feature.id, 20);
    }

    #[test]
    fn a_polygon_hole_is_clipped_into_the_same_tile_part_as_its_outer_ring() {
        let outer = [
            Lonlat { lon: -0.1280, lat: 51.5070 }, Lonlat { lon: -0.1270, lat: 51.5070 },
            Lonlat { lon: -0.1270, lat: 51.5080 }, Lonlat { lon: -0.1280, lat: 51.5080 },
            Lonlat { lon: -0.1280, lat: 51.5070 },
        ];
        let hole = [
            Lonlat { lon: -0.1278, lat: 51.5072 }, Lonlat { lon: -0.1272, lat: 51.5072 },
            Lonlat { lon: -0.1272, lat: 51.5078 }, Lonlat { lon: -0.1278, lat: 51.5078 },
            Lonlat { lon: -0.1278, lat: 51.5072 },
        ];

        let parts = prepare_polygon_with_holes(42, &[("natural", "water")], &outer, &[&hole], 16);

        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].feature.holes.len(), 1);
    }

    #[test]
    fn a_road_crossing_the_antimeridian_stays_at_the_world_seam() {
        let road = [
            Lonlat { lon: 179.999, lat: 0.0 },
            Lonlat { lon: -179.999, lat: 0.0 },
        ];

        let parts = prepare_features(7, &[("highway", "primary")], &road, 12);

        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|part| part.tile.x == 0 || part.tile.x == 4095));
    }

    #[test]
    fn a_polygon_crossing_the_antimeridian_stays_at_the_world_seam() {
        let water = [
            Lonlat { lon: 179.999, lat: 0.001 }, Lonlat { lon: -179.999, lat: 0.001 },
            Lonlat { lon: -179.999, lat: 0.002 }, Lonlat { lon: 179.999, lat: 0.002 },
            Lonlat { lon: 179.999, lat: 0.001 },
        ];

        let parts = prepare_features(8, &[("natural", "water")], &water, 12);

        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|part| part.tile.x == 0 || part.tile.x == 4095));
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
        let building = tile
            .section(Class::Building.code())
            .expect("building section")
            .features()
            .next()
            .expect("building")
            .expect("feature")
            .building;

        assert_eq!(building, Some(maps2_tile::BuildingView::flat(0, 90)));
    }

    #[test]
    fn resolver_rejects_a_corrupt_pbf_file() {
        let file = tempfile::NamedTempFile::new().expect("temporary PBF");
        std::fs::write(file.path(), b"not an osm pbf").expect("write corrupt bytes");

        assert!(resolve_osm_pbf(file.path(), 16).is_err());
    }

    #[test]
    fn dem_grid_samples_north_up_cells() {
        let grid = DemGrid::new(-1.0, 51.0, 2, 2, vec![10.0, 20.0, 30.0, 40.0]).expect("grid");

        assert!((grid.sample(-0.8, 51.8) - 10.0).abs() < f32::EPSILON);
        assert!((grid.sample(-0.2, 51.2) - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn dem_grid_with_bounds_samples_a_regional_north_up_raster() {
        let grid = DemGrid::with_bounds([-2.0, 50.0, 2.0, 54.0], 2, 2, vec![10.0, 20.0, 30.0, 40.0])
            .expect("regional grid");

        assert!((grid.sample(-1.5, 53.5) - 10.0).abs() < f32::EPSILON);
        assert!((grid.sample(1.5, 50.5) - 40.0).abs() < f32::EPSILON);
    }

    #[test]
    fn copernicus_loader_rejects_non_tiff_data() {
        let file = tempfile::NamedTempFile::new().expect("temporary DEM");
        std::fs::write(file.path(), b"not a TIFF").expect("write corrupt bytes");

        assert!(load_copernicus_dem(file.path(), -1.0, 51.0).is_err());
    }

    #[test]
    fn terrain_adapter_writes_a_complete_mt2_height_raster() {
        let grid = DemGrid::new(-180.0, -85.0, 1, 1, vec![17.0]).expect("grid");
        let bytes = height_raster_for_tile(&grid, TileId { z: 0, x: 0, y: 0 });
        let raster = HeightsRaster::parse(&bytes).expect("height raster");

        assert!((raster.metres(0, 0) - 17.0).abs() < f32::EPSILON);
        assert!((raster.metres(255, 255) - 17.0).abs() < f32::EPSILON);
    }

    #[test]
    fn terrain_package_writer_adds_heights_to_covered_vector_tiles() {
        let vertices = [
            Lonlat { lon: -0.1278, lat: 51.5074 },
            Lonlat { lon: -0.1277, lat: 51.5074 },
            Lonlat { lon: -0.1278, lat: 51.5074 },
        ];
        let feature = prepare_feature(17, &[("building", "yes")], &vertices, 16).expect("one tile");
        let grid = DemGrid::new(-1.0, 51.0, 1, 1, vec![17.0]).expect("grid");

        let tiles = build_tiles_with_terrain(&[feature], &grid).expect("terrain package");
        let tile = TileView::parse(&tiles[0].1).expect("valid MT2");

        assert!(tile.raster(CLASS_HEIGHTS).is_some());
    }

    #[test]
    fn terrain_package_writer_uses_each_covering_degree_cell() {
        let west = prepare_feature(
            17,
            &[("building", "yes")],
            &[
                Lonlat { lon: -0.1278, lat: 51.5074 },
                Lonlat { lon: -0.1277, lat: 51.5074 },
                Lonlat { lon: -0.1278, lat: 51.5074 },
            ],
            16,
        )
        .expect("western feature");
        let east = prepare_feature(
            18,
            &[("building", "yes")],
            &[
                Lonlat { lon: 0.1278, lat: 51.5074 },
                Lonlat { lon: 0.1279, lat: 51.5074 },
                Lonlat { lon: 0.1278, lat: 51.5074 },
            ],
            16,
        )
        .expect("eastern feature");
        let west_grid = DemGrid::new(-1.0, 51.0, 1, 1, vec![17.0]).expect("western grid");
        let east_grid = DemGrid::new(0.0, 51.0, 1, 1, vec![23.0]).expect("eastern grid");

        let tiles = build_tiles_with_terrains(&[west, east], &[west_grid, east_grid]).expect("terrain package");

        assert!(tiles
            .iter()
            .all(|(_, bytes)| TileView::parse(bytes).expect("valid MT2").raster(CLASS_HEIGHTS).is_some()));
    }
}
