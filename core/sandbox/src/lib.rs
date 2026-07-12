//! Per-OS worker sandbox setup + WorkerTransport platform impls. [ADR-008, ADR-016, ADR-031]
//! SAFETY: contains unsafe code for mmap, OS sandbox APIs, and handle inheritance.
//!         Each unsafe block carries a SAFETY comment. [ADR-027]

pub mod confinement; // platform sandbox: seccomp-bpf / AppContainer / Sandbox profile
pub mod spawn;
pub mod transport; // WorkerTransport impls: UnixWorkerTransport / WindowsWorkerTransport // worker-process spawn + pre-sandbox channel establishment
