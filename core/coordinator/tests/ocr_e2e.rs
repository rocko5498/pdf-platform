//! End-to-end OCR scheduling test: a real sandboxed utility worker, through
//! `coordinator::ocr::run_ocr_for_page`. [ADR-018, FR-OCR-1]
//!
//! Uses a blank single-page fixture (no scanned image content), so the
//! recognized-text outcome is deterministic across dev machines regardless
//! of whether Tesseract is installed: either the engine is unavailable, or
//! it runs and finds no text on a blank page. Both are legitimate
//! `OcrOutcome::Failed` results — this test proves the self-render→
//! recognize→decode wiring reaches the real dispatch path, not that OCR
//! "worked."

use std::io::Write as _;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use jobs::utility_pool::UtilityPool;
use jobs::{JobEvent, JobGraph, JobPriority, JobScheduler, JobSpec};
use coordinator::ocr::{run_ocr_for_page, OcrOutcome, OcrPageContext, DEFAULT_CONFIDENCE_THRESHOLD};
use ocr_bridge::PreprocessOptions;

fn worker_path() -> PathBuf {
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

fn one_page_pdf() -> Vec<u8> {
    let mut bytes = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for object in [
        "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n",
        "2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n",
        "3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
    ] {
        offsets.push(bytes.len());
        bytes.extend_from_slice(object.as_bytes());
    }
    let xref = bytes.len();
    bytes.extend_from_slice(b"xref\n0 4\n0000000000 65535 f \n");
    for offset in offsets {
        writeln!(bytes, "{offset:010} 00000 n ").unwrap();
    }
    write!(
        bytes,
        "trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n"
    )
    .unwrap();
    bytes
}

fn temp_pdf(bytes: &[u8]) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "pdf-platform-ocr-e2e-{}.pdf",
        std::process::id()
    ));
    std::fs::write(&path, bytes).unwrap();
    path
}

const PAGE_OBJECT_BYTES: &[u8] =
    b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n";

#[test]
fn ocr_dispatch_reaches_render_recognize_and_decode() {
    let path = temp_pdf(&one_page_pdf());

    let pool = UtilityPool::new(worker_path(), 1).expect("utility pool");
    let document = std::fs::File::open(&path).expect("open for utility pool");

    let pool = Arc::new(pool);
    let document = Arc::new(document);
    let outcome: Arc<Mutex<Option<OcrOutcome>>> = Arc::new(Mutex::new(None));

    let exec_pool = pool.clone();
    let exec_document = document.clone();
    let observed = outcome.clone();

    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        let page = OcrPageContext {
            page_index: 0,
            page_obj_num: 3,
            original_page_bytes: PAGE_OBJECT_BYTES.to_vec(),
            page_width_pt: 612.0,
            page_height_pt: 792.0,
            next_obj_num: 4,
        };
        let result = run_ocr_for_page(
            &exec_pool,
            &exec_document,
            None,
            spec.id,
            context,
            page,
            "eng",
            PreprocessOptions::default(),
            DEFAULT_CONFIDENCE_THRESHOLD,
        )?;
        *observed.lock().unwrap() = Some(result);
        Ok(())
    })
    .unwrap();

    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(1, "ocr-schedule", JobPriority::Maintenance)]).unwrap())
        .unwrap();

    let mut dispatch_failed_message = None;
    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(20)) {
            Some(JobEvent::Completed { job: 1 }) => break,
            Some(JobEvent::Failed { job: 1, message }) => {
                dispatch_failed_message = Some(message);
                break;
            }
            Some(_) => {}
            None => panic!("timed out waiting for ocr schedule job"),
        }
    }
    scheduler.shutdown();
    let _ = std::fs::remove_file(&path);

    // The dispatch itself must never fail with "unsupported operation" (the
    // dead-code bug this session found and fixed) — any failure here must be
    // a legitimate engine/recognition outcome, surfaced via JobRunError.
    if let Some(message) = dispatch_failed_message {
        let lower = message.to_lowercase();
        assert!(
            lower.contains("tesseract") || lower.contains("unavailable") || lower.contains("no text"),
            "unexpected ocr dispatch failure: {message}"
        );
        return;
    }

    let captured = outcome.lock().unwrap().take().expect("outcome captured");
    match captured {
        OcrOutcome::Applied(group) => {
            // Tesseract installed and (implausibly) found text on a blank page —
            // still a legitimate outcome; just confirm the group is well-formed.
            assert_eq!(group.name, "OCR page 1");
        }
        OcrOutcome::Uncertain { threshold, .. } => {
            assert_eq!(threshold, DEFAULT_CONFIDENCE_THRESHOLD);
        }
        OcrOutcome::Failed(message) => {
            let lower = message.to_lowercase();
            assert!(
                lower.contains("tesseract")
                    || lower.contains("unavailable")
                    || lower.contains("no text"),
                "unexpected ocr outcome failure: {message}"
            );
        }
    }
}
