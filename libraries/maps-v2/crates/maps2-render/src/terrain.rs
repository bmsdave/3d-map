//! Relief: the ground surface, its normals and its shading.
//!
//! Everything the terrain shaders do is written here first, in plain
//! arithmetic that runs under `cargo test`. The fragment shader shades
//! per pixel and this shades per sample, but it is the same formula —
//! so "the north-west flank is the lit one" is a claim about the shader
//! that a native test can hold to account.
//!
//! The surface itself is a plain grid over the tile, tessellated because
//! a sphere needs vertices to bend and a displaced mountain needs
//! vertices to rise. On the sheet the same grid is a flat quad.

use maps2_style::{relief_z_factor, LIGHT_ALTITUDE_DEG, LIGHT_AZIMUTH_DEG};
use maps2_tile::HeightsRaster;
use maps2_units::{to_lonlat, TileCoord, TileId, TilePoint, EARTH_CIRCUMFERENCE_METRES};
use num_traits::ToPrimitive;

use crate::FillVertex;

/// Cells per side of the ground mesh. Thirty-two keeps the globe's limb
/// within a pixel of a true circle at card sizes, and the mesh small
/// enough to upload once and reuse for every tile.
pub const GROUND_MESH_CELLS: u32 = 32;

/// One tile's ground surface, in tile grid coordinates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GroundMesh {
    pub vertices: Vec<FillVertex>,
    pub indices: Vec<u32>,
}

/// A grid spanning the tile edge to edge, so neighbours meet exactly.
#[must_use]
pub fn ground_mesh(cells: u32) -> GroundMesh {
    let side = cells + 1;
    let coord = |i: u32| (f64::from(i) / f64::from(cells) * 65535.0).round().to_u16().unwrap_or(u16::MAX);
    let mut mesh = GroundMesh::default();
    for j in 0..side {
        for i in 0..side {
            mesh.vertices.push(FillVertex { x: coord(i), y: coord(j) });
        }
    }
    for j in 0..cells {
        for i in 0..cells {
            let at = |di: u32, dj: u32| (j + dj) * side + i + di;
            mesh.indices.extend([at(0, 0), at(1, 0), at(1, 1)]);
            mesh.indices.extend([at(0, 0), at(1, 1), at(0, 1)]);
        }
    }
    mesh
}

/// Ground metres between two neighbouring height samples of a tile.
/// Mercator is conformal, so one number serves both directions.
#[must_use]
pub fn texel_metres(id: TileId) -> f32 {
    let centre = to_lonlat(TilePoint { tile: id, coord: TileCoord(32768, 32768) });
    let tiles = f64::from(1_u32 << id.z);
    let samples = f64::from(u32::try_from(maps2_tile::HEIGHTS_SIDE).unwrap_or_default().saturating_sub(1));
    (EARTH_CIRCUMFERENCE_METRES * centre.lat.to_radians().cos() / tiles / samples).to_f32().unwrap_or(f32::MAX)
}

/// Slope of the surface at a sample, east and north, already exaggerated
/// by `z_factor`. Off-tile neighbours clamp to the edge.
#[must_use]
pub fn gradient_at(
    raster: &HeightsRaster<'_>,
    x: i32,
    y: i32,
    texel_metres: f32,
    z_factor: f32,
) -> (f32, f32) {
    let span = 2.0 * texel_metres;
    let east = (raster.metres(x + 1, y) - raster.metres(x - 1, y)) / span;
    // Rows run north to south, so the row above is the northern one.
    let north = (raster.metres(x, y - 1) - raster.metres(x, y + 1)) / span;
    (east * z_factor, north * z_factor)
}

/// How much of a highlight survives. The quiet canvas is already a
/// near-white ground: at full strength every lit slope clips to white
/// and the crest loses the shape the shading was drawn for. Shadows
/// have the whole range below them and are left alone.
pub const HIGHLIGHT_GAIN: f32 = 0.35;

/// Lambert shading relative to flat ground: 1.0 leaves the colour as it
/// is, above brightens, below darkens, 0 is the shadow side.
#[must_use]
pub fn relative_shade(dz_east: f32, dz_north: f32) -> f32 {
    let normal = normalise([-dz_east, -dz_north, 1.0]);
    let light = light_vector();
    let lambert = normal[0] * light[0] + normal[1] * light[1] + normal[2] * light[2];
    let raw = lambert.max(0.0) / light[2];
    if raw > 1.0 { 1.0 + (raw - 1.0) * HIGHLIGHT_GAIN } else { raw }
}

/// Unit vector towards the light: north-west, by convention.
fn light_vector() -> [f32; 3] {
    let azimuth = LIGHT_AZIMUTH_DEG.to_radians();
    let altitude = LIGHT_ALTITUDE_DEG.to_radians();
    [
        azimuth.sin() * altitude.cos(),
        azimuth.cos() * altitude.cos(),
        altitude.sin(),
    ]
}

fn normalise(v: [f32; 3]) -> [f32; 3] {
    let length = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    [v[0] / length, v[1] / length, v[2] / length]
}

/// Vertical exaggeration the shading uses. On the sheet it is the
/// style's expressiveness; on the globe it is whatever the vertices
/// were actually displaced by, so the light describes the geometry the
/// viewer is looking at.
#[must_use]
pub fn shading_z_factor(expressiveness: f32, exaggeration: f32, globeness: f32) -> f32 {
    let g = globeness.clamp(0.0, 1.0);
    relief_z_factor(expressiveness) * (1.0 - g) + exaggeration * g
}

/// `r / R` for a point of the globe: `r = R(1 + h·k·g)`. It fades with
/// globeness because a flat sheet has no radius to swell.
#[must_use]
pub fn relief_radius_scale(metres: f32, exaggeration: f32, globeness: f32) -> f32 {
    let radius = (EARTH_CIRCUMFERENCE_METRES / std::f64::consts::TAU).to_f32().unwrap_or(f32::MAX);
    1.0 + (metres / radius) * exaggeration * globeness.clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use maps2_fixtures::{ridge_tile_bytes, EALING, RIDGE_DETAIL_LEVEL};
    use maps2_tile::{HeightsRaster, TileView, CLASS_HEIGHTS};
    use maps2_units::locate;

    const FLAT: f32 = 1.0;

    #[test]
    fn flat_ground_is_shaded_exactly_as_itself() {
        // The whole point of a relative shade: where there is no slope
        // the quiet canvas keeps its own colour, untouched.
        assert_eq!(relative_shade(0.0, 0.0), FLAT);
    }

    #[test]
    fn the_light_itself_stands_to_the_north_west_and_above_the_horizon() {
        let light = light_vector();
        assert!(light[0] < 0.0, "the light must come from the west: {light:?}");
        assert!(light[1] > 0.0, "and from the north: {light:?}");
        assert!(light[2] > 0.0, "and from above: {light:?}");
    }

    #[test]
    fn the_north_west_light_lifts_slopes_that_face_it() {
        // A slope facing north-west rises towards the south-east: it
        // climbs eastward and falls northward.
        let facing_light = relative_shade(0.4, -0.4);
        let facing_away = relative_shade(-0.4, 0.4);
        assert!(facing_light > FLAT, "north-west slope not lit: {facing_light}");
        assert!(facing_away < FLAT, "south-east slope not shaded: {facing_away}");
        // A slope steep enough turns its back on the light entirely,
        // and the shade bottoms out instead of going negative.
        assert_eq!(relative_shade(-40.0, 40.0), 0.0);
    }

    #[test]
    fn expressiveness_only_changes_how_loud_the_same_slope_reads() {
        let gentle = relative_shade(0.02, -0.02);
        let loud = relative_shade(0.02 * 20.0, -0.02 * 20.0);
        assert!(loud > gentle && gentle > FLAT, "{gentle} then {loud}");
        // Flat ground stays flat however loud the style is.
        assert_eq!(relative_shade(0.0 * 30.0, 0.0), FLAT);
    }

    #[test]
    fn shading_follows_the_geometry_the_globe_actually_shows() {
        // On the sheet the style's expressiveness governs; on the globe
        // the shading must match the exaggeration the vertices were
        // displaced by, or the light would describe a different planet.
        assert_eq!(shading_z_factor(0.0, 40.0, 0.0), 1.0);
        assert_eq!(shading_z_factor(1.0, 40.0, 1.0), 40.0);
        let blend = shading_z_factor(0.0, 41.0, 0.5);
        assert!((blend - 21.0).abs() < 1e-6, "blend was {blend}");
    }

    #[test]
    fn the_sphere_swells_with_height_and_only_while_it_is_a_sphere() {
        assert_eq!(relief_radius_scale(3000.0, 40.0, 0.0), 1.0, "flat sheet must not swell");
        assert_eq!(relief_radius_scale(0.0, 40.0, 1.0), 1.0, "sea level is the sphere");
        let everest = relief_radius_scale(8848.0, 40.0, 1.0);
        assert!(everest > 1.05 && everest < 1.06, "Everest scaled to {everest}");
        assert!(relief_radius_scale(3000.0, 80.0, 1.0) > relief_radius_scale(3000.0, 40.0, 1.0));
        // A trench sinks below the sphere, it does not fold through it.
        assert!(relief_radius_scale(-10_000.0, 40.0, 1.0) < 1.0);
        assert!(relief_radius_scale(-10_000.0, 40.0, 1.0) > 0.9);
    }

    #[test]
    fn the_ground_mesh_spans_its_tile_edge_to_edge() {
        let mesh = ground_mesh(4);
        assert_eq!(mesh.vertices.len(), 25);
        assert_eq!(mesh.indices.len(), 4 * 4 * 6);
        assert_eq!(mesh.vertices[0], FillVertex { x: 0, y: 0 });
        assert_eq!(mesh.vertices[24], FillVertex { x: 65535, y: 65535 });
        // Exact edges are what makes two tiles meet without a crack.
        assert!(mesh.vertices.iter().filter(|v| v.x == 0).count() == 5);
        assert!(mesh.vertices.iter().filter(|v| v.y == 65535).count() == 5);
        assert!(mesh.indices.iter().all(|i| (*i as usize) < mesh.vertices.len()));
        for triangle in mesh.indices.chunks(3) {
            assert!(triangle[0] != triangle[1] && triangle[1] != triangle[2]);
        }
    }

    #[test]
    fn a_height_sample_step_is_ground_metres_at_the_tiles_latitude() {
        // z0: the whole equator across 255 steps.
        let world = texel_metres(TileId { z: 0, x: 0, y: 0 });
        assert!((world - 40_075_016.0 / 255.0).abs() / world < 0.01, "z0 step {world} m");
        // Deeper levels are proportionally finer.
        let deep = texel_metres(locate(EALING, 8).tile);
        assert!(deep < world / 256.0, "z8 step {deep} m");
    }

    #[test]
    fn the_lit_flank_of_the_fixture_ridge_is_brighter_than_the_shaded_one() {
        // The graphical claim, checked without a pixel: walk the real
        // fixture raster and compare the two flanks of the peak.
        let id = locate(EALING, RIDGE_DETAIL_LEVEL).tile;
        let bytes = ridge_tile_bytes(id);
        let tile = TileView::parse(&bytes).expect("parses");
        let raster = HeightsRaster::parse(tile.raster(CLASS_HEIGHTS).expect("heights"))
            .expect("full raster");
        let step = texel_metres(id);
        let shade_at = |x: i32, y: i32| {
            let (de, dn) = gradient_at(&raster, x, y, step, 10.0);
            relative_shade(de, dn)
        };
        let peak = peak_sample(&raster);
        let lit = shade_at(peak.0 - 6, peak.1 - 6);
        let dark = shade_at(peak.0 + 6, peak.1 + 6);
        assert!(lit > dark, "north-west flank {lit} is not brighter than {dark}");
    }

    fn peak_sample(raster: &HeightsRaster<'_>) -> (i32, i32) {
        let mut best = (0, 0);
        let mut best_height = f32::MIN;
        for y in 0..256 {
            for x in 0..256 {
                let h = raster.metres(x, y);
                if h > best_height {
                    best_height = h;
                    best = (x, y);
                }
            }
        }
        best
    }
}
