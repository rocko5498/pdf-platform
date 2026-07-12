//! Structured tracing + privacy-redaction wrappers. [ADR-020, ADR-025]
//! Privacy rule: document content / file paths are wrapped in `Redacted<T>`;
//! logging them raw only compiles in debug builds. [ADR-020]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod init;     // initialise tracing subscriber; ring-buffer backend
pub mod redact;   // Redacted<T> wrapper — redacts Display/Debug in release
