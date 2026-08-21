//! Path-free utility indexing metadata for canonical text in shared memory. [ADR-019]

const MAGIC: &[u8; 4] = b"IXQ1";
const FRAME_BYTES: usize = 37;
const MAX_TEXT_BYTES: u32 = 16 * 1024 * 1024;

/// Metadata paired with one bounded shared-memory canonical-text input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexUtilityRequest {
    /// Opaque source identity assigned in Z0.
    pub source: [u8; 16],
    /// Source file revision identity.
    pub revision: u64,
    /// Zero-based page index.
    pub page: u32,
    /// Canonical extraction reliability flag.
    pub reliable: bool,
    /// Exact UTF-8 bytes in the shared-memory input.
    pub text_length: u32,
}

/// Invalid bounded index request metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndexUtilityCodecError {
    /// Magic, length, boolean, or trailing bytes are invalid.
    Malformed,
    /// Canonical text exceeds the fixed utility shared-memory ceiling.
    LimitExceeded,
}

/// Encode path-free indexing metadata.
pub fn encode_index_request(
    request: &IndexUtilityRequest,
) -> Result<Vec<u8>, IndexUtilityCodecError> {
    if request.text_length > MAX_TEXT_BYTES {
        return Err(IndexUtilityCodecError::LimitExceeded);
    }
    let mut bytes = Vec::with_capacity(FRAME_BYTES);
    bytes.extend_from_slice(MAGIC);
    bytes.extend_from_slice(&request.source);
    bytes.extend_from_slice(&request.revision.to_le_bytes());
    bytes.extend_from_slice(&request.page.to_le_bytes());
    bytes.push(request.reliable.into());
    bytes.extend_from_slice(&request.text_length.to_le_bytes());
    Ok(bytes)
}

/// Decode and validate path-free indexing metadata.
pub fn decode_index_request(bytes: &[u8]) -> Result<IndexUtilityRequest, IndexUtilityCodecError> {
    if bytes.len() != FRAME_BYTES || &bytes[..4] != MAGIC {
        return Err(IndexUtilityCodecError::Malformed);
    }
    let reliable = match bytes[32] {
        0 => false,
        1 => true,
        _ => return Err(IndexUtilityCodecError::Malformed),
    };
    let request = IndexUtilityRequest {
        source: bytes[4..20].try_into().expect("sixteen bytes"),
        revision: u64::from_le_bytes(bytes[20..28].try_into().expect("eight bytes")),
        page: u32::from_le_bytes(bytes[28..32].try_into().expect("four bytes")),
        reliable,
        text_length: u32::from_le_bytes(bytes[33..37].try_into().expect("four bytes")),
    };
    if request.text_length > MAX_TEXT_BYTES {
        return Err(IndexUtilityCodecError::LimitExceeded);
    }
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexing_request_round_trips_without_a_path() {
        let request = IndexUtilityRequest {
            source: [7; 16],
            revision: 11,
            page: 3,
            reliable: false,
            text_length: 4096,
        };
        assert_eq!(
            decode_index_request(&encode_index_request(&request).unwrap()).unwrap(),
            request
        );
    }
}
