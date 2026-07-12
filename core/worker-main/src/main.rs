//! Z1 worker binary. Sandboxed; never holds authoritative document state. [ADR-008, SDS §2.3]
//! SAFETY: sandbox setup and mmap in sandbox crate carry SAFETY comments. [ADR-027]

fn main() {
    // M0 sequence (SDS §3.1 steps 3–6):
    // 1. Receive inherited IPC fd/handle (ADR-031; established pre-sandbox by coordinator)
    // 2. Receive brokered file handle; mmap read-only
    // 3. Bootstrap parse: xref + trailer (pdf-cos)
    // 4. Initialise engine backend (engine-pdfium or engine-hayro via feature flag)
    // 5. Run IPC handler loop (timeout-based recv per ADR-031 §6):
    //    - dispatch RasterizeTile requests to rayon pool
    //    - write tiles into pre-negotiated shmem buffers
    //    - reply TileReady with shmem handle descriptor
    //    - check generation; discard stale rayon results (SDS §5.3, ADR-031 §6)
    todo!("M0 worker main")
}
