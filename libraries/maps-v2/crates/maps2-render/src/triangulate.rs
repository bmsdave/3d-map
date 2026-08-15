//! Ear clipping for simple polygons. Fixtures only need rectangles,
//! but concave rings are already legal input — an L-shaped building
//! must not wait for a rewrite here.

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

fn point_in_triangle(p: Point, a: Point, b: Point, c: Point) -> bool {
    let d1 = cross(a, b, p);
    let d2 = cross(b, c, p);
    let d3 = cross(c, a, p);
    d1 >= 0.0 && d2 >= 0.0 && d3 >= 0.0
}

fn is_ear(ring: &[Point], remaining: &[u32], at: usize) -> bool {
    let n = remaining.len();
    let a = ring[remaining[(at + n - 1) % n] as usize];
    let b = ring[remaining[at] as usize];
    let c = ring[remaining[(at + 1) % n] as usize];
    if cross(a, b, c) <= 0.0 {
        return false;
    }
    for other in 0..n {
        if other == at || other == (at + n - 1) % n || other == (at + 1) % n {
            continue;
        }
        if point_in_triangle(ring[remaining[other] as usize], a, b, c) {
            return false;
        }
    }
    true
}

/// Triangulates a simple ring (no closing duplicate) into index
/// triples over the input. Winding of the input does not matter.
#[must_use]
pub fn triangulate(ring: &[Point]) -> Vec<[u32; 3]> {
    if ring.len() < 3 {
        return Vec::new();
    }
    let mut remaining: Vec<u32> = (0..u32::try_from(ring.len()).unwrap_or_default()).collect();
    if signed_area(ring) < 0.0 {
        remaining.reverse();
    }
    let mut triangles = Vec::with_capacity(ring.len() - 2);
    while remaining.len() > 3 {
        let ear = (0..remaining.len()).find(|&i| is_ear(ring, &remaining, i));
        // A simple ring always has an ear; degenerate input falls back
        // to clipping blindly rather than looping forever.
        let ear = ear.unwrap_or(0);
        let n = remaining.len();
        triangles.push([
            remaining[(ear + n - 1) % n],
            remaining[ear],
            remaining[(ear + 1) % n],
        ]);
        remaining.remove(ear);
    }
    triangles.push([remaining[0], remaining[1], remaining[2]]);
    triangles
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
