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
        // Must exceed UtilityPool::response_timeout (30s), or this races the
        // pool and reports "timed out" for a job the pool would have failed
        // honestly. Debug-build OCR preprocessing is megapixel work in
        // unoptimized loops, so 20s was not enough. [ADR-022]
        match scheduler.recv_event_timeout(Duration::from_secs(60)) {
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

/// Proves an OCR job participates correctly in `JobScheduler`'s existing
/// idempotent-retry-once-after-worker-loss mechanism (`jobs::lib`'s own
/// `idempotent_job_retries_one_worker_crash` proves the mechanism generically
/// with a mocked executor; this proves a *real* `run_ocr_for_page` call is
/// what actually runs on the retried attempt, not just a bare mock).
/// [ADR-009]
#[test]
fn ocr_job_is_idempotent_and_retries_after_simulated_worker_loss() {
    // Environment-tolerant like the other OCR e2e tests: on a machine
    // without Tesseract, the real (retried) attempt legitimately fails too
    // (EngineUnavailable, a JobRunError::Execution -- correctly NOT retried
    // further, unlike the simulated WorkerCrashed first attempt). What this
    // proves either way: exactly one retry happens, and the retried attempt
    // is a genuine run_ocr_for_page call (captured below), not a mock.
    let path = temp_pdf(&one_page_pdf());
    let pool = Arc::new(UtilityPool::new(worker_path(), 1).unwrap());
    let document = Arc::new(std::fs::File::open(&path).unwrap());
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let second_attempt: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));

    let exec_pool = pool.clone();
    let exec_document = document.clone();
    let exec_attempts = attempts.clone();
    let exec_second_attempt = second_attempt.clone();
    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        if exec_attempts.fetch_add(1, std::sync::atomic::Ordering::Relaxed) == 0 {
            // Simulate the utility worker being lost before this attempt
            // could run at all -- the same failure shape a real crash
            // produces (JobRunError::WorkerCrashed), before ever touching
            // run_ocr_for_page.
            return Err(jobs::JobRunError::WorkerCrashed(
                "simulated utility worker loss".into(),
            ));
        }
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
        );
        *exec_second_attempt.lock().unwrap() = Some(match &result {
            Ok(OcrOutcome::Applied(_)) => "applied".to_string(),
            Ok(OcrOutcome::Uncertain { .. }) => "uncertain".to_string(),
            Ok(OcrOutcome::Failed(message)) => format!("failed: {message}"),
            Err(error) => format!("job-error: {error:?}"),
        });
        result?;
        Ok(())
    })
    .unwrap();

    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(1, "ocr-schedule", JobPriority::Maintenance).idempotent()]).unwrap())
        .unwrap();

    loop {
        // Must exceed UtilityPool::response_timeout (30s), or this races the
        // pool and reports "timed out" for a job the pool would have failed
        // honestly. Debug-build OCR preprocessing is megapixel work in
        // unoptimized loops, so 20s was not enough. [ADR-022]
        match scheduler.recv_event_timeout(Duration::from_secs(60)) {
            Some(JobEvent::Completed { job: 1 }) | Some(JobEvent::Failed { job: 1, .. }) => break,
            Some(_) => {}
            None => panic!("timed out waiting for the retried ocr job"),
        }
    }
    scheduler.shutdown();
    let _ = std::fs::remove_file(&path);

    assert_eq!(
        attempts.load(std::sync::atomic::Ordering::Relaxed),
        2,
        "idempotent ocr job must retry exactly once after simulated worker loss"
    );
    let second = second_attempt.lock().unwrap().take();
    let second = second.expect("the retried attempt must have actually run run_ocr_for_page");
    let lower = second.to_lowercase();
    assert!(
        lower.contains("applied")
            || lower.contains("uncertain")
            || lower.contains("tesseract")
            || lower.contains("unavailable")
            || lower.contains("no text"),
        "unexpected retried-attempt outcome: {second}"
    );
}

/// Proves a cancelled OCR job never reaches the sandboxed worker at all --
/// `JobScheduler` checks cancellation before dispatch (see
/// `jobs::lib`'s dispatch loop) and short-circuits to `JobEvent::Cancelled`
/// without ever invoking the executor. Blocks the scheduler's only worker
/// thread with an unrelated job first so the OCR job is guaranteed to still
/// be pending (and therefore cancellable) when cancelled. [ADR-009]
#[test]
fn ocr_job_never_dispatches_to_the_pool_when_cancelled_before_start() {
    let path = temp_pdf(&one_page_pdf());
    let pool = Arc::new(UtilityPool::new(worker_path(), 1).unwrap());
    let document = Arc::new(std::fs::File::open(&path).unwrap());
    let outcome: Arc<Mutex<Option<OcrOutcome>>> = Arc::new(Mutex::new(None));

    let exec_pool = pool.clone();
    let exec_document = document.clone();
    let exec_outcome = outcome.clone();
    let scheduler = JobScheduler::new_typed(1, 2, move |spec, context| {
        if spec.id == 1 {
            // Occupy the sole worker thread so job 2 (the OCR job) is
            // guaranteed to still be pending when the test cancels it.
            std::thread::sleep(Duration::from_millis(300));
            return Ok(());
        }
        if context.is_cancelled() {
            return Err(jobs::JobRunError::Execution("cancelled before dispatch".into()));
        }
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
        *exec_outcome.lock().unwrap() = Some(result);
        Ok(())
    })
    .unwrap();

    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(1, "blocker", JobPriority::Maintenance)]).unwrap())
        .unwrap();
    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(2, "ocr-schedule", JobPriority::Maintenance)]).unwrap())
        .unwrap();
    assert!(
        scheduler.cancel(2),
        "ocr job must be tracked and cancellable while still pending"
    );

    let mut seen: std::collections::HashMap<u64, &str> = std::collections::HashMap::new();
    while seen.len() < 2 {
        match scheduler.recv_event_timeout(Duration::from_secs(5)) {
            Some(JobEvent::Completed { job }) => {
                seen.insert(job, "completed");
            }
            Some(JobEvent::Cancelled { job }) => {
                seen.insert(job, "cancelled");
            }
            Some(JobEvent::Failed { job, .. }) => {
                seen.insert(job, "failed");
            }
            Some(_) => {}
            None => panic!("timed out waiting for terminal events"),
        }
    }
    scheduler.shutdown();
    let _ = std::fs::remove_file(&path);

    assert_eq!(seen.get(&1), Some(&"completed"));
    assert_eq!(
        seen.get(&2),
        Some(&"cancelled"),
        "ocr job must be cancelled cooperatively, not dispatched to the worker"
    );
    assert!(
        outcome.lock().unwrap().is_none(),
        "a cancelled ocr job must never actually run recognition"
    );
}
