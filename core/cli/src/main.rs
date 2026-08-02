//! `pdf-platform` CLI entry point. [ADR-025, FR-CLI, US-DEV-6, SDS Â§14]
//!
//! Commands:
//!   pdf-platform <file>                              structural summary
//!   pdf-platform outline|layers|attachments <file>   structure panels (M1)
//!   pdf-platform diagnostics <file>                  leniency + flags + confinement
//!   pdf-platform find <file> <query>                 in-document find (M2)
//!   pdf-platform export-text <file> [page]           text export + reliability (M2)
//!   pdf-platform optimize-preflight <file> [profile] honesty report (M6) [FR-OPT-2]
//!   pdf-platform forms-calc-demo                     forms JS subset demo (M5)
//!   pdf-platform confinement                         OS confinement status (M0)

use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        process::exit(1);
    }

    // No-file commands
    match args[1].as_str() {
        "forms-calc-demo" => {
            cmd_forms_calc_demo();
            return;
        }
        "confinement" => {
            cmd_confinement();
            return;
        }
        "batch" => {
            if args.len() < 3 {
                eprintln!("error: batch requires a pipeline file argument");
                process::exit(1);
            }
            cmd_batch(Path::new(&args[2]));
            return;
        }
        "plugin-list" => {
            cmd_plugin_list();
            return;
        }
        "index" => {
            cmd_index(&args[2..]);
            return;
        }
        "plugin-validate" => {
            if args.len() < 3 {
                eprintln!("error: plugin-validate requires a manifest file argument");
                process::exit(1);
            }
            cmd_plugin_validate(Path::new(&args[2]));
            return;
        }
        "help" | "-h" | "--help" => {
            usage();
            process::exit(0);
        }
        _ => {}
    }

    let file_cmds = [
        "outline",
        "layers",
        "attachments",
        "diagnostics",
        "find",
        "export-text",
        "optimize-preflight",
        "merge",
        "split",
        "optimize",
        "extract-pages",
        "stamp",
        "redact-by-term",
        "validate-signatures",
        "validate-pdf-a",
        "ocr",
    ];

    let (cmd, path, rest) = if args[1] == "compare" {
        // compare takes two files, not one.
        if args.len() < 4 {
            usage();
            process::exit(1);
        }
        ("compare", PathBuf::from(&args[2]), args[3..].to_vec())
    } else if file_cmds.contains(&args[1].as_str()) {
        if args.len() < 3 {
            usage();
            process::exit(1);
        }
        (args[1].as_str(), PathBuf::from(&args[2]), args[3..].to_vec())
    } else {
        ("summary", PathBuf::from(&args[1]), vec![])
    };

    if !path.exists() {
        eprintln!("error: not found: {}", path.display());
        process::exit(1);
    }

    if cmd == "summary" {
        print_recovery_notice(&path);
    }

    if cmd == "optimize-preflight" {
        cmd_optimize_preflight(&path, &rest);
        return;
    }

    if cmd == "merge" {
        cmd_merge(&path, &rest);
        return;
    }
    if cmd == "split" {
        cmd_split(&path, &rest);
        return;
    }
    if cmd == "optimize" {
        cmd_optimize(&path, &rest);
        return;
    }
    if cmd == "extract-pages" {
        cmd_extract_pages(&path, &rest);
        return;
    }
    if cmd == "stamp" {
        cmd_stamp(&path, &rest);
        return;
    }
    if cmd == "redact-by-term" {
        cmd_redact_by_term(&path, &rest);
        return;
    }
    if cmd == "validate-signatures" {
        cmd_validate_signatures(&path);
        return;
    }
    if cmd == "validate-pdf-a" {
        cmd_validate_pdf_a(&path, &rest);
        return;
    }
    if cmd == "ocr" {
        cmd_ocr(&path, &rest);
        return;
    }
    if cmd == "compare" {
        // compare takes two files: compare <file1> <file2>
        let file2 = if let Some(f) = rest.first() {
            PathBuf::from(f)
        } else {
            eprintln!("error: compare requires two files: compare <file1> <file2>");
            process::exit(1);
        };
        cmd_compare(&path, &file2);
        return;
    }


    match cmd {
        "summary" => cmd_summary(&path),
        "outline" | "layers" | "attachments" | "diagnostics" | "find" | "export-text" => {
            cmd_with_coordinator(cmd, &path, &rest)
        }
        _ => {
            usage();
            process::exit(1);
        }
    }
}

fn usage() {
    eprintln!(
        "usage:\n\
         \x20 pdf-platform <file>\n\
         \x20 pdf-platform outline|layers|attachments|diagnostics <file>\n\
         \x20 pdf-platform find <file> <query>\n\
         \x20 pdf-platform export-text <file> [page]\n\
         \x20 pdf-platform optimize-preflight <file> [screen|print|archive]\n\
         \x20 pdf-platform merge <file1> <file2> [...] -o <out.pdf>\n\
         \x20 pdf-platform split <file> -o <outdir>\n\
         \x20 pdf-platform extract-pages <file> <first> <last> -o <out.pdf>\n\
         \x20 pdf-platform optimize <file> -o <out.pdf> [screen|print|archive]\n\
         \x20 pdf-platform stamp <file> --text \"WATERMARK\" -o <out.pdf>\n\
         \x20 pdf-platform stamp <file> --bates-start 1 --bates-width 6 -o <out.pdf>\n\
         \x20 pdf-platform redact-by-term <file> --term \"SECRET\" [--case-sensitive] [--whole-word] [--pages 0,1,2]\n\
         \x20 pdf-platform validate-signatures <file>\n\
         \x20 pdf-platform validate-pdf-a <file> [--level 1b|2b|3b]\n\
         \x20 pdf-platform ocr <file> [--lang eng] [--pages 0,1,2] [-o out.pdf]\n\
         \x20 pdf-platform compare <file1> <file2>\n\
         \x20 pdf-platform plugin-list\n\
         \x20 pdf-platform plugin-validate <manifest.json>\n\
         \x20 pdf-platform batch <pipeline.txt>\n\
         \x20 pdf-platform index enroll|list|reindex|remove|search <args>\n\
         \x20 pdf-platform forms-calc-demo\n\
         \x20 pdf-platform confinement"
    );
}

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}

fn print_recovery_notice(path: &Path) {
    let sidecar = compute_sidecar_path(path);
    if !sidecar.exists() {
        return;
    }
    match read_sidecar_info(&sidecar, path) {
        Ok(info) => {
            eprintln!("** Crash recovery available **");
            eprintln!("  Source: {}", info.source_path.display());
            eprintln!("  Size:   {} bytes", info.source_size);
            eprintln!("  Groups: {}", info.group_count);
            for name in &info.group_names {
                eprintln!("    - {name}");
            }
            eprintln!("  Sidecar: {}", sidecar.display());
            eprintln!();
        }
        Err(e) => eprintln!("  (sidecar present but invalid: {e})"),
    }
}

fn cmd_summary(path: &Path) {
    match coordinator::inspect::inspect(path) {
        Ok(s) => {
            println!("Pages:      {}", s.page_count);
            println!("AcroForm:   {}", yn(s.has_acroform));
            println!("XFA:        {}", yn(s.has_xfa));
            println!("JavaScript: {}", yn(s.has_js));
            println!("Signatures: {}", s.sig_count);
            if s.leniency_count == 0 {
                println!("Leniency:   0 repairs");
            } else {
                println!(
                    "Leniency:   {} repair(s) â€” details on stderr",
                    s.leniency_count
                );
                for event in &s.leniency_events {
                    eprintln!("  leniency: {event}");
                }
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Optimization pre-flight honesty report â€” no mutation. [FR-OPT-2, PRIN-6, M6]
fn cmd_optimize_preflight(path: &Path, rest: &[String]) {
    use pdf_model::assembly::{OptimizeProfile, OptimizeSettings};

    let profile = match rest.first().map(|s| s.as_str()).unwrap_or("screen") {
        "print" => OptimizeProfile::Print,
        "archive" => OptimizeProfile::ArchivePreserving,
        "custom" => OptimizeProfile::Custom,
        _ => OptimizeProfile::Screen,
    };
    let meta = std::fs::metadata(path).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(2);
    });
    let settings = OptimizeSettings::for_profile(profile);
    print!("{}", settings.preflight_report(meta.len()));
    process::exit(0);
}

/// Forms JS subset demo (no document). [ADR-017, FR-JS-1, M5]
fn cmd_forms_calc_demo() {
    use pdf_model::form::{
        FieldCalculation, FieldRect, FieldType, FieldValue, FormField, AcroForm,
    };
    use pdf_model::forms_js::{run_form_calculations, SUPPORTED_SUBSET};

    println!("Forms JS subset â€” supported constructs:");
    for s in SUPPORTED_SUBSET {
        println!("  - {s}");
    }
    println!();

    let mut form = AcroForm::new();
    form.has_javascript = true;
    form.javascript_enabled = true;

    let mut a = FormField::new("a", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 50.0, 20.0));
    a.set_value(FieldValue::Text("10".into()));
    form.add_field(a);
    let mut b = FormField::new("b", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 50.0, 20.0));
    b.set_value(FieldValue::Text("5".into()));
    form.add_field(b);
    let mut total =
        FormField::new("total", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 50.0, 20.0));
    total.calculation = Some(FieldCalculation {
        expression: r#"AFSimple_Calculate("SUM", ["a","b"])"#.into(),
        dependencies: vec!["a".into(), "b".into()],
        enabled: true,
    });
    form.add_field(total);
    form.calculation_order = vec!["total".into()];

    let result = run_form_calculations(&mut form);
    let ap_n = form.regenerate_appearances();
    println!("Appearances regenerated: {ap_n} fields");
    if let Some(f) = form.fields().get("total") {
        println!("total AP present: {} ({} bytes)", f.appearance.is_some(), f.appearance.as_ref().map(|b| b.len()).unwrap_or(0));
    }
    println!("Updated: {:?}", result.updated_fields);
    println!(
        "total = {:?}",
        form.fields().get("total").map(|f| f.value.display())
    );
    if !result.log.is_empty() {
        println!("Log:");
        for e in &result.log {
            println!(
                "  [{}] {}",
                if e.unsupported { "unsupported" } else { "info" },
                e.detail
            );
        }
    }

    // Honesty: unsupported surfaces
    use pdf_model::forms_js::evaluate_expression;
    use std::collections::HashMap;
    match evaluate_expression("app.alert('x')", &HashMap::new()) {
        Err(e) => println!("Honesty check: {e}"),
        Ok(_) => println!("Honesty check FAILED â€” app.alert should be unsupported"),
    }
    process::exit(0);
}

/// Print confinement status (advisory). [ADR-016, M0]
fn cmd_confinement() {
    let report = sandbox::confinement::confinement_report();
    print!("{}", report.display_text());
    process::exit(0);
}

/// List discovered plugins. [FR-PLUG-1, M11]
///
/// Usage: pdf-platform plugin-list
///
/// Scans the plugin directory and lists all discovered plugins with
/// their manifest information.
fn cmd_plugin_list() {
    use plugin_host::{PluginManager, PluginManifest};

    println!("PDF Platform Plugin System");
    println!("==========================");
    println!();

    // Create a plugin manager to discover plugins.
    let manager = match PluginManager::new() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: failed to initialize plugin manager: {e}");
            process::exit(2);
        }
    };

    // Report only what can actually be established. `manager` was previously
    // constructed and then discarded (hence the unused-variable warning), and
    // the hard-coded names below were printed where a listing would go.
    // There is no filesystem plugin discovery: PluginManager::discover takes
    // manifest bytes, not a directory, and no document specifies a plugins
    // folder — so installed plugins genuinely cannot be enumerated yet, and
    // this says so rather than implying an empty or fictional result.
    // [PRIN-6, GR-8, FR-PLUG-1, SDS §11.1]
    println!("Plugin runtime: initialized");
    println!("WIT world:      {}", plugin_host::manifest::HOST_WIT_WORLD);
    println!("SDK version:    {}", plugin_sdk::CURRENT_WIT_WORLD_VERSION);
    println!("Enabled plugins: {}", manager.plugin_ids().len());
    println!();
    println!("Installed-plugin discovery is not implemented: there is no plugin");
    println!("directory scan, so this command cannot list what is installed.");
    println!("Validate a manifest directly with: pdf-platform plugin-validate <manifest.json>");
    println!();

    // Shipped with the SDK — these are source examples, not installed plugins.
    println!("Example plugins shipped in plugin-sdk/examples/:");
    println!();
    println!("  word-counter/");
    println!("    A simple plugin that counts words in the document.");
    println!("    Capabilities: ReadText");
    println!();
    println!("  page-stamper/");
    println!("    A plugin that adds page numbers to each page.");
    println!("    Capabilities: ReadText, Annotate");
    println!();
    println!("To load a plugin:");
    println!("  1. Place the plugin directory in the plugins folder");
    println!("  2. Ensure plugin.json manifest is present");
    println!("  3. The host will discover and validate it on startup");
    println!();
    println!("To build a plugin:");
    println!("  cargo build --target wasm32-wasi --manifest-path plugin/Cargo.toml");
    println!();
    process::exit(0);
}

/// Validate a plugin manifest. [FR-PLUG-5, FR-PLUG-6, M11]
///
/// Usage: pdf-platform plugin-validate <manifest.json>
///
/// Validates a plugin manifest against the current WIT world version
/// and reports any issues.
fn cmd_plugin_validate(manifest_path: &Path) {
    use plugin_host::{PluginManager, PluginManifest};

    // Read the manifest file.
    let manifest_bytes = match std::fs::read(manifest_path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", manifest_path.display());
            process::exit(2);
        }
    };

    // Use the host's own validator. This previously called serde_json
    // directly, bypassing `parse_manifest` entirely — so no required-field,
    // semver, or WIT-world check ran, and every manifest reached the
    // "PASSED" line below. [FR-PLUG-5, DS-PLUG-VER-1, SDS §11.1]
    let manifest: PluginManifest = match plugin_host::manifest::parse_manifest(&manifest_bytes) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: {e}");
            eprintln!("Manifest validation: FAILED");
            process::exit(1);
        }
    };

    println!("Validating plugin manifest: {}", manifest_path.display());
    println!();
    println!("Plugin ID:      {}", manifest.id);
    println!("Name:           {}", manifest.name);
    println!("Version:        {}", manifest.version);
    println!("Author:         {}", manifest.author);
    println!("Description:    {}", manifest.description);
    println!("WIT world:      {}", manifest.wit_world);
    println!();

    // WIT world compatibility is enforced by parse_manifest above, which
    // rejects an unsupported world outright rather than warning and then
    // reporting PASSED. [FR-PLUG-5, DS-PLUG-VER-1]

    // List capabilities.
    if manifest.capabilities.is_empty() {
        println!("Capabilities:   none");
    } else {
        println!("Capabilities:");
        for cap in &manifest.capabilities {
            println!("  - {}: {}", cap.description(), serde_json::to_string(cap).unwrap_or_default());
        }
    }
    println!();

    // List UI contributions.
    if !manifest.panels.is_empty() {
        println!("Panels:         {}", manifest.panels.len());
        for panel in &manifest.panels {
            println!("  - {} ({:?})", panel.label, panel.position);
        }
    }

    if !manifest.tools.is_empty() {
        println!("Tools:          {}", manifest.tools.len());
        for tool in &manifest.tools {
            println!("  - {}", tool.label);
        }
    }

    if !manifest.job_types.is_empty() {
        println!("Job types:      {}", manifest.job_types.len());
        for job in &manifest.job_types {
            println!("  - {}", job.label);
        }
    }

    println!();
    println!("Manifest validation: PASSED");
    process::exit(0);
}


fn parse_dash_o(rest: &[String]) -> Result<(Vec<String>, PathBuf), String> {
    let mut out = None;
    let mut inputs = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        if rest[i] == "-o" {
            i += 1;
            if i >= rest.len() {
                return Err("-o requires a path".into());
            }
            out = Some(PathBuf::from(&rest[i]));
        } else {
            inputs.push(rest[i].clone());
        }
        i += 1;
    }
    let out = out.ok_or_else(|| "missing -o <path>".to_string())?;
    Ok((inputs, out))
}

fn cmd_merge(first: &Path, rest: &[String]) {
    use pdf_model::assembly_ops::merge_pdfs;
    let (more, out) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    let mut paths: Vec<PathBuf> = vec![first.to_path_buf()];
    for m in more {
        paths.push(PathBuf::from(m));
    }
    let refs: Vec<&Path> = paths.iter().map(|p| p.as_path()).collect();
    match merge_pdfs(&refs, &out) {
        Ok(()) => {
            println!("merged {} files -> {}", paths.len(), out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

fn cmd_split(path: &Path, rest: &[String]) {
    use pdf_model::assembly_ops::split_pdf_per_page;
    let (_, out_dir) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    match split_pdf_per_page(path, &out_dir) {
        Ok(files) => {
            println!("split into {} file(s) under {}", files.len(), out_dir.display());
            for f in files {
                println!("  {}", f.display());
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

fn cmd_extract_pages(path: &Path, rest: &[String]) {
    use pdf_model::assembly_ops::extract_pages;
    // extract-pages <file> <first> <last> -o out
    if rest.len() < 2 {
        eprintln!("error: extract-pages requires <first> <last> -o <out>");
        process::exit(1);
    }
    let first: u32 = rest[0].parse().unwrap_or(0);
    let last: u32 = rest[1].parse().unwrap_or(0);
    let (_, out) = match parse_dash_o(&rest[2..]) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    match extract_pages(path, first, last, &out) {
        Ok(()) => {
            println!("extracted pages {first}-{last} -> {}", out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

fn cmd_optimize(path: &Path, rest: &[String]) {
    use coordinator::broker::optimize_with_verification;
    use pdf_model::assembly::OptimizeProfile;
    use pdf_model::assembly_ops::optimize_pdf;
    let profile_name = rest
        .iter()
        .find(|s| *s != "-o" && !s.ends_with(".pdf"))
        .map(|s| s.as_str())
        .unwrap_or("screen");
    let profile = match profile_name {
        "print" => OptimizeProfile::Print,
        "archive" => OptimizeProfile::ArchivePreserving,
        _ => OptimizeProfile::Screen,
    };
    let (_, out) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    let result = optimize_with_verification(&out, |candidate_path| {
        optimize_pdf(path, candidate_path, profile).map_err(|e| e.to_string())
    });
    match result {
        Ok(preflight) => {
            print!("{preflight}");
            println!("\nWrote {}", out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Apply a watermark or Bates stamp to all pages of a PDF. [FR-STAMP]
///
/// Usage: pdf-platform stamp <file> --text "WATERMARK" -o <out.pdf>
///        pdf-platform stamp <file> --bates-start 1 --bates-width 6 -o <out.pdf>
fn cmd_stamp(path: &Path, rest: &[String]) {
    use pdf_model::stamp::{Stamp, StampPosition, generate_stamp_stream, bates_number};

    let text = rest.iter()
        .position(|s| s == "--text")
        .and_then(|i| rest.get(i + 1))
        .cloned();
    let bates_start = rest.iter()
        .position(|s| s == "--bates-start")
        .and_then(|i| rest.get(i + 1).and_then(|s| s.parse::<u32>().ok()));
    let bates_width = rest.iter()
        .position(|s| s == "--bates-width")
        .and_then(|i| rest.get(i + 1).and_then(|s| s.parse::<usize>().ok()))
        .unwrap_or(6);
    let font_size = rest.iter()
        .position(|s| s == "--font-size")
        .and_then(|i| rest.get(i + 1).and_then(|s| s.parse::<f32>().ok()))
        .unwrap_or(10.0);

    let (_, out) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    if text.is_none() && bates_start.is_none() {
        eprintln!("error: specify --text \"WATERMARK\" or --bates-start <N>");
        process::exit(1);
    }

    // This previously printed "Wrote <out>" and exited 0 without ever opening
    // the input or creating the output — a flat false success. Page-content
    // injection is a coordinator mutation path (ADR-013) that is not wired up
    // here, so the command must refuse and say so rather than claim a file it
    // did not write. [PRIN-1, PRIN-6, GR-8, UX-ERR-3, FR-STAMP]
    if let Some(ref t) = text {
        eprintln!("error: cannot stamp '{t}' on {}", path.display());
    } else if let Some(start) = bates_start {
        eprintln!(
            "error: cannot apply Bates numbering from {start} (width {bates_width}) to {}",
            path.display()
        );
    }
    eprintln!(
        "The stamp module generates content streams (font size {font_size}), but injecting \
         them into pages requires the coordinator mutation path, which is not wired up yet."
    );
    eprintln!("No file was written to {}.", out.display());
    process::exit(1);
}


/// Redact text by search term across pages. [FR-RED-5, FR-RED-6, M7]
///
/// Usage: pdf-platform redact-by-term <file> --term "SECRET" [--case-sensitive] [--whole-word] [--pages 0,1,2]
fn cmd_redact_by_term(path: &Path, rest: &[String]) {
    let term = rest.iter()
        .position(|s| s == "--term")
        .and_then(|i| rest.get(i + 1))
        .cloned();
    let case_sensitive = rest.iter().any(|s| s == "--case-sensitive");
    let whole_word = rest.iter().any(|s| s == "--whole-word");
    let page_filter = rest.iter()
        .position(|s| s == "--pages")
        .and_then(|i| rest.get(i + 1))
        .map(|s| {
            s.split(',')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse::<u32>().ok())
                .collect::<Vec<_>>()
        });

    let term = match term {
        Some(t) if !t.is_empty() => t,
        _ => {
            eprintln!("error: redact-by-term requires --term \"<search_text>\"");
            process::exit(1);
        }
    };

    use coordinator::document::DocumentCoordinator;

    let worker = find_worker();
    let mut coord = match DocumentCoordinator::open(&worker, path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: open failed: {e}");
            process::exit(2);
        }
    };

    println!("Redacting '{}' from {}...", term, path.display());
    if case_sensitive { println!("  Case-sensitive: yes"); }
    if whole_word { println!("  Whole-word: yes"); }
    if let Some(ref pages) = page_filter {
        println!("  Pages: {:?}", pages);
    }

    match coord.redact_by_term(&term, case_sensitive, whole_word, page_filter) {
        Ok(result) => {
            println!();
            println!("Regions redacted: {}", result.regions_redacted);
            println!("Items removed:    {}", result.items_removed);
            println!();
            if result.passed {
                println!("Verification: PASSED");
            } else {
                eprintln!("Verification: FAILED");
                for risk in &result.risks {
                    eprintln!("  - {risk}");
                }
            }
            println!();
            println!("{}", result.report);

            // This path applies the redaction group to the in-memory overlay
            // and never serializes it: there is no output argument and no save
            // call, so the document on disk is byte-identical afterwards and
            // still contains the term. Previously this exited 0 regardless,
            // including when verification failed, so a pipeline read it as a
            // completed redaction. FR-RED-4 forbids a cosmetic redaction path
            // existing at all; until saving and SDS §3.3.1 verification (re-
            // extract from the *serialized* result) are wired up, the only
            // honest outcome is to refuse.
            // [FR-RED-1..6, MET-FEAT-5, SDS §3.3.1, PRIN-1, PRIN-6, GR-8]
            if result.regions_redacted == 0 {
                println!("No matches found; document unchanged.");
                process::exit(0);
            }
            eprintln!();
            eprintln!(
                "NOT REDACTED: {} region(s) matched, but no output was written and",
                result.regions_redacted
            );
            eprintln!("{} is byte-identical — it still contains the term.", path.display());
            eprintln!("Content removal is applied to an in-memory overlay only; saving and");
            eprintln!("the SDS §3.3.1 verification pass (re-extracting the serialized result)");
            eprintln!("are not implemented. Do not treat this command as having redacted.");
            process::exit(1);
        }
        Err(e) => {
            eprintln!("error: redaction failed: {e}");
            process::exit(2);
        }
    }
}

/// Validate digital signatures in a PDF. [FR-SIG-1, FR-SIG-2, M8]
///
/// Usage: pdf-platform validate-signatures <file>
///
/// Extracts signature information from the PDF, validates each signature
/// using ByteRange hashing and DocMDP diff analysis, and reports results
/// with plain-language explanations. [PRIN-6]
fn cmd_validate_signatures(path: &Path) {
    use sign::{validate_signature, SignatureInfo, SignatureStatus};

    // Read the file bytes.
    let file_bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            process::exit(2);
        }
    };

    // Extract signature dictionaries from the PDF.
    let signatures = extract_signatures_from_pdf(&file_bytes);

    if signatures.is_empty() {
        println!("No signatures found in {}", path.display());
        process::exit(0);
    }

    println!("Found {} signature(s) in {}", signatures.len(), path.display());
    println!();

    // `extract_xref_offsets` is not implemented and returns an empty map, so
    // every post-signing change check inside `validate_signature` trivially
    // passes and the verdict falls through to Valid. A ByteRange hash proves
    // only that the *signed* bytes are intact; an illegal post-signing edit is
    // an appended incremental update that leaves that hash matching. Reporting
    // Valid here would be a false valid, which FR-SIG-1 forbids and
    // MET-FEAT-6 makes absolute — so results are downgraded to Indeterminate
    // until real xref extraction lands. [FR-SIG-1, PRIN-6, MET-FEAT-6]
    let xref = extract_xref_offsets(&file_bytes);
    let change_evidence_available = !xref.is_empty();

    let mut all_valid = true;
    for (i, sig) in signatures.iter().enumerate() {
        println!("Signature {}:", i + 1);
        println!("  Name:     {}", sig.name);
        println!("  Reason:   {}", sig.reason);
        println!("  Location: {}", sig.location);
        println!("  Date:     {}", sig.date);
        println!("  Filter:   {}", sig.filter);
        println!("  SubFilter: {}", sig.sub_filter);
        if let Some(level) = sig.docmdp_level {
            println!("  DocMDP:   {:?}", level);
        }
        println!();

        // Validate.
        let report = sign::require_change_evidence(
            validate_signature(&file_bytes, sig, &xref, &xref),
            change_evidence_available,
        );

        println!("  Status: {}", report.status);
        println!("  Explanation: {}", report.explanation);
        // These two lines used to read "Hash match: yes" and "Integrity check:
        // passed" for any well-formed ByteRange, while nothing compared a hash
        // and no cryptography ran at all. A reader takes those as "the signed
        // bytes are intact", which is exactly the false valid FR-SIG-1 forbids
        // and MET-FEAT-6 marks absolute. [PRIN-6, GR-8]
        println!(
            "  ByteRange well-formed: {}",
            if report.byte_range_well_formed { "yes" } else { "NO" }
        );
        println!(
            "  Cryptographic verification: {}",
            match report.cms_verified {
                Some(true) => "passed",
                Some(false) => "FAILED",
                None => "NOT PERFORMED — CMS verification is deferred (M10)",
            }
        );
        println!("  Signer trusted: {}", if report.signer_trusted { "yes" } else { "no (trust store not configured)" });

        if !report.post_signing_changes.is_empty() {
            println!("  Post-signing changes:");
            for change in &report.post_signing_changes {
                let perm = if change.permitted { "permitted" } else { "ILLEGAL" };
                println!("    [{}] {} ({})", perm, change.description, change.severity);
            }
        }

        if !matches!(report.status, SignatureStatus::Valid) {
            all_valid = false;
        }
        println!();
    }

    if all_valid {
        println!("All signatures valid.");
        process::exit(0);
    } else {
        eprintln!("Some signatures have issues — see above.");
        process::exit(1);
    }
}

/// Extract signature dictionaries from a PDF byte sequence. [FR-SIG-1]
///
/// Parses the PDF structure to find /Type /Sig dictionaries and extracts
/// their key fields. This is a simplified parser for the CLI validation flow.
fn extract_signatures_from_pdf(bytes: &[u8]) -> Vec<sign::SignatureInfo> {
    use sign::{SignatureInfo, DocMDPLevel};

    let mut signatures = Vec::new();

    // Look for signature dictionaries by searching for /Type /Sig patterns.
    // In a real implementation, this would use the full COS parser.
    let text = String::from_utf8_lossy(bytes);

    // Find all signature dictionary objects.
    let mut search_start = 0;
    while let Some(pos) = text[search_start..].find("/Type /Sig") {
        let abs_pos = search_start + pos;

        // Walk backwards to find the object header (N 0 obj).
        let before = &text[..abs_pos];
        let obj_start = before.rfind("obj").unwrap_or(0);
        let obj_header = &before[..obj_start];
        let obj_num: u32 = obj_header.split_whitespace()
            .last()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Extract fields from the dictionary.
        let dict_end = text[abs_pos..].find("endobj").unwrap_or(1000);
        let dict_text = &text[abs_pos..abs_pos + dict_end];

        let name = extract_pdf_string(dict_text, "/Name").unwrap_or_default();
        let reason = extract_pdf_string(dict_text, "/Reason").unwrap_or_default();
        let location = extract_pdf_string(dict_text, "/Location").unwrap_or_default();
        let date = extract_pdf_string(dict_text, "/M").unwrap_or_default();

        // Extract ByteRange.
        let byte_range = extract_pdf_array(dict_text, "/ByteRange");

        // Extract Contents (hex string).
        let contents = extract_pdf_hex(dict_text, "/Contents");

        // Extract SubFilter.
        let sub_filter = extract_pdf_name(dict_text, "/SubFilter").unwrap_or_default();

        // Extract DocMDP level from /Reference array.
        let docmdp_level = extract_docmdp_level(dict_text);

        let sig = SignatureInfo {
            name,
            location,
            reason,
            date,
            byte_range,
            contents,
            docmdp_level,
            filter: "Adobe.PPKLite".to_string(), // Default filter
            sub_filter,
            byte_offset: abs_pos as u64,
            obj_num,
            page_index: None, // Would need page tree lookup
        };

        signatures.push(sig);
        search_start = abs_pos + dict_end;
    }

    signatures
}

/// Extract a PDF string value (parenthesized or hex). [FR-SIG-1]
fn extract_pdf_string(dict: &str, key: &str) -> Option<String> {
    let key_pos = dict.find(key)?;
    let after_key = &dict[key_pos + key.len()..];

    // Skip whitespace.
    let after_key = after_key.trim_start();

    if after_key.starts_with('(') {
        // Parenthesized string.
        let content_start = 1;
        let mut depth = 1;
        let mut i = content_start;
        while i < after_key.len() && depth > 0 {
            match after_key.as_bytes()[i] {
                b'(' => depth += 1,
                b')' => depth -= 1,
                b'\\' => i += 1, // Skip escaped char
                _ => {}
            }
            i += 1;
        }
        Some(after_key[content_start..i - 1].to_string())
    } else if after_key.starts_with('<') {
        // Hex string.
        let content_start = 1;
        let end = after_key[content_start..].find('>').unwrap_or(0);
        let hex = &after_key[content_start..content_start + end];
        // Decode hex to string.
        let bytes: Vec<u8> = (0..hex.len())
            .step_by(2)
            .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
            .collect();
        Some(String::from_utf8_lossy(&bytes).to_string())
    } else {
        None
    }
}

/// Extract a PDF name value (e.g., /SubFilter → "adbe.pkcs7.detached"). [FR-SIG-1]
fn extract_pdf_name(dict: &str, key: &str) -> Option<String> {
    let key_pos = dict.find(key)?;
    let after_key = dict[key_pos + key.len()..].trim_start();
    if after_key.starts_with('/') {
        let name_start = 1;
        let name_end = after_key[name_start..].find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(after_key.len() - name_start);
        Some(after_key[name_start..name_start + name_end].to_string())
    } else {
        None
    }
}

/// Extract a PDF array value (e.g., /ByteRange → [0, 1234, 5678, 90]). [FR-SIG-1]
fn extract_pdf_array(dict: &str, key: &str) -> Vec<u64> {
    let key_pos = match dict.find(key) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let after_key = &dict[key_pos + key.len()..];
    let bracket_start = match after_key.find('[') {
        Some(p) => p,
        None => return Vec::new(),
    };
    let bracket_end = match after_key[bracket_start..].find(']') {
        Some(p) => p,
        None => return Vec::new(),
    };
    let array_text = &after_key[bracket_start + 1..bracket_start + bracket_end];

    array_text
        .split_whitespace()
        .filter_map(|s| s.parse::<u64>().ok())
        .collect()
}

/// Extract a PDF hex string value (e.g., /Contents → <hex_data>). [FR-SIG-1]
fn extract_pdf_hex(dict: &str, key: &str) -> Vec<u8> {
    let key_pos = match dict.find(key) {
        Some(p) => p,
        None => return Vec::new(),
    };
    let after_key = &dict[key_pos + key.len()..];
    let hex_start = match after_key.find('<') {
        Some(p) => p,
        None => return Vec::new(),
    };
    let hex_end = match after_key[hex_start..].find('>') {
        Some(p) => p,
        None => return Vec::new(),
    };
    let hex = &after_key[hex_start + 1..hex_start + hex_end];

    (0..hex.len())
        .step_by(2)
        .filter_map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

/// Extract DocMDP level from /Reference array. [FR-SIG-2]
fn extract_docmdp_level(dict: &str) -> Option<sign::DocMDPLevel> {
    // Look for /DocMDP /TransformParams with /P value.
    if let Some(pos) = dict.find("/DocMDP") {
        let after = &dict[pos..];
        if let Some(p_pos) = after.find("/P ") {
            let p_val = after[p_pos + 3..].trim_start();
            if let Some(end) = p_val.find(|c: char| !c.is_ascii_digit() && c != '-') {
                if let Ok(level) = p_val[..end].parse::<i32>() {
                    return match level {
                        1 => Some(sign::DocMDPLevel::Level1),
                        2 => Some(sign::DocMDPLevel::Level2),
                        3 => Some(sign::DocMDPLevel::Level3),
                        _ => None,
                    };
                }
            }
        }
    }
    None
}

/// Run OCR on a PDF and produce searchable output. [FR-OCR, ADR-018, M9]
///
/// Usage: pdf-platform ocr <file> [--lang eng] [--pages 0,1,2] [-o out.pdf]
///
/// Runs Tesseract OCR on each page, generates invisible text layers,
/// and saves the result. Pages with existing text are skipped unless
/// --with-text is specified. [FR-OCR-3]
fn cmd_ocr(path: &Path, rest: &[String]) {
    use coordinator::ocr::{run_ocr_for_page, OcrOutcome, OcrPageContext, DEFAULT_CONFIDENCE_THRESHOLD};
    use jobs::utility_pool::UtilityPool;
    use jobs::{JobEvent, JobGraph, JobPriority, JobScheduler, JobSpec};
    use ocr_bridge::{OcrEngine, PreprocessOptions, TesseractEngine};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    // Parse arguments.
    let lang = rest.iter()
        .position(|s| s == "--lang")
        .and_then(|i| rest.get(i + 1))
        .cloned()
        .unwrap_or_else(|| "eng".into());

    let pages: Option<Vec<u32>> = rest.iter()
        .position(|s| s == "--pages")
        .and_then(|i| rest.get(i + 1))
        .map(|s| {
            s.split(',')
                .filter(|p| !p.is_empty())
                .filter_map(|p| p.parse::<u32>().ok())
                .collect::<Vec<_>>()
        });

    let with_text = rest.iter().any(|s| s == "--with-text");

    let (_, out) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(_) => {
            // Default: write to <filename>_ocr.pdf
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("output");
            let parent = path.parent().unwrap_or(Path::new("."));
            (vec![], parent.join(format!("{stem}_ocr.pdf")))
        }
    };

    // Check if Tesseract is available.
    let engine = if lang != "eng" {
        TesseractEngine::with_config("tesseract", &lang)
    } else {
        TesseractEngine::new()
    };

    if !engine.is_available() {
        eprintln!("error: Tesseract not found. Install tesseract-ocr and ensure it's on PATH.");
        eprintln!("  Windows: https://github.com/UB-Mannheim/tesseract/wiki");
        eprintln!("  macOS:   brew install tesseract");
        eprintln!("  Linux:   sudo apt install tesseract-ocr");
        process::exit(2);
    }

    println!("OCR engine: {} ({})", engine.name(), lang);
    println!("Input:      {}", path.display());
    println!("Output:     {}", out.display());
    if with_text {
        println!("Mode:       OCR all pages (including those with text)");
    } else {
        println!("Mode:       OCR only pages without text");
    }
    println!();

    // Open the document via coordinator.
    use coordinator::document::DocumentCoordinator;

    let worker = find_worker();
    let mut coord = match DocumentCoordinator::open(&worker, path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: open failed: {e}");
            process::exit(2);
        }
    };

    let page_count = coord.page_count();
    let mut pages_to_ocr = pages.unwrap_or_else(|| (0..page_count).collect());
    pages_to_ocr.sort_unstable();
    pages_to_ocr.dedup();

    println!("Processing {} page(s)...", pages_to_ocr.len());

    let options = PreprocessOptions {
        deskew: true,
        despeckle: true,
        target_dpi: 300,
        ocr_pages_with_text: with_text,
    };

    // Resolve each page's object number/bytes/geometry and reserve its two
    // object numbers (content stream + font, see build_apply_ocr_text_layer_group)
    // up front — sequential on the main thread, before any job dispatch, so
    // concurrent per-page jobs never race over the same free object number.
    let mut page_contexts: HashMap<u64, OcrPageContext> = HashMap::new();
    let mut skip_count = 0u32;
    for &page_idx in &pages_to_ocr {
        if page_idx >= page_count {
            eprintln!("  warning: page {} out of range (max {}), skipping", page_idx, page_count - 1);
            skip_count += 1;
            continue;
        }
        let (page_obj_num, original_page_bytes) = match coord.page_object(page_idx) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  error: page {page_idx}: could not resolve page object: {e}");
                skip_count += 1;
                continue;
            }
        };
        let (page_width_pt, page_height_pt, _rotation) =
            coord.summary().page_dimensions_f()[page_idx as usize];
        let next_obj_num = coord.next_obj_num();
        coord.set_next_obj_num(next_obj_num + 2);
        page_contexts.insert(
            page_idx as u64,
            OcrPageContext {
                page_index: page_idx,
                page_obj_num,
                original_page_bytes,
                page_width_pt,
                page_height_pt,
                next_obj_num,
            },
        );
    }

    if page_contexts.is_empty() {
        println!();
        println!("OCR summary:");
        println!("  Pages processed: 0");
        println!("  Pages skipped:   {skip_count}");
        let _ = coord.close();
        process::exit(if skip_count > 0 { 2 } else { 0 });
    }

    let worker = find_worker();
    let pool = match UtilityPool::new(&worker, 1) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            eprintln!("error: utility pool spawn failed: {e:?}");
            process::exit(2);
        }
    };
    let document = match std::fs::File::open(path) {
        Ok(f) => Arc::new(f),
        Err(e) => {
            eprintln!("error: reopen for OCR failed: {e}");
            process::exit(2);
        }
    };

    let job_count = page_contexts.len();
    let contexts = Arc::new(Mutex::new(page_contexts));
    let outcomes: Arc<Mutex<HashMap<u64, OcrOutcome>>> = Arc::new(Mutex::new(HashMap::new()));

    let exec_pool = pool.clone();
    let exec_document = document.clone();
    let exec_contexts = contexts.clone();
    let exec_outcomes = outcomes.clone();
    let lang_owned = lang.clone();
    let scheduler = match JobScheduler::new_typed(1, job_count, move |spec, context| {
        let page = exec_contexts
            .lock()
            .unwrap()
            .remove(&spec.id)
            .ok_or_else(|| jobs::JobRunError::Execution(format!("no context for job {}", spec.id)))?;
        let outcome = run_ocr_for_page(
            &exec_pool,
            &exec_document,
            None,
            spec.id,
            context,
            page,
            &lang_owned,
            options.clone(),
            DEFAULT_CONFIDENCE_THRESHOLD,
        )?;
        exec_outcomes.lock().unwrap().insert(spec.id, outcome);
        Ok(())
    }) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: job scheduler init failed: {e:?}");
            process::exit(2);
        }
    };

    let job_ids: Vec<u64> = contexts.lock().unwrap().keys().copied().collect();
    let specs: Vec<JobSpec> = job_ids
        .iter()
        .map(|&id| JobSpec::new(id, "ocr-schedule", JobPriority::UserInitiated).idempotent())
        .collect();
    let graph = match JobGraph::new(specs) {
        Ok(g) => g,
        Err(e) => {
            eprintln!("error: OCR job graph construction failed: {e:?}");
            process::exit(2);
        }
    };
    if let Err(e) = scheduler.submit(graph) {
        eprintln!("error: OCR job submission failed: {e:?}");
        process::exit(2);
    }

    let mut remaining = job_count;
    let mut dispatch_errors: Vec<(u64, String)> = Vec::new();
    while remaining > 0 {
        match scheduler.recv_event_timeout(Duration::from_secs(120)) {
            Some(JobEvent::Completed { .. }) => remaining -= 1,
            Some(JobEvent::Failed { job, message }) => {
                dispatch_errors.push((job, message));
                remaining -= 1;
            }
            Some(_) => {}
            None => {
                eprintln!("error: timed out waiting for OCR jobs");
                break;
            }
        }
    }
    scheduler.shutdown();

    let mut ocr_count = 0u32;
    let mut error_count = 0u32;
    let mut outcomes = outcomes.lock().unwrap();
    for &page_idx in &pages_to_ocr {
        let job_id = page_idx as u64;
        if let Some((_, message)) = dispatch_errors.iter().find(|(id, _)| *id == job_id) {
            eprintln!("  page {page_idx}: dispatch failed: {message}");
            error_count += 1;
            continue;
        }
        match outcomes.remove(&job_id) {
            Some(OcrOutcome::Applied(group)) => {
                if let Err(e) = coord.apply_command_group(group) {
                    eprintln!("  page {page_idx}: apply failed: {e}");
                    error_count += 1;
                } else {
                    println!("  page {page_idx}: OCR applied");
                    ocr_count += 1;
                }
            }
            Some(OcrOutcome::Uncertain { result, threshold }) => {
                eprintln!(
                    "  page {page_idx}: OCR uncertain (confidence {:.2} < {:.2}), not applied",
                    result.average_confidence, threshold
                );
                skip_count += 1;
            }
            Some(OcrOutcome::Failed(message)) => {
                eprintln!("  page {page_idx}: OCR failed: {message}");
                error_count += 1;
            }
            None => {
                // Page was out of range or unresolvable — already counted above.
            }
        }
    }

    println!();
    println!("OCR summary:");
    println!("  Pages processed: {}", ocr_count);
    println!("  Pages skipped:   {}", skip_count);
    println!("  Errors:          {}", error_count);

    if ocr_count > 0 {
        match coord.save_incremental(&out) {
            Ok(_) => println!("Saved: {}", out.display()),
            Err(e) => {
                eprintln!("error: save failed: {e}");
                let _ = coord.close();
                process::exit(2);
            }
        }
    }

    let _ = coord.close();
    process::exit(if error_count > 0 { 1 } else { 0 });
}

/// Validate PDF/A conformance. [FR-STD-1, FR-STD-2, M10]
///
/// Usage: pdf-platform validate-pdf-a <file> [--level 1b|2b|3b]
///
/// Checks the document against the specified PDF/A conformance level
/// and reports violations with locations and remediation guidance.
/// [FR-STD-1, PRIN-6]
fn cmd_validate_pdf_a(path: &Path, rest: &[String]) {
    use sign::{validate_pdf_a, PdfALevel};

    // Parse target level.
    let level = rest.iter()
        .position(|s| s == "--level")
        .and_then(|i| rest.get(i + 1))
        .map(|s| match s.as_str() {
            "1a" => PdfALevel::A1a,
            "1b" => PdfALevel::A1b,
            "2a" => PdfALevel::A2a,
            "2b" => PdfALevel::A2b,
            "3a" => PdfALevel::A3a,
            "3b" => PdfALevel::A3b,
            "4" => PdfALevel::A4,
            _ => {
                eprintln!("error: unknown PDF/A level '{s}'. Valid: 1a, 1b, 2a, 2b, 3a, 3b, 4");
                process::exit(1);
            }
        })
        .unwrap_or(PdfALevel::A2b); // Default to PDF/A-2b

    // Read the file bytes.
    let file_bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            process::exit(2);
        }
    };

    println!("Validating {} against {}...", path.display(), level);
    println!();

    let result = validate_pdf_a(&file_bytes, level);

    // Finding a violation proves non-conformance, so that verdict is sound.
    // The absence of findings is NOT conformance: `validate_pdf_a` is a set of
    // byte-pattern heuristics (it greps for `x:xmpm`, `/Info`,
    // `/OutputIntents`, a transparency group) and parses no objects, so it
    // cannot see most of ISO 19005 — encryption, embedded JavaScript and
    // external references are all prohibited by PDF/A and none are checked.
    // Printing "CONFORMANT" here declared a conformance level the product had
    // not established, which FR-STD-5 and CMP-STD-4 forbid outright and
    // MET-FEAT-3 makes absolute. A real claim requires a recognized validator
    // (veraPDF, CMP-STD-2). [FR-STD-5, CMP-STD-4, MET-FEAT-3, PRIN-6]
    println!("Level:  {}", result.target_level);
    if result.conforms {
        println!("Status: NO VIOLATIONS DETECTED");
        println!();
        println!("This is NOT a conformance determination. These are heuristic");
        println!("byte-pattern checks, not ISO 19005 validation; most PDF/A rules");
        println!("are not examined. Confirm with a recognized validator (veraPDF)");
        println!("before claiming conformance.");
    } else {
        println!("Status: NON-CONFORMANT (violations found)");
    }

    if !result.warnings.is_empty() {
        println!();
        println!("Warnings ({}):", result.warnings.len());
        for warning in &result.warnings {
            println!("  - {warning}");
        }
    }

    if !result.errors.is_empty() {
        println!();
        println!("Errors ({}):", result.errors.len());
        for error in &result.errors {
            println!("  - {error}");
        }
    }

    // Exit 0 = no violations detected by these heuristics (usable for pipeline
    // gating, FR-STD-6); exit 1 = violations found. Neither is a conformance
    // claim. [FR-STD-5, FR-STD-6, CMP-STD-4]
    if result.conforms && result.errors.is_empty() {
        println!();
        println!(
            "No {} violations detected by the heuristic checks.",
            result.target_level
        );
        process::exit(0);
    } else {
        eprintln!();
        eprintln!("Document does not pass {} validation.", result.target_level);
        process::exit(1);
    }
}

/// Compare two PDF documents. [FR-CMP, M12]
///
/// Usage: pdf-platform compare <file1> <file2>
///
/// Extracts text from both documents and reports differences.
/// Shows page-by-page comparison with added/removed/changed content.
fn cmd_compare(path1: &Path, path2: &Path) {
    use coordinator::document::DocumentCoordinator;
    use text_extract::compare::{diff_lines, DiffQuality, LineDiff};

    // Set when any page exceeded the alignment bound, so the summary can say
    // the result is no longer reflow-resilient instead of implying it is.
    // [GR-7, FR-CMP-3, PRIN-6]
    let mut fell_back = false;

    // Open both documents.
    let worker = find_worker();

    let mut coord1 = match DocumentCoordinator::open(&worker, path1) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot open {}: {e}", path1.display());
            process::exit(2);
        }
    };

    let mut coord2 = match DocumentCoordinator::open(&worker, path2) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: cannot open {}: {e}", path2.display());
            process::exit(2);
        }
    };

    let pages1 = coord1.page_count();
    let pages2 = coord2.page_count();

    println!("Comparing:");
    println!("  File 1: {} ({} pages)", path1.display(), pages1);
    println!("  File 2: {} ({} pages)", path2.display(), pages2);
    println!();

    // Compare page counts.
    if pages1 != pages2 {
        println!("Page count differs: {} vs {}", pages1, pages2);
        println!();
    }

    // Extract and compare text from each page.
    let max_pages = pages1.max(pages2);
    let mut total_diffs = 0u32;
    let mut pages_with_diffs = 0u32;

    for page in 0..max_pages {
        let text1 = if page < pages1 {
            coord1.get_page_text(page)
                .map(|m| m.full_text())
                .unwrap_or_default()
        } else {
            String::new()
        };

        let text2 = if page < pages2 {
            coord2.get_page_text(page)
                .map(|m| m.full_text())
                .unwrap_or_default()
        } else {
            String::new()
        };

        if text1 == text2 {
            continue;
        }

        pages_with_diffs += 1;
        println!("Page {}:", page + 1);

        // Align by longest common subsequence rather than pairing lines by
        // index. Index pairing reported every line after an insertion as
        // changed — the "raw positional diff" FR-CMP-3 rules out in favour of
        // meaningful change detection. [FR-CMP-3]
        let lines1: Vec<&str> = text1.lines().collect();
        let lines2: Vec<&str> = text2.lines().collect();
        let (ops, quality) = diff_lines(&lines1, &lines2);
        if quality == DiffQuality::PositionalFallback {
            fell_back = true;
        }

        for op in &ops {
            match op {
                LineDiff::Same(_) => {}
                LineDiff::Removed(line) => {
                    println!("  - {line}");
                    total_diffs += 1;
                }
                LineDiff::Added(line) => {
                    println!("  + {line}");
                    total_diffs += 1;
                }
            }
        }
        println!();
    }

    // Summary.
    println!("Summary:");
    println!("  Pages compared: {}", max_pages);
    println!("  Pages with differences: {}", pages_with_diffs);
    println!("  Total line changes: {}", total_diffs);
    if fell_back {
        println!();
        println!("Note: at least one page exceeded the alignment bound, so its lines");
        println!("were paired by position. Those results are not reflow-resilient.");
    }

    let _ = coord1.close();
    let _ = coord2.close();

    if pages_with_diffs == 0 {
        println!();
        println!("Documents are identical (text content).");
        process::exit(0);
    } else {
        process::exit(1);
    }
}

/// Extract xref offsets from the PDF. [FR-SIG-2]
fn extract_xref_offsets(_bytes: &[u8]) -> Vec<(u32, u64)> {
    // Simplified: return empty for now. A proper implementation would
    // parse the xref table/stream to get object→offset mappings.
    Vec::new()
}
///
/// Pipeline file format: one step per section, type=merge/split/etc.
/// See `pdf_model::batch::BatchPipeline::serialize()` for the format.
fn cmd_batch(pipeline_path: &Path) {
    use coordinator::broker::optimize_with_verification;
    use pdf_model::batch::{BatchPipeline, BatchStep, execute_pipeline_with};

    if !pipeline_path.exists() {
        eprintln!("error: pipeline file not found: {}", pipeline_path.display());
        process::exit(1);
    }

    let content = std::fs::read_to_string(pipeline_path)
        .unwrap_or_else(|e| {
            eprintln!("error: cannot read pipeline file: {e}");
            process::exit(1);
        });

    // Parse the pipeline file.
    let mut pipeline = BatchPipeline::new(
        pipeline_path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("batch")
    );

    let mut current_type: Option<String> = None;
    let mut current_inputs: Vec<PathBuf> = Vec::new();
    let mut current_output: Option<PathBuf> = None;
    let mut current_output_dir: Option<PathBuf> = None;
    let mut current_pages_per_file: u32 = 2;
    let mut current_first: u32 = 1;
    let mut current_last: u32 = 1;
    let mut current_text: Option<String> = None;
    let mut current_profile: Option<String> = None;
    let mut current_start: u32 = 1;
    let mut current_width: usize = 6;

    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("steps=") {
            continue; // Header, ignore.
        }

        if line.starts_with("type=") {
            // Flush previous step.
            if let Some(ref step_type) = current_type {
                flush_batch_step(
                    &mut pipeline, step_type, &current_inputs,
                    current_output.as_deref(), current_output_dir.as_deref(),
                    current_pages_per_file, current_first, current_last,
                    current_text.as_deref(), current_profile.as_deref(),
                    current_start, current_width,
                );
            }
            current_type = Some(line[5..].to_string());
            current_inputs.clear();
            current_output = None;
            current_output_dir = None;
            current_text = None;
            current_profile = None;
        } else if let Some(val) = line.strip_prefix("input=") {
            current_inputs.push(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("output=") {
            current_output = Some(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("output_dir=") {
            current_output_dir = Some(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("pages_per_file=") {
            current_pages_per_file = val.parse().unwrap_or(2);
        } else if let Some(val) = line.strip_prefix("first=") {
            current_first = val.parse().unwrap_or(1);
        } else if let Some(val) = line.strip_prefix("last=") {
            current_last = val.parse().unwrap_or(1);
        } else if let Some(val) = line.strip_prefix("text=") {
            current_text = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("profile=") {
            current_profile = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("start=") {
            current_start = val.parse().unwrap_or(1);
        } else if let Some(val) = line.strip_prefix("width=") {
            current_width = val.parse().unwrap_or(6);
        }
    }
    // Flush last step.
    if let Some(ref step_type) = current_type {
        flush_batch_step(
            &mut pipeline, step_type, &current_inputs,
            current_output.as_deref(), current_output_dir.as_deref(),
            current_pages_per_file, current_first, current_last,
            current_text.as_deref(), current_profile.as_deref(),
            current_start, current_width,
        );
    }

    if pipeline.step_count() == 0 {
        eprintln!("error: no steps in pipeline");
        process::exit(1);
    }

    println!("Executing pipeline '{}' ({} steps)...", pipeline.name, pipeline.step_count());
    // Same verified candidate/publish safety net `optimize` gets standalone
    // (FR-BATCH: identical behavior via GUI/CLI/batch — see cmd_optimize).
    let results = execute_pipeline_with(&pipeline, &|input, output, profile| {
        optimize_with_verification(output, |candidate_path| {
            pdf_model::assembly_ops::optimize_pdf(input, candidate_path, profile)
                .map_err(|e| e.to_string())
        })
    });

    let mut all_ok = true;
    for (i, result) in results.iter().enumerate() {
        let status = if result.success { "ok" } else { "FAILED" };
        println!("  Step {}: {} ({}ms) — {}", i + 1, status, result.duration_ms, result.message);
        if !result.success {
            all_ok = false;
            break;
        }
    }

    if all_ok {
        println!("Pipeline completed successfully.");
        process::exit(0);
    } else {
        eprintln!("Pipeline failed.");
        process::exit(2);
    }
}

/// Cross-document index state directory (per-user app state, not beside any
/// document — matches the existing sidecar-journal convention in
/// `DocumentCoordinator::compute_sidecar_path`). [ADR-019 §3, ADR-021]
fn index_state_dir() -> PathBuf {
    std::env::temp_dir().join("pdf-platform-index")
}

/// Cross-document indexing: enroll/list/reindex/remove/search. [ADR-019 §3]
///
/// Usage:
///   pdf-platform index enroll <dir>
///   pdf-platform index list
///   pdf-platform index reindex [dir]   (all enrollments if omitted)
///   pdf-platform index remove <dir>
///   pdf-platform index search <query>
fn cmd_index(args: &[String]) {
    use coordinator::broker::{load_enrollment_registry, save_enrollment_registry};
    use coordinator::indexing::{
        indexing_summary, load_registry, reindex_enrollment, remove_enrollment_files,
        save_registry,
    };
    use search::tantivy_backend::CrossDocumentIndex;

    let Some(sub) = args.first() else {
        eprintln!("error: index requires a subcommand: enroll|list|reindex|remove|search");
        process::exit(1);
    };

    let state_dir = index_state_dir();
    if let Err(e) = std::fs::create_dir_all(&state_dir) {
        eprintln!("error: cannot create index state directory: {e}");
        process::exit(2);
    }
    let enrollment_path = state_dir.join("enrollments.bin");
    let file_registry_path = state_dir.join("file-registry.bin");
    let tantivy_dir = state_dir.join("tantivy");

    let mut enrollment_registry = match load_enrollment_registry(&enrollment_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("error: cannot load enrollment registry: {e}");
            process::exit(2);
        }
    };

    match sub.as_str() {
        "enroll" => {
            let Some(dir) = args.get(1) else {
                eprintln!("error: index enroll requires a directory argument");
                process::exit(1);
            };
            match enrollment_registry.enroll(Path::new(dir)) {
                Ok(id) => {
                    if let Err(e) = save_enrollment_registry(&enrollment_registry, &enrollment_path) {
                        eprintln!("error: cannot save enrollment registry: {e}");
                        process::exit(2);
                    }
                    println!("Enrolled: {dir}");
                    println!("Enrollment id: {}", hex_id(&id));
                }
                Err(e) => {
                    eprintln!("error: enroll failed: {e:?}");
                    process::exit(2);
                }
            }
        }
        "list" => {
            let file_registry = load_registry(&file_registry_path).unwrap_or_default();
            let enrollments: Vec<_> = enrollment_registry.enrollments().collect();
            if enrollments.is_empty() {
                println!("No enrolled roots.");
            } else {
                println!("Enrolled roots:");
                for (id, root) in &enrollments {
                    println!("  {} — {}", hex_id(id), root.display());
                }
            }
            if let Ok(index) = CrossDocumentIndex::open_or_create(&tantivy_dir) {
                let summary = indexing_summary(&file_registry, &index, &tantivy_dir);
                println!();
                println!("Tracked files: {}", summary.tracked_file_count);
                println!("Index size:    {} bytes", summary.disk_size_bytes);
            }
        }
        "reindex" => {
            let worker = find_worker();
            let mut file_registry = load_registry(&file_registry_path).unwrap_or_default();
            let mut index = match CrossDocumentIndex::open_or_create(&tantivy_dir) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("error: cannot open index: {e}");
                    process::exit(2);
                }
            };
            let targets: Vec<([u8; 16], PathBuf)> = match args.get(1) {
                Some(dir) => {
                    // Enrolled roots are stored canonicalized (see `enroll`);
                    // compare against the canonical form of the argument too,
                    // or this silently matches nothing (e.g. Windows
                    // canonicalize prepends \\?\).
                    let canonical = Path::new(dir)
                        .canonicalize()
                        .unwrap_or_else(|_| PathBuf::from(dir));
                    enrollment_registry
                        .enrollments()
                        .filter(|(_, root)| *root == canonical)
                        .map(|(id, root)| (id, root.to_path_buf()))
                        .collect()
                }
                None => enrollment_registry
                    .enrollments()
                    .map(|(id, root)| (id, root.to_path_buf()))
                    .collect(),
            };
            if targets.is_empty() {
                eprintln!("error: no matching enrollment(s) to reindex");
                process::exit(1);
            }
            let mut any_errors = false;
            for (id, root) in &targets {
                let report = reindex_enrollment(
                    &worker,
                    &enrollment_registry,
                    *id,
                    root,
                    &mut file_registry,
                    &mut index,
                );
                println!(
                    "{}: scanned {}, reindexed {}, skipped {}, pages {}",
                    root.display(),
                    report.files_scanned,
                    report.files_reindexed,
                    report.files_skipped_unchanged,
                    report.pages_indexed
                );
                for (path, message) in &report.errors {
                    eprintln!("  error: {}: {message}", path.display());
                    any_errors = true;
                }
            }
            if let Err(e) = save_registry(&file_registry, &file_registry_path) {
                eprintln!("error: cannot save file registry: {e}");
                process::exit(2);
            }
            process::exit(if any_errors { 1 } else { 0 });
        }
        "remove" => {
            let Some(dir) = args.get(1) else {
                eprintln!("error: index remove requires a directory argument");
                process::exit(1);
            };
            let mut file_registry = load_registry(&file_registry_path).unwrap_or_default();
            let mut index = match CrossDocumentIndex::open_or_create(&tantivy_dir) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("error: cannot open index: {e}");
                    process::exit(2);
                }
            };
            let removed_files = match remove_enrollment_files(&mut file_registry, &mut index, Path::new(dir)) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("error: index removal failed: {e}");
                    process::exit(2);
                }
            };
            let canonical_dir = Path::new(dir)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(dir));
            let ids: Vec<[u8; 16]> = enrollment_registry
                .enrollments()
                .filter(|(_, root)| *root == canonical_dir)
                .map(|(id, _)| id)
                .collect();
            for id in ids {
                enrollment_registry.remove(id);
            }
            if let Err(e) = save_enrollment_registry(&enrollment_registry, &enrollment_path) {
                eprintln!("error: cannot save enrollment registry: {e}");
                process::exit(2);
            }
            if let Err(e) = save_registry(&file_registry, &file_registry_path) {
                eprintln!("error: cannot save file registry: {e}");
                process::exit(2);
            }
            println!("Removed enrollment for {dir} ({removed_files} file(s) unindexed).");
        }
        "search" => {
            let Some(query) = args.get(1) else {
                eprintln!("error: index search requires a query argument");
                process::exit(1);
            };
            let index = match CrossDocumentIndex::open_or_create(&tantivy_dir) {
                Ok(i) => i,
                Err(e) => {
                    eprintln!("error: cannot open index: {e}");
                    process::exit(2);
                }
            };
            match index.search(query, 20) {
                Ok(hits) if hits.is_empty() => println!("No matches."),
                Ok(hits) => {
                    for hit in hits {
                        println!(
                            "source={} page={} reliable={} score={}",
                            hex_id(&hit.source),
                            hit.page,
                            hit.reliable,
                            hit.score_milli
                        );
                    }
                }
                Err(e) => {
                    eprintln!("error: search failed: {e}");
                    process::exit(2);
                }
            }
        }
        other => {
            eprintln!("error: unknown index subcommand '{other}' (enroll|list|reindex|remove|search)");
            process::exit(1);
        }
    }
}

fn hex_id(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

fn flush_batch_step(
    pipeline: &mut pdf_model::batch::BatchPipeline,
    step_type: &str,
    inputs: &[PathBuf],
    output: Option<&Path>,
    output_dir: Option<&Path>,
    pages_per_file: u32,
    first: u32,
    last: u32,
    text: Option<&str>,
    profile: Option<&str>,
    start: u32,
    width: usize,
) {
    match step_type {
        "merge" => {
            if inputs.len() >= 2 {
                if let Some(out) = output {
                    pipeline.add_step(pdf_model::batch::BatchStep::Merge {
                        inputs: inputs.to_vec(),
                        output: out.to_path_buf(),
                    });
                }
            }
        }
        "split_per_page" => {
            if let Some(inp) = inputs.first() {
                if let Some(dir) = output_dir {
                    pipeline.add_step(pdf_model::batch::BatchStep::SplitPerPage {
                        input: inp.clone(),
                        output_dir: dir.to_path_buf(),
                    });
                }
            }
        }
        "split_chunked" => {
            if let Some(inp) = inputs.first() {
                if let Some(dir) = output_dir {
                    pipeline.add_step(pdf_model::batch::BatchStep::SplitChunked {
                        input: inp.clone(),
                        pages_per_file,
                        output_dir: dir.to_path_buf(),
                    });
                }
            }
        }
        "extract_pages" => {
            if let Some(inp) = inputs.first() {
                if let Some(out) = output {
                    pipeline.add_step(pdf_model::batch::BatchStep::ExtractPages {
                        input: inp.clone(),
                        first,
                        last,
                        output: out.to_path_buf(),
                    });
                }
            }
        }
        "optimize" => {
            if let Some(inp) = inputs.first() {
                if let Some(out) = output {
                    pipeline.add_step(pdf_model::batch::BatchStep::Optimize {
                        input: inp.clone(),
                        output: out.to_path_buf(),
                        profile: profile.unwrap_or("screen").to_string(),
                    });
                }
            }
        }
        "watermark" => {
            if let Some(inp) = inputs.first() {
                if let Some(out) = output {
                    pipeline.add_step(pdf_model::batch::BatchStep::Watermark {
                        input: inp.clone(),
                        text: text.unwrap_or("").to_string(),
                        output: out.to_path_buf(),
                    });
                }
            }
        }
        "bates_number" => {
            if let Some(inp) = inputs.first() {
                if let Some(out) = output {
                    pipeline.add_step(pdf_model::batch::BatchStep::BatesNumber {
                        input: inp.clone(),
                        start,
                        width,
                        output: out.to_path_buf(),
                    });
                }
            }
        }
        _ => {
            eprintln!("warning: unknown step type '{step_type}', skipping");
        }
    }
}

fn find_worker() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    dir.pop();
    let name = if cfg!(windows) {
        "worker.exe"
    } else {
        "worker"
    };
    let debug = dir.join("target").join("debug").join(name);
    if debug.exists() {
        return debug;
    }
    let release = dir.join("target").join("release").join(name);
    if release.exists() {
        return release;
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let cand = parent.join(name);
            if cand.exists() {
                return cand;
            }
        }
    }
    eprintln!("error: worker binary not found (build -p worker-main first)");
    process::exit(3);
}

fn cmd_with_coordinator(cmd: &str, path: &Path, rest: &[String]) {
    use coordinator::document::DocumentCoordinator;

    let worker = find_worker();
    let mut coord = match DocumentCoordinator::open(&worker, path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: open failed: {e}");
            process::exit(2);
        }
    };

    match cmd {
        "outline" => match coord.get_outline() {
            Ok(r) => {
                println!("Outline entries: {} (total {})", r.count, r.total);
                if !r.data.is_empty() {
                    println!("Data: {}", r.data);
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(2);
            }
        },
        "layers" => match coord.get_layers() {
            Ok(r) => {
                println!("Layers: {}", yn(r.flag));
                println!("Groups: {} (total {})", r.count, r.total);
            }
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(2);
            }
        },
        "attachments" => match coord.get_attachments() {
            Ok(r) => {
                println!("Attachments: {}", r.count);
                if !r.data.is_empty() {
                    println!("Data: {}", r.data);
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                process::exit(2);
            }
        },
        "diagnostics" => {
            let d = coord.diagnostics_report();
            println!("Pages:            {}", d.page_count);
            println!("Leniency events:  {}", d.leniency_count);
            for e in &d.leniency_events {
                println!("  - {e}");
            }
            println!("AcroForm:         {}", yn(d.has_acroform));
            println!("JavaScript:       {}", yn(d.has_js));
            println!("XFA:              {}", yn(d.has_xfa));
            println!("Signatures:       {}", d.sig_count);
            println!("Text cache pages: {}", d.text_cache_pages);
            println!("Dirty:            {}", yn(d.dirty));
            println!("Can undo/redo:    {}/{}", yn(d.can_undo), yn(d.can_redo));
            println!();
            println!("--- Confinement ---");
            print!("{}", sandbox::confinement::confinement_report().display_text());
        }
        "find" => {
            if rest.is_empty() {
                eprintln!("error: find requires a query");
                process::exit(1);
            }
            let query = rest.join(" ");
            match coord.find_in_document(&query) {
                Ok(results) => {
                    let total: usize = results.iter().map(|r| r.matches.len()).sum();
                    let unreliable = results.iter().filter(|p| !p.reliable).count();
                    println!("Query: {query:?}");
                    println!("Hits:  {total} across {} page(s)", results.len());
                    if unreliable > 0 {
                        eprintln!(
                            "warning: {unreliable} page(s) have unreliable text layer (ToUnicode pathology)"
                        );
                    }
                    for page in results {
                        for m in &page.matches {
                            println!(
                                "  page {} line {} offset {} len {} reliable={}",
                                page.page_index,
                                m.line_index,
                                m.char_offset,
                                m.char_len,
                                page.reliable
                            );
                            if let Some(box_) = coord.selection_boxes_for_match(
                                page.page_index,
                                m.line_index,
                                m.char_offset,
                                m.char_len,
                            ) {
                                println!(
                                    "    box: ({:.1},{:.1}) {:.1}x{:.1}",
                                    box_.x, box_.y, box_.width, box_.height
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(2);
                }
            }
        }
        "export-text" => {
            // M2: text export with reliability flag. [FR-SRCH, ADR-019, PRIN-6]
            let page: u32 = rest
                .first()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            match coord.get_page_text(page) {
                Ok(model) => {
                    println!("page={}", model.page_index);
                    println!("reliable={}", model.reliable);
                    println!("chars={}", model.char_count);
                    println!("structured={}", model.has_structure);
                    if !model.reliable {
                        eprintln!(
                            "warning: text layer flagged unreliable â€” do not treat as authoritative"
                        );
                    }
                    println!("---");
                    println!("{}", model.full_text());
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    process::exit(2);
                }
            }
        }
        _ => unreachable!(),
    }

    let _ = coord.close();
    process::exit(0);
}

fn compute_sidecar_path(doc_path: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let canonical = doc_path
        .canonicalize()
        .unwrap_or_else(|_| doc_path.to_path_buf());
    let mut hasher = DefaultHasher::new();
    canonical.hash(&mut hasher);
    let hash = hasher.finish();

    let dir = std::env::temp_dir().join("pdf-platform-journals");
    dir.join(format!("{hash:016x}.journal"))
}

struct SidecarInfo {
    source_path: PathBuf,
    source_size: u64,
    group_count: usize,
    group_names: Vec<String>,
}

fn read_sidecar_info(sidecar_path: &Path, doc_path: &Path) -> Result<SidecarInfo, String> {
    let data = std::fs::read(sidecar_path).map_err(|e| format!("failed to read sidecar: {e}"))?;
    let text = String::from_utf8_lossy(&data);

    let mut source_path = None;
    let mut source_size = None;
    let mut journal_start = 0;

    for (i, line) in text.lines().enumerate() {
        if let Some(rest) = line.strip_prefix("SOURCE_PATH:") {
            source_path = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("SOURCE_SIZE:") {
            source_size = rest.parse().ok();
        } else if line == "---" {
            journal_start = i + 1;
            break;
        }
    }

    let source_path = PathBuf::from(source_path.ok_or("missing SOURCE_PATH")?);
    let source_size = source_size.ok_or("missing SOURCE_SIZE")?;

    let canonical_doc = doc_path
        .canonicalize()
        .unwrap_or_else(|_| doc_path.to_path_buf());
    let canonical_sidecar = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.clone());
    if canonical_doc != canonical_sidecar
        && canonical_doc.to_string_lossy() != canonical_sidecar.to_string_lossy()
    {
        return Err("sidecar is for a different document".into());
    }

    let journal_text = text
        .lines()
        .skip(journal_start)
        .collect::<Vec<_>>()
        .join("\n");
    let group_count = journal_text
        .lines()
        .filter(|l| l.starts_with("GROUP:"))
        .count();
    let group_names: Vec<String> = journal_text
        .lines()
        .filter_map(|l| l.strip_prefix("GROUP:"))
        .map(|s| s.to_string())
        .collect();

    Ok(SidecarInfo {
        source_path,
        source_size,
        group_count,
        group_names,
    })
}
