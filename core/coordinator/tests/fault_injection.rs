//! M3 fault-injection integration tests. [ADR-022, SDS §10.6]
//!
//! Validates M3 exit criteria:
//! - Fault-injection suite passes (worker-kill, coordinator-kill, torn-append)
//! - Undo across a crash restores state
//! - Incremental saves preserve untouched bytes
//! - Torn save truncates to a valid revision

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use coordinator::broker::open_read_only;
use coordinator::document::DocumentCoordinator;
use coordinator::session::WorkerSession;
use pdf_cos::scan::scan_structure;
use pdf_model::command::{CommandGroup, SetObjectCommand};
use pdf_model::journal::UndoJournal;
use pdf_model::overlay::CowOverlay;
use pdf_write::IncrementalWriter;

fn worker_path() -> PathBuf {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.parent().unwrap().join("target");
    let debug = target_dir.join("debug").join(if cfg!(windows) { "worker.exe" } else { "worker" });
    if debug.exists() {
        return debug;
    }
    let release = target_dir.join("release").join(if cfg!(windows) { "worker.exe" } else { "worker" });
    if release.exists() {
        return release;
    }
    panic!("worker binary not found");
}

fn generate_pdf(num_pages: u32) -> (Vec<u8>, HashMap<u32, u32>) {
    let mut buf = Vec::with_capacity(256 + num_pages as usize * 80);
    let mut offsets = Vec::new();

    buf.extend_from_slice(b"%PDF-1.4\n");

    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    offsets.push(buf.len() as u32);
    let kids: Vec<String> = (0..num_pages).map(|i| format!("{} 0 R", i + 3)).collect();
    write!(buf, "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.join(" "), num_pages).unwrap();

    for i in 0..num_pages {
        offsets.push(buf.len() as u32);
        write!(buf, "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
            i + 3).unwrap();
    }

    let xref_offset = buf.len() as u32;
    write!(&mut buf, "xref\n0 {}\n", num_pages + 3).unwrap();
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        write!(&mut buf, "{:010} 00000 n \n", offset).unwrap();
    }
    write!(buf, "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        num_pages + 3, xref_offset).unwrap();

    let obj_offsets: HashMap<u32, u32> = offsets.iter().enumerate()
        .map(|(i, &off)| ((i + 1) as u32, off)).collect();
    (buf, obj_offsets)
}

fn temp_pdf(bytes: &[u8], label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("fault-inj-{label}.pdf"));
    std::fs::write(&path, bytes).expect("write temp PDF");
    path
}

// ---------------------------------------------------------------------------
// FI-1: Worker-kill mid-render → respawn preserves state [SDS §10.1]
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_worker_kill_preserves_state() {
    let (pdf_bytes, _) = generate_pdf(3);
    let path = temp_pdf(&pdf_bytes, "worker-kill");
    let brokered = open_read_only(&path).expect("broker");
    let mut session = WorkerSession::spawn_with_document(&worker_path(), brokered).expect("spawn");

    // Inspect before kill.
    let before = session.inspect().expect("inspect before kill");
    assert_eq!(before.page_count, 3);

    // Kill the worker.
    session.kill_worker().expect("kill");
    let death = session.poll(Duration::from_secs(2)).expect("poll death");
    assert_eq!(death.len(), 1);

    // Respawn.
    session.respawn().expect("respawn");
    assert!(session.is_alive());

    // Inspect after respawn — should get identical results.
    let after = session.inspect().expect("inspect after respawn");
    assert_eq!(after.page_count, before.page_count);
    assert_eq!(after.has_acroform, before.has_acroform);
    assert_eq!(after.has_xfa, before.has_xfa);
    // page_count/has_acroform/has_xfa all come from the mmap-based COS scan and
    // survive even when the respawned worker failed to load an engine.
    // page_dimensions come from the engine, so they are what actually proves the
    // respawn restored a working worker rather than a degraded one. [GR-8, SDS §10.1]
    assert_eq!(
        after.page_dimensions, before.page_dimensions,
        "respawned worker lost its engine"
    );

    session.send(b"quit").expect("quit");
    let _ = session.poll(Duration::from_secs(2));
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// FI-2: Coordinator-kill mid-mutation → replay restores within budget [SDS §10.2]
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_coordinator_kill_replay_restores() {
    let (pdf_bytes, _) = generate_pdf(2);
    let path = temp_pdf(&pdf_bytes, "coord-kill");
    let sidecar_path;

    // Phase 1: Open, mutate 3 times, "crash".
    {
        let mut coord = DocumentCoordinator::open(&worker_path(), &path).expect("open");
        sidecar_path = coord.sidecar_path().to_path_buf();

        for i in 0..3 {
            let mut group = CommandGroup::new(format!("Crash edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("crash-{i}").into_bytes(),
                old_bytes: None,
            }));
            coord.apply_command_group(group).expect("apply");
        }

        // Verify sidecar has 3 groups.
        let data = std::fs::read(&sidecar_path).expect("read sidecar");
        let text = String::from_utf8_lossy(&data);
        let group_count = text.lines().filter(|l| l.starts_with("GROUP:")).count();
        assert_eq!(group_count, 3, "sidecar should have 3 groups before crash");

        // Simulate crash.
        drop(coord);
    }

    assert!(sidecar_path.exists(), "sidecar survives crash");

    // Phase 2: Reopen, recover, verify state.
    {
        let (mut coord, recovery) = DocumentCoordinator::open_with_recovery(&worker_path(), &path)
            .expect("open_with_recovery");

        assert!(recovery.is_some(), "should find sidecar");
        let info = recovery.unwrap();
        assert_eq!(info.group_count, 3, "recovered 3 groups");

        // State should be restored.
        assert!(coord.is_dirty());
        assert_eq!(coord.undo_depth(), 3);

        // Undo all — should work correctly.
        for i in (0..3).rev() {
            assert_eq!(coord.undo_name(), Some(format!("Crash edit {i}").as_str()));
            coord.undo().expect("undo");
        }
        assert!(!coord.can_undo());

        coord.close().expect("close");
    }

    assert!(!sidecar_path.exists(), "sidecar deleted on close");
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// FI-3: Torn append → truncation to valid revision [SDS §10.5]
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_torn_append_truncates_to_valid() {
    let (original_bytes, original_offsets) = generate_pdf(3);

    // Simulate a torn write: write original bytes + partial xref.
    let mut torn = original_bytes.clone();
    let modified = b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Torn >>\nendobj\n";
    torn.extend_from_slice(modified);

    // Write a partial xref (truncated mid-entry).
    let xref_start = torn.len();
    torn.extend_from_slice(b"xref\n0 6\n");
    torn.extend_from_slice(b"0000000000 65535 f \n");
    // Only write 2 of the 6 entries — torn!
    let orig_text = String::from_utf8_lossy(&original_bytes);
    let orig_xref_pos = orig_text.rfind("xref\n").unwrap() as u32;
    // Entry for obj 1
    write!(&mut torn, "{:010} 00000 n \n", original_offsets[&1]).unwrap();
    // Entry for obj 2
    write!(&mut torn, "{:010} 00000 n \n", original_offsets[&2]).unwrap();
    // TORN HERE — missing entries for obj 3-5 and trailer

    let path = temp_pdf(&torn, "torn-append");

    // The scanner MUST either:
    // a) Find the ORIGINAL xref (via startxref in original bytes) → valid revision (3 pages)
    // b) Find the torn xref and detect it's incomplete → fail gracefully with leniency
    // NEVER: crash, panic, or silently use the torn revision.
    let result = scan_structure(&path);
    match result {
        Ok(ds) => {
            // Scanner found the original xref — valid-revision guarantee [SDS §10.5].
            assert_eq!(ds.page_count, 3,
                "torn-append must resolve to original valid revision (3 pages), not the torn one");
        }
        Err(e) => {
            // Scanner detected torn xref and failed gracefully — acceptable.
            // The key assertion: it did NOT crash or produce a valid result
            // with wrong page count.
            eprintln!("torn append detected and rejected: {e} — expected behavior");
        }
    }

    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// FI-4: Incremental save preserves untouched bytes [SDS §3.4]
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_incremental_save_preserves_untouched() {
    let (original_bytes, original_offsets) = generate_pdf(5);

    // Modify only objects 3 and 5.
    let mut overlay = CowOverlay::new();
    overlay.set_object(3, b"3 0 obj\n<< /Type /Page /Modified 3 >>\nendobj\n".to_vec());
    overlay.set_object(5, b"5 0 obj\n<< /Type /Page /Modified 5 >>\nendobj\n".to_vec());
    overlay.bump_revision();

    let mut output = original_bytes.clone();
    let result = IncrementalWriter::write_incremental(
        &mut output,
        &overlay,
        0,
        8,
        &original_offsets,
        original_bytes.len() as u32,
    ).expect("incremental write");

    assert!(result.objects_written >= 2);

    // Verify untouched objects (1, 2, 4, 6, 7) are NOT in the output
    // as modified — only their original offsets should appear in xref.
    let text = String::from_utf8_lossy(&output);
    let appended = &text[original_bytes.len()..];

    // The appended section should contain objects 3 and 5.
    assert!(appended.contains("3 0 obj"), "modified obj 3 should be appended");
    assert!(appended.contains("5 0 obj"), "modified obj 5 should be appended");

    // Objects 1, 2, 4 should NOT be appended.
    // (obj 1 and 2 appear in the original, not in the appended section)
    let first_appended_obj = appended.lines().find(|l| l.ends_with(" obj"))
        .unwrap_or("");
    assert!(first_appended_obj.starts_with("3 ") || first_appended_obj.starts_with("5 "),
        "first appended object should be 3 or 5, got: {first_appended_obj}");

    // Scan the output — should be valid.
    let path = temp_pdf(&output, "incremental-preserve");
    let ds = scan_structure(&path).expect("saved PDF should be valid");
    assert_eq!(ds.page_count, 5, "page count preserved");

    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// FI-5: Worker-kill during text extraction → graceful error [SDS §10.1]
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_worker_kill_during_extract() {
    let (pdf_bytes, _) = generate_pdf(3);
    let path = temp_pdf(&pdf_bytes, "extract-kill");
    let brokered = open_read_only(&path).expect("broker");
    let mut session = WorkerSession::spawn_with_document(&worker_path(), brokered).expect("spawn");

    // Start an extraction.
    let cid = session.next_correlation_id();
    let cmd = protocol::commands::Command::ExtractPage {
        correlation_id: cid,
        page_index: 0,
    };
    let body = protocol::commands::encode_command(&cmd);
    session.send(&body).expect("send extract");

    // Kill worker while extraction is in flight.
    std::thread::sleep(Duration::from_millis(50));
    session.kill_worker().expect("kill");
    let _ = session.poll(Duration::from_secs(2));

    // Respawn and verify worker is functional.
    session.respawn().expect("respawn");
    let summary = session.inspect().expect("inspect after kill");
    assert_eq!(summary.page_count, 3, "worker functional after kill during extract");

    session.send(b"quit").expect("quit");
    let _ = session.poll(Duration::from_secs(2));
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// FI-6: Multiple rapid mutations → journal consistency [SDS §10.3]
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_rapid_mutations_journal_consistent() {
    let (pdf_bytes, _) = generate_pdf(3);
    let path = temp_pdf(&pdf_bytes, "rapid-mutations");

    let mut coord = DocumentCoordinator::open(&worker_path(), &path).expect("open");
    let sidecar_path = coord.sidecar_path().to_path_buf();

    // Apply 10 rapid mutations.
    for i in 0..10 {
        let mut group = CommandGroup::new(format!("Rapid {i}"));
        group.push(Box::new(SetObjectCommand {
            obj_num: 100 + i,
            new_bytes: format!("rapid-{i}").into_bytes(),
            old_bytes: None,
        }));
        coord.apply_command_group(group).expect("apply");
    }

    // Verify journal state.
    assert_eq!(coord.undo_depth(), 10);
    assert_eq!(coord.revision(), 10);

    // Verify sidecar is consistent.
    let data = std::fs::read(&sidecar_path).expect("read sidecar");
    let text = String::from_utf8_lossy(&data);
    let group_count = text.lines().filter(|l| l.starts_with("GROUP:")).count();
    assert_eq!(group_count, 10, "sidecar should have 10 groups");

    // Undo all.
    for _ in 0..10 {
        coord.undo().expect("undo");
    }
    assert_eq!(coord.undo_depth(), 0);
    assert_eq!(coord.redo_depth(), 10);

    // Sidecar should be deleted (no applied groups to persist).
    assert!(!sidecar_path.exists(), "sidecar should be deleted after undo all");

    coord.close().expect("close");
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// FI-7: Save then crash → sidecar is gone (save is recovery point) [SDS §10.3]
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_save_clears_sidecar() {
    let (pdf_bytes, _) = generate_pdf(2);
    let path = temp_pdf(&pdf_bytes, "save-clear");

    let mut coord = DocumentCoordinator::open(&worker_path(), &path).expect("open");
    let sidecar_path = coord.sidecar_path().to_path_buf();

    // Mutate.
    let mut group = CommandGroup::new("Pre-save");
    group.push(Box::new(SetObjectCommand {
        obj_num: 10,
        new_bytes: b"dirty".to_vec(),
        old_bytes: None,
    }));
    coord.apply_command_group(group).expect("apply");
    assert!(sidecar_path.exists());

    // Save — sidecar should be deleted.
    let save_path = std::env::temp_dir().join("fault-inj-save-clear.pdf");
    coord.save_incremental(&save_path).expect("save");
    assert!(!sidecar_path.exists(), "sidecar deleted after save");

    // Simulate crash after save.
    drop(coord);

    // Reopen — no recovery needed.
    let (mut coord2, recovery) = DocumentCoordinator::open_with_recovery(&worker_path(), &path)
        .expect("open after save-crash");
    assert!(recovery.is_none(), "no recovery after clean save");
    assert!(!coord2.is_dirty());

    coord2.close().expect("close");
    std::fs::remove_file(&save_path).ok();
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// FI-8: Incremental save preserves signature objects [ADR-012, SDS §3.4]
//
// By construction, IncrementalWriter only appends dirty objects from the
// overlay. Signature dictionaries (/ByteRange, /Contents) are never dirty
// at M3, so their original bytes are preserved. This test verifies that
// claim explicitly.
// ---------------------------------------------------------------------------

#[test]
fn fault_inject_incremental_save_preserves_signatures() {
    // Build a PDF with a signature dictionary (obj 4).
    let mut buf = Vec::with_capacity(512);
    let mut offsets = Vec::new();

    buf.extend_from_slice(b"%PDF-1.4\n");

    // obj 1: Catalog
    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // obj 2: Pages
    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");

    // obj 3: Page
    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n");

    // obj 4: Signature dictionary (the bytes we must preserve)
    let sig_content = b"4 0 obj\n<< /Type /Sig /Filter /Adobe.PPKLite /SubFilter /adbe.pkcs7.detached /ByteRange [0 100 200 50] /Contents (FAKE_SIG_DATA_HERE_1234567890) >>\nendobj\n";
    offsets.push(buf.len() as u32);
    let sig_offset_in_original = buf.len() as u32;
    buf.extend_from_slice(sig_content);

    let original_sig_bytes = sig_content.to_vec();

    // xref
    let xref_offset = buf.len() as u32;
    write!(&mut buf, "xref\n0 5\n").unwrap();
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        write!(&mut buf, "{:010} 00000 n \n", offset).unwrap();
    }
    write!(buf, "trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n", xref_offset).unwrap();

    let original_bytes = buf.clone();
    let original_offsets: HashMap<u32, u32> = offsets.iter().enumerate()
        .map(|(i, &off)| ((i + 1) as u32, off)).collect();

    // Modify object 3 (a page) — NOT the signature.
    let mut overlay = CowOverlay::new();
    overlay.set_object(3, b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Modified true >>\nendobj\n".to_vec());
    overlay.bump_revision();

    let mut output = original_bytes.clone();
    IncrementalWriter::write_incremental(
        &mut output,
        &overlay,
        0,
        5,
        &original_offsets,
        original_bytes.len() as u32,
    ).expect("incremental write");

    // The signature object bytes must appear in the output at their
    // original location (untouched region before the append point).
    let output_sig_region = &output[sig_offset_in_original as usize..][..original_sig_bytes.len()];
    assert_eq!(
        output_sig_region,
        &original_sig_bytes,
        "signature object bytes must be preserved byte-for-byte after incremental save"
    );

    // Verify the saved PDF scans valid.
    let path = temp_pdf(&output, "sig-preserve");
    let ds = scan_structure(&path).expect("saved PDF with signature should be valid");
    assert_eq!(ds.page_count, 1, "page count preserved");

    std::fs::remove_file(&path).ok();
}
