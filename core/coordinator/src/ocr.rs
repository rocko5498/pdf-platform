//! Coordinator-side OCR job scheduling. [ADR-018, SDS §2.5, FR-OCR-1..5]
//!
//! Dispatches recognition to the sandboxed utility pool as an `"ocr"` job —
//! the worker renders the page itself (via its own loaded document engine)
//! and recognizes in the same process, so no page raster crosses IPC in
//! either direction. Only when confidence clears the threshold does this
//! build the `ApplyOcrTextLayerCommand` group linking the invisible text
//! layer. Low-confidence results are surfaced, never silently applied.
//! [FR-OCR-4]

use std::fs::File;

use jobs::utility_pool::UtilityPool;
use jobs::{JobContext, JobPriority, JobRunError, JobSpec};
use ocr_bridge::{
    decode_utility_result, encode_utility_request, generate_text_layer_stream,
    scale_blocks_to_page, OcrPageResult, OcrUtilityRequest, PreprocessOptions,
};
use pdf_model::command::CommandGroup;
use pdf_model::ocr_layer::build_apply_ocr_text_layer_group;

/// Default minimum average confidence to auto-apply a text layer. [FR-OCR-4]
///
/// Below this, the result is surfaced as [`OcrOutcome::Uncertain`] rather
/// than silently inserted. Conservative starting point; tune against the
/// scan corpus (ADR-022 §2) once one exists for this path.
pub const DEFAULT_CONFIDENCE_THRESHOLD: f32 = 0.5;

/// Outcome of running OCR for one page.
#[derive(Debug)]
pub enum OcrOutcome {
    /// Confidence cleared the threshold; ready to apply to the journal.
    Applied(CommandGroup),
    /// Recognition succeeded but confidence was too low to trust. [FR-OCR-4]
    Uncertain {
        /// The raw (unapplied) recognition result, for diagnostics/manual review.
        result: OcrPageResult,
        /// The threshold it failed to clear.
        threshold: f32,
    },
    /// Recognition failed outright (engine unavailable, no text found, etc).
    Failed(String),
}

/// Everything about the target page that `run_ocr_for_page` cannot itself
/// derive (it never touches the overlay or engine directly).
pub struct OcrPageContext {
    /// 0-based page index.
    pub page_index: u32,
    /// 1-based PDF object number of the page dictionary.
    pub page_obj_num: u32,
    /// Current serialized page object bytes (pre-OCR), for the Command's undo.
    pub original_page_bytes: Vec<u8>,
    /// Page width in PDF points (MediaBox).
    pub page_width_pt: f32,
    /// Page height in PDF points (MediaBox).
    pub page_height_pt: f32,
    /// Next free PDF object number (two are consumed on success — see
    /// [`build_apply_ocr_text_layer_group`]).
    pub next_obj_num: u32,
}

/// Run OCR for one page via the sandboxed utility pool and build the
/// apply-ready Command group.
///
/// Intended as (or called from) a `JobScheduler` executor closure — reuses
/// the scheduler's own cooperative-cancellation and idempotent-retry-once
/// handling rather than reimplementing either. [ADR-009]
pub fn run_ocr_for_page(
    pool: &UtilityPool,
    document: &File,
    password: Option<&str>,
    job_id: u64,
    context: JobContext,
    page: OcrPageContext,
    language: &str,
    options: PreprocessOptions,
    confidence_threshold: f32,
) -> Result<OcrOutcome, JobRunError> {
    let request = OcrUtilityRequest {
        page_index: page.page_index,
        language: language.to_string(),
        options,
    };
    let request_bytes = encode_utility_request(&request).map_err(|error| {
        JobRunError::Execution(format!("ocr request encode failed: {error:?}"))
    })?;

    let spec = JobSpec::new(job_id, "ocr", JobPriority::Maintenance).idempotent();
    let output = pool.execute_prepared_with_document(spec, context, document, password, {
        move |_preparation| Ok(vec![protocol::utility_jobs::UtilityJobInput::Inline(
            request_bytes,
        )])
    })?;

    let result = match decode_utility_result(&output) {
        Ok(result) => result,
        Err(error) => {
            return Ok(OcrOutcome::Failed(format!(
                "ocr result decode failed: {error:?}"
            )))
        }
    };
    if !result.success {
        return Ok(OcrOutcome::Failed(
            result
                .error
                .clone()
                .unwrap_or_else(|| "ocr recognition failed".into()),
        ));
    }
    if result.average_confidence < confidence_threshold {
        return Ok(OcrOutcome::Uncertain {
            result,
            threshold: confidence_threshold,
        });
    }

    let scaled_blocks = scale_blocks_to_page(
        &result.blocks,
        result.raster_width,
        result.raster_height,
        page.page_width_pt,
        page.page_height_pt,
    );
    let stream = generate_text_layer_stream(&scaled_blocks, page.page_height_pt);
    let (group, _next) = build_apply_ocr_text_layer_group(
        page.page_index,
        page.page_obj_num,
        page.original_page_bytes,
        &stream,
        page.next_obj_num,
    )
    .map_err(|error| JobRunError::Execution(format!("text layer patch failed: {error}")))?;

    Ok(OcrOutcome::Applied(group))
}
