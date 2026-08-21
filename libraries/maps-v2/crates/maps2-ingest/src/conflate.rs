//! Making one map out of several sources.
//!
//! Merging sources by concatenating their features produces a package
//! that carries the same thing twice: Natural Earth's London and OSM's
//! London are a kilometre apart, the M25 is a generalised line in one
//! source and a run of ways in the other, and a coastline is simplified
//! two different ways on either side of a package boundary. Nothing
//! downstream can repair that — by tile time the features have lost the
//! knowledge of which source they came from, so the renderer is left
//! reconciling at sixty frames a second what the build should have
//! settled once.
//!
//! This module settles it. Every source states where it speaks for the
//! map, over which levels, and how strongly; conflation then resolves
//! the overlaps before a single tile is written.

use std::collections::HashSet;

use maps2_style::Class;
use maps2_units::{to_lonlat, Lonlat, TilePoint};

use crate::PreparedFeature;

/// Metres per degree of latitude, and of longitude at the equator. The
/// distances here are matching tolerances between two renderings of the
/// same town, so a spherical approximation is far more precision than
/// the question needs.
const METRES_PER_DEGREE: f64 = 111_320.0;

/// How near two same-named places from different sources have to be
/// before they are taken to be one place. Generous: source disagreement
/// about where a city "is" runs to kilometres, because one may mean the
/// historic centre and another the centroid of the built-up area.
pub const PLACE_MATCH_METRES: f64 = 25_000.0;

/// Where a source speaks for the map, over which levels, and how
/// strongly it speaks there.
///
/// Precedence is only ever consulted where two sources overlap, so the
/// numbers carry no meaning of their own — a detailed city extract
/// simply outranks a generalised world one inside the ground it covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerClaim {
    pub precedence: u8,
    /// `[west, south, east, north]`, degrees.
    pub bounds: [f64; 4],
    pub min_level: u8,
    pub max_level: u8,
}

impl LayerClaim {
    #[must_use]
    pub fn covers_level(&self, level: u8) -> bool {
        (self.min_level..=self.max_level).contains(&level)
    }

    #[must_use]
    pub fn contains(&self, point: Lonlat) -> bool {
        let [west, south, east, north] = self.bounds;
        (west..=east).contains(&point.lon) && (south..=north).contains(&point.lat)
    }
}

/// One source's contribution at one level, with its claim on the map.
pub struct SourceLayer {
    pub claim: LayerClaim,
    pub features: Vec<PreparedFeature>,
}

/// Why a feature was dropped, so a build can report what it reconciled
/// rather than silently losing data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ConflationReport {
    /// Dropped because a stronger source covers that ground.
    pub covered: usize,
    /// Dropped because a stronger source already names that place.
    pub matched: usize,
    pub kept: usize,
}

/// Resolves every source's claim at one level into a single feature set.
///
/// Two rules, in order. **Coverage**: inside the bounds of a stronger
/// source that is active at this level, the weaker source is silent —
/// this is what stops a world road network from being drawn underneath
/// a city's own. **Identity**: a place a stronger source has already
/// named is not named again by a weaker one, even outside its bounds,
/// which is what stops one city carrying two labels a kilometre apart.
///
/// Identity matching is asked only of places. A road is a line whose
/// two renderings share no vertex and often no midpoint, so matching it
/// by position would be guesswork; coverage already settles roads, and
/// pretending otherwise would drop real geometry on a coincidence.
#[must_use]
pub fn conflate(level: u8, layers: Vec<SourceLayer>) -> (Vec<PreparedFeature>, ConflationReport) {
    let mut active: Vec<SourceLayer> =
        layers.into_iter().filter(|layer| layer.claim.covers_level(level)).collect();
    active.sort_by_key(|layer| std::cmp::Reverse(layer.claim.precedence));

    let mut kept: Vec<PreparedFeature> = Vec::new();
    let mut report = ConflationReport::default();
    // Claims from *strictly* stronger layers only. Sources of equal
    // precedence are peers describing different things — coastline,
    // borders, roads, places — and are all global, so treating an
    // already-processed peer as stronger let whichever sorted first
    // silence every other layer on Earth.
    let mut stronger: Vec<LayerClaim> = Vec::new();
    let mut pending: Vec<LayerClaim> = Vec::new();
    let mut current: Option<u8> = None;
    let mut named: Vec<(String, Lonlat)> = Vec::new();

    for layer in active {
        if current != Some(layer.claim.precedence) {
            stronger.append(&mut pending);
            current = Some(layer.claim.precedence);
        }
        let mut mine = Vec::new();
        let mut mine_named = Vec::new();
        for feature in layer.features {
            let point = feature_point(&feature);
            if point.is_some_and(|at| stronger.iter().any(|claim| claim.contains(at))) {
                report.covered += 1;
                continue;
            }
            match place_identity(&feature, point) {
                Some(identity) if already_named(&named, &identity) => {
                    report.matched += 1;
                    continue;
                }
                Some(identity) => mine_named.push(identity),
                None => {}
            }
            mine.push(feature);
        }
        pending.push(layer.claim);
        named.extend(mine_named);
        kept.append(&mut mine);
    }
    report.kept = kept.len();
    (kept, report)
}

/// The name and position a place is matched on, or `None` for anything
/// that is not a named place: only point classes carry an identity that
/// two sources can independently arrive at.
fn place_identity(
    feature: &PreparedFeature, point: Option<Lonlat>,
) -> Option<(String, Lonlat)> {
    let at = point.filter(|_| names_a_place(feature.class))?;
    let name = normalised_name(&feature.feature.name);
    (!name.is_empty()).then_some((name, at))
}

fn already_named(named: &[(String, Lonlat)], identity: &(String, Lonlat)) -> bool {
    named.iter().any(|(seen, was)| {
        *seen == identity.0 && metres_between(*was, identity.1) <= PLACE_MATCH_METRES
    })
}

/// Only point classes carry a place identity worth matching on.
fn names_a_place(class: Class) -> bool {
    matches!(class, Class::Label | Class::Poi)
}

/// Case and surrounding space are source formatting, not identity.
fn normalised_name(name: &str) -> String {
    name.trim().to_lowercase()
}

/// A feature's position on the ground: the middle vertex, so a clipped
/// line is represented by a point actually on it.
fn feature_point(feature: &PreparedFeature) -> Option<Lonlat> {
    let vertices = &feature.feature.vertices;
    let coord = *vertices.get(vertices.len() / 2)?;
    Some(to_lonlat(TilePoint { tile: feature.tile, coord }))
}

fn metres_between(a: Lonlat, b: Lonlat) -> f64 {
    let mean_lat = f64::midpoint(a.lat, b.lat).to_radians();
    let dx = (b.lon - a.lon) * mean_lat.cos() * METRES_PER_DEGREE;
    let dy = (b.lat - a.lat) * METRES_PER_DEGREE;
    dx.hypot(dy)
}

/// The levels any of these claims speaks for, ascending. A build walks
/// this rather than a hand-written range, so a plan that leaves a gap
/// between its sources is visible as a gap in the pyramid.
#[must_use]
pub fn claimed_levels(claims: &[LayerClaim]) -> Vec<u8> {
    let mut levels: Vec<u8> = claims
        .iter()
        .flat_map(|claim| claim.min_level..=claim.max_level)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    levels.sort_unstable();
    levels
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_tile::{FeatureDraft, FeatureFlags, RoofType, MaterialClass};
    use maps2_units::{locate, TileId};

    /// Greater London, roughly — the ground the city extract speaks for.
    const CITY_BOUNDS: [f64; 4] = [-0.62, 51.25, 0.35, 51.72];
    const WORLD_BOUNDS: [f64; 4] = [-180.0, -85.06, 180.0, 85.06];

    fn claim(precedence: u8, bounds: [f64; 4], levels: (u8, u8)) -> LayerClaim {
        LayerClaim { precedence, bounds, min_level: levels.0, max_level: levels.1 }
    }

    fn feature(class: Class, name: &str, at: Lonlat, level: u8) -> PreparedFeature {
        let point = locate(at, level);
        PreparedFeature {
            tile: point.tile,
            class,
            feature: FeatureDraft {
                id: 1,
                flags: FeatureFlags::default(),
                rank: 0,
                name: name.to_string(),
                vertices: vec![point.coord],
                holes: Vec::new(),
            },
            building_height: None,
            roof: RoofType::Flat,
            material: MaterialClass::Unknown,
            base_height_dm: 0,
        }
    }

    fn names(features: &[PreparedFeature]) -> Vec<&str> {
        features.iter().map(|f| f.feature.name.as_str()).collect()
    }

    #[test]
    fn inside_the_city_the_city_speaks_and_the_world_source_is_silent() {
        // The M25 is a generalised line in the world source and a run of
        // ways in the city one. Both drawn is two motorways.
        let level = 12;
        let inside = Lonlat { lon: -0.12, lat: 51.51 };
        let world = SourceLayer {
            claim: claim(10, WORLD_BOUNDS, (1, 16)),
            features: vec![feature(Class::RoadMotorway, "M25", inside, level)],
        };
        let city = SourceLayer {
            claim: claim(90, CITY_BOUNDS, (12, 16)),
            features: vec![feature(Class::RoadMotorway, "M25 (osm)", inside, level)],
        };

        let (kept, report) = conflate(level, vec![world, city]);

        assert_eq!(names(&kept), ["M25 (osm)"]);
        assert_eq!(report.covered, 1);
    }

    #[test]
    fn peers_of_equal_precedence_do_not_silence_each_other() {
        // Found by running a real build, not by reading the code:
        // coastline, borders, roads and places are four global layers of
        // equal precedence, and treating an already-processed peer as
        // stronger let whichever sorted first drop every feature of the
        // other three — 118,341 of them, which was all of them.
        let level = 5;
        let at = Lonlat { lon: 2.35, lat: 48.85 };
        let peers = vec![
            SourceLayer {
                claim: claim(10, WORLD_BOUNDS, (1, 7)),
                features: vec![feature(Class::Water, "sea", at, level)],
            },
            SourceLayer {
                claim: claim(10, WORLD_BOUNDS, (1, 7)),
                features: vec![feature(Class::RoadMotorway, "A1", at, level)],
            },
            SourceLayer {
                claim: claim(10, WORLD_BOUNDS, (1, 7)),
                features: vec![feature(Class::Label, "Paris", at, level)],
            },
        ];

        let (kept, report) = conflate(level, peers);

        assert_eq!(kept.len(), 3, "three peers describing different things");
        assert_eq!(report.covered, 0);
    }

    #[test]
    fn outside_the_city_the_world_source_still_speaks() {
        let level = 12;
        let paris = Lonlat { lon: 2.35, lat: 48.85 };
        let world = SourceLayer {
            claim: claim(10, WORLD_BOUNDS, (1, 16)),
            features: vec![feature(Class::RoadMotorway, "A1", paris, level)],
        };
        let city = SourceLayer { claim: claim(90, CITY_BOUNDS, (12, 16)), features: Vec::new() };

        let (kept, report) = conflate(level, vec![world, city]);

        assert_eq!(names(&kept), ["A1"]);
        assert_eq!(report.covered, 0);
    }

    #[test]
    fn a_level_the_city_does_not_reach_leaves_the_world_source_alone() {
        // At world zoom the city extract is not active at all, so its
        // bounds must not silence anything.
        let level = 5;
        let inside = Lonlat { lon: -0.12, lat: 51.51 };
        let world = SourceLayer {
            claim: claim(10, WORLD_BOUNDS, (1, 11)),
            features: vec![feature(Class::Label, "London", inside, level)],
        };
        let city = SourceLayer {
            claim: claim(90, CITY_BOUNDS, (12, 16)),
            features: vec![feature(Class::Label, "never built at z5", inside, level)],
        };

        let (kept, _) = conflate(level, vec![world, city]);

        assert_eq!(names(&kept), ["London"]);
    }

    #[test]
    fn one_city_gets_one_label_even_when_the_sources_disagree_about_where_it_is() {
        // Measured on the live map: Natural Earth's London and OSM's
        // London sit about a kilometre apart. Coverage alone would not
        // catch a case like this if the point fell outside the city
        // bounds, so identity has to be matched as well.
        let level = 10;
        let ne_london = Lonlat { lon: -0.1180, lat: 51.5100 };
        let osm_london = Lonlat { lon: -0.1278, lat: 51.5074 };
        assert!(
            metres_between(ne_london, osm_london) < PLACE_MATCH_METRES,
            "the fixture has to be inside the match radius to test matching",
        );
        let world = SourceLayer {
            claim: claim(10, WORLD_BOUNDS, (1, 16)),
            features: vec![feature(Class::Label, "London", ne_london, level)],
        };
        let city = SourceLayer {
            // Deliberately claims no ground, so only identity can decide.
            claim: claim(90, [0.0, 0.0, 0.0, 0.0], (1, 16)),
            features: vec![feature(Class::Label, "London", osm_london, level)],
        };

        let (kept, report) = conflate(level, vec![world, city]);

        assert_eq!(kept.len(), 1, "one city, one label");
        assert_eq!(report.matched, 1);
    }

    #[test]
    fn two_far_apart_towns_of_the_same_name_are_two_places() {
        let level = 10;
        let one = Lonlat { lon: -0.12, lat: 51.51 };
        let other = Lonlat { lon: -2.60, lat: 51.45 };
        assert!(metres_between(one, other) > PLACE_MATCH_METRES);
        let strong = SourceLayer {
            claim: claim(90, [0.0, 0.0, 0.0, 0.0], (1, 16)),
            features: vec![feature(Class::Label, "Newport", one, level)],
        };
        let weak = SourceLayer {
            claim: claim(10, WORLD_BOUNDS, (1, 16)),
            features: vec![feature(Class::Label, "Newport", other, level)],
        };

        let (kept, report) = conflate(level, vec![strong, weak]);

        assert_eq!(kept.len(), 2);
        assert_eq!(report.matched, 0);
    }

    #[test]
    fn identity_is_matched_for_places_but_never_guessed_for_roads() {
        // Two renderings of one road share no vertex, so position tells
        // you nothing about whether they are the same road. Coverage is
        // the only honest rule for lines.
        let level = 10;
        let at = Lonlat { lon: 2.35, lat: 48.85 };
        let strong = SourceLayer {
            claim: claim(90, [0.0, 0.0, 0.0, 0.0], (1, 16)),
            features: vec![feature(Class::RoadMotorway, "A1", at, level)],
        };
        let weak = SourceLayer {
            claim: claim(10, WORLD_BOUNDS, (1, 16)),
            features: vec![feature(Class::RoadMotorway, "A1", at, level)],
        };

        let (kept, report) = conflate(level, vec![strong, weak]);

        assert_eq!(kept.len(), 2, "roads are settled by coverage, not by name");
        assert_eq!(report.matched, 0);
    }

    #[test]
    fn the_pyramid_is_every_level_some_source_claims() {
        let claims = [
            claim(10, WORLD_BOUNDS, (1, 11)),
            claim(90, CITY_BOUNDS, (8, 16)),
        ];
        assert_eq!(claimed_levels(&claims), (1..=16).collect::<Vec<u8>>());
    }

    #[test]
    fn a_gap_between_the_sources_shows_up_as_a_gap_in_the_pyramid() {
        // The z8-z11 hole the two-package build had, stated as data
        // rather than discovered by zooming into it.
        let claims = [
            claim(10, WORLD_BOUNDS, (1, 7)),
            claim(90, CITY_BOUNDS, (12, 16)),
        ];
        let levels = claimed_levels(&claims);
        assert!(!levels.contains(&8));
        assert!(!levels.contains(&11));
    }

    #[test]
    fn a_tile_id_outside_the_claim_is_not_silenced_by_it() {
        let claim = claim(90, CITY_BOUNDS, (12, 16));
        assert!(claim.contains(Lonlat { lon: -0.12, lat: 51.51 }));
        assert!(!claim.contains(Lonlat { lon: 2.35, lat: 48.85 }));
        assert!(claim.covers_level(12) && !claim.covers_level(11));
        let _ = TileId { z: 12, x: 0, y: 0 };
    }
}

