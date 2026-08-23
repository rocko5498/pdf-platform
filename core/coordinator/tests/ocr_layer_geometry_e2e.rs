//! The OCR text layer must land on the words it transcribed. [FR-OCR-1, FR-OCR-2]
//!
//! Recognition returns boxes in **raster pixels**, with the origin at the top
//! left. A PDF text layer is placed in **points**, origin bottom left. Three
//! conversions sit between the two — the render scale, `scale_blocks_to_page`,
//! and the Y flip in `generate_text_layer_stream` — and each one is a place
//! where two layers can agree on a number and disagree about what it means.
//!
//! `ocr_e2e` proves the words come back. Nothing proved they come back *in the
//! right place*: a layer written upside down, or at raster scale, still
//! produces selectable text, and every existing assertion passes. Selection,
//! copy and find all read that geometry.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use coordinator::document::DocumentCoordinator;
use coordinator::ocr::{run_ocr_for_page, OcrOutcome, OcrPageContext, DEFAULT_CONFIDENCE_THRESHOLD};
use jobs::utility_pool::UtilityPool;
use jobs::{JobEvent, JobGraph, JobPriority, JobScheduler, JobSpec};
use ocr_bridge::PreprocessOptions;

fn ocr_required() -> bool {
    std::env::var_os("PDF_PLATFORM_REQUIRE_OCR").is_some()
}

fn worker_path() -> PathBuf {
    let exe = std::env::current_exe().expect("current exe");
    let mut dir = exe.parent().expect("exe dir").to_path_buf();
    if dir.ends_with("deps") {
        dir.pop();
    }
    dir.join(if cfg!(windows) { "worker.exe" } else { "worker" })
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("tools")
        .join("corpus-diff")
        .join("fixtures")
        .join(name)
}

/// Lines of extracted text with their boxes.
fn lines(path: &PathBuf) -> Vec<(String, f32, f32, f32, f32)> {
    let mut coord = DocumentCoordinator::open(&worker_path(), path).expect("open");
    let model = coord.get_page_text(0).expect("extract");
    let extracted: Vec<_> = model
        .lines
        .iter()
        .map(|line| {
            (
                line.text.to_lowercase(),
                line.x,
                line.y,
                line.width,
                line.height,
            )
        })
        .collect();
    let _ = coord.close();
    extracted
}

#[test]
fn the_ocr_text_layer_lands_on_the_words_it_transcribed() {
    if !ocr_required() {
        eprintln!("skip: PDF_PLATFORM_REQUIRE_OCR is not set, so no engine is guaranteed");
        return;
    }

    let source = fixture("text-latin.pdf");
    assert!(source.is_file(), "fixture missing: {}", source.display());

    let dir = std::env::temp_dir().join(format!("pdf-platform-ocr-geom-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let working = dir.join("page.pdf");
    std::fs::copy(&source, &working).expect("copy fixture");

    // Where the visible words actually are, before anything is added.
    let before = lines(&working);
    let (visible_text, visible_x, visible_y, _, visible_height) = before
        .first()
        .cloned()
        .expect("the fixture draws at least one line of text");

    // Build the page context the way the CLI does.
    let mut coord = DocumentCoordinator::open(&worker_path(), &working).expect("open");
    let (page_obj_num, original_page_bytes) = coord.page_object(0).expect("page object");
    let (page_width_pt, page_height_pt, _) = coord.summary().page_dimensions_f()[0];
    let next_obj_num = coord.next_obj_num();
    coord.set_next_obj_num(next_obj_num + 2);

    let pool = Arc::new(UtilityPool::new(worker_path(), 1).expect("utility pool"));
    let document = Arc::new(std::fs::File::open(&working).expect("open for pool"));
    let outcome: Arc<Mutex<Option<OcrOutcome>>> = Arc::new(Mutex::new(None));

    let exec_pool = pool.clone();
    let exec_document = document.clone();
    let observed = outcome.clone();
    let context_parts = (page_obj_num, original_page_bytes, page_width_pt, page_height_pt, next_obj_num);

    let scheduler = JobScheduler::new_typed(1, 1, move |spec, context| {
        let (page_obj_num, ref original_page_bytes, page_width_pt, page_height_pt, next_obj_num) =
            context_parts;
        let result = run_ocr_for_page(
            &exec_pool,
            &exec_document,
            None,
            spec.id,
            context,
            OcrPageContext {
                page_index: 0,
                page_obj_num,
                original_page_bytes: original_page_bytes.clone(),
                page_width_pt,
                page_height_pt,
                next_obj_num,
            },
            "eng",
            PreprocessOptions {
                deskew: false,
                despeckle: false,
                target_dpi: 150,
                ocr_pages_with_text: true,
            },
            DEFAULT_CONFIDENCE_THRESHOLD,
        )?;
        *observed.lock().unwrap() = Some(result);
        Ok(())
    })
    .expect("scheduler");

    scheduler
        .submit(JobGraph::new(vec![JobSpec::new(1, "ocr", JobPriority::Maintenance)]).unwrap())
        .expect("submit");

    let mut failure = None;
    loop {
        match scheduler.recv_event_timeout(Duration::from_secs(120)) {
            Some(JobEvent::Completed { job: 1 }) => break,
            Some(JobEvent::Failed { job: 1, message }) => {
                failure = Some(message);
                break;
            }
            Some(_) => {}
            None => panic!("timed out waiting for the OCR job"),
        }
    }
    scheduler.shutdown();

    assert!(
        failure.is_none(),
        "PDF_PLATFORM_REQUIRE_OCR is set, so OCR must reach a recognition outcome: {failure:?}"
    );

    let group = match outcome.lock().unwrap().take().expect("outcome") {
        OcrOutcome::Applied(group) => group,
        OcrOutcome::Uncertain { result, threshold } => panic!(
            "recognition of a clean 12pt page was below {threshold}: {:?}",
            result.full_text
        ),
        OcrOutcome::Failed(message) => panic!("recognition failed: {message}"),
    };

    coord.apply_command_group(group).expect("apply text layer");
    let output = dir.join("ocr.pdf");
    coord.save_incremental(&output).expect("save");
    let _ = coord.close();

    // Read the layer back the way selection and find do.
    let after = lines(&output);
    let word = visible_text
        .split_whitespace()
        .find(|w| w.len() > 3)
        .expect("the fixture draws a word worth matching")
        .to_string();

    let added: Vec<_> = after
        .iter()
        .filter(|(text, x, y, _, _)| {
            text.contains(&word) && !before.iter().any(|(t, bx, by, _, _)| t == text && bx == x && by == y)
        })
        .collect();

    assert!(
        !added.is_empty(),
        "no new line containing {word:?} was added; the layer is missing or \
         transcribed something else.\nbefore: {before:?}\nafter: {after:?}"
    );

    // A Y flip that went the wrong way puts the layer at (page_height - y)
    // instead of y — hundreds of points away on a letter page. A layer left at
    // raster scale lands off the page entirely. Half the line's height is a
    // tight enough bound to catch both and loose enough for a recognizer that
    // boxes the ink rather than the glyph cell.
    let tolerance = (visible_height * 2.0).max(12.0);
    let best = added
        .iter()
        .min_by(|a, b| {
            let da = (a.2 - visible_y).abs();
            let db = (b.2 - visible_y).abs();
            da.partial_cmp(&db).unwrap()
        })
        .expect("checked non-empty");

    assert!(
        (best.2 - visible_y).abs() <= tolerance,
        "the OCR layer for {word:?} sits at y={} but the visible text is at y={visible_y} \
         (tolerance {tolerance}); a Y flip or a scale is wrong",
        best.2
    );
    assert!(
        (best.1 - visible_x).abs() <= tolerance * 2.0,
        "the OCR layer for {word:?} sits at x={} but the visible text is at x={visible_x}",
        best.1
    );

    let _ = std::fs::remove_dir_all(&dir);
}
