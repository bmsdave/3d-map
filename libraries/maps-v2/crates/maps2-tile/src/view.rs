//! Reading a tile: views over the caller's bytes, nothing copied.

use std::ops::Range;

use maps2_units::{TileCoord, TileId};

use crate::varint::{read_varint, zigzag_decode};
use crate::{
    ClassCode, FeatureFlags, TileError, TileHeader, FORMAT_VERSION, MAGIC, RASTER_CLASS_BASE,
};

const HEADER_BYTES: usize = 20;
const SECTION_ENTRY_BYTES: usize = 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileView<'a> {
    bytes: &'a [u8],
    header: TileHeader,
    section_count: usize,
}

impl<'a> TileView<'a> {
    /// Validates the header and the section table only; section
    /// payloads are touched lazily, when iterated.
    ///
    /// # Errors
    ///
    /// Returns [`TileError`] when the header or section table is malformed.
    pub fn parse(bytes: &'a [u8]) -> Result<Self, TileError> {
        if bytes.len() < HEADER_BYTES {
            return Err(TileError::TooShort);
        }
        if bytes[0..4] != MAGIC {
            return Err(TileError::BadMagic);
        }
        let version = u16::from_le_bytes([bytes[4], bytes[5]]);
        if version != FORMAT_VERSION {
            return Err(TileError::UnsupportedVersion(version));
        }
        let id = TileId {
            z: bytes[6],
            x: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            y: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        };
        let section_count = usize::from(u16::from_le_bytes([bytes[16], bytes[17]]));
        if bytes.len() < HEADER_BYTES + section_count * SECTION_ENTRY_BYTES {
            return Err(TileError::TooShort);
        }
        Ok(Self { bytes, header: TileHeader { version, id }, section_count })
    }

    #[must_use]
    pub fn header(&self) -> TileHeader {
        self.header
    }

    /// Byte range of a section's payload inside the whole buffer.
    /// Exposed for tests and diagnostics; rendering uses [`Self::section`].
    #[must_use]
    pub fn section_span(&self, class: ClassCode) -> Option<Range<usize>> {
        let payload_base = HEADER_BYTES + self.section_count * SECTION_ENTRY_BYTES;
        for entry in 0..self.section_count {
            let at = HEADER_BYTES + entry * SECTION_ENTRY_BYTES;
            let entry_class = u16::from_le_bytes([self.bytes[at], self.bytes[at + 1]]);
            if entry_class != class {
                continue;
            }
            let offset = u32::from_le_bytes([
                self.bytes[at + 2],
                self.bytes[at + 3],
                self.bytes[at + 4],
                self.bytes[at + 5],
            ]) as usize;
            let len = u32::from_le_bytes([
                self.bytes[at + 6],
                self.bytes[at + 7],
                self.bytes[at + 8],
                self.bytes[at + 9],
            ]) as usize;
            let start = payload_base + offset;
            return Some(start..start + len);
        }
        None
    }

    #[must_use]
    pub fn section(&self, class: ClassCode) -> Option<SectionView<'a>> {
        if class >= RASTER_CLASS_BASE {
            return None;
        }
        let span = self.section_span(class)?;
        let bytes = self.bytes.get(span).unwrap_or(&[]);
        Some(SectionView { bytes })
    }

    /// The opaque payload of a raster section (class ≥ 0xFF00).
    #[must_use]
    pub fn raster(&self, class: ClassCode) -> Option<&'a [u8]> {
        if class < RASTER_CLASS_BASE {
            return None;
        }
        let span = self.section_span(class)?;
        self.bytes.get(span)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SectionView<'a> {
    bytes: &'a [u8],
}

impl<'a> SectionView<'a> {
    #[must_use]
    pub fn features(&self) -> FeaturesIter<'a> {
        if self.bytes.len() < 2 {
            return FeaturesIter { bytes: self.bytes, pos: 0, remaining: 0, damaged: true };
        }
        let count = usize::from(u16::from_le_bytes([self.bytes[0], self.bytes[1]]));
        FeaturesIter { bytes: self.bytes, pos: 2, remaining: count, damaged: false }
    }
}

pub struct FeaturesIter<'a> {
    bytes: &'a [u8],
    pos: usize,
    remaining: usize,
    damaged: bool,
}

impl<'a> Iterator for FeaturesIter<'a> {
    type Item = Result<FeatureView<'a>, TileError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.damaged {
            self.damaged = false;
            self.remaining = 0;
            return Some(Err(TileError::Truncated));
        }
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(self.decode_one().inspect_err(|_| self.remaining = 0))
    }
}

impl<'a> FeaturesIter<'a> {
    fn decode_one(&mut self) -> Result<FeatureView<'a>, TileError> {
        let head = self
            .bytes
            .get(self.pos..self.pos + 8)
            .ok_or(TileError::Truncated)?;
        let id = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let flags = head[4];
        let rank = head[5];
        let name_len = usize::from(u16::from_le_bytes([head[6], head[7]]));
        self.pos += 8;
        let name_bytes = self
            .bytes
            .get(self.pos..self.pos + name_len)
            .ok_or(TileError::Truncated)?;
        let name = std::str::from_utf8(name_bytes).map_err(|_| TileError::BadText)?;
        self.pos += name_len;
        let count_bytes = self
            .bytes
            .get(self.pos..self.pos + 2)
            .ok_or(TileError::Truncated)?;
        let vertex_count = usize::from(u16::from_le_bytes([count_bytes[0], count_bytes[1]]));
        self.pos += 2;
        let geometry_start = self.pos;
        skip_geometry(self.bytes, &mut self.pos, vertex_count)?;
        Ok(FeatureView {
            id,
            flags,
            rank,
            name,
            vertex_count,
            geometry: &self.bytes[geometry_start..self.pos],
        })
    }
}

fn skip_geometry(bytes: &[u8], pos: &mut usize, vertex_count: usize) -> Result<(), TileError> {
    if vertex_count == 0 {
        return Err(TileError::Truncated);
    }
    if bytes.len() < *pos + 4 {
        return Err(TileError::Truncated);
    }
    *pos += 4;
    for _ in 1..vertex_count {
        read_varint(bytes, pos)?;
        read_varint(bytes, pos)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FeatureView<'a> {
    pub id: u32,
    pub flags: FeatureFlags,
    pub rank: u8,
    pub name: &'a str,
    vertex_count: usize,
    geometry: &'a [u8],
}

impl<'a> FeatureView<'a> {
    /// Lazily decodes vertices from the delta stream.
    #[must_use]
    pub fn vertices(&self) -> VerticesIter<'a> {
        VerticesIter {
            bytes: self.geometry,
            pos: 0,
            remaining: self.vertex_count,
            prev: None,
        }
    }
}

pub struct VerticesIter<'a> {
    bytes: &'a [u8],
    pos: usize,
    remaining: usize,
    prev: Option<TileCoord>,
}

impl Iterator for VerticesIter<'_> {
    type Item = Result<TileCoord, TileError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.remaining == 0 {
            return None;
        }
        self.remaining -= 1;
        Some(self.decode_one().inspect_err(|_| self.remaining = 0))
    }
}

impl VerticesIter<'_> {
    fn decode_one(&mut self) -> Result<TileCoord, TileError> {
        let vertex = match self.prev {
            None => {
                let raw = self
                    .bytes
                    .get(self.pos..self.pos + 4)
                    .ok_or(TileError::Truncated)?;
                self.pos += 4;
                TileCoord(
                    u16::from_le_bytes([raw[0], raw[1]]),
                    u16::from_le_bytes([raw[2], raw[3]]),
                )
            }
            Some(prev) => {
                let dx = zigzag_decode(read_varint(self.bytes, &mut self.pos)?);
                let dy = zigzag_decode(read_varint(self.bytes, &mut self.pos)?);
                let x = i32::from(prev.0) + dx;
                let y = i32::from(prev.1) + dy;
                let x = u16::try_from(x).map_err(|_| TileError::DeltaOutOfRange)?;
                let y = u16::try_from(y).map_err(|_| TileError::DeltaOutOfRange)?;
                TileCoord(x, y)
            }
        };
        self.prev = Some(vertex);
        Ok(vertex)
    }
}
