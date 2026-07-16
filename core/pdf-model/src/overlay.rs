//! Copy-on-write overlay over the original document. [ADR-006, SDS §3.1]
//!
//! The overlay is the mutation layer: original bytes are immutable ground
//! truth; every mutation creates a new object version in the overlay keyed
//! by revision number. The overlay is what gets serialized during
//! incremental save. [ADR-012]
//!
//! Invariants:
//! - Unparsed/unknown objects are preserved byte-exact.
//! - No operation may rewrite objects it did not logically touch.
//! - Every mutation is expressed as a Command producing a delta. [ADR-013]

use std::collections::HashMap;

/// An object version in the overlay: raw bytes of the serialized PDF object.
#[derive(Debug, Clone)]
pub struct ObjectVersion {
    /// The object's byte representation (including "N 0 obj\n...\nendobj\n").
    pub bytes: Vec<u8>,
    /// Revision when this version was created.
    pub revision: u64,
}

/// Copy-on-write overlay over the original document. [ADR-006]
///
/// Holds the set of object versions that have been modified since the
/// document was opened. The original file bytes (not stored here — they
/// live in the mmap) are the ground truth for any object NOT in the overlay.
#[derive(Debug, Clone)]
pub struct CowOverlay {
    /// Object versions keyed by (object_number, revision).
    /// Object number is the PDF indirect object number (1-based).
    versions: HashMap<(u32, u64), ObjectVersion>,
    /// Current revision counter. Incremented on each mutation.
    current_revision: u64,
    /// Set of object numbers that have been modified in the current revision.
    dirty_objects: Vec<u32>,
}

impl CowOverlay {
    /// Create a fresh overlay (no mutations yet).
    pub fn new() -> Self {
        Self {
            versions: HashMap::new(),
            current_revision: 0,
            dirty_objects: Vec::new(),
        }
    }

    /// Current revision number.
    pub fn revision(&self) -> u64 {
        self.current_revision
    }

    /// Bump the revision (called when a Command is applied).
    pub fn bump_revision(&mut self) -> u64 {
        self.current_revision += 1;
        self.dirty_objects.clear();
        self.current_revision
    }

    /// Apply a mutation to an object: store the new version in the overlay.
    ///
    /// `obj_num` is the 1-based PDF indirect object number.
    /// `bytes` is the complete serialized object (including header/footer).
    pub fn set_object(&mut self, obj_num: u32, bytes: Vec<u8>) {
        let key = (obj_num, self.current_revision);
        self.versions.insert(key, ObjectVersion {
            bytes,
            revision: self.current_revision,
        });
        self.dirty_objects.push(obj_num);
    }

    /// Get the latest version of an object from the overlay.
    ///
    /// Returns the bytes if this object has been modified; None means
    /// the original file bytes should be used.
    pub fn get_object(&self, obj_num: u32) -> Option<&[u8]> {
        // Find the highest revision for this object number.
        let mut best: Option<&ObjectVersion> = None;
        for (key, ver) in &self.versions {
            if key.0 == obj_num {
                if let Some(b) = best {
                    if ver.revision > b.revision {
                        best = Some(ver);
                    }
                } else {
                    best = Some(ver);
                }
            }
        }
        best.map(|v| v.bytes.as_slice())
    }

    /// Get the latest version of an object at or before a specific revision.
    pub fn get_object_at_revision(&self, obj_num: u32, revision: u64) -> Option<&[u8]> {
        let mut best: Option<&ObjectVersion> = None;
        for (key, ver) in &self.versions {
            if key.0 == obj_num && ver.revision <= revision {
                if let Some(b) = best {
                    if ver.revision > b.revision {
                        best = Some(ver);
                    }
                } else {
                    best = Some(ver);
                }
            }
        }
        best.map(|v| v.bytes.as_slice())
    }

    /// Object numbers modified in the current revision.
    pub fn dirty_objects(&self) -> &[u32] {
        &self.dirty_objects
    }

    /// Whether any objects have been modified.
    pub fn is_dirty(&self) -> bool {
        !self.dirty_objects.is_empty()
    }

    /// Total number of object versions in the overlay.
    pub fn version_count(&self) -> usize {
        self.versions.len()
    }

    /// Clear the overlay (e.g., after a successful save).
    pub fn clear(&mut self) {
        self.versions.clear();
        self.dirty_objects.clear();
    }

    /// Create a snapshot of the current overlay state for undo purposes.
    /// Returns the list of (obj_num, bytes) for all dirty objects.
    pub fn snapshot_dirty(&self) -> Vec<(u32, Vec<u8>)> {
        self.dirty_objects.iter()
            .filter_map(|&obj_num| {
                self.get_object(obj_num).map(|bytes| (obj_num, bytes.to_vec()))
            })
            .collect()
    }

    /// Restore objects from a snapshot (for undo).
    pub fn restore_snapshot(&mut self, snapshot: &[(u32, Vec<u8>)], revision: u64) {
        for (obj_num, bytes) in snapshot {
            let key = (*obj_num, revision);
            self.versions.insert(key, ObjectVersion {
                bytes: bytes.clone(),
                revision,
            });
        }
    }
}

impl Default for CowOverlay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlay_starts_empty() {
        let overlay = CowOverlay::new();
        assert_eq!(overlay.revision(), 0);
        assert!(!overlay.is_dirty());
        assert!(overlay.get_object(1).is_none());
    }

    #[test]
    fn set_and_get_object() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"1 0 obj\n<< /Type /Catalog >>\nendobj\n".to_vec());
        assert!(overlay.is_dirty());
        assert!(overlay.get_object(1).is_some());
        assert_eq!(overlay.dirty_objects(), &[1]);
    }

    #[test]
    fn revision_bump() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"v1".to_vec());
        assert_eq!(overlay.revision(), 0);

        let rev = overlay.bump_revision();
        assert_eq!(rev, 1);
        assert!(!overlay.is_dirty()); // dirty cleared on bump

        overlay.set_object(2, b"v2".to_vec());
        assert_eq!(overlay.dirty_objects(), &[2]);
    }

    #[test]
    fn get_object_at_revision() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"rev0".to_vec());
        overlay.bump_revision();
        overlay.set_object(1, b"rev1".to_vec());

        assert_eq!(overlay.get_object_at_revision(1, 0), Some(b"rev0".as_slice()));
        assert_eq!(overlay.get_object_at_revision(1, 1), Some(b"rev1".as_slice()));
        assert_eq!(overlay.get_object_at_revision(1, 5), Some(b"rev1".as_slice()));
    }

    #[test]
    fn snapshot_and_restore() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"original".to_vec());
        overlay.set_object(2, b"original2".to_vec());

        let snap = overlay.snapshot_dirty();
        assert_eq!(snap.len(), 2);

        // Modify further.
        overlay.set_object(1, b"modified".to_vec());
        assert_eq!(overlay.get_object(1), Some(b"modified".as_slice()));

        // Restore from snapshot.
        overlay.restore_snapshot(&snap, 0);
        // The snapshot was at revision 0, so restoring adds at revision 0.
        assert!(overlay.get_object(1).is_some());
    }

    #[test]
    fn clear_resets() {
        let mut overlay = CowOverlay::new();
        overlay.set_object(1, b"data".to_vec());
        overlay.bump_revision();
        overlay.set_object(2, b"data2".to_vec());

        overlay.clear();
        assert!(!overlay.is_dirty());
        assert_eq!(overlay.version_count(), 0);
    }
}
