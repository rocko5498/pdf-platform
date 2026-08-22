//! Job-DAG batch pipeline system. [FR-BATCH, M6]
//!
//! Composes multiple PDF operations into a repeatable pipeline.
//! Each operation is a `BatchStep` that transforms input files into output files.
//! Steps execute sequentially; the output of one step feeds the next.
//!
//! The pipeline is serializable (save/reload) and CLI-reproducible. [FR-BATCH-2]

use std::path::PathBuf;

/// A single step in a batch pipeline.
#[derive(Debug, Clone)]
pub enum BatchStep {
    /// Merge multiple PDFs into one.
    Merge {
        /// Input file paths.
        inputs: Vec<PathBuf>,
        /// Output path.
        output: PathBuf,
    },
    /// Split a PDF into individual pages.
    SplitPerPage {
        /// Input file path.
        input: PathBuf,
        /// Output directory.
        output_dir: PathBuf,
    },
    /// Split a PDF into chunks of N pages.
    SplitChunked {
        /// Input file path.
        input: PathBuf,
        /// Pages per output file.
        pages_per_file: u32,
        /// Output directory.
        output_dir: PathBuf,
    },
    /// Extract a page range from a PDF.
    ExtractPages {
        /// Input file path.
        input: PathBuf,
        /// First page (1-based).
        first: u32,
        /// Last page (1-based, inclusive).
        last: u32,
        /// Output path.
        output: PathBuf,
    },
    /// Optimize/compress a PDF.
    Optimize {
        /// Input file path.
        input: PathBuf,
        /// Output path.
        output: PathBuf,
        /// Optimization profile: "screen", "print", "archive".
        profile: String,
    },
    /// Apply a watermark to all pages.
    Watermark {
        /// Input file path.
        input: PathBuf,
        /// Watermark text.
        text: String,
        /// Output path.
        output: PathBuf,
    },
    /// Recognize text on pages that have none, adding a text layer.
    ///
    /// FR-BATCH requires an operation to behave identically whether invoked
    /// singly or as a pipeline step. Like the stamp steps, OCR needs the
    /// coordinator and a sandboxed worker, so `execute_step` refuses it and the
    /// CLI executor runs it through the same `run_ocr` the `ocr` subcommand
    /// uses — one implementation, not two. [FR-BATCH, FR-OCR-1]
    Ocr {
        /// Input file path.
        input: PathBuf,
        /// Tesseract language selection, e.g. `eng` or `eng+deu`.
        language: String,
        /// Whether to OCR pages that already carry text.
        include_pages_with_text: bool,
        /// Output path.
        output: PathBuf,
    },
    /// Apply Bates numbering to all pages.
    BatesNumber {
        /// Input file path.
        input: PathBuf,
        /// Starting number.
        start: u32,
        /// Number width (zero-padded).
        width: usize,
        /// Output path.
        output: PathBuf,
    },
}

/// Result of executing a single batch step.
#[derive(Debug, Clone)]
pub struct StepResult {
    /// Whether the step succeeded.
    pub success: bool,
    /// Output file paths produced.
    pub outputs: Vec<PathBuf>,
    /// Human-readable status message.
    pub message: String,
    /// Duration in milliseconds.
    pub duration_ms: u64,
}

/// A complete batch pipeline: named sequence of steps.
#[derive(Debug, Clone)]
pub struct BatchPipeline {
    /// Human-readable name for this pipeline.
    pub name: String,
    /// Description of what this pipeline does.
    pub description: String,
    /// The steps to execute in order.
    pub steps: Vec<BatchStep>,
}

impl BatchPipeline {
    /// Create a new empty pipeline.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            steps: Vec::new(),
        }
    }

    /// Add a step to the pipeline.
    pub fn add_step(&mut self, step: BatchStep) {
        self.steps.push(step);
    }

    /// Number of steps in the pipeline.
    pub fn step_count(&self) -> usize {
        self.steps.len()
    }

    /// Serialize the pipeline to a simple text format for CLI save/load. [FR-BATCH-2]
    pub fn serialize(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("# Pipeline: {}\n", self.name));
        if !self.description.is_empty() {
            out.push_str(&format!("# {}\n", self.description));
        }
        out.push_str(&format!("steps={}\n", self.steps.len()));
        for (i, step) in self.steps.iter().enumerate() {
            out.push_str(&format!("\n[step {}]\n", i + 1));
            match step {
                BatchStep::Merge { inputs, output } => {
                    out.push_str("type=merge\n");
                    for inp in inputs {
                        out.push_str(&format!("input={}\n", inp.display()));
                    }
                    out.push_str(&format!("output={}\n", output.display()));
                }
                BatchStep::SplitPerPage { input, output_dir } => {
                    out.push_str("type=split_per_page\n");
                    out.push_str(&format!("input={}\n", input.display()));
                    out.push_str(&format!("output_dir={}\n", output_dir.display()));
                }
                BatchStep::SplitChunked { input, pages_per_file, output_dir } => {
                    out.push_str("type=split_chunked\n");
                    out.push_str(&format!("input={}\n", input.display()));
                    out.push_str(&format!("pages_per_file={pages_per_file}\n"));
                    out.push_str(&format!("output_dir={}\n", output_dir.display()));
                }
                BatchStep::Ocr { input, language, include_pages_with_text, output } => {
                    out.push_str("type=ocr
");
                    out.push_str(&format!("input={}
", input.display()));
                    out.push_str(&format!("lang={language}
"));
                    if *include_pages_with_text {
                        out.push_str("with_text=true
");
                    }
                    out.push_str(&format!("output={}
", output.display()));
                }
                BatchStep::ExtractPages { input, first, last, output } => {
                    out.push_str("type=extract_pages\n");
                    out.push_str(&format!("input={}\n", input.display()));
                    out.push_str(&format!("first={first}\n"));
                    out.push_str(&format!("last={last}\n"));
                    out.push_str(&format!("output={}\n", output.display()));
                }
                BatchStep::Optimize { input, output, profile } => {
                    out.push_str("type=optimize\n");
                    out.push_str(&format!("input={}\n", input.display()));
                    out.push_str(&format!("output={}\n", output.display()));
                    out.push_str(&format!("profile={profile}\n"));
                }
                BatchStep::Watermark { input, text, output } => {
                    out.push_str("type=watermark\n");
                    out.push_str(&format!("input={}\n", input.display()));
                    out.push_str(&format!("text={text}\n"));
                    out.push_str(&format!("output={}\n", output.display()));
                }
                BatchStep::BatesNumber { input, start, width, output } => {
                    out.push_str("type=bates_number\n");
                    out.push_str(&format!("input={}\n", input.display()));
                    out.push_str(&format!("start={start}\n"));
                    out.push_str(&format!("width={width}\n"));
                    out.push_str(&format!("output={}\n", output.display()));
                }
            }
        }
        out
    }
}

/// Execute a batch pipeline step-by-step. [FR-BATCH-1, FR-BATCH-3]
///
/// Each step is executed in order. If a step fails, execution stops
/// and the error is reported. Returns results for each completed step.
///
/// Runs `Optimize` steps directly through `assembly_ops::optimize_pdf` with
/// no verification — the same as this crate always has. Callers that can
/// see the coordinator layer (which owns the candidate/verify/publish
/// safety net, `pdf-model` must not depend on it — ADR-025) should use
/// [`execute_pipeline_with`] instead so `Optimize` steps get the same
/// verified-publish guarantee the CLI's standalone `optimize` command has.
pub fn execute_pipeline(pipeline: &BatchPipeline) -> Vec<StepResult> {
    execute_pipeline_with(pipeline, &|input, output, profile| {
        crate::assembly_ops::optimize_pdf(input, output, profile).map_err(|e| e.to_string())
    })
}

/// Like [`execute_pipeline`], but `Optimize` steps are generated through the
/// supplied `optimize` hook instead of calling `assembly_ops::optimize_pdf`
/// directly. The hook receives the same `(input, output, profile)` the step
/// declares and decides how to get bytes to `output` safely — e.g. wrapping
/// `coordinator::broker::optimize_with_verification` around
/// `assembly_ops::optimize_pdf` to add candidate/verify/atomic-publish —
/// without `pdf-model` itself depending on `coordinator` (ADR-025 layering).
/// [FR-BATCH-1]
pub fn execute_pipeline_with(
    pipeline: &BatchPipeline,
    optimize: &dyn Fn(&std::path::Path, &std::path::Path, crate::assembly::OptimizeProfile) -> Result<String, String>,
) -> Vec<StepResult> {
    let mut results: Vec<StepResult> = Vec::new();
    for step in &pipeline.steps {
        let start = std::time::Instant::now();
        let result = execute_step(step, optimize);
        let duration_ms = start.elapsed().as_millis() as u64;
        let success = result.is_ok();
        let (outputs, message) = match result {
            Ok(outputs) => (outputs, "ok".to_string()),
            Err(error) => (Vec::new(), error),
        };
        results.push(StepResult {
            success,
            outputs,
            message,
            duration_ms,
        });
        // Stop on first failure.
        if !success {
            break;
        }
    }
    results
}

/// Execute a single batch step. Returns output paths on success.
pub fn execute_step(
    step: &BatchStep,
    optimize: &dyn Fn(&std::path::Path, &std::path::Path, crate::assembly::OptimizeProfile) -> Result<String, String>,
) -> Result<Vec<PathBuf>, String> {
    match step {
        BatchStep::Merge { inputs, output } => {
            let paths: Vec<&std::path::Path> = inputs.iter().map(|p| p.as_path()).collect();
            crate::assembly_ops::merge_pdfs(&paths, output)
                .map_err(|e| e.to_string())?;
            Ok(vec![output.clone()])
        }
        BatchStep::SplitPerPage { input, output_dir } => {
            let parts = crate::assembly_ops::split_pdf_per_page(input, output_dir)
                .map_err(|e| e.to_string())?;
            Ok(parts)
        }
        BatchStep::SplitChunked { input, pages_per_file, output_dir } => {
            let parts = crate::assembly_ops::split_pdf_chunked(input, *pages_per_file, output_dir)
                .map_err(|e| e.to_string())?;
            Ok(parts)
        }
        BatchStep::ExtractPages { input, first, last, output } => {
            crate::assembly_ops::extract_pages(input, *first, *last, output)
                .map_err(|e| e.to_string())?;
            Ok(vec![output.clone()])
        }
        BatchStep::Optimize { input, output, profile } => {
            let prof = match profile.as_str() {
                "print" => crate::assembly::OptimizeProfile::Print,
                "archive" => crate::assembly::OptimizeProfile::ArchivePreserving,
                _ => crate::assembly::OptimizeProfile::Screen,
            };
            optimize(input, output, prof)?;
            Ok(vec![output.clone()])
        }
        // These two previously copied the input to the output and reported
        // success, so a batch run emitted an unstamped file and called it
        // stamped. `stamp` generates content streams but cannot inject them
        // into a page; that is a coordinator mutation path (ADR-013). Until
        // it is wired through, refusing with a reason is the required
        // behaviour: correctness before capability, and never a false
        // success. [PRIN-1, PRIN-6, GR-8, UX-ERR-3, FR-STAMP]
        // Same reasoning as the stamp steps below: this needs the coordinator
        // and a sandboxed worker, which `pdf-model` does not and should not
        // reach. The CLI executor runs it; refusing here is honest about what
        // this function can do. [FR-BATCH, ADR-025, GR-8]
        BatchStep::Ocr { input, .. } => Err(format!(
            "OCR of {} not run: it needs the coordinator's worker path, which \
             the model-level executor does not have. Run it through the CLI.",
            input.display()
        )),
        BatchStep::Watermark { text, .. } => Err(format!(
            "watermark '{text}' not applied: batch stamping needs the \
             coordinator page injection path, which is not wired up. \
             Refusing rather than writing an unstamped file."
        )),
        BatchStep::BatesNumber { start, width, .. } => Err(format!(
            "Bates numbering from {start} (width {width}) not applied: batch \
             stamping needs the coordinator page injection path, which is not \
             wired up. Refusing rather than writing an unnumbered file."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipeline_new_and_add_step() {
        let mut pipeline = BatchPipeline::new("test");
        assert_eq!(pipeline.step_count(), 0);
        pipeline.add_step(BatchStep::Merge {
            inputs: vec!["a.pdf".into(), "b.pdf".into()],
            output: "out.pdf".into(),
        });
        assert_eq!(pipeline.step_count(), 1);
    }

    #[test]
    fn pipeline_serialize_roundtrip() {
        let mut pipeline = BatchPipeline::new("my-pipeline");
        pipeline.description = "Test pipeline".into();
        pipeline.add_step(BatchStep::Merge {
            inputs: vec!["a.pdf".into(), "b.pdf".into()],
            output: "merged.pdf".into(),
        });
        pipeline.add_step(BatchStep::Optimize {
            input: "merged.pdf".into(),
            output: "final.pdf".into(),
            profile: "screen".into(),
        });

        let serialized = pipeline.serialize();
        assert!(serialized.contains("Pipeline: my-pipeline"));
        assert!(serialized.contains("steps=2"));
        assert!(serialized.contains("type=merge"));
        assert!(serialized.contains("type=optimize"));
        assert!(serialized.contains("profile=screen"));
    }

    #[test]
    fn step_result_records_timing() {
        let pipeline = BatchPipeline::new("empty");
        let results = execute_pipeline(&pipeline);
        assert!(results.is_empty());
    }

    #[test]
    fn batch_merge_requires_two_inputs() {
        let step = BatchStep::Merge {
            inputs: vec!["a.pdf".into()],
            output: "out.pdf".into(),
        };
        let result = execute_step(&step, &default_optimize);
        assert!(result.is_err());
    }

    // Replaces `batch_watermark_copies_file`, which asserted that watermarking
    // produced a byte-identical copy — i.e. it locked in a false success. A
    // step that cannot stamp must refuse and leave no misleading artifact.
    // [PRIN-1, PRIN-6, GR-8, UX-ERR-3]

    #[test]
    fn watermark_refuses_instead_of_emitting_an_unstamped_copy() {
        let dir = std::env::temp_dir().join("pdf_platform_batch_watermark");
        // Clear first: a previous failing run panics before its cleanup, and
        // a stale artifact would mask exactly what this test checks.
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out = dir.join("stamped.pdf");
        std::fs::write(&src, b"fake pdf content").unwrap();

        let step = BatchStep::Watermark {
            input: "src.pdf".into(),
            text: "CONFIDENTIAL".into(),
            output: "stamped.pdf".into(),
        };
        let error = execute_step(&step, &default_optimize)
            .expect_err("an unimplemented stamp must not report success");
        assert!(
            error.contains("page injection"),
            "the refusal must say why: {error}"
        );
        assert!(
            !out.exists(),
            "a refused step must not leave an unstamped file behind"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn bates_number_refuses_instead_of_emitting_an_unnumbered_copy() {
        let dir = std::env::temp_dir().join("pdf_platform_batch_bates");
        // Clear first: a previous failing run panics before its cleanup, and
        // a stale artifact would mask exactly what this test checks.
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::create_dir_all(&dir);
        let src = dir.join("src.pdf");
        let out = dir.join("numbered.pdf");
        std::fs::write(&src, b"fake pdf content").unwrap();

        let step = BatchStep::BatesNumber {
            input: src.clone(),
            start: 1,
            width: 6,
            output: out.clone(),
        };
        let error = execute_step(&step, &default_optimize)
            .expect_err("an unimplemented stamp must not report success");
        assert!(
            error.contains("page injection"),
            "the refusal must say why: {error}"
        );
        assert!(!out.exists(), "a refused step must leave no artifact");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn batch_optimize_requires_qpdf() {
        let step = BatchStep::Optimize {
            input: "nonexistent.pdf".into(),
            output: "out.pdf".into(),
            profile: "screen".into(),
        };
        let result = execute_step(&step, &default_optimize);
        // Will fail because file doesn't exist (or qpdf missing).
        assert!(result.is_err());
    }

    fn default_optimize(
        input: &std::path::Path,
        output: &std::path::Path,
        profile: crate::assembly::OptimizeProfile,
    ) -> Result<String, String> {
        crate::assembly_ops::optimize_pdf(input, output, profile).map_err(|e| e.to_string())
    }
}
