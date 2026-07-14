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

    /// Parse from a numeric wire value (used by event codec).
    pub fn from_u32(v: u32) -> Option<Self> {
        match v {
            0 => Some(PixelFormat::Rgba8),
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
    /// 0-based page index this tile belongs to.
    pub page: u32,
    /// Column in the tile grid.
    pub col: u32,
    /// Row in the tile grid.
    pub row: u32,
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

/// Encode a `TILE_READY` control-frame body (text v2).
///
/// v2 adds `page`, `col`, `row` fields so the coordinator can correlate
/// responses without a pending-request map. v1 decoders ignore unknown fields
/// so this is backward-compatible.
pub fn encode_tile_ready(d: &TileSlotDesc) -> Vec<u8> {
    format!(
        "TILE_READY\nv2\noffset={}\nlen={}\nformat={}\ngeneration={}\npage={}\ncol={}\nrow={}\n",
        d.offset,
        d.len,
        d.format.as_str(),
        d.generation,
        d.page,
        d.col,
        d.row,
    )
    .into_bytes()
}

/// Decode a `TILE_READY` frame body (v1 or v2).
///
/// v2 includes tile identity fields (`page`, `col`, `row`). v1 frames
/// default these to 0 for backward compatibility.
pub fn decode_tile_ready(body: &[u8]) -> Result<TileSlotDesc, TileReadyDecodeError> {
    let text = std::str::from_utf8(body).map_err(|_| TileReadyDecodeError::InvalidUtf8)?;
    let mut lines = text.lines();
    if lines.next() != Some("TILE_READY") {
        return Err(TileReadyDecodeError::BadHeader);
    }
    let version = lines.next().ok_or(TileReadyDecodeError::BadHeader)?;
    // Accept both v1 and v2.
    if version != "v1" && version != "v2" {
        return Err(TileReadyDecodeError::BadHeader);
    }
    let mut offset = None;
    let mut len = None;
    let mut format = None;
    let mut generation = None;
    let mut page = 0u32;
    let mut col = 0u32;
    let mut row = 0u32;
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
            "page" => {
                page = v.parse().map_err(|_| TileReadyDecodeError::BadField("page"))?;
            }
            "col" => {
                col = v.parse().map_err(|_| TileReadyDecodeError::BadField("col"))?;
            }
            "row" => {
                row = v.parse().map_err(|_| TileReadyDecodeError::BadField("row"))?;
            }
            _ => {}
        }
    }
    Ok(TileSlotDesc {
        offset: offset.ok_or(TileReadyDecodeError::BadField("offset"))?,
        len: len.ok_or(TileReadyDecodeError::BadField("len"))?,
        format: format.ok_or(TileReadyDecodeError::BadField("format"))?,
        generation: generation.ok_or(TileReadyDecodeError::BadField("generation"))?,
        page,
        col,
        row,
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
            page: 1,
            col: 2,
            row: 3,
        };
        let bytes = encode_tile_ready(&d);
        assert_eq!(decode_tile_ready(&bytes).unwrap(), d);
    }

    #[test]
    fn tile_ready_v1_backward_compat() {
        // v1 frames should decode with page/col/row defaulting to 0.
        let v1_body = b"TILE_READY\nv1\noffset=0\nlen=262144\nformat=rgba8\ngeneration=1\n";
        let desc = decode_tile_ready(v1_body).unwrap();
        assert_eq!(desc.offset, 0);
        assert_eq!(desc.generation, 1);
        assert_eq!(desc.page, 0);
        assert_eq!(desc.col, 0);
        assert_eq!(desc.row, 0);
    }
}
