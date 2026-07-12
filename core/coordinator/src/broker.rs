//! Document broker: privileged file open in Z0. [SDS §2.2.6, §3.1 step 2, ADR-016]
//!
//! Lower zones must not open arbitrary paths; the broker validates and opens.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

/// A document file opened by the broker for a session.
///
/// Holds the OS `File` (inherited into the worker as FD/HANDLE) and the path
/// for Z0 identity / logging only — path is not sent to Z1. [GR-1, handle-inherit]
pub struct BrokeredFile {
    path: PathBuf,
    file: File,
}

impl BrokeredFile {
    /// Path that was opened (Z0 only; not passed to the worker).
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Borrow the opened file (read-only intent; re-inherited on respawn).
    pub fn file(&self) -> &File {
        &self.file
    }
}

/// Open a document path read-only after basic validation. [SDS §3.1 step 2]
pub fn open_read_only(path: &Path) -> io::Result<BrokeredFile> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path is not a regular file",
        ));
    }
    let file = File::open(path)?;
    Ok(BrokeredFile {
        path: path.to_path_buf(),
        file,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn open_read_only_temp_file() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!(
            "pdf-platform-broker-test-{}-{}.bin",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let mut f = File::create(&p).unwrap();
            f.write_all(b"%PDF-1.4 test").unwrap();
        }
        let b = open_read_only(&p).expect("open");
        assert_eq!(b.path(), p.as_path());
        let _ = std::fs::remove_file(&p);
    }
}
