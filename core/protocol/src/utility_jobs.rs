//! Bounded utility-job command/result wire codec. [ADR-009, ADR-031]

const COMMAND_MAGIC_V1: &[u8; 4] = b"UJQ1";
const COMMAND_MAGIC: &[u8; 4] = b"UJQ2";
const EVENT_MAGIC: &[u8; 4] = b"UJR1";
const MAX_TEXT_BYTES: usize = 64 * 1024;
const MAX_INPUTS: usize = 16;
const MAX_INLINE_BYTES: usize = 64 * 1024;

/// One declarative job dispatched to a utility worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UtilityJobCommand {
    /// Request/response correlation identifier.
    pub correlation_id: u64,
    /// Scheduler job identifier.
    pub job_id: u64,
    /// Operation name understood by the utility operation registry.
    pub operation: String,
    /// Bounded inputs or opaque shared-memory grants.
    pub inputs: Vec<UtilityJobInput>,
}

/// Input supplied to a utility job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UtilityJobInput {
    /// Small control data carried directly in the framed request.
    Inline(Vec<u8>),
    /// Bounded region in broker-approved shared memory.
    SharedMemory {
        /// Opaque process-local capability identifier.
        grant_id: [u8; 16],
        /// Byte offset from the start of the granted region.
        offset: u64,
        /// Number of accessible bytes.
        length: u64,
    },
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
    if command.operation.len() > MAX_TEXT_BYTES || command.inputs.len() > MAX_INPUTS {
        return Err(UtilityCodecError::LimitExceeded);
    }
    let mut bytes = Vec::new();
    bytes.extend_from_slice(COMMAND_MAGIC);
    bytes.extend_from_slice(&command.correlation_id.to_le_bytes());
    bytes.extend_from_slice(&command.job_id.to_le_bytes());
    bytes.extend_from_slice(&(command.operation.len() as u32).to_le_bytes());
    bytes.extend_from_slice(command.operation.as_bytes());
    bytes.push(command.inputs.len() as u8);
    for input in &command.inputs {
        match input {
            UtilityJobInput::Inline(data) => {
                if data.len() > MAX_INLINE_BYTES {
                    return Err(UtilityCodecError::LimitExceeded);
                }
                bytes.push(0);
                bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
                bytes.extend_from_slice(data);
            }
            UtilityJobInput::SharedMemory {
                grant_id,
                offset,
                length,
            } => {
                offset
                    .checked_add(*length)
                    .ok_or(UtilityCodecError::Malformed)?;
                bytes.push(1);
                bytes.extend_from_slice(grant_id);
                bytes.extend_from_slice(&offset.to_le_bytes());
                bytes.extend_from_slice(&length.to_le_bytes());
            }
        }
    }
    Ok(bytes)
}

/// Decode a utility-job request.
pub fn decode_command(bytes: &[u8]) -> Result<UtilityJobCommand, UtilityCodecError> {
    if bytes.starts_with(COMMAND_MAGIC_V1) {
        let (correlation_id, job_id, tag, operation) = decode(bytes, COMMAND_MAGIC_V1)?;
        if tag != 0 {
            return Err(UtilityCodecError::Malformed);
        }
        return Ok(UtilityJobCommand {
            correlation_id,
            job_id,
            operation,
            inputs: Vec::new(),
        });
    }
    let mut cursor = Cursor::new(bytes);
    if cursor.take(4)? != COMMAND_MAGIC {
        return Err(UtilityCodecError::Malformed);
    }
    let correlation_id = cursor.u64()?;
    let job_id = cursor.u64()?;
    let operation_len = cursor.u32()? as usize;
    if operation_len > MAX_TEXT_BYTES {
        return Err(UtilityCodecError::LimitExceeded);
    }
    let operation = std::str::from_utf8(cursor.take(operation_len)?)
        .map_err(|_| UtilityCodecError::InvalidUtf8)?
        .to_owned();
    let input_count = cursor.byte()? as usize;
    if input_count > MAX_INPUTS {
        return Err(UtilityCodecError::LimitExceeded);
    }
    let mut inputs = Vec::with_capacity(input_count);
    for _ in 0..input_count {
        match cursor.byte()? {
            0 => {
                let length = cursor.u32()? as usize;
                if length > MAX_INLINE_BYTES {
                    return Err(UtilityCodecError::LimitExceeded);
                }
                inputs.push(UtilityJobInput::Inline(cursor.take(length)?.to_vec()));
            }
            1 => {
                let grant_id = cursor.take(16)?.try_into().expect("sixteen bytes");
                let offset = cursor.u64()?;
                let length = cursor.u64()?;
                offset
                    .checked_add(length)
                    .ok_or(UtilityCodecError::Malformed)?;
                inputs.push(UtilityJobInput::SharedMemory {
                    grant_id,
                    offset,
                    length,
                });
            }
            _ => return Err(UtilityCodecError::Malformed),
        }
    }
    if !cursor.is_empty() {
        return Err(UtilityCodecError::Malformed);
    }
    Ok(UtilityJobCommand {
        correlation_id,
        job_id,
        operation,
        inputs,
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

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], UtilityCodecError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(UtilityCodecError::Malformed)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(UtilityCodecError::Malformed)?;
        self.offset = end;
        Ok(value)
    }

    fn byte(&mut self) -> Result<u8, UtilityCodecError> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, UtilityCodecError> {
        Ok(u32::from_le_bytes(
            self.take(4)?.try_into().expect("four bytes"),
        ))
    }

    fn u64(&mut self) -> Result<u64, UtilityCodecError> {
        Ok(u64::from_le_bytes(
            self.take(8)?.try_into().expect("eight bytes"),
        ))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
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
            inputs: Vec::new(),
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
            inputs: Vec::new(),
        };
        assert_eq!(
            encode_command(&command),
            Err(UtilityCodecError::LimitExceeded)
        );
    }

    #[test]
    fn command_round_trips_bounded_inputs() {
        let command = UtilityJobCommand {
            correlation_id: 5,
            job_id: 8,
            operation: "ocr".into(),
            inputs: vec![
                UtilityJobInput::Inline(b"eng".to_vec()),
                UtilityJobInput::SharedMemory {
                    grant_id: [7; 16],
                    offset: 4096,
                    length: 8192,
                },
            ],
        };
        assert_eq!(
            decode_command(&encode_command(&command).unwrap()).unwrap(),
            command
        );
    }

    #[test]
    fn command_rejects_overflowing_shared_memory_range() {
        let command = UtilityJobCommand {
            correlation_id: 1,
            job_id: 2,
            operation: "ocr".into(),
            inputs: vec![UtilityJobInput::SharedMemory {
                grant_id: [1; 16],
                offset: u64::MAX,
                length: 1,
            }],
        };
        assert_eq!(encode_command(&command), Err(UtilityCodecError::Malformed));
    }
}
