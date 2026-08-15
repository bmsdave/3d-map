//! Building a tile: used by fixtures now, by the pipeline later.

use maps2_units::TileId;

use crate::varint::{write_varint, zigzag_encode};
use crate::{
    BuildingDraft, ClassCode, FeatureDraft, TileError, FORMAT_VERSION, MAGIC, RASTER_CLASS_BASE,
};

const SECTION_ENTRY_BYTES: usize = 10;

enum SectionDraft {
    Vector(Vec<(FeatureDraft, Option<BuildingDraft>)>),
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
        self.push_feature(class, feature, None);
    }

    /// Appends a building footprint with its vertical extent.
    pub fn push_building(&mut self, class: ClassCode, feature: FeatureDraft, building: BuildingDraft) {
        self.push_feature(class, feature, Some(building));
    }

    fn push_feature(&mut self, class: ClassCode, feature: FeatureDraft, building: Option<BuildingDraft>) {
        debug_assert!(!feature.vertices.is_empty(), "a feature needs at least one vertex");
        debug_assert!(class < RASTER_CLASS_BASE, "raster classes take push_raster");
        if let Some((_, SectionDraft::Vector(features))) =
            self.sections.iter_mut().find(|(c, _)| *c == class)
        {
            features.push((feature, building));
            return;
        }
        self.sections.push((class, SectionDraft::Vector(vec![(feature, building)])));
    }

    /// Set the opaque payload of a raster section (heights and future
    /// friends). One payload per class.
    pub fn push_raster(&mut self, class: ClassCode, payload: Vec<u8>) {
        debug_assert!(class >= RASTER_CLASS_BASE, "vector classes take push");
        self.sections.push((class, SectionDraft::Raster(payload)));
    }

    /// # Errors
    ///
    /// Returns [`TileError::TooLarge`] when a v1 length field cannot encode
    /// the supplied data, or [`TileError::EmptyGeometry`] for a feature with
    /// no first vertex.
    pub fn build(&self) -> Result<Vec<u8>, TileError> {
        let payloads: Result<Vec<Vec<u8>>, TileError> = self
            .sections
            .iter()
            .map(|(_, section)| match section {
                SectionDraft::Vector(features) => encode_section(features),
                SectionDraft::Raster(payload) => Ok(payload.clone()),
            })
            .collect();
        let payloads = payloads?;
        let mut out = header_and_table(self.id, &self.sections, &payloads)?;
        for payload in &payloads {
            out.extend_from_slice(payload);
        }
        Ok(out)
    }
}

fn header_and_table(
    id: TileId,
    sections: &[(ClassCode, SectionDraft)],
    payloads: &[Vec<u8>],
) -> Result<Vec<u8>, TileError> {
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
    out.push(id.z);
    out.push(0);
    out.extend_from_slice(&id.x.to_le_bytes());
    out.extend_from_slice(&id.y.to_le_bytes());
    out.extend_from_slice(&checked_u16(sections.len())?.to_le_bytes());
    out.extend_from_slice(&0_u16.to_le_bytes());
    let mut offset: u32 = 0;
    for ((class, _), payload) in sections.iter().zip(payloads) {
        out.extend_from_slice(&class.to_le_bytes());
        out.extend_from_slice(&offset.to_le_bytes());
        let length = checked_u32(payload.len())?;
        out.extend_from_slice(&length.to_le_bytes());
        offset = offset.checked_add(length).ok_or(TileError::TooLarge)?;
    }
    debug_assert_eq!(out.len(), 20 + sections.len() * SECTION_ENTRY_BYTES);
    Ok(out)
}

fn encode_section(features: &[(FeatureDraft, Option<BuildingDraft>)]) -> Result<Vec<u8>, TileError> {
    let mut out = Vec::new();
    out.extend_from_slice(&checked_u16(features.len())?.to_le_bytes());
    for (feature, building) in features {
        encode_feature(&mut out, feature, *building)?;
    }
    Ok(out)
}

fn encode_feature(
    out: &mut Vec<u8>,
    feature: &FeatureDraft,
    building: Option<BuildingDraft>,
) -> Result<(), TileError> {
    let first = *feature.vertices.first().ok_or(TileError::EmptyGeometry)?;
    out.extend_from_slice(&feature.id.to_le_bytes());
    out.push(feature.flags);
    out.push(feature.rank);
    encode_building(out, building)?;
    out.extend_from_slice(&checked_u16(feature.name.len())?.to_le_bytes());
    out.extend_from_slice(feature.name.as_bytes());
    encode_geometry(out, &feature.vertices, first)?;
    out.extend_from_slice(&checked_u16(feature.holes.len())?.to_le_bytes());
    for hole in &feature.holes {
        let first = *hole.first().ok_or(TileError::EmptyGeometry)?;
        encode_geometry(out, hole, first)?;
    }
    Ok(())
}

fn encode_geometry(out: &mut Vec<u8>, vertices: &[maps2_units::TileCoord], first: maps2_units::TileCoord) -> Result<(), TileError> {
    out.extend_from_slice(&checked_u16(vertices.len())?.to_le_bytes());
    out.extend_from_slice(&first.0.to_le_bytes());
    out.extend_from_slice(&first.1.to_le_bytes());
    let mut prev = first;
    for vertex in &vertices[1..] {
        write_varint(out, zigzag_encode(i32::from(vertex.0) - i32::from(prev.0)));
        write_varint(out, zigzag_encode(i32::from(vertex.1) - i32::from(prev.1)));
        prev = *vertex;
    }
    Ok(())
}

fn encode_building(out: &mut Vec<u8>, building: Option<BuildingDraft>) -> Result<(), TileError> {
    let building = building.unwrap_or(BuildingDraft::flat(0, 0));
    if building.top_height_dm != 0 && building.top_height_dm <= building.base_height_dm {
        return Err(TileError::BadBuilding);
    }
    out.extend_from_slice(&building.base_height_dm.to_le_bytes());
    out.extend_from_slice(&building.top_height_dm.to_le_bytes());
    out.push(building.roof as u8);
    Ok(())
}

fn checked_u16(value: usize) -> Result<u16, TileError> {
    u16::try_from(value).map_err(|_| TileError::TooLarge)
}

fn checked_u32(value: usize) -> Result<u32, TileError> {
    u32::try_from(value).map_err(|_| TileError::TooLarge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TileError;
    use maps2_units::TileCoord;

    #[test]
    fn build_rejects_a_section_with_more_than_u16_max_features() {
        let mut builder = TileBuilder::new(TileId { z: 0, x: 0, y: 0 });
        for id in 0..=u16::MAX {
            builder.push(1, FeatureDraft::geometry(u64::from(id), 0, vec![TileCoord(0, 0)]));
        }

        assert_eq!(builder.build(), Err(TileError::TooLarge));
    }
}
