//! Building a tile: used by fixtures now, by the pipeline later.

use maps2_units::TileId;

use crate::varint::{write_varint, zigzag_encode};
use crate::{ClassCode, FeatureDraft, FORMAT_VERSION, MAGIC, RASTER_CLASS_BASE};

const SECTION_ENTRY_BYTES: usize = 10;

enum SectionDraft {
    Vector(Vec<FeatureDraft>),
    Raster(Vec<u8>),
}

pub struct TileBuilder {
    id: TileId,
    sections: Vec<(ClassCode, SectionDraft)>,
}

impl TileBuilder {
    #[must_use]
    pub fn new(id: TileId) -> Self {
        Self { id, sections: Vec::new() }
    }

    /// Append a feature to the section of `class`, creating the section
    /// on first use. Section order in the file is first-use order.
    pub fn push(&mut self, class: ClassCode, feature: FeatureDraft) {
        debug_assert!(!feature.vertices.is_empty(), "a feature needs at least one vertex");
        debug_assert!(class < RASTER_CLASS_BASE, "raster classes take push_raster");
        if let Some((_, SectionDraft::Vector(features))) =
            self.sections.iter_mut().find(|(c, _)| *c == class)
        {
            features.push(feature);
            return;
        }
        self.sections.push((class, SectionDraft::Vector(vec![feature])));
    }

    /// Set the opaque payload of a raster section (heights and future
    /// friends). One payload per class.
    pub fn push_raster(&mut self, class: ClassCode, payload: Vec<u8>) {
        debug_assert!(class >= RASTER_CLASS_BASE, "vector classes take push");
        self.sections.push((class, SectionDraft::Raster(payload)));
    }

    #[must_use]
    pub fn build(&self) -> Vec<u8> {
        let payloads: Vec<Vec<u8>> = self
            .sections
            .iter()
            .map(|(_, section)| match section {
                SectionDraft::Vector(features) => encode_section(features),
                SectionDraft::Raster(payload) => payload.clone(),
            })
            .collect();
        let mut out = header_and_table(self.id, &self.sections, &payloads);
        for payload in &payloads {
            out.extend_from_slice(payload);
        }
        out
    }
}

fn header_and_table(
    id: TileId,
    sections: &[(ClassCode, SectionDraft)],
    payloads: &[Vec<u8>],
) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.push(id.z);
    out.push(0);
    out.extend_from_slice(&id.x.to_le_bytes());
    out.extend_from_slice(&id.y.to_le_bytes());
    out.extend_from_slice(&(sections.len() as u16).to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    let mut offset: u32 = 0;
    for ((class, _), payload) in sections.iter().zip(payloads) {
        out.extend_from_slice(&class.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        offset += payload.len() as u32;
    }
    debug_assert_eq!(out.len(), 20 + sections.len() * SECTION_ENTRY_BYTES);
    out
}

fn encode_section(features: &[FeatureDraft]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(&(features.len() as u16).to_le_bytes());
    for feature in features {
        encode_feature(&mut out, feature);
    }
    out
}

fn encode_feature(out: &mut Vec<u8>, feature: &FeatureDraft) {
    out.extend_from_slice(&feature.id.to_le_bytes());
    out.push(feature.flags);
    out.push(feature.rank);
    out.extend_from_slice(&(feature.name.len() as u16).to_le_bytes());
    out.extend_from_slice(feature.name.as_bytes());
    out.extend_from_slice(&(feature.vertices.len() as u16).to_le_bytes());
    let first = feature.vertices[0];
    out.extend_from_slice(&first.0.to_le_bytes());
    out.extend_from_slice(&first.1.to_le_bytes());
    let mut prev = first;
    for vertex in &feature.vertices[1..] {
        write_varint(out, zigzag_encode(i32::from(vertex.0) - i32::from(prev.0)));
        write_varint(out, zigzag_encode(i32::from(vertex.1) - i32::from(prev.1)));
        prev = *vertex;
    }
}
