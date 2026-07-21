//! Bounded utility-thumbnail request/result metadata. [ADR-007, ADR-009, ADR-011]

const REQUEST_MAGIC: &[u8; 4] = b"THQ1";
const RESULT_MAGIC: &[u8; 4] = b"THR1";
const FRAME_BYTES: usize = 36;
const MAX_THUMBNAIL_EDGE: u32 = 1024;

/// One bounded page-thumbnail raster request.
#[derive(Debug, Clone, PartialEq)]
pub struct ThumbnailRequest {
    /// Zero-based page index.
    pub page: u32,
    /// Requested RGBA8 output width.
    pub width: u32,
    /// Requested RGBA8 output height.
    pub height: u32,
    /// Engine raster scale.
    pub scale: f32,
    /// Render generation used to discard stale output.
    pub generation: u64,
    /// Document revision used to discard stale output.
    pub revision: u64,
}

/// Validated metadata for pixels written to the job's shared-memory output grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThumbnailResult {
    /// Zero-based page index.
    pub page: u32,
    /// RGBA8 output width.
    pub width: u32,
    /// RGBA8 output height.
    pub height: u32,
    /// Exact number of output bytes.
    pub byte_length: u32,
    /// Render generation copied from the request.
    pub generation: u64,
    /// Document revision copied from the request.
    pub revision: u64,
}

impl ThumbnailResult {
    /// Whether this result still belongs to the coordinator's current state.
    pub fn is_current(&self, generation: u64, revision: u64) -> bool {
        self.generation == generation && self.revision == revision
    }
}

/// Invalid thumbnail metadata at a process trust boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailCodecError {
    /// Magic, length, scale, or byte count is invalid.
    Malformed,
    /// Requested dimensions exceed the declared thumbnail bound.
    LimitExceeded,
}

/// Encode one bounded thumbnail request.
pub fn encode_thumbnail_request(
    request: &ThumbnailRequest,
) -> Result<Vec<u8>, ThumbnailCodecError> {
    validate_dimensions(request.width, request.height)?;
    if !request.scale.is_finite() || request.scale <= 0.0 || request.scale > 16.0 {
        return Err(ThumbnailCodecError::Malformed);
    }
    let mut bytes = Vec::with_capacity(FRAME_BYTES);
    bytes.extend_from_slice(REQUEST_MAGIC);
    bytes.extend_from_slice(&request.page.to_le_bytes());
    bytes.extend_from_slice(&request.width.to_le_bytes());
    bytes.extend_from_slice(&request.height.to_le_bytes());
    bytes.extend_from_slice(&request.scale.to_le_bytes());
    bytes.extend_from_slice(&request.generation.to_le_bytes());
    bytes.extend_from_slice(&request.revision.to_le_bytes());
    Ok(bytes)
}

/// Decode one bounded thumbnail request.
pub fn decode_thumbnail_request(bytes: &[u8]) -> Result<ThumbnailRequest, ThumbnailCodecError> {
    if bytes.len() != FRAME_BYTES || &bytes[..4] != REQUEST_MAGIC {
        return Err(ThumbnailCodecError::Malformed);
    }
    let request = ThumbnailRequest {
        page: u32_at(bytes, 4),
        width: u32_at(bytes, 8),
        height: u32_at(bytes, 12),
        scale: f32::from_le_bytes(bytes[16..20].try_into().expect("four bytes")),
        generation: u64_at(bytes, 20),
        revision: u64_at(bytes, 28),
    };
    encode_thumbnail_request(&request)?;
    Ok(request)
}

/// Encode validated thumbnail output metadata.
pub fn encode_thumbnail_result(result: &ThumbnailResult) -> Result<Vec<u8>, ThumbnailCodecError> {
    let byte_length = validate_dimensions(result.width, result.height)?;
    if result.byte_length != byte_length {
        return Err(ThumbnailCodecError::Malformed);
    }
    let mut bytes = Vec::with_capacity(FRAME_BYTES);
    bytes.extend_from_slice(RESULT_MAGIC);
    bytes.extend_from_slice(&result.page.to_le_bytes());
    bytes.extend_from_slice(&result.width.to_le_bytes());
    bytes.extend_from_slice(&result.height.to_le_bytes());
    bytes.extend_from_slice(&result.byte_length.to_le_bytes());
    bytes.extend_from_slice(&result.generation.to_le_bytes());
    bytes.extend_from_slice(&result.revision.to_le_bytes());
    Ok(bytes)
}

/// Decode and validate thumbnail output metadata.
pub fn decode_thumbnail_result(bytes: &[u8]) -> Result<ThumbnailResult, ThumbnailCodecError> {
    if bytes.len() != FRAME_BYTES || &bytes[..4] != RESULT_MAGIC {
        return Err(ThumbnailCodecError::Malformed);
    }
    let result = ThumbnailResult {
        page: u32_at(bytes, 4),
        width: u32_at(bytes, 8),
        height: u32_at(bytes, 12),
        byte_length: u32_at(bytes, 16),
        generation: u64_at(bytes, 20),
        revision: u64_at(bytes, 28),
    };
    encode_thumbnail_result(&result)?;
    Ok(result)
}

fn validate_dimensions(width: u32, height: u32) -> Result<u32, ThumbnailCodecError> {
    if width == 0 || height == 0 || width > MAX_THUMBNAIL_EDGE || height > MAX_THUMBNAIL_EDGE {
        return Err(ThumbnailCodecError::LimitExceeded);
    }
    width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(ThumbnailCodecError::LimitExceeded)
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("four bytes"))
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("eight bytes"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_request_and_result_round_trip() {
        let request = ThumbnailRequest {
            page: 7,
            width: 160,
            height: 240,
            scale: 0.25,
            generation: 3,
            revision: 9,
        };
        assert_eq!(
            decode_thumbnail_request(&encode_thumbnail_request(&request).unwrap()).unwrap(),
            request
        );

        let result = ThumbnailResult {
            page: 7,
            width: 160,
            height: 240,
            byte_length: 153_600,
            generation: 3,
            revision: 9,
        };
        assert_eq!(
            decode_thumbnail_result(&encode_thumbnail_result(&result).unwrap()).unwrap(),
            result
        );
        assert!(result.is_current(3, 9));
        assert!(!result.is_current(4, 9));
    }

    #[test]
    fn thumbnail_request_rejects_oversized_dimensions() {
        let request = ThumbnailRequest {
            page: 0,
            width: 1025,
            height: 1,
            scale: 1.0,
            generation: 0,
            revision: 0,
        };
        assert_eq!(
            encode_thumbnail_request(&request),
            Err(ThumbnailCodecError::LimitExceeded)
        );
    }
}
