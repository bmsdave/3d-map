//! What the data means and how it is shown: classes, entry bands, the
//! quiet-canvas palette, zoom ramps.
//!
//! The tile carries raw class codes; this crate owns their semantics.
//! A band is an eligibility gate, not a display rule: geometry classes
//! enter whole at their band, point classes (POI, labels) additionally
//! compete by rank after admission. Colour and width live here, never
//! in the data package — changing the palette must not rebuild tiles.

mod relief;

pub use relief::{
    hypsometric_tint, relief_z_factor, HYPSOMETRIC_STOPS, LIGHT_ALTITUDE_DEG, LIGHT_AZIMUTH_DEG,
    RELIEF_Z_FACTOR_MAX,
};

use maps2_units::Zoom;

/// Everything a tile section can be. Discriminants are the wire codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum Class {
    Land = 0,
    Water = 1,
    Park = 2,
    Building = 3,
    RoadMotorway = 4,
    RoadTrunk = 5,
    RoadPrimary = 6,
    RoadSecondary = 7,
    RoadResidential = 8,
    RoadService = 9,
    RoadPath = 10,
    Poi = 11,
    Label = 12,
}

pub const ALL_CLASSES: [Class; 13] = [
    Class::Land,
    Class::Water,
    Class::Park,
    Class::Building,
    Class::RoadMotorway,
    Class::RoadTrunk,
    Class::RoadPrimary,
    Class::RoadSecondary,
    Class::RoadResidential,
    Class::RoadService,
    Class::RoadPath,
    Class::Poi,
    Class::Label,
];

impl Class {
    #[must_use]
    pub fn code(self) -> u16 {
        self as u16
    }

    #[must_use]
    pub fn from_code(code: u16) -> Option<Self> {
        ALL_CLASSES.into_iter().find(|c| c.code() == code)
    }

    /// Draw order rank for roads: higher passes over lower at junctions.
    #[must_use]
    pub fn road_rank(self) -> Option<u8> {
        match self {
            Class::RoadMotorway => Some(6),
            Class::RoadTrunk => Some(5),
            Class::RoadPrimary => Some(4),
            Class::RoadSecondary => Some(3),
            Class::RoadResidential => Some(2),
            Class::RoadService => Some(1),
            Class::RoadPath => Some(0),
            _ => None,
        }
    }
}

/// The seven entry bands. A class is absent below its band and hands
/// over above it; for point classes the band is only admission.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    World,
    Region,
    City,
    District,
    Street,
    Address,
    Micro,
}

pub const ALL_BANDS: [Band; 7] = [
    Band::World,
    Band::Region,
    Band::City,
    Band::District,
    Band::Street,
    Band::Address,
    Band::Micro,
];

impl Band {
    /// Band by its name as shown in cards and debug ("Street").
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        ALL_BANDS.into_iter().find(|b| format!("{b:?}") == name)
    }

    #[must_use]
    pub fn entry_zoom(self) -> f64 {
        match self {
            Band::World => 0.0,
            Band::Region => 5.0,
            Band::City => 8.0,
            Band::District => 10.0,
            Band::Street => 12.0,
            Band::Address => 14.0,
            Band::Micro => 16.0,
        }
    }

    /// The band a camera zoom falls into.
    #[must_use]
    pub fn at(zoom: Zoom) -> Self {
        let mut current = Band::World;
        for band in ALL_BANDS {
            if zoom.value() >= band.entry_zoom() {
                current = band;
            }
        }
        current
    }
}

/// The band a class becomes eligible at.
#[must_use]
pub fn entry_band(class: Class) -> Band {
    match class {
        Class::Land | Class::Water | Class::Label => Band::World,
        Class::RoadMotorway | Class::RoadTrunk => Band::Region,
        Class::Park | Class::RoadPrimary => Band::City,
        Class::RoadSecondary => Band::District,
        Class::RoadResidential => Band::Street,
        Class::RoadService | Class::RoadPath | Class::Poi => Band::Address,
        Class::Building => Band::Micro,
    }
}

/// RGBA, 0–255. The quiet canvas: a light, low-contrast base that
/// analytics overlays read on top of; contrast is reserved for text.
#[must_use]
pub fn fill_color(class: Class) -> [u8; 4] {
    match class {
        Class::Land => [0xF8, 0xF7, 0xF4, 0xFF],
        Class::Water => [0xC9, 0xD6, 0xDB, 0xFF],
        Class::Park => [0xE2, 0xEA, 0xDD, 0xFF],
        Class::Building => [0xEC, 0xE9, 0xE4, 0xFF],
        // Roads fill white over a grey casing; POI and labels are not
        // fills, the values are their accent colours.
        Class::RoadMotorway
        | Class::RoadTrunk
        | Class::RoadPrimary
        | Class::RoadSecondary
        | Class::RoadResidential
        | Class::RoadService
        | Class::RoadPath => [0xFF, 0xFF, 0xFF, 0xFF],
        Class::Poi => [0x8A, 0x8A, 0x86, 0xFF],
        Class::Label => [0x3A, 0x3A, 0x37, 0xFF],
    }
}

/// What is behind the world: the space around the globe, and the gap
/// where no tile has arrived. Deliberately not the land colour — an
/// empty canvas that reads as ground is a map that lies.
#[must_use]
pub fn background_color() -> [u8; 4] {
    [0xE3, 0xE5, 0xE7, 0xFF]
}

/// A piecewise-linear ramp over zoom: exact at stops, linear between,
/// clamped outside. Stops must be sorted by zoom.
#[derive(Clone, Copy, Debug)]
pub struct Ramp<const N: usize> {
    stops: [(f64, f32); N],
}

impl<const N: usize> Ramp<N> {
    #[must_use]
    pub const fn new(stops: [(f64, f32); N]) -> Self {
        Self { stops }
    }

    #[must_use]
    pub fn sample(&self, zoom: Zoom) -> f32 {
        let z = zoom.value();
        let (first, last) = (self.stops[0], self.stops[N - 1]);
        if z <= first.0 {
            return first.1;
        }
        if z >= last.0 {
            return last.1;
        }
        for window in self.stops.windows(2) {
            let (z0, v0) = window[0];
            let (z1, v1) = window[1];
            if z >= z0 && z <= z1 {
                let t = ((z - z0) / (z1 - z0)) as f32;
                return v0 + (v1 - v0) * t;
            }
        }
        last.1
    }
}

/// Road fill width in screen pixels — a symbol width, not a ground
/// measurement (v1 baked metres and grew a 48 km road).
#[must_use]
pub fn road_width_px(class: Class, zoom: Zoom) -> f32 {
    match class {
        Class::RoadMotorway => {
            Ramp::new([(5.0, 1.5), (10.0, 3.0), (14.0, 6.0), (18.0, 14.0)]).sample(zoom)
        }
        Class::RoadTrunk => {
            Ramp::new([(5.0, 1.2), (10.0, 2.5), (14.0, 5.0), (18.0, 12.0)]).sample(zoom)
        }
        Class::RoadPrimary => {
            Ramp::new([(8.0, 1.0), (12.0, 2.5), (16.0, 5.0), (18.0, 10.0)]).sample(zoom)
        }
        Class::RoadSecondary => Ramp::new([(10.0, 1.0), (14.0, 2.5), (18.0, 8.0)]).sample(zoom),
        Class::RoadResidential => Ramp::new([(12.0, 1.0), (16.0, 3.0), (18.0, 6.0)]).sample(zoom),
        Class::RoadService => Ramp::new([(14.0, 0.8), (18.0, 4.0)]).sample(zoom),
        Class::RoadPath => Ramp::new([(14.0, 0.6), (18.0, 2.0)]).sample(zoom),
        _ => 0.0,
    }
}

/// Casing adds this many pixels around a road fill.
pub const CASING_EXTRA_PX: f32 = 2.0;

/// Feature flag bits. The format only carries the byte (see
/// `docs/tile-format.md`); what a bit means is decided here.
pub const FLAG_ONE_WAY: u8 = 1 << 0;
pub const FLAG_BRIDGE: u8 = 1 << 1;
pub const FLAG_TUNNEL: u8 = 1 << 2;

/// The grey every road is outlined with. One colour for all classes on
/// purpose: casings that share a colour merge at a junction into a
/// single outline instead of drawing a seam through it.
pub const ROAD_CASING_COLOR: [u8; 4] = [0xA8, 0xA6, 0xA1, 0xFF];

/// A tunnel is the same road seen through the ground: dimmed, and its
/// casing dashed. Lengths are screen pixels, like every road width.
pub const TUNNEL_ALPHA: f32 = 0.55;
pub const TUNNEL_DASH_PERIOD_PX: f32 = 14.0;
pub const TUNNEL_DASH_ON_PX: f32 = 8.0;

/// Width of the entry ramp in zoom units: a class fades in across
/// `[entry, entry + RAMP_SPAN]` instead of popping. Below its entry a
/// class does not exist — its tiles do not even carry it.
pub const RAMP_SPAN: f64 = 0.5;

/// Composition alpha of a class at a zoom. An override pins the
/// composition to a band regardless of zoom (the lab's toggle cards и
/// debugging) — then the answer is exact 0 or 1.
#[must_use]
pub fn class_alpha(class: Class, zoom: Zoom, override_band: Option<Band>) -> f32 {
    let entry = entry_band(class);
    if let Some(band) = override_band {
        return if entry <= band { 1.0 } else { 0.0 };
    }
    let from_entry = zoom.value() - entry.entry_zoom();
    if entry == Band::World {
        return 1.0;
    }
    (from_entry / RAMP_SPAN).clamp(0.0, 1.0) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn class_codes_round_trip_and_unknown_codes_are_none() {
        for class in ALL_CLASSES {
            assert_eq!(Class::from_code(class.code()), Some(class));
        }
        assert_eq!(Class::from_code(999), None);
    }

    #[test]
    fn every_class_has_a_band_and_buildings_wait_for_micro() {
        assert_eq!(entry_band(Class::Land), Band::World);
        assert_eq!(entry_band(Class::Building), Band::Micro);
        assert_eq!(entry_band(Class::RoadResidential), Band::Street);
        // The v1 fault on record: buildings at z10 would be a lie.
        assert!(entry_band(Class::Building).entry_zoom() > 10.0);
    }

    #[test]
    fn band_at_zoom_follows_entry_boundaries() {
        assert_eq!(Band::at(Zoom::new(0.0)), Band::World);
        assert_eq!(Band::at(Zoom::new(4.99)), Band::World);
        assert_eq!(Band::at(Zoom::new(5.0)), Band::Region);
        assert_eq!(Band::at(Zoom::new(14.37)), Band::Address);
        assert_eq!(Band::at(Zoom::new(22.0)), Band::Micro);
    }

    #[test]
    fn ramps_are_exact_at_stops_linear_between_and_clamped_outside() {
        let ramp = Ramp::new([(10.0, 2.0), (14.0, 6.0)]);
        assert_eq!(ramp.sample(Zoom::new(10.0)), 2.0);
        assert_eq!(ramp.sample(Zoom::new(14.0)), 6.0);
        assert_eq!(ramp.sample(Zoom::new(12.0)), 4.0);
        assert_eq!(ramp.sample(Zoom::new(0.0)), 2.0);
        assert_eq!(ramp.sample(Zoom::new(20.0)), 6.0);
    }

    #[test]
    fn road_widths_grow_with_zoom_and_with_class_rank() {
        let z14 = Zoom::new(14.0);
        assert!(road_width_px(Class::RoadMotorway, z14) > road_width_px(Class::RoadService, z14));
        assert!(
            road_width_px(Class::RoadPrimary, Zoom::new(16.0))
                > road_width_px(Class::RoadPrimary, Zoom::new(12.0))
        );
        assert_eq!(road_width_px(Class::Water, z14), 0.0);
    }

    #[test]
    fn quiet_canvas_keeps_neighbouring_fills_distinct() {
        let distinct = [Class::Land, Class::Water, Class::Park, Class::Building];
        for (i, a) in distinct.iter().enumerate() {
            for b in &distinct[i + 1..] {
                assert_ne!(fill_color(*a), fill_color(*b), "{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn the_void_behind_the_world_is_not_mistakable_for_land() {
        let void = background_color();
        let land = fill_color(Class::Land);
        let distance: u32 = (0..3).map(|c| u32::from(void[c].abs_diff(land[c]))).sum();
        assert!(distance >= 20, "the empty canvas reads as ground: {distance}");
    }

    #[test]
    fn composition_alpha_ramps_in_at_the_entry_and_obeys_overrides() {
        let c = Class::Building; // entry Micro, z16
        assert_eq!(class_alpha(c, Zoom::new(15.9), None), 0.0);
        assert_eq!(class_alpha(c, Zoom::new(16.0), None), 0.0);
        let mid = class_alpha(c, Zoom::new(16.25), None);
        assert!((mid - 0.5).abs() < 1e-6, "mid was {mid}");
        assert_eq!(class_alpha(c, Zoom::new(16.5), None), 1.0);
        assert_eq!(class_alpha(c, Zoom::new(20.0), None), 1.0);
        // Override pins composition regardless of zoom, exactly.
        assert_eq!(class_alpha(c, Zoom::new(5.0), Some(Band::Micro)), 1.0);
        assert_eq!(class_alpha(c, Zoom::new(20.0), Some(Band::Street)), 0.0);
        // World classes are always fully present.
        assert_eq!(class_alpha(Class::Land, Zoom::new(0.0), None), 1.0);
    }

    #[test]
    fn the_casing_is_darker_than_every_road_fill() {
        let luminance = |c: [u8; 4]| u32::from(c[0]) + u32::from(c[1]) + u32::from(c[2]);
        for class in ALL_CLASSES {
            if class.road_rank().is_none() {
                continue;
            }
            assert!(
                luminance(ROAD_CASING_COLOR) < luminance(fill_color(class)),
                "{class:?} casing must be darker than its fill",
            );
        }
    }

    #[test]
    fn a_feature_can_carry_flags_independently() {
        let bits = [FLAG_ONE_WAY, FLAG_BRIDGE, FLAG_TUNNEL];
        for (i, a) in bits.iter().enumerate() {
            assert_eq!(a.count_ones(), 1, "flag {i} must be a single bit");
            for b in &bits[i + 1..] {
                assert_eq!(a & b, 0, "flags must not share a bit");
            }
        }
    }

    #[test]
    fn road_ranks_order_the_draw_passes() {
        assert!(Class::RoadMotorway.road_rank() > Class::RoadService.road_rank());
        assert_eq!(Class::Water.road_rank(), None);
    }
}
