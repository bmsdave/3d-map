//! Zigzag varint for geometry deltas: small deltas — one byte.

use crate::TileError;

pub fn zigzag_encode(value: i32) -> u32 {
    ((value << 1) ^ (value >> 31)).cast_unsigned()
}

pub fn zigzag_decode(value: u32) -> i32 {
    (value >> 1).cast_signed() ^ -(value & 1).cast_signed()
}

pub fn write_varint(out: &mut Vec<u8>, mut value: u32) {
    loop {
        let byte = (value & 0x7F) as u8;
        value >>= 7;
        if value == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// Reads one varint; advances `pos`. More than 5 bytes cannot encode a
/// u32 and is reported as damage, not consumed silently.
pub fn read_varint(bytes: &[u8], pos: &mut usize) -> Result<u32, TileError> {
    let mut value: u32 = 0;
    for shift_step in 0..5 {
        let byte = *bytes.get(*pos).ok_or(TileError::Truncated)?;
        *pos += 1;
        let payload = u32::from(byte & 0x7F);
        let shift = shift_step * 7;
        if shift == 28 && payload > 0x0F {
            return Err(TileError::BadVarint);
        }
        value |= payload << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err(TileError::BadVarint)
}
