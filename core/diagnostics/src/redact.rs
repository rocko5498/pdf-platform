//! Redaction wrapper — prevents accidental logging of sensitive values. [SDS §11]

use std::fmt;

/// Wraps a value so its `Debug`/`Display` output is replaced with `[REDACTED]`.
/// Use for file paths, user data, or anything that must not appear in logs.
pub struct Redacted<T>(pub T);

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[REDACTED]")
    }
}
