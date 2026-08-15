//! Relief in the style: where the light comes from, how loud the
//! shading is allowed to be, and the hypsometric ramp.
//!
//! Light from the north-west is the cartographic convention, not a
//! taste: lit from the other side the same terrain reads inverted, and
//! valleys become ridges. Expressiveness is a style parameter because
//! true-scale shading on a quiet canvas is almost invisible — a slope of
//! 200 m over 30 km is a fifth of a degree — and how loud to make it is
//! a question about the map, not about the data.

use num_traits::ToPrimitive;

/// Where the sun stands, degrees clockwise from north.
pub const LIGHT_AZIMUTH_DEG: f32 = 315.0;
/// How high it stands above the horizon, degrees.
pub const LIGHT_ALTITUDE_DEG: f32 = 45.0;

/// Vertical exaggeration of the shading at full expressiveness.
pub const RELIEF_Z_FACTOR_MAX: f32 = 30.0;

/// Expressiveness `0..1` → vertical exaggeration of the gradient the
/// shading is computed from. Geometry is untouched; only the light lies.
#[must_use]
pub fn relief_z_factor(expressiveness: f32) -> f32 {
    let e = expressiveness.clamp(0.0, 1.0);
    1.0 + (RELIEF_Z_FACTOR_MAX - 1.0) * e
}

/// Metres above sea level → tint. Muted on purpose: the ramp says
/// "higher", it does not shout "look here" (VISION: contrast is
/// reserved for text and for the application's own data).
pub const HYPSOMETRIC_STOPS: [(f32, [u8; 3]); 5] = [
    (0.0, [0xDE, 0xEA, 0xD6]),
    (400.0, [0xEC, 0xE8, 0xCE]),
    (1200.0, [0xE6, 0xD8, 0xBC]),
    (2400.0, [0xDC, 0xC8, 0xB0]),
    (3600.0, [0xF5, 0xF3, 0xF1]),
];

/// The ramp sampled: exact at stops, linear between, clamped outside.
#[must_use]
pub fn hypsometric_tint(metres: f32) -> [u8; 3] {
    let first = HYPSOMETRIC_STOPS[0];
    let last = HYPSOMETRIC_STOPS[HYPSOMETRIC_STOPS.len() - 1];
    if metres <= first.0 {
        return first.1;
    }
    if metres >= last.0 {
        return last.1;
    }
    for window in HYPSOMETRIC_STOPS.windows(2) {
        let ((z0, c0), (z1, c1)) = (window[0], window[1]);
        if metres >= z0 && metres <= z1 {
            let t = (metres - z0) / (z1 - z0);
            return blend(c0, c1, t);
        }
    }
    last.1
}

fn blend(from: [u8; 3], to: [u8; 3], t: f32) -> [u8; 3] {
    let channel = |i: usize| {
        let a = f32::from(from[i]);
        (a + (f32::from(to[i]) - a) * t).round().clamp(0.0, 255.0).to_u8().unwrap_or_default()
    };
    [channel(0), channel(1), channel(2)]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < f32::EPSILON, "{actual} != {expected}");
    }

    #[test]
    fn the_light_stands_in_the_north_west_by_convention() {
        // The cartographic convention, and the reason a relief map does
        // not read as a hole in the ground: light from the upper left.
        assert_close(LIGHT_AZIMUTH_DEG, 315.0);
        // Neither at the zenith (which flattens everything) nor on the
        // horizon (which turns half the map black).
        assert_close(LIGHT_ALTITUDE_DEG.clamp(15.0, 75.0), LIGHT_ALTITUDE_DEG);
    }

    #[test]
    fn expressiveness_runs_from_true_scale_to_readable() {
        // Nought is honest terrain, which on a quiet canvas is nearly
        // invisible; one is the loudest the style allows.
        assert_close(relief_z_factor(0.0), 1.0);
        assert_close(relief_z_factor(1.0), RELIEF_Z_FACTOR_MAX);
        assert!(relief_z_factor(0.5) > relief_z_factor(0.25));
        // Out of range is clamped, never inverted.
        assert_close(relief_z_factor(-3.0), 1.0);
        assert_close(relief_z_factor(9.0), RELIEF_Z_FACTOR_MAX);
    }

    #[test]
    fn the_hypsometric_ramp_is_exact_at_stops_and_clamped_outside() {
        assert_eq!(hypsometric_tint(0.0), HYPSOMETRIC_STOPS[0].1);
        let last = HYPSOMETRIC_STOPS[HYPSOMETRIC_STOPS.len() - 1];
        assert_eq!(hypsometric_tint(last.0), last.1);
        assert_eq!(hypsometric_tint(-500.0), HYPSOMETRIC_STOPS[0].1);
        assert_eq!(hypsometric_tint(99_000.0), last.1);
    }

    #[test]
    fn the_ramp_walks_between_its_stops_without_jumping() {
        let (z0, c0) = HYPSOMETRIC_STOPS[0];
        let (z1, c1) = HYPSOMETRIC_STOPS[1];
        let mid = hypsometric_tint(f32::midpoint(z0, z1));
        for channel in 0..3 {
            let (a, b) = (f32::from(c0[channel]), f32::from(c1[channel]));
            let expected = f32::midpoint(a, b);
            let got = f32::from(mid[channel]);
            assert!((got - expected).abs() <= 1.0, "channel {channel}: {got} vs {expected}");
        }
    }

    #[test]
    fn the_ramp_stays_quiet_and_still_separates_lowland_from_peak() {
        // Quiet canvas: no channel screams (nothing saturated), but
        // neither may two heights read as the same ground.
        let distance = |a: [u8; 3], b: [u8; 3]| -> u32 {
            (0..3).map(|c| u32::from(a[c].abs_diff(b[c]))).sum()
        };
        for pair in HYPSOMETRIC_STOPS.windows(2) {
            let step = distance(pair[0].1, pair[1].1);
            assert!(step >= 20, "stops {} and {} too close: {step}", pair[0].0, pair[1].0);
        }
        let low = hypsometric_tint(0.0);
        let peak = hypsometric_tint(HYPSOMETRIC_STOPS[HYPSOMETRIC_STOPS.len() - 1].0);
        assert!(distance(low, peak) >= 50, "lowland and peak too close");
        for stop in HYPSOMETRIC_STOPS {
            let (max, min) = (
                stop.1.iter().max().expect("channel"),
                stop.1.iter().min().expect("channel"),
            );
            assert!(max - min < 60, "stop {:?} is loud for a quiet canvas", stop.0);
        }
    }
}
