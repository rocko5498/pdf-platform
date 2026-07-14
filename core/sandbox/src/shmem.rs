//! Cross-process shared memory region for tile pixels. [ADR-007, ADR-011, SDS §4.2, §6.3]
//!
//! Backing store is a temporary file mapped with `memmap2` (MAP_SHARED semantics).
//! The `File` is inherited into the worker as FD/HANDLE (same pattern as documents).
//!
//! **Bound (GR-7):** callers choose `len`; M0 smoke uses one `TILE_RGBA8_BYTES` slot.
//!
//! Dependency: `memmap2` 0.9 (MIT) — already used by `pdf-cos`. Exit seam: replace
//! mapping backend behind this module (ADR-028).

use std::fs::File;
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use memmap2::MmapMut;

/// Monotonic counter to guarantee unique temp-file names even when two
/// `create` calls land in the same nanosecond on the same PID. [GR-7]
static SHMEM_SEQ: AtomicU64 = AtomicU64::new(1);

/// Parent-side shared region: open file + mutable mapping.
pub struct SharedRegion {
    _path: PathBuf,
    file: File,
    map: MmapMut,
    len: usize,
}

impl SharedRegion {
    /// Create a new shared region of exactly `len` bytes (zero-filled).
    pub fn create(len: usize) -> io::Result<Self> {
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "shared region len must be > 0",
            ));
        }
        let seq = SHMEM_SEQ.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "pdf-platform-shmem-{}-{}-{}.bin",
            std::process::id(),
            seq,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let mut file = File::options()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)?;
        file.set_len(len as u64)?;
        file.flush()?;

        // SAFETY: `file` is a regular file of length `len` we exclusively own;
        // mapping is shared so the worker's inherited FD mapping sees our writes
        // and vice versa. We keep `file` open for the region lifetime.
        let map = unsafe { MmapMut::map_mut(&file)? };
        debug_assert_eq!(map.len(), len);

        // Unlink path on Unix so the inode lives only via open FDs (optional cleanup).
        // On Windows, delete after open may fail if still mapped — best-effort.
        let _ = std::fs::remove_file(&path);

        Ok(Self {
            _path: path,
            file,
            map,
            len,
        })
    }

    /// Region length in bytes.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether the region is empty (always false for successful `create`).
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// File to inherit into the worker.
    pub fn file(&self) -> &File {
        &self.file
    }

    /// Parent view of the shared bytes (read).
    pub fn as_slice(&self) -> &[u8] {
        &self.map
    }

    /// Parent view of the shared bytes (write).
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.map
    }

    /// Flush mapping to the file (best-effort visibility).
    pub fn flush(&self) -> io::Result<()> {
        self.map.flush()
    }
}

/// Map an already-opened shared-memory file (worker side).
pub fn map_shmem_file(file: &File, len: usize) -> io::Result<MmapMut> {
    // SAFETY: `file` is the inherited region FD/HANDLE; length must match parent `create`.
    let map = unsafe { MmapMut::map_mut(file)? };
    if map.len() < len {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("shmem map too small: {} < {len}", map.len()),
        ));
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::handles::TILE_RGBA8_BYTES;

    #[test]
    fn create_tile_sized_region() {
        let r = SharedRegion::create(TILE_RGBA8_BYTES).expect("create");
        assert_eq!(r.len(), TILE_RGBA8_BYTES);
        assert_eq!(r.as_slice().len(), TILE_RGBA8_BYTES);
    }
}
