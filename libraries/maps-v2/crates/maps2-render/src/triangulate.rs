//! Triangulation for simple polygons, with and without holes. Both
//! paths go through `earcutr`: a real building footprint is a concave
//! ring with a courtyard's worth of vertices, and the ear search that
//! reads best on paper rescans every remaining vertex per ear — which
//! is a tile's whole decode budget spent on one block of flats.

pub type Point = (f64, f64);

fn signed_area(ring: &[Point]) -> f64 {
    let mut doubled = 0.0;
    for i in 0..ring.len() {
        let (x0, y0) = ring[i];
        let (x1, y1) = ring[(i + 1) % ring.len()];
        doubled += x0 * y1 - x1 * y0;
    }
    doubled / 2.0
}

fn cross(o: Point, a: Point, b: Point) -> f64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

/// Triangulates a simple ring (no closing duplicate) into index
/// triples over the input. Winding of the input does not matter.
#[must_use]
pub fn triangulate(ring: &[Point]) -> Vec<[u32; 3]> {
    if ring.len() < 3 {
        return Vec::new();
    }
    let coordinates = ring.iter().flat_map(|(x, y)| [*x, *y]).collect::<Vec<f64>>();
    earcut(&coordinates, &[])
}

/// The one call into `earcutr`, in the one index space both entry points
/// want: triples over the flattened coordinate list they were given.
fn earcut(coordinates: &[f64], hole_indices: &[usize]) -> Vec<[u32; 3]> {
    earcutr::earcut(coordinates, hole_indices, 2)
        .unwrap_or_default()
        .chunks_exact(3)
        .filter_map(triangle_indices)
        .collect()
}

/// Triangulates an outer ring with zero or more interior holes. The returned
/// vertex order is the index space of its triangles.
#[must_use]
pub fn triangulate_with_holes(outer: &[Point], holes: &[&[Point]]) -> (Vec<Point>, Vec<[u32; 3]>) {
    let rings = std::iter::once(outer).chain(holes.iter().copied())
        .map(open_ring).filter(|ring| ring.len() >= 3).collect::<Vec<_>>();
    if rings.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let hole_indices = rings.iter().skip(1).scan(rings[0].len(), |offset, ring| {
        let index = *offset;
        *offset += ring.len();
        Some(index)
    }).collect::<Vec<_>>();
    let vertices = rings.into_iter().flatten().collect::<Vec<_>>();
    let coordinates = vertices.iter().flat_map(|(x, y)| [*x, *y]).collect::<Vec<_>>();
    (vertices, earcut(&coordinates, &hole_indices))
}

fn triangle_indices(indices: &[usize]) -> Option<[u32; 3]> {
    Some([u32::try_from(indices[0]).ok()?, u32::try_from(indices[1]).ok()?, u32::try_from(indices[2]).ok()?])
}

fn open_ring(ring: &[Point]) -> Vec<Point> {
    match ring {
        [first, .., last] if first == last => ring[..ring.len() - 1].to_vec(),
        _ => ring.to_vec(),
    }
}

/// Unsigned area of a set of triangles over `ring` — the test currency:
/// triangulation must conserve the polygon's area.
#[must_use]
pub fn triangles_area(ring: &[Point], triangles: &[[u32; 3]]) -> f64 {
    triangles
        .iter()
        .map(|t| {
            let a = ring[t[0] as usize];
            let b = ring[t[1] as usize];
            let c = ring[t[2] as usize];
            (cross(a, b, c) / 2.0).abs()
        })
        .sum()
}

/// Unsigned area of the ring itself.
#[must_use]
pub fn ring_area(ring: &[Point]) -> f64 {
    signed_area(ring).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hole_is_excluded_from_a_polygon_fill() {
        let outer = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0), (0.0, 10.0)];
        let hole = [(3.0, 3.0), (7.0, 3.0), (7.0, 7.0), (3.0, 7.0)];

        let (vertices, triangles) = triangulate_with_holes(&outer, &[&hole]);

        assert!((triangles_area(&vertices, &triangles) - 84.0).abs() < f64::EPSILON);
    }
}
