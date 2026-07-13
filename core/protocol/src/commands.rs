//! Commands: shell/CLI → coordinator → worker. [ADR-031, SDS §5]
//!
//! M0: text-based protocol over framed IPC. Commands are newline-delimited
//! key=value pairs with a type header. Upgrade to bincode later if needed.

/// Error decoding a command frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandDecodeError {
    /// Not valid UTF-8.
    InvalidUtf8,
    /// Missing or unknown command type header.
    UnknownCommand,
    /// Missing or invalid field.
    BadField(&'static str),
}

impl std::fmt::Display for CommandDecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUtf8 => write!(f, "command not utf-8"),
            Self::UnknownCommand => write!(f, "unknown command type"),
            Self::BadField(k) => write!(f, "bad field: {k}"),
        }
    }
}

impl std::error::Error for CommandDecodeError {}

/// A render-tile request sent from coordinator to worker. [ADR-007, SDS §6]
#[derive(Debug, Clone, PartialEq)]
pub struct RenderTileRequest {
    /// 0-based page index.
    pub page: u32,
    /// Device-space X origin.
    pub x: u32,
    /// Device-space Y origin.
    pub y: u32,
    /// Width in device pixels.
    pub w: u32,
    /// Height in device pixels.
    pub h: u32,
    /// Scale factor.
    pub scale: f32,
    /// Render generation for invalidation.
    pub generation: u64,
    /// Byte offset into the shared-memory region where output goes.
    pub slot_offset: u32,
}

/// Encode a `RenderTileRequest` as a text frame body.
pub fn encode_render_tile(req: &RenderTileRequest) -> Vec<u8> {
    format!(
        "render_tile\nv1\npage={}\nx={}\ny={}\nw={}\nh={}\nscale={}\ngeneration={}\nslot_offset={}\n",
        req.page, req.x, req.y, req.w, req.h, req.scale, req.generation, req.slot_offset,
    )
    .into_bytes()
}

/// Decode a `RenderTileRequest` from a text frame body.
pub fn decode_render_tile(body: &[u8]) -> Result<RenderTileRequest, CommandDecodeError> {
    let text = std::str::from_utf8(body).map_err(|_| CommandDecodeError::InvalidUtf8)?;
    let mut lines = text.lines();
    if lines.next() != Some("render_tile") {
        return Err(CommandDecodeError::UnknownCommand);
    }
    if lines.next() != Some("v1") {
        return Err(CommandDecodeError::UnknownCommand);
    }
    let mut page = None;
    let mut x = None;
    let mut y = None;
    let mut w = None;
    let mut h = None;
    let mut scale = None;
    let mut generation = None;
    let mut slot_offset = None;
    for line in lines {
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        match k {
            "page" => page = Some(v.parse().map_err(|_| CommandDecodeError::BadField("page"))?),
            "x" => x = Some(v.parse().map_err(|_| CommandDecodeError::BadField("x"))?),
            "y" => y = Some(v.parse().map_err(|_| CommandDecodeError::BadField("y"))?),
            "w" => w = Some(v.parse().map_err(|_| CommandDecodeError::BadField("w"))?),
            "h" => h = Some(v.parse().map_err(|_| CommandDecodeError::BadField("h"))?),
            "scale" => {
                scale = Some(v.parse().map_err(|_| CommandDecodeError::BadField("scale"))?)
            }
            "generation" => {
                generation =
                    Some(v.parse().map_err(|_| CommandDecodeError::BadField("generation"))?)
            }
            "slot_offset" => {
                slot_offset =
                    Some(v.parse().map_err(|_| CommandDecodeError::BadField("slot_offset"))?)
            }
            _ => {}
        }
    }
    Ok(RenderTileRequest {
        page: page.ok_or(CommandDecodeError::BadField("page"))?,
        x: x.ok_or(CommandDecodeError::BadField("x"))?,
        y: y.ok_or(CommandDecodeError::BadField("y"))?,
        w: w.ok_or(CommandDecodeError::BadField("w"))?,
        h: h.ok_or(CommandDecodeError::BadField("h"))?,
        scale: scale.ok_or(CommandDecodeError::BadField("scale"))?,
        generation: generation.ok_or(CommandDecodeError::BadField("generation"))?,
        slot_offset: slot_offset.ok_or(CommandDecodeError::BadField("slot_offset"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_tile_codec_roundtrip() {
        let req = RenderTileRequest {
            page: 0,
            x: 0,
            y: 0,
            w: 256,
            h: 256,
            scale: 1.5,
            generation: 42,
            slot_offset: 0,
        };
        let bytes = encode_render_tile(&req);
        let decoded = decode_render_tile(&bytes).unwrap();
        assert_eq!(req, decoded);
    }

    #[test]
    fn render_tile_with_offset() {
        let req = RenderTileRequest {
            page: 3,
            x: 256,
            y: 512,
            w: 128,
            h: 64,
            scale: 2.0,
            generation: 7,
            slot_offset: 262_144,
        };
        let bytes = encode_render_tile(&req);
        let decoded = decode_render_tile(&bytes).unwrap();
        assert_eq!(req, decoded);
    }
}
