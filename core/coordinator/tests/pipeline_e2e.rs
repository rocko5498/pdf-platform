//! End-to-end pipeline test: generate PDF → spawn worker → inspect →
//! fetch objects → scan → incremental save → verify output. [SDS §14 M0/M3]
//!
//! Uses a hand-crafted PDF generator and the real worker process.

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use coordinator::broker::open_read_only;
use coordinator::session::WorkerSession;
use pdf_cos::scan::scan_structure;
use pdf_model::overlay::CowOverlay;
use pdf_write::IncrementalWriter;
use protocol::commands::{encode_command, Command};
use protocol::events::{decode_worker_event, WorkerEvent};

fn worker_path() -> PathBuf {
    // Prefer the newest built worker so e2e tracks rebuilds. [SDS M0]
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = manifest_dir.parent().unwrap().join("target");
    let name = if cfg!(windows) { "worker.exe" } else { "worker" };
    let debug = target_dir.join("debug").join(name);
    let release = target_dir.join("release").join(name);
    match (debug.exists(), release.exists()) {
        (true, true) => {
            let d = std::fs::metadata(&debug).and_then(|m| m.modified()).ok();
            let r = std::fs::metadata(&release).and_then(|m| m.modified()).ok();
            if r >= d {
                release
            } else {
                debug
            }
        }
        (true, false) => debug,
        (false, true) => release,
        (false, false) => panic!(
            "worker binary not found at {} or {}",
            debug.display(),
            release.display()
        ),
    }
}

/// Generate a minimal N-page PDF with correct xref offsets.
/// Returns (bytes, xref_offset, object_offsets).
fn generate_pdf(num_pages: u32) -> (Vec<u8>, u32, HashMap<u32, u32>) {
    let mut buf = Vec::with_capacity(256 + num_pages as usize * 80);
    let mut offsets = Vec::with_capacity(num_pages as usize + 3);

    buf.extend_from_slice(b"%PDF-1.4\n");

    // Object 1: Catalog
    offsets.push(buf.len() as u32);
    buf.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");

    // Object 2: Pages (parent)
    offsets.push(buf.len() as u32);
    let kids: Vec<String> = (0..num_pages)
        .map(|i| format!("{} 0 R", i + 3))
        .collect();
    write!(
        buf,
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.join(" "),
        num_pages
    )
    .unwrap();

    // Objects 3..N+2: Page objects
    for i in 0..num_pages {
        offsets.push(buf.len() as u32);
        write!(
            buf,
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
            i + 3
        )
        .unwrap();
    }

    // xref table
    let xref_offset = buf.len() as u32;
    write!(&mut buf, "xref\n0 {}\n", num_pages + 3).unwrap();
    buf.extend_from_slice(b"0000000000 65535 f \n");
    for offset in &offsets {
        write!(&mut buf, "{:010} 00000 n \n", offset).unwrap();
    }

    // Trailer
    write!(
        buf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        num_pages + 3,
        xref_offset
    )
    .unwrap();

    let obj_offsets: HashMap<u32, u32> = offsets.iter().enumerate()
        .map(|(i, &off)| ((i + 1) as u32, off))
        .collect();

    (buf, xref_offset, obj_offsets)
}

/// Write PDF bytes to a temp file and return the path.
fn temp_pdf(bytes: &[u8], label: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("pipeline-e2e-{label}.pdf"));
    std::fs::write(&path, bytes).expect("write temp PDF");
    path
}

/// Scan PDF bytes by writing to a temp file first.
fn scan_pdf_bytes(bytes: &[u8], label: &str) -> pdf_cos::scan::DocumentStructure {
    let path = temp_pdf(bytes, &format!("scan-{label}"));
    let result = scan_structure(&path).expect("scan should succeed");
    std::fs::remove_file(&path).ok();
    result
}

// ---------------------------------------------------------------------------
// Test 1: Scan a generated PDF — structural summary is correct
// ---------------------------------------------------------------------------

#[test]
fn e2e_scan_generated_pdf() {
    let (pdf_bytes, _, _) = generate_pdf(3);
    let ds = scan_pdf_bytes(&pdf_bytes, "e2e-scan");

    assert_eq!(ds.page_count, 3);
    assert!(!ds.has_acroform);
    assert!(!ds.has_xfa);
    assert!(!ds.has_js);
    assert_eq!(ds.sig_count, 0);
    assert!(ds.leniency.is_empty());
    assert_eq!(ds.xref_offsets.len(), 5); // obj 1..5 (catalog + pages + 3 pages)
}

// ---------------------------------------------------------------------------
// Test 2: Spawn worker, inspect, get object bytes — end-to-end IPC
// ---------------------------------------------------------------------------

#[test]
fn e2e_worker_inspect_and_get_object() {
    let (pdf_bytes, _, expected_offsets) = generate_pdf(3);
    let path = temp_pdf(&pdf_bytes, "inspect");
    let brokered = open_read_only(&path).expect("broker open");
    let mut session =
        WorkerSession::spawn_with_document(&worker_path(), brokered).expect("spawn with doc");

    // Inspect
    let summary = session.inspect().expect("inspect");
    assert_eq!(summary.page_count, 3, "page count mismatch");
    assert!(!summary.has_acroform);

    // Verify xref offsets are reported in the summary
    for (obj_num, expected_offset) in &expected_offsets {
        let actual = summary.original_offsets.get(obj_num)
            .unwrap_or_else(|| panic!("missing xref offset for obj {obj_num}"));
        assert_eq!(actual, expected_offset,
            "obj {obj_num}: expected offset {expected_offset}, got {actual}");
    }

    // Fetch catalog (object 1) via GetObject
    let catalog = session.get_object(1).expect("get catalog");
    let catalog_text = String::from_utf8_lossy(&catalog);
    assert!(catalog_text.contains("/Type /Catalog"), "catalog content: {catalog_text}");
    assert!(catalog_text.contains("/Pages 2 0 R"), "catalog should reference Pages: {catalog_text}");

    // Fetch Pages (object 2)
    let pages = session.get_object(2).expect("get pages");
    let pages_text = String::from_utf8_lossy(&pages);
    assert!(pages_text.contains("/Type /Pages"), "pages content: {pages_text}");
    assert!(pages_text.contains("/Count 3"), "pages should have Count 3: {pages_text}");
    assert!(pages_text.contains("3 0 R"), "pages should reference page 3: {pages_text}");
    assert!(pages_text.contains("4 0 R"), "pages should reference page 4: {pages_text}");
    assert!(pages_text.contains("5 0 R"), "pages should reference page 5: {pages_text}");

    // Fetch a page object (object 3)
    let page = session.get_object(3).expect("get page 3");
    let page_text = String::from_utf8_lossy(&page);
    assert!(page_text.contains("/Type /Page"), "page content: {page_text}");
    assert!(page_text.contains("/MediaBox [0 0 612 792]"), "page should have MediaBox: {page_text}");

    // Quit
    session.send(b"quit").expect("quit");
    let _ = session.poll(Duration::from_secs(2));

    // Cleanup
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// Test 3: Incremental save with real xref offsets — produces valid PDF
// ---------------------------------------------------------------------------

#[test]
fn e2e_incremental_save_debug() {
    let (original_bytes, original_xref_offset, original_offsets) = generate_pdf(3);

    eprintln!("=== ORIGINAL PDF ===");
    eprintln!("  size: {} bytes", original_bytes.len());
    eprintln!("  original xref offset: {original_xref_offset}");
    eprintln!("  original offsets: {original_offsets:?}");

    // Simulate an edit: modify object 3 (first page) in the overlay.
    let mut overlay = CowOverlay::new();
    let modified_page = b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Modified >>\nendobj\n";
    overlay.set_object(3, modified_page.to_vec());
    overlay.bump_revision();

    // Write incremental update.
    let mut output = original_bytes.clone();
    let result = IncrementalWriter::write_incremental(
        &mut output,
        &overlay,
        original_xref_offset, // use the REAL previous xref offset
        6,
        &original_offsets,
        original_bytes.len() as u32,
    )
    .expect("incremental write");

    eprintln!("=== INCREMENTAL SAVE ===");
    eprintln!("  total size: {} bytes", output.len());
    eprintln!("  objects written: {}", result.objects_written);
    eprintln!("  xref offset: {}", result.xref_offset);
    eprintln!("  bytes appended: {}", result.bytes_appended);

    // Print the appended section
    let appended = &output[original_bytes.len()..];
    let appended_text = String::from_utf8_lossy(appended);
    eprintln!("=== APPENDED SECTION ===");
    for (i, line) in appended_text.lines().enumerate() {
        eprintln!("  {i}: {line}");
    }

    // Verify startxref is findable (use rfind for the LAST one)
    let text = String::from_utf8_lossy(&output);
    let startxref_pos = text.rfind("startxref\n");
    eprintln!("  startxref found at: {startxref_pos:?}");

    // Verify the xref section starts with "xref"
    let xref_section = &text[result.xref_offset as usize..];
    eprintln!("  xref section starts with: {:?}", &xref_section[..20.min(xref_section.len())]);

    // Scan
    let path = temp_pdf(&output, "debug-save");
    let ds = scan_structure(&path).expect("scan should succeed");
    assert_eq!(ds.page_count, 3);
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// Test 4: Full coordinator pipeline — open → inspect → get objects → modify → save
// ---------------------------------------------------------------------------

#[test]
fn e2e_coordinator_full_pipeline() {
    let (pdf_bytes, _, _) = generate_pdf(5);
    let path = temp_pdf(&pdf_bytes, "coordinator-pipeline");
    let brokered = open_read_only(&path).expect("broker open");
    let mut session =
        WorkerSession::spawn_with_document(&worker_path(), brokered).expect("spawn with doc");

    // 1. Inspect
    let summary = session.inspect().expect("inspect");
    assert_eq!(summary.page_count, 5);

    // 2. Get the Pages object and verify Kids array
    let pages = session.get_object(2).expect("get pages");
    let pages_text = String::from_utf8_lossy(&pages);
    assert!(pages_text.contains("/Count 5"), "should have 5 pages");

    // Parse kid references
    let mut kid_refs = Vec::new();
    if let Some(start) = pages_text.find("/Kids [") {
        let array_start = start + "/Kids [".len();
        if let Some(end) = pages_text[array_start..].find(']') {
            let array = &pages_text[array_start..array_start + end];
            let tokens: Vec<&str> = array.split_whitespace().collect();
            for chunk in tokens.chunks(3) {
                if chunk.len() == 3 && chunk[2] == "R" {
                    if let Ok(num) = chunk[0].parse::<u32>() {
                        kid_refs.push(num);
                    }
                }
            }
        }
    }
    assert_eq!(kid_refs.len(), 5, "should have 5 kid references");

    // 3. Verify all page objects exist
    for (i, &obj_num) in kid_refs.iter().enumerate() {
        let page = session.get_object(obj_num).expect(&format!("get page obj {obj_num}"));
        let page_text = String::from_utf8_lossy(&page);
        assert!(page_text.contains("/Type /Page"),
            "obj {obj_num} should be a Page: {page_text}");
        assert!(page_text.contains(&format!("/Parent 2 0 R")),
            "obj {obj_num} should have correct parent: {page_text}");
    }

    // 4. Simulate a mutation: delete page index 2 (object 5)
    //    New Kids array: [3 0 R, 4 0 R, 6 0 R, 7 0 R]
    let new_pages_obj = format!(
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R 4 0 R 6 0 R 7 0 R] /Count 4 >>\nendobj\n"
    );
    let mut overlay = CowOverlay::new();
    overlay.set_object(2, new_pages_obj.into_bytes());
    overlay.bump_revision();

    // 5. Incremental save
    let mut output = pdf_bytes.clone();
    let result = IncrementalWriter::write_incremental(
        &mut output,
        &overlay,
        0,
        8,
        &summary.original_offsets,
        pdf_bytes.len() as u32,
    )
    .expect("incremental save");

    assert!(result.objects_written >= 1);

    // 6. Verify the saved output — Kids array now has 4 pages
    let ds = scan_pdf_bytes(&output, "coordinator-save");
    assert_eq!(ds.page_count, 4, "Kids array was modified to have 4 pages");

    // 7. Quit worker
    session.send(b"quit").expect("quit");
    let _ = session.poll(Duration::from_secs(2));

    // Cleanup
    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// Test 5: Protocol roundtrip — all commands and events
// ---------------------------------------------------------------------------

#[test]
fn e2e_protocol_command_roundtrip() {
    use protocol::commands::{decode_command, encode_command};

    let commands = vec![
        Command::Inspect { correlation_id: 1 },
        Command::ExtractPage { correlation_id: 2, page_index: 0 },
        Command::GetOutline { correlation_id: 3 },
        Command::GetLayers { correlation_id: 4 },
        Command::GetAttachments { correlation_id: 5 },
        Command::GetObject { correlation_id: 6, obj_num: 1 },
        Command::DeletePages { correlation_id: 7, page_indices: vec![0, 2] },
        Command::RotatePages { correlation_id: 8, page_indices: vec![1], degrees: 90 },
        Command::AddAnnotation {
            correlation_id: 9,
            page_index: 0,
            annotation_type: "highlight".into(),
            rect: "10,20,100,12".into(),
            contents: None,
            color: Some("1,0,0,1".into()),
        },
        Command::DeleteAnnotation {
            correlation_id: 10,
            page_index: 1,
            annotation_id: 42,
        },
    ];

    for cmd in &commands {
        let bytes = encode_command(cmd);
        let decoded = decode_command(&bytes)
            .expect(&format!("failed to decode command: {cmd:?}"));
        assert_eq!(*cmd, decoded, "roundtrip failed for: {cmd:?}");
    }
}

// ---------------------------------------------------------------------------
// Test 6: Scan → save → rescan pipeline preserves structure
// ---------------------------------------------------------------------------

#[test]
fn e2e_scan_save_rescan_preserves_structure() {
    let (pdf_bytes, _, offsets) = generate_pdf(10);

    // Scan original
    let ds1 = scan_pdf_bytes(&pdf_bytes, "rescan-original");
    assert_eq!(ds1.page_count, 10);

    // Create overlay with one modified object
    let mut overlay = CowOverlay::new();
    let modified = b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [] >>\nendobj\n";
    overlay.set_object(3, modified.to_vec());
    overlay.bump_revision();

    // Save
    let mut output = pdf_bytes.clone();
    let result = IncrementalWriter::write_incremental(
        &mut output,
        &overlay,
        0,
        13,
        &offsets,
        pdf_bytes.len() as u32,
    )
    .expect("save");

    // Rescan
    let ds2 = scan_pdf_bytes(&output, "rescan-saved");
    assert_eq!(ds2.page_count, 10, "page count preserved after save");

    // The saved file should have the correct startxref (use rfind for the LAST one)
    let text = String::from_utf8_lossy(&output);
    let startxref_pos = text.rfind("startxref\n").expect("startxref not found");
    let after = &text[startxref_pos + 10..];
    let offset_str = after.lines().next().unwrap().trim();
    let reported: u32 = offset_str.parse().unwrap();
    assert_eq!(reported, result.xref_offset,
        "startxref ({reported}) should match xref_offset ({})", result.xref_offset);

    // Verify the object was actually written
    let xref_text = &text[result.xref_offset as usize..];
    assert!(xref_text.starts_with("xref\n"), "xref section should start at the reported offset");
}

// ---------------------------------------------------------------------------
// Test 7: Crash recovery — open → mutate → drop → reopen → recover
// ---------------------------------------------------------------------------

#[test]
fn e2e_crash_recovery_full_pipeline() {
    use coordinator::document::DocumentCoordinator;
    use pdf_model::command::{CommandGroup, SetObjectCommand};

    let (pdf_bytes, _, _) = generate_pdf(3);
    let path = temp_pdf(&pdf_bytes, "crash-recovery");

    // ---- Phase 1: Open, mutate, simulate crash ----
    let sidecar_path;
    {
        let mut coord = DocumentCoordinator::open(&worker_path(), &path)
            .expect("open document");

        sidecar_path = coord.sidecar_path().to_path_buf();

        // Apply 3 mutations.
        for i in 0..3 {
            let mut group = CommandGroup::new(format!("Pre-crash edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("{} 0 obj\n<< /CrashTest {} >>\nendobj\n", 10 + i, i).into_bytes(),
                old_bytes: None,
            }));
            coord.apply_command_group(group).expect("apply mutation");
        }

        // Verify sidecar exists with correct content.
        assert!(sidecar_path.exists(), "sidecar should exist after mutations");
        let sidecar_data = std::fs::read(&sidecar_path).expect("read sidecar");
        let sidecar_text = String::from_utf8_lossy(&sidecar_data);
        assert!(sidecar_text.contains("GROUP:Pre-crash edit 0"));
        assert!(sidecar_text.contains("GROUP:Pre-crash edit 2"));
        assert!(sidecar_text.contains("REVISION:3"));

        // Verify journal state before crash.
        assert_eq!(coord.undo_depth(), 3);
        assert!(!coord.can_redo());

        // Simulate crash: drop coordinator without close().
        // The sidecar persists on disk.
        drop(coord);
    }

    // ---- Phase 2: Verify sidecar survived the "crash" ----
    assert!(sidecar_path.exists(), "sidecar should survive crash (no clean close)");

    // ---- Phase 3: Reopen and recover ----
    {
        let mut coord = DocumentCoordinator::open(&worker_path(), &path)
            .expect("reopen document");

        // Check for orphaned sidecar.
        let recovery = coord.check_sidecar();
        assert!(recovery.is_some(), "should find orphaned sidecar");
        let info = recovery.unwrap();
        assert_eq!(info.group_count, 3, "should have 3 groups");
        assert_eq!(info.group_names, vec![
            "Pre-crash edit 0",
            "Pre-crash edit 1",
            "Pre-crash edit 2",
        ]);

        // Replay the journal to get the command groups.
        let groups = coord.replay_sidecar().expect("replay sidecar");
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].name, "Pre-crash edit 0");
        assert_eq!(groups[1].name, "Pre-crash edit 1");
        assert_eq!(groups[2].name, "Pre-crash edit 2");

        // Verify the overlay was NOT automatically restored —
        // the coordinator opens with a fresh overlay (the caller
        // applies the recovered groups).
        assert!(!coord.is_dirty(), "fresh open should not be dirty");

        // Apply the recovered groups to reconstruct pre-crash state.
        for group in groups {
            coord.apply_command_group(group).expect("apply recovered group");
        }

        // Verify state matches pre-crash.
        assert!(coord.is_dirty());
        assert_eq!(coord.undo_depth(), 3);
        assert_eq!(coord.undo_name(), Some("Pre-crash edit 2"));

        // Verify we can undo all the way back.
        for i in (0..3).rev() {
            let name = coord.undo_name().unwrap();
            assert_eq!(name, format!("Pre-crash edit {i}"));
            coord.undo().expect("undo recovered group");
        }
        assert!(!coord.can_undo());

        // Clean up.
        coord.close().expect("close");
    }

    // Verify sidecar is cleaned up.
    assert!(!sidecar_path.exists(), "sidecar should be deleted after close");

    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// Test 8: Crash recovery with undo before crash
// ---------------------------------------------------------------------------

#[test]
fn e2e_crash_recovery_with_undo() {
    use coordinator::document::DocumentCoordinator;
    use pdf_model::command::{CommandGroup, SetObjectCommand};

    let (pdf_bytes, _, _) = generate_pdf(2);
    let path = temp_pdf(&pdf_bytes, "crash-undo");

    let sidecar_path;
    {
        let mut coord = DocumentCoordinator::open(&worker_path(), &path)
            .expect("open");
        sidecar_path = coord.sidecar_path().to_path_buf();

        // Apply 4 edits.
        for i in 0..4 {
            let mut group = CommandGroup::new(format!("Edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("v{i}").into_bytes(),
                old_bytes: None,
            }));
            coord.apply_command_group(group).expect("apply");
        }
        assert_eq!(coord.undo_depth(), 4);

        // Undo 2 edits before "crash".
        coord.undo().expect("undo 1");
        coord.undo().expect("undo 2");
        assert_eq!(coord.undo_depth(), 2);
        assert_eq!(coord.redo_depth(), 2);

        // Crash — sidecar persists.
        drop(coord);
    }

    assert!(sidecar_path.exists());

    // Reopen and recover.
    {
        let mut coord = DocumentCoordinator::open(&worker_path(), &path)
            .expect("reopen");

        let groups = coord.replay_sidecar().expect("replay");
        // Journal should have all 4 groups (undo doesn't remove from journal,
        // it just moves them to the redo stack — but the sidecar only
        // serializes applied groups).
        // Actually, the sidecar serializes the current applied state.
        // After 4 applies and 2 undos, the applied stack has 2 groups.
        assert_eq!(groups.len(), 2, "should have 2 applied groups after 2 undos");
        assert_eq!(groups[0].name, "Edit 0");
        assert_eq!(groups[1].name, "Edit 1");

        // Apply recovered groups.
        for group in groups {
            coord.apply_command_group(group).expect("apply recovered");
        }
        assert_eq!(coord.undo_depth(), 2);

        coord.close().expect("close");
    }

    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// Test 9: Crash recovery — sidecar identity mismatch rejected
// ---------------------------------------------------------------------------

#[test]
fn e2e_crash_recovery_wrong_document_rejected() {
    use coordinator::document::DocumentCoordinator;

    let (pdf1_bytes, _, _) = generate_pdf(2);
    let (pdf2_bytes, _, _) = generate_pdf(3);
    let path1 = temp_pdf(&pdf1_bytes, "crash-id-1");
    let path2 = temp_pdf(&pdf2_bytes, "crash-id-2");

    // Open doc1, mutate, crash.
    let sidecar_path;
    {
        use pdf_model::command::{CommandGroup, SetObjectCommand};
        let mut coord = DocumentCoordinator::open(&worker_path(), &path1)
            .expect("open doc1");
        sidecar_path = coord.sidecar_path().to_path_buf();

        let mut group = CommandGroup::new("Doc1 edit");
        group.push(Box::new(SetObjectCommand {
            obj_num: 10,
            new_bytes: b"doc1-data".to_vec(),
            old_bytes: None,
        }));
        coord.apply_command_group(group).expect("apply");
        drop(coord);
    }

    assert!(sidecar_path.exists());

    // Open doc2 — it should NOT see doc1's sidecar.
    {
        let coord2 = DocumentCoordinator::open(&worker_path(), &path2)
            .expect("open doc2");

        // doc2 has a different sidecar path (different hash).
        let recovery = coord2.check_sidecar();
        assert!(recovery.is_none(), "doc2 should not find doc1's sidecar");

        // Even if we manually point to doc1's sidecar, it should be rejected.
        let result = DocumentCoordinator::read_sidecar(&sidecar_path, &path2);
        assert!(result.is_err(), "should reject sidecar for wrong document");
    }

    // Cleanup.
    let _ = std::fs::remove_file(&sidecar_path);
    std::fs::remove_file(&path1).ok();
    std::fs::remove_file(&path2).ok();
}

// ---------------------------------------------------------------------------
// Test 10: open_with_recovery — full auto-recovery flow
// ---------------------------------------------------------------------------

#[test]
fn e2e_open_with_recovery_auto_replays() {
    use coordinator::document::DocumentCoordinator;
    use pdf_model::command::{CommandGroup, SetObjectCommand};

    let (pdf_bytes, _, _) = generate_pdf(3);
    let path = temp_pdf(&pdf_bytes, "open-recovery");

    // Phase 1: Mutate and crash.
    {
        let mut coord = DocumentCoordinator::open(&worker_path(), &path)
            .expect("open");
        for i in 0..3 {
            let mut group = CommandGroup::new(format!("AutoR edit {i}"));
            group.push(Box::new(SetObjectCommand {
                obj_num: 10 + i,
                new_bytes: format!("auto-r-{i}").into_bytes(),
                old_bytes: None,
            }));
            coord.apply_command_group(group).expect("apply");
        }
        drop(coord);
    }

    // Phase 2: Open with recovery — should auto-replay.
    {
        let (mut coord, recovery) = DocumentCoordinator::open_with_recovery(&worker_path(), &path)
            .expect("open_with_recovery");

        // Recovery info should be present.
        assert!(recovery.is_some(), "should detect sidecar");
        let info = recovery.unwrap();
        assert_eq!(info.group_count, 3);
        assert_eq!(info.group_names, vec![
            "AutoR edit 0", "AutoR edit 1", "AutoR edit 2",
        ]);

        // State should be restored — dirty with 3 undoable groups.
        assert!(coord.is_dirty(), "should be dirty after recovery");
        assert_eq!(coord.undo_depth(), 3);
        assert_eq!(coord.undo_name(), Some("AutoR edit 2"));

        // Undo all recovered edits.
        for i in (0..3).rev() {
            assert_eq!(coord.undo_name(), Some(format!("AutoR edit {i}").as_str()));
            coord.undo().expect("undo recovered");
        }
        assert!(!coord.can_undo());

        // Sidecar should be deleted (no applied groups remain).
        assert!(!coord.sidecar_path().exists());

        coord.close().expect("close");
    }

    std::fs::remove_file(&path).ok();
}

// ---------------------------------------------------------------------------
// Test 11: open_with_recovery — no sidecar, clean open
// ---------------------------------------------------------------------------

#[test]
fn e2e_open_with_recovery_clean_open() {
    use coordinator::document::DocumentCoordinator;

    let (pdf_bytes, _, _) = generate_pdf(2);
    let path = temp_pdf(&pdf_bytes, "open-clean");

    let (coord, recovery) = DocumentCoordinator::open_with_recovery(&worker_path(), &path)
        .expect("open_with_recovery");

    // No sidecar — clean open.
    assert!(recovery.is_none(), "no sidecar for fresh document");
    assert!(!coord.is_dirty());
    assert_eq!(coord.undo_depth(), 0);

    // Sidecar should not exist.
    assert!(!coord.sidecar_path().exists());

    drop(coord);
    std::fs::remove_file(&path).ok();
}


// ---------------------------------------------------------------------------
// Outline / structure uses the same loaded engine (no second PDFium open)
// ---------------------------------------------------------------------------

#[test]
fn e2e_get_outline_with_engine() {
    let (pdf_bytes, _, _) = generate_pdf(3);
    let path = temp_pdf(&pdf_bytes, "outline-engine");
    let brokered = open_read_only(&path).expect("broker open");
    let mut session =
        WorkerSession::spawn_with_document(&worker_path(), brokered).expect("spawn with doc");

    let summary = session.inspect().expect("inspect");
    assert_eq!(summary.page_count, 3);

    // Must not fail with "no engine loaded" after single-engine worker fix. [ADR-005]
    let outline = session
        .get_outline()
        .unwrap_or_else(|e| panic!("GetOutline failed (engine missing?): {e}"));
    assert_eq!(outline.kind, "outline");
    // Generated PDF has no outline — empty count is OK.

    let layers = session
        .get_layers()
        .unwrap_or_else(|e| panic!("GetLayers failed: {e}"));
    assert_eq!(layers.kind, "layers");

    let atts = session
        .get_attachments()
        .unwrap_or_else(|e| panic!("GetAttachments failed: {e}"));
    assert_eq!(atts.kind, "attachments");

    std::fs::remove_file(&path).ok();
}
