//! Tracing subscriber initialisation. [SDS §11]
// ponytail: call once at process start; no-op if called again

/// Initialise a human-readable tracing subscriber from `RUST_LOG`.
/// No-op (returns false) if a subscriber is already installed.
pub fn init_tracing() -> bool {
    use tracing_subscriber::{fmt, EnvFilter};
    fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .try_init()
        .is_ok()
}
