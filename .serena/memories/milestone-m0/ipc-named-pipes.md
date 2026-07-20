# M0: Windows Named Pipe IPC Implementation

## Date: 2026-07-13

## What was done
- Replaced Windows TCP loopback transport with named pipes per ADR-031
- Implemented raw FFI bindings: CreateNamedPipeW, ConnectNamedPipe, CreateFileW, ReadFile, WriteFile, CancelSynchronousIo, CloseHandle, GetLastError
- Created `PipeHandle` (Send-safe, owns handle) and `BorrowedHandle` (Copy, non-owning for threads)
- Thread-based `recv_with_timeout` using `CancelSynchronousIo` for timeout support
- Updated `spawn.rs`: ENV_IPC_PORT -> ENV_IPC_PIPE, Windows spawn creates named pipe server
- Updated Unix spawn to clean up legacy TCP env var

## Architecture decisions
- Raw FFI instead of windows-sys to minimize dependency surface (ADR-028)
- Named pipe server creates pipe, worker connects via CreateFileW (client)
- Thread-based timeout: spawn reader thread, cancel with CancelSynchronousIo on timeout
- Decoder ownership transferred to thread via Option::take() to avoid borrow issues
- Default security for M0; AppContainer SA deferred to confinement hardening

## Test results
- All 6 sandbox tests pass (3 transport + 3 spawn/confinement)
- All 16 protocol tests pass
- Full workspace compiles with 0 errors (18 pre-existing warnings)

## Files modified
- core/sandbox/src/transport.rs - Windows named pipe implementation
- core/sandbox/src/spawn.rs - Updated to use named pipes
- core/sandbox/Cargo.toml - Removed windows-sys (using raw FFI)
