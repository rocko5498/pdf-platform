//! Bounded utility-job command/result wire codec. [ADR-009, ADR-031]

const COMMAND_MAGIC: &[u8; 4] = b"UJQ1";
const EVENT_MAGIC: &[u8; 4] = b"UJR1";
const MAX_TEXT_BYTES: usize = 64 * 1024;

/// One declarative job dispatched to a utility worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UtilityJobCommand {
    /// Request/response correlation identifier.
    pub correlation_id: u64,
    /// Scheduler job identifier.
    pub job_id: u64,
    /// Operation name understood by the utility operation registry.
    pub operation: String,
}

/// Terminal result returned by a utility worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UtilityJobEvent {
    /// Operation completed successfully.
    Completed {
        /// Correlation identifier.
        correlation_id: u64,
        /// Scheduler job identifier.
        job_id: u64,
    },
    /// Operation ran but failed.
    Failed {
        /// Correlation identifier.
        correlation_id: u64,
        /// Scheduler job identifier.
        job_id: u64,
        /// Human-readable failure detail.
        message: String,
    },
}

/// Invalid or oversized utility-job frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UtilityCodecError {
    /// Frame magic or structure is invalid.
    Malformed,
    /// Text payload is not UTF-8.
    InvalidUtf8,
    /// A declared text field exceeds its bound.
    LimitExceeded,
}

/// Encode a utility-job request.
pub fn encode_command(command: &UtilityJobCommand) -> Result<Vec<u8>, UtilityCodecError> {
    encode(
        COMMAND_MAGIC,
        command.correlation_id,
        command.job_id,
        0,
        &command.operation,
    )
}

/// Decode a utility-job request.
pub fn decode_command(bytes: &[u8]) -> Result<UtilityJobCommand, UtilityCodecError> {
    let (correlation_id, job_id, tag, operation) = decode(bytes, COMMAND_MAGIC)?;
    if tag != 0 {
        return Err(UtilityCodecError::Malformed);
    }
    Ok(UtilityJobCommand {
        correlation_id,
        job_id,
        operation,
    })
}

/// Encode a utility-job terminal event.
pub fn encode_event(event: &UtilityJobEvent) -> Result<Vec<u8>, UtilityCodecError> {
    match event {
        UtilityJobEvent::Completed {
            correlation_id,
            job_id,
        } => encode(EVENT_MAGIC, *correlation_id, *job_id, 0, ""),
        UtilityJobEvent::Failed {
            correlation_id,
            job_id,
            message,
        } => encode(EVENT_MAGIC, *correlation_id, *job_id, 1, message),
    }
}

/// Decode a utility-job terminal event.
pub fn decode_event(bytes: &[u8]) -> Result<UtilityJobEvent, UtilityCodecError> {
    let (correlation_id, job_id, tag, text) = decode(bytes, EVENT_MAGIC)?;
    match tag {
        0 if text.is_empty() => Ok(UtilityJobEvent::Completed {
            correlation_id,
            job_id,
        }),
        1 => Ok(UtilityJobEvent::Failed {
            correlation_id,
            job_id,
            message: text,
        }),
        _ => Err(UtilityCodecError::Malformed),
    }
}

fn encode(
    magic: &[u8; 4],
    correlation_id: u64,
    job_id: u64,
    tag: u8,
    text: &str,
) -> Result<Vec<u8>, UtilityCodecError> {
    if text.len() > MAX_TEXT_BYTES {
        return Err(UtilityCodecError::LimitExceeded);
    }
    let mut bytes = Vec::with_capacity(25 + text.len());
    bytes.extend_from_slice(magic);
    bytes.extend_from_slice(&correlation_id.to_le_bytes());
    bytes.extend_from_slice(&job_id.to_le_bytes());
    bytes.push(tag);
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    Ok(bytes)
}

fn decode(bytes: &[u8], magic: &[u8; 4]) -> Result<(u64, u64, u8, String), UtilityCodecError> {
    if bytes.len() < 25 || &bytes[..4] != magic {
        return Err(UtilityCodecError::Malformed);
    }
    let correlation_id = u64::from_le_bytes(bytes[4..12].try_into().expect("eight bytes"));
    let job_id = u64::from_le_bytes(bytes[12..20].try_into().expect("eight bytes"));
    let tag = bytes[20];
    let length = u32::from_le_bytes(bytes[21..25].try_into().expect("four bytes")) as usize;
    if length > MAX_TEXT_BYTES || bytes.len() != 25 + length {
        return Err(if length > MAX_TEXT_BYTES {
            UtilityCodecError::LimitExceeded
        } else {
            UtilityCodecError::Malformed
        });
    }
    let text = std::str::from_utf8(&bytes[25..])
        .map_err(|_| UtilityCodecError::InvalidUtf8)?
        .to_owned();
    Ok((correlation_id, job_id, tag, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_and_events_round_trip() {
        let command = UtilityJobCommand {
            correlation_id: 4,
            job_id: 9,
            operation: "noop".into(),
        };
        assert_eq!(
            decode_command(&encode_command(&command).unwrap()).unwrap(),
            command
        );
        let failed = UtilityJobEvent::Failed {
            correlation_id: 4,
            job_id: 9,
            message: "no handler".into(),
        };
        assert_eq!(
            decode_event(&encode_event(&failed).unwrap()).unwrap(),
            failed
        );
    }

    #[test]
    fn codec_rejects_truncation_and_oversized_text() {
        assert_eq!(decode_command(b"UJQ1"), Err(UtilityCodecError::Malformed));
        let command = UtilityJobCommand {
            correlation_id: 1,
            job_id: 2,
            operation: "x".repeat(MAX_TEXT_BYTES + 1),
        };
        assert_eq!(
            encode_command(&command),
            Err(UtilityCodecError::LimitExceeded)
        );
    }
}
