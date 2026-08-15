//! Glyph skeletons in em units, y up, baseline at 0.
//!
//! **This is not Noto Sans.** The roadmap asks for a real face, and the
//! roadmap is right about the reason — Unicode has to come from
//! somewhere. What it gets for now is a monoline skeleton font written
//! out here as polylines and stroked into the distance field. The trade
//! is deliberate: the atlas is then byte-identical on every machine, no
//! font file has to be fetched, vendored or licensed, and no golden
//! downstream of it depends on which fonts a developer happens to have
//! installed. Swapping in a real face means replacing this file and the
//! rasteriser's input — nothing above [`crate::Atlas`] knows the
//! difference. Shaping, Cyrillic and CJK stay recorded debt either way.

use num_traits::ToPrimitive;

pub const CHARSET: &str =
    " !\"'(),-./0123456789:;?ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

pub const CAP_HEIGHT_EM: f32 = 0.70;
pub const X_HEIGHT_EM: f32 = 0.50;
pub const ASCENDER_EM: f32 = 0.72;
pub const DESCENDER_EM: f32 = -0.20;

/// Half the pen width the skeletons are stroked with.
pub const STROKE_HALF_EM: f32 = 0.045;
/// Space between a glyph's rightmost ink and the next glyph's origin.
pub const SIDE_BEARING_EM: f32 = 0.08;
/// The one advance that cannot come from a bounding box.
pub const SPACE_ADVANCE_EM: f32 = 0.26;

type Polyline = Vec<(f32, f32)>;

fn line(points: &[(f32, f32)]) -> Polyline {
    points.to_vec()
}

/// A polyline approximation of an elliptical arc, degrees, y up.
fn arc(centre: (f32, f32), radii: (f32, f32), from_deg: f32, to_deg: f32) -> Polyline {
    let steps = 16;
    (0..=steps)
        .map(|i| {
            let t = (to_deg - from_deg).mul_add(i.to_f32().unwrap_or_default() / steps.to_f32().unwrap_or(1.0), from_deg);
            let (sin, cos) = t.to_radians().sin_cos();
            (radii.0.mul_add(cos, centre.0), radii.1.mul_add(sin, centre.1))
        })
        .collect()
}

fn ring(centre: (f32, f32), radii: (f32, f32)) -> Polyline {
    arc(centre, radii, 0.0, 360.0)
}

/// The skeleton of one character, or nothing for a blank or unknown one.
#[must_use]
pub fn strokes(ch: char) -> Vec<Polyline> {
    match ch {
        'A'..='M' => upper_a_m(ch),
        'N'..='Z' => upper_n_z(ch),
        'a'..='m' => lower_a_m(ch),
        'n'..='z' => lower_n_z(ch),
        '0'..='9' => digit(ch),
        _ => punctuation(ch),
    }
}

/// Pen movement after the glyph: its own ink plus one side bearing.
#[must_use]
pub fn advance_em(ch: char) -> f32 {
    if ch == ' ' {
        return SPACE_ADVANCE_EM;
    }
    let right = strokes(ch)
        .iter()
        .flatten()
        .map(|p| p.0)
        .fold(f32::NEG_INFINITY, f32::max);
    if right.is_finite() {
        right + STROKE_HALF_EM + SIDE_BEARING_EM
    } else {
        SPACE_ADVANCE_EM
    }
}

fn upper_a_m(ch: char) -> Vec<Polyline> {
    match ch {
        'A' => vec![line(&[(0.0, 0.0), (0.26, 0.70), (0.52, 0.0)]), line(&[(0.09, 0.24), (0.43, 0.24)])],
        'B' => vec![line(&[(0.0, 0.0), (0.0, 0.70)]), arc((0.02, 0.535), (0.40, 0.165), -90.0, 90.0), arc((0.02, 0.185), (0.46, 0.185), -90.0, 90.0)],
        'C' => vec![arc((0.27, 0.35), (0.25, 0.35), 40.0, 320.0)],
        'D' => vec![line(&[(0.0, 0.0), (0.0, 0.70)]), arc((0.0, 0.35), (0.48, 0.35), -90.0, 90.0)],
        'E' => vec![line(&[(0.50, 0.70), (0.0, 0.70), (0.0, 0.0), (0.50, 0.0)]), line(&[(0.0, 0.36), (0.40, 0.36)])],
        'F' => vec![line(&[(0.50, 0.70), (0.0, 0.70), (0.0, 0.0)]), line(&[(0.0, 0.36), (0.40, 0.36)])],
        'G' => vec![arc((0.27, 0.35), (0.25, 0.35), 40.0, 320.0), line(&[(0.28, 0.30), (0.50, 0.30), (0.50, 0.13)])],
        'H' => vec![line(&[(0.0, 0.0), (0.0, 0.70)]), line(&[(0.52, 0.0), (0.52, 0.70)]), line(&[(0.0, 0.36), (0.52, 0.36)])],
        'I' => vec![line(&[(0.08, 0.0), (0.08, 0.70)])],
        'J' => vec![line(&[(0.40, 0.70), (0.40, 0.18)]), arc((0.20, 0.18), (0.20, 0.18), 0.0, -180.0)],
        'K' => vec![line(&[(0.0, 0.0), (0.0, 0.70)]), line(&[(0.46, 0.70), (0.03, 0.30)]), line(&[(0.14, 0.40), (0.48, 0.0)])],
        'L' => vec![line(&[(0.0, 0.70), (0.0, 0.0), (0.46, 0.0)])],
        _ => vec![line(&[(0.0, 0.0), (0.0, 0.70), (0.29, 0.18), (0.58, 0.70), (0.58, 0.0)])],
    }
}

fn upper_n_z(ch: char) -> Vec<Polyline> {
    match ch {
        'N' => vec![line(&[(0.0, 0.0), (0.0, 0.70), (0.52, 0.0), (0.52, 0.70)])],
        'O' => vec![ring((0.27, 0.35), (0.27, 0.35))],
        'P' => vec![line(&[(0.0, 0.0), (0.0, 0.70)]), arc((0.02, 0.535), (0.44, 0.165), -90.0, 90.0)],
        'Q' => vec![ring((0.27, 0.35), (0.27, 0.35)), line(&[(0.34, 0.16), (0.56, -0.06)])],
        'R' => vec![line(&[(0.0, 0.0), (0.0, 0.70)]), arc((0.02, 0.535), (0.42, 0.165), -90.0, 90.0), line(&[(0.22, 0.37), (0.50, 0.0)])],
        'S' => vec![line(&[(0.46, 0.60), (0.36, 0.69), (0.18, 0.70), (0.05, 0.63), (0.03, 0.52), (0.10, 0.43), (0.26, 0.37), (0.42, 0.31), (0.49, 0.21), (0.46, 0.09), (0.33, 0.01), (0.15, 0.01), (0.04, 0.09)])],
        'T' => vec![line(&[(0.0, 0.70), (0.52, 0.70)]), line(&[(0.26, 0.70), (0.26, 0.0)])],
        'U' => vec![[line(&[(0.0, 0.70)]), arc((0.26, 0.20), (0.26, 0.20), 180.0, 360.0), line(&[(0.52, 0.70)])].concat()],
        'V' => vec![line(&[(0.0, 0.70), (0.26, 0.0), (0.52, 0.70)])],
        'W' => vec![line(&[(0.0, 0.70), (0.16, 0.0), (0.34, 0.48), (0.52, 0.0), (0.68, 0.70)])],
        'X' => vec![line(&[(0.0, 0.0), (0.52, 0.70)]), line(&[(0.0, 0.70), (0.52, 0.0)])],
        'Y' => vec![line(&[(0.0, 0.70), (0.26, 0.36), (0.52, 0.70)]), line(&[(0.26, 0.36), (0.26, 0.0)])],
        _ => vec![line(&[(0.0, 0.70), (0.52, 0.70), (0.0, 0.0), (0.52, 0.0)])],
    }
}

fn lower_a_m(ch: char) -> Vec<Polyline> {
    match ch {
        'a' => vec![ring((0.22, 0.25), (0.20, 0.25)), line(&[(0.42, 0.50), (0.42, 0.0)])],
        'b' => vec![line(&[(0.0, 0.0), (0.0, 0.72)]), ring((0.22, 0.25), (0.20, 0.25))],
        'c' => vec![arc((0.23, 0.25), (0.20, 0.25), 40.0, 320.0)],
        'd' => vec![line(&[(0.42, 0.0), (0.42, 0.72)]), ring((0.22, 0.25), (0.20, 0.25))],
        'e' => vec![line(&[(0.03, 0.27), (0.43, 0.27)]), arc((0.23, 0.25), (0.20, 0.25), 5.0, 320.0)],
        'f' => vec![line(&[(0.36, 0.68), (0.24, 0.72), (0.14, 0.66), (0.14, 0.0)]), line(&[(0.0, 0.50), (0.34, 0.50)])],
        'g' => vec![ring((0.22, 0.25), (0.20, 0.25)), line(&[(0.42, 0.50), (0.42, -0.06), (0.34, -0.17), (0.18, -0.20), (0.06, -0.15)])],
        'h' => vec![line(&[(0.0, 0.0), (0.0, 0.72)]), line(&[(0.0, 0.30), (0.08, 0.44), (0.22, 0.50), (0.36, 0.45), (0.42, 0.32), (0.42, 0.0)])],
        'i' => vec![line(&[(0.08, 0.0), (0.08, 0.50)]), line(&[(0.08, 0.64), (0.08, 0.68)])],
        'j' => vec![line(&[(0.20, 0.50), (0.20, -0.08), (0.12, -0.17), (0.0, -0.18)]), line(&[(0.20, 0.64), (0.20, 0.68)])],
        'k' => vec![line(&[(0.0, 0.0), (0.0, 0.72)]), line(&[(0.38, 0.50), (0.03, 0.20)]), line(&[(0.13, 0.28), (0.40, 0.0)])],
        'l' => vec![line(&[(0.08, 0.0), (0.08, 0.72)])],
        _ => vec![line(&[(0.0, 0.0), (0.0, 0.50)]), line(&[(0.0, 0.32), (0.06, 0.46), (0.17, 0.50), (0.28, 0.44), (0.30, 0.32), (0.30, 0.0)]), line(&[(0.30, 0.32), (0.36, 0.46), (0.47, 0.50), (0.58, 0.44), (0.60, 0.32), (0.60, 0.0)])],
    }
}

fn lower_n_z(ch: char) -> Vec<Polyline> {
    match ch {
        'n' => vec![line(&[(0.0, 0.0), (0.0, 0.50)]), line(&[(0.0, 0.30), (0.08, 0.44), (0.22, 0.50), (0.36, 0.45), (0.42, 0.32), (0.42, 0.0)])],
        'o' => vec![ring((0.22, 0.25), (0.22, 0.25))],
        'p' => vec![line(&[(0.0, -0.20), (0.0, 0.50)]), ring((0.22, 0.25), (0.20, 0.25))],
        'q' => vec![line(&[(0.42, -0.20), (0.42, 0.50)]), ring((0.22, 0.25), (0.20, 0.25))],
        'r' => vec![line(&[(0.0, 0.0), (0.0, 0.50)]), line(&[(0.0, 0.32), (0.10, 0.46), (0.26, 0.50), (0.36, 0.48)])],
        's' => vec![line(&[(0.38, 0.43), (0.28, 0.50), (0.13, 0.50), (0.04, 0.44), (0.05, 0.35), (0.16, 0.30), (0.30, 0.26), (0.38, 0.19), (0.35, 0.06), (0.22, 0.0), (0.08, 0.02), (0.02, 0.08)])],
        't' => vec![line(&[(0.14, 0.68), (0.14, 0.10), (0.22, 0.0), (0.32, 0.02)]), line(&[(0.0, 0.50), (0.32, 0.50)])],
        'u' => vec![line(&[(0.0, 0.50), (0.0, 0.18), (0.08, 0.04), (0.22, 0.0), (0.36, 0.05), (0.42, 0.18)]), line(&[(0.42, 0.50), (0.42, 0.0)])],
        'v' => vec![line(&[(0.0, 0.50), (0.22, 0.0), (0.44, 0.50)])],
        'w' => vec![line(&[(0.0, 0.50), (0.14, 0.0), (0.28, 0.34), (0.42, 0.0), (0.56, 0.50)])],
        'x' => vec![line(&[(0.0, 0.0), (0.42, 0.50)]), line(&[(0.0, 0.50), (0.42, 0.0)])],
        'y' => vec![line(&[(0.0, 0.50), (0.22, 0.02)]), line(&[(0.44, 0.50), (0.16, -0.14), (0.04, -0.19)])],
        _ => vec![line(&[(0.0, 0.50), (0.42, 0.50), (0.0, 0.0), (0.42, 0.0)])],
    }
}

fn digit(ch: char) -> Vec<Polyline> {
    match ch {
        '0' => vec![ring((0.25, 0.35), (0.24, 0.35))],
        '1' => vec![line(&[(0.04, 0.55), (0.24, 0.70), (0.24, 0.0)])],
        '2' => vec![line(&[(0.02, 0.56), (0.08, 0.67), (0.24, 0.71), (0.40, 0.66), (0.45, 0.53), (0.40, 0.40), (0.02, 0.0), (0.48, 0.0)])],
        '3' => vec![line(&[(0.03, 0.62), (0.14, 0.70), (0.30, 0.70), (0.42, 0.62), (0.40, 0.48), (0.24, 0.40)]), line(&[(0.24, 0.40), (0.42, 0.34), (0.46, 0.20), (0.36, 0.04), (0.18, 0.0), (0.04, 0.07)])],
        '4' => vec![line(&[(0.36, 0.0), (0.36, 0.70), (0.0, 0.20), (0.50, 0.20)])],
        '5' => vec![line(&[(0.44, 0.70), (0.10, 0.70), (0.06, 0.42), (0.20, 0.47), (0.36, 0.42), (0.45, 0.29), (0.42, 0.13), (0.28, 0.01), (0.10, 0.03)])],
        '6' => vec![line(&[(0.42, 0.64), (0.28, 0.71), (0.12, 0.63), (0.04, 0.42), (0.04, 0.22)]), ring((0.25, 0.20), (0.21, 0.20))],
        '7' => vec![line(&[(0.0, 0.70), (0.48, 0.70), (0.16, 0.0)])],
        '8' => vec![ring((0.25, 0.53), (0.19, 0.17)), ring((0.25, 0.19), (0.23, 0.19))],
        _ => vec![line(&[(0.08, 0.06), (0.22, -0.01), (0.38, 0.07), (0.46, 0.28), (0.46, 0.48)]), ring((0.25, 0.50), (0.21, 0.20))],
    }
}

fn punctuation(ch: char) -> Vec<Polyline> {
    match ch {
        '!' => vec![line(&[(0.08, 0.70), (0.08, 0.18)]), line(&[(0.08, 0.04), (0.08, 0.02)])],
        '"' => vec![line(&[(0.05, 0.70), (0.03, 0.52)]), line(&[(0.17, 0.70), (0.15, 0.52)])],
        '\'' => vec![line(&[(0.06, 0.70), (0.04, 0.52)])],
        '(' => vec![arc((0.24, 0.30), (0.20, 0.44), 130.0, 230.0)],
        ')' => vec![arc((0.02, 0.30), (0.20, 0.44), -50.0, 50.0)],
        ',' => vec![line(&[(0.10, 0.06), (0.03, -0.10)])],
        '-' => vec![line(&[(0.0, 0.28), (0.28, 0.28)])],
        '.' => vec![line(&[(0.06, 0.04), (0.06, 0.02)])],
        '/' => vec![line(&[(0.0, -0.04), (0.32, 0.72)])],
        ':' => vec![line(&[(0.06, 0.34), (0.06, 0.32)]), line(&[(0.06, 0.04), (0.06, 0.02)])],
        ';' => vec![line(&[(0.06, 0.34), (0.06, 0.32)]), line(&[(0.10, 0.06), (0.03, -0.10)])],
        '?' => vec![line(&[(0.02, 0.56), (0.08, 0.68), (0.24, 0.71), (0.38, 0.64), (0.38, 0.50), (0.22, 0.40), (0.20, 0.24)]), line(&[(0.20, 0.04), (0.20, 0.02)])],
        _ => Vec::new(),
    }
}
