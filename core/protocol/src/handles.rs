//! Shared-memory tile descriptors for cross-process bulk pixels. [ADR-004, ADR-007, SDS §6]
//!
//! Control plane still uses framed IPC; this module describes **where** pixels live
//! inside a pre-negotiated shared region (not how to create the region).

/// Device-space tile edge in logical pixels. [SDS §6.2]
pub const TILE_EDGE_PX: u32 = 256;

/// Bytes for one RGBA8 tile (`TILE_EDGE_PX`² × 4). [GR-7 bound unit]
pub const TILE_RGBA8_BYTES: usize =
    (TILE_EDGE_PX as usize) * (TILE_EDGE_PX as usize) * 4;

/// Pixel layout in a tile buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 8-bit per channel, RGBA order, tightly packed.
    Rgba8,
}

impl PixelFormat {
    /// Bytes per pixel for this format.
    pub fn bytes_per_pixel(self) -> usize {
        match self {
            PixelFormat::Rgba8 => 4,
        }
    }

    /// Wire label for text codecs.
    pub fn as_str(self) -> &'static str {
        match self {
            PixelFormat::Rgba8 => "rgba8",
        }
    }

    /// Parse wire label.
    pub fn from_str_label(s: &str) -> Option<Self> {
        match s {
            "rgba8" | "RGBA8" => Some(PixelFormat::Rgba8),
            _ => None,
        }
    }
}

/// Descriptor for one tile slot inside a shared region. [SDS §6.3]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TileSlotDesc {
    /// Byte offset into the shared region.
    pub offset: u32,
    /// Byte length of the tile payload.
    pub len: u32,
    /// Pixel format of the payload.
    pub format: PixelFormat,
    /// Render generation (stale if mismatched). [SDS §5.3]
    pub generation: u64,
}

/// Magic prefix the worker writes for the M0 shmem smoke test.
pub const SHMEM_SMOKE_MAGIC: &[u8; 8] = b"PDFSHMEM";

/// Error decoding a `TILE_READY` frame body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TileReadyDecodeError {
    /// Not valid UTF-8.
    InvalidUtf8,
    /// Missing/bad header or version.
    BadHeader,
    /// Missing or invalid field.
    BadField(&'static str),
}

impl std::fmt::Display for TileReadyDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TileReadyDecodeError::InvalidUtf8 => write!(f, "tile ready not utf-8"),
            TileReadyDecodeError::BadHeader => write!(f, "bad TILE_READY header"),
            TileReadyDecodeError::BadField(k) => write!(f, "bad field: {k}"),
        }
    }
}

impl std::error::Error for TileReadyDecodeError {}

/// Encode a `TILE_READY` control-frame body (text v1).
pub fn encode_tile_ready(d: &TileSlotDesc) -> Vec<u8> {
    format!(
        "TILE_READY\nv1\noffset={}\nlen={}\nformat={}\ngeneration={}\n",
        d.offset,
        d.len,
        d.format.as_str(),
        d.generation
    )
    .into_bytes()
}

/// Decode a `TILE_READY` frame body.
pub fn decode_tile_ready(body: &[u8]) -> Result<TileSlotDesc, TileReadyDecodeError> {
    let text = std::str::from_utf8(body).map_err(|_| TileReadyDecodeError::InvalidUtf8)?;
    let mut lines = text.lines();
    if lines.next() != Some("TILE_READY") {
        return Err(TileReadyDecodeError::BadHeader);
    }
    if lines.next() != Some("v1") {
        return Err(TileReadyDecodeError::BadHeader);
    }
    let mut offset = None;
    let mut len = None;
    let mut format = None;
    let mut generation = None;
    for line in lines {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "offset" => {
                offset = Some(
                    v.parse()
                        .map_err(|_| TileReadyDecodeError::BadField("offset"))?,
                );
            }
            "len" => {
                len = Some(
                    v.parse()
                        .map_err(|_| TileReadyDecodeError::BadField("len"))?,
                );
            }
            "format" => {
                format = Some(
                    PixelFormat::from_str_label(v)
                        .ok_or(TileReadyDecodeError::BadField("format"))?,
                );
            }
            "generation" => {
                generation = Some(
                    v.parse()
                        .map_err(|_| TileReadyDecodeError::BadField("generation"))?,
                );
            }
            _ => {}
        }
    }
    Ok(TileSlotDesc {
        offset: offset.ok_or(TileReadyDecodeError::BadField("offset"))?,
        len: len.ok_or(TileReadyDecodeError::BadField("len"))?,
        format: format.ok_or(TileReadyDecodeError::BadField("format"))?,
        generation: generation.ok_or(TileReadyDecodeError::BadField("generation"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_rgba8_size() {
        assert_eq!(TILE_RGBA8_BYTES, 262_144);
    }

    #[test]
    fn tile_ready_codec_roundtrip() {
        let d = TileSlotDesc {
            offset: 0,
            len: TILE_RGBA8_BYTES as u32,
            format: PixelFormat::Rgba8,
            generation: 3,
        };
        let bytes = encode_tile_ready(&d);
        assert_eq!(decode_tile_ready(&bytes).unwrap(), d);
    }
}
