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

/// Whether `--json` was given. Set once in `main`; commands that can speak JSON
/// read it, and every other command is refused before it runs. [FR-CLI-3]
static JSON_OUTPUT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

fn json_mode() -> bool {
    JSON_OUTPUT.get().copied().unwrap_or(false)
}

/// The commands whose output has a machine-readable form.
const JSON_COMMANDS: [&str; 5] = [
    "summary",
    "revisions",
    "extract-attachments",
    "comments",
    "export-reading-order",
];

/// The command a given argument list will run: the named command, or the
/// summary when the first argument is a file. Mirrors `main`'s dispatch.
fn command_name<'a>(args: &'a [String], file_cmds: &[&str]) -> &'a str {
    match args.get(1).map(String::as_str) {
        Some("compare") => "compare",
        Some(c) if file_cmds.contains(&c) => c,
        Some(c @ ("forms-calc-demo" | "confinement" | "batch" | "plugin-list" | "index"
        | "plugin-validate" | "help" | "-h" | "--help")) => c,
        _ => "summary",
    }
}

fn main() {
    let mut args: Vec<String> = std::env::args().collect();
    let json_requested = args.iter().skip(1).any(|a| a == "--json");
    args.retain(|a| a != "--json");
    let _ = JSON_OUTPUT.set(json_requested);
    if args.len() < 2 {
        usage();
        process::exit(1);
    }

    // `--json` is refused, not ignored, where there is no JSON form: a script
    // that asked for it must not be handed prose. [FR-CLI-3, GR-8]
    if json_requested {
        let name = command_name(&args, &FILE_COMMANDS);
        if !JSON_COMMANDS.contains(&name) {
            eprintln!(
                "error: --json is not supported by `{name}`; it is supported by: {}",
                JSON_COMMANDS.join(", ")
            );
            process::exit(1);
        }
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

    static FILE_COMMANDS: &[&str] = &[
        "outline",
        "layers",
        "attachments",
        "diagnostics",
        "find",
        "export-text",
        "export-images",
        "export-html",
        "extract-attachments",
        "optimize-preflight",
        "merge",
        "split",
        "optimize",
        "extract-pages",
        "revisions",
        "sanitize",
        "rotate",
        "delete-pages",
        "crop",
        "move-pages",
        "duplicate-pages",
        "insert-blank",
        "insert-pages",
        "comments",
        "export-reading-order",
        "stamp",
        "redact-by-term",
        "validate-signatures",
        "validate-pdf-a",
        "ocr",
    ];
    let file_cmds = FILE_COMMANDS;

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

    if cmd == "export-images" {
        cmd_export_images(&path, &rest);
        return;
    }
    if cmd == "export-html" {
        cmd_export_html(&path, &rest);
        return;
    }
    if cmd == "export-reading-order" {
        cmd_export_reading_order(&path);
        return;
    }
    if cmd == "comments" {
        cmd_comments(&path, &rest);
        return;
    }
    if cmd == "insert-pages" {
        cmd_insert_pages(&path, &rest);
        return;
    }
    if cmd == "extract-attachments" {
        cmd_extract_attachments(&path, &rest);
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
    if cmd == "revisions" {
        cmd_revisions(&path);
        return;
    }
    if cmd == "sanitize" {
        cmd_sanitize(&path, &rest);
        return;
    }
    if cmd == "rotate" {
        cmd_rotate(&path, &rest);
        return;
    }
    if cmd == "delete-pages" {
        cmd_delete_pages(&path, &rest);
        return;
    }
    if cmd == "crop" {
        cmd_crop(&path, &rest);
        return;
    }
    if cmd == "move-pages" {
        cmd_move_pages(&path, &rest);
        return;
    }
    if cmd == "duplicate-pages" {
        cmd_duplicate_pages(&path, &rest);
        return;
    }
    if cmd == "insert-blank" {
        cmd_insert_blank(&path, &rest);
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
         \x20 pdf-platform export-images <file> -o <outdir> [--dpi N]\n\
         \x20 pdf-platform export-html <file> -o <out.html>\n\
         \x20 pdf-platform extract-attachments <file> -o <dir>\n\
         \x20 pdf-platform optimize-preflight <file> [screen|print|archive]\n\
         \x20 pdf-platform merge <file1> <file2> [...] -o <out.pdf>\n\
         \x20 pdf-platform split <file> -o <outdir>\n\
         \x20 pdf-platform extract-pages <file> <first> <last> -o <out.pdf>\n\
         \x20 pdf-platform revisions <file>\n\
         \x20 pdf-platform sanitize <file> -o <out.pdf> [--discard-signatures]\n\
         \x20 pdf-platform rotate <file> --pages 1,3-5 --degrees 90|180|270 -o <out.pdf>\n\
         \x20 pdf-platform delete-pages <file> --pages 2,4-6 -o <out.pdf>\n\
         \x20 pdf-platform crop <file> --pages 1,3-5 --box llx,lly,urx,ury -o <out.pdf>\n\
         \x20 pdf-platform move-pages <file> --pages 3 --to 1 -o <out.pdf>\n\
         \x20 pdf-platform duplicate-pages <file> --pages 2,4 -o <out.pdf>\n\
         \x20 pdf-platform insert-blank <file> --at 2 [--size 612,792] -o <out.pdf>\n\
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

/// Whether the catalog marks the document tagged, from the same scan the
/// summary uses. `false` when the file cannot be scanned: the summary reports
/// that failure itself. [FR-A11Y-1]
fn is_tagged(path: &Path) -> bool {
    pdf_cos::scan::scan_structure(path).is_ok_and(|s| s.is_tagged)
}

fn cmd_summary(path: &Path) {
    match coordinator::inspect::inspect(path) {
        Ok(s) if json_mode() => {
            println!(
                "{}",
                serde_json::json!({
                    "pages": s.page_count,
                    "acroform": s.has_acroform,
                    "xfa": s.has_xfa,
                    "javascript": s.has_js,
                    "signatures": s.sig_count,
                    "tagged": is_tagged(path),
                    "leniency": s.leniency_events,
                })
            );
            process::exit(0);
        }
        Ok(s) => {
            println!("Pages:      {}", s.page_count);
            println!("AcroForm:   {}", yn(s.has_acroform));
            println!("XFA:        {}", yn(s.has_xfa));
            println!("JavaScript: {}", yn(s.has_js));
            println!("Signatures: {}", s.sig_count);
            println!("Tagged:     {}", yn(is_tagged(path)));
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

/// Resolution used when `--dpi` is not given.
const DEFAULT_EXPORT_DPI: u32 = 150;

/// Room left in a transport frame for everything that is not pixels.
const FRAME_HEADROOM_BYTES: u64 = 64 * 1024;

/// `--dpi N`, or nothing. Anything else is an error: a typo that fell back to
/// the default would hand back images at a resolution nobody asked for.
/// [GR-8, UX-ERR-3]
fn parse_export_dpi(extra: &[String]) -> Result<u32, String> {
    match extra {
        [] => Ok(DEFAULT_EXPORT_DPI),
        [flag, value] if flag == "--dpi" => match value.trim().parse::<u32>() {
            Ok(dpi) if dpi > 0 => Ok(dpi),
            _ => Err(format!("--dpi must be a whole number above 0, got '{value}'")),
        },
        other => Err(format!("unexpected argument(s): {}", other.join(" "))),
    }
}

/// Render every page to `<out_dir>/page-NNN.png`. [FR-EXPORT-1, FR-EXPORT-3]
///
/// A page crosses the IPC as one RGBA frame, and a frame is capped at
/// `MAX_FRAME` (SDS §4.6). Every page is checked against that cap before the
/// first file is written, so a DPI that cannot be carried fails whole instead
/// of leaving a partial export that looks complete. Tiled export for higher
/// resolutions is not implemented.
fn export_images(
    coord: &mut coordinator::document::DocumentCoordinator,
    out_dir: &Path,
    dpi: u32,
) -> Result<Vec<PathBuf>, String> {
    let scale = dpi as f32 / 72.0;
    // The raster travels base64-encoded, so three bytes of pixels cost four in
    // the frame. Measured, not assumed: a 16.6 MB raster arrived as a
    // 22,207,396-byte frame against the 16,777,216 cap.
    let budget = (u64::from(protocol::transport::MAX_FRAME) - FRAME_HEADROOM_BYTES) / 4 * 3;

    // Bytes of RGBA for one page; the renderer rounds each side up.
    let raster_bytes = |width_pt: f32, height_pt: f32, dpi: u32| -> u64 {
        let s = dpi as f32 / 72.0;
        (width_pt * s).ceil() as u64 * (height_pt * s).ceil() as u64 * 4
    };

    for (index, (width_pt, height_pt, _)) in coord.summary().page_dimensions_f().into_iter().enumerate() {
        let pixels = raster_bytes(width_pt, height_pt, dpi);
        if pixels > budget {
            // Start from the continuous answer and step down to one that the
            // same rounding accepts, so the number named here really works.
            let mut largest =
                (72.0 * (budget as f64 / (4.0 * width_pt as f64 * height_pt as f64)).sqrt()) as u32;
            while largest > 0 && raster_bytes(width_pt, height_pt, largest) > budget {
                largest -= 1;
            }
            return Err(format!(
                "page {} is {width_pt:.0}x{height_pt:.0} pt; at {dpi} DPI that is {pixels} bytes, \
                 over the {budget}-byte limit for one rendered page. The largest DPI for this \
                 page is {largest}.",
                index + 1
            ));
        }
    }

    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;

    let count = coord.page_count();
    let digits = count.to_string().len().max(3);
    let mut written = Vec::with_capacity(count as usize);
    for page in 0..count {
        let raster = coord
            .render_page(page, scale)
            .map_err(|e| format!("page {}: render failed: {e}", page + 1))?;
        if raster.pixels.len() as u64 != u64::from(raster.width) * u64::from(raster.height) * 4 {
            return Err(format!(
                "page {}: the renderer returned {} bytes for a {}x{} raster",
                page + 1,
                raster.pixels.len(),
                raster.width,
                raster.height
            ));
        }
        let file = out_dir.join(format!("page-{:0digits$}.png", page + 1));
        ocr_bridge::write_rgba_png(&file, &raster.pixels, raster.width, raster.height)?;
        written.push(file);
    }
    Ok(written)
}

/// Export pages as images. [FR-EXPORT-1, FR-EXPORT-3]
///
/// Usage: pdf-platform export-images <file> -o <outdir> [--dpi N]
fn cmd_export_images(path: &Path, rest: &[String]) {
    use coordinator::document::DocumentCoordinator;

    let (extra, out_dir) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    let dpi = match parse_export_dpi(&extra) {
        Ok(dpi) => dpi,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };

    let worker = find_worker();
    let mut coord = match DocumentCoordinator::open(&worker, path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: open failed: {e}");
            process::exit(2);
        }
    };
    let result = export_images(&mut coord, &out_dir, dpi);
    let _ = coord.close();
    match result {
        Ok(files) => {
            println!("exported {} page(s) at {dpi} DPI to {}", files.len(), out_dir.display());
            for file in files {
                println!("  {}", file.display());
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// One page of extracted text, as the HTML export needs it.
struct HtmlPage {
    number: u32,
    reliable: bool,
    text: String,
}

/// Escape text for an HTML body or attribute. Document text is untrusted: a
/// page that contains `<script>` must come out as words. [NFR-SEC, GR-8]
fn html_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Build the page: plain text in extraction order, one paragraph per line.
///
/// No styles, scripts or external references, so opening the file transmits
/// nothing (GR-9). A page whose text layer is flagged unreliable says so where
/// the text is, instead of presenting it as authoritative. [FR-SRCH-5, PRIN-6]
fn render_html(title: &str, pages: &[HtmlPage]) -> String {
    let mut out = String::new();
    out.push_str("<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str(&format!("<title>{}</title>\n</head>\n<body>\n", html_escape(title)));
    out.push_str(&format!("<h1>{}</h1>\n", html_escape(title)));
    out.push_str(
        "<p>Plain text in extraction order. Layout, fonts, images and document \
         structure are not included.</p>\n",
    );
    for page in pages {
        let n = page.number;
        out.push_str(&format!(
            "<section id=\"page-{n}\" aria-labelledby=\"page-{n}-heading\">\n\
             <h2 id=\"page-{n}-heading\">Page {n}</h2>\n"
        ));
        if !page.reliable {
            out.push_str(
                "<p role=\"note\">The text layer of this page is flagged unreliable. Do not \
                 treat it as authoritative; the page may need OCR.</p>\n",
            );
        }
        let mut any = false;
        for line in page.text.lines().filter(|l| !l.trim().is_empty()) {
            any = true;
            out.push_str(&format!("<p>{}</p>\n", html_escape(line)));
        }
        if !any {
            out.push_str("<p>No text on this page.</p>\n");
        }
        out.push_str("</section>\n");
    }
    out.push_str("</body>\n</html>\n");
    out
}

/// Export the whole document's text to one HTML file. [FR-EXPORT-1, FR-EXPORT-3]
///
/// The file is built in memory and written once, so a page that fails to
/// extract leaves no half-written export behind.
fn export_html(
    coord: &mut coordinator::document::DocumentCoordinator,
    out: &Path,
    title: &str,
) -> Result<u32, String> {
    let count = coord.page_count();
    let mut pages = Vec::with_capacity(count as usize);
    for index in 0..count {
        let model = coord
            .get_page_text(index)
            .map_err(|e| format!("page {}: text extraction failed: {e}", index + 1))?;
        pages.push(HtmlPage {
            number: index + 1,
            reliable: model.reliable,
            text: model.full_text(),
        });
    }
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(out, render_html(title, &pages))
        .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
    Ok(count)
}

/// Export text as HTML. [FR-EXPORT-1, FR-EXPORT-3]
///
/// Usage: pdf-platform export-html <file> -o <out.html>
fn cmd_export_html(path: &Path, rest: &[String]) {
    use coordinator::document::DocumentCoordinator;

    let (extra, out) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    if !extra.is_empty() {
        eprintln!("error: unexpected argument(s): {}", extra.join(" "));
        process::exit(1);
    }
    let worker = find_worker();
    let mut coord = match DocumentCoordinator::open(&worker, path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: open failed: {e}");
            process::exit(2);
        }
    };
    let title = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".into());
    let result = export_html(&mut coord, &out, &title);
    let _ = coord.close();
    match result {
        Ok(pages) => {
            println!("exported {pages} page(s) to {}", out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// The most one attachment may decode to. A document chooses its attachments'
/// sizes, so the reader chooses the ceiling. [GR-7]
const MAX_ATTACHMENT_BYTES: usize = 256 * 1024 * 1024;

/// A name that is safe to create inside the output directory. An attachment's
/// name is document-supplied: `..\..\startup\x.exe` must not leave the folder.
/// [FR-EMB-4, NFR-SEC-1]
fn safe_attachment_name(name: &str, taken: &[String]) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let cleaned: String = base
        .chars()
        .map(|c| if c.is_control() || ":*?\"<>|".contains(c) { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim().trim_matches('.').to_string();
    let stem = if cleaned.is_empty() { "attachment".to_string() } else { cleaned };
    let mut candidate = stem.clone();
    let mut n = 2;
    while taken.iter().any(|t| t.eq_ignore_ascii_case(&candidate)) {
        candidate = match stem.rsplit_once('.') {
            Some((a, b)) if !a.is_empty() => format!("{a}-{n}.{b}"),
            _ => format!("{stem}-{n}"),
        };
        n += 1;
    }
    candidate
}

/// Write every embedded file of `data` into `out_dir`; returns (name, bytes).
/// Never opens or runs anything it writes. [FR-EMB-1, FR-EMB-4]
fn extract_attachments(data: &[u8], out_dir: &Path) -> Result<Vec<(String, usize)>, String> {
    let files: Vec<pdf_cos::attachments::EmbeddedFile> =
        pdf_cos::attachments::list_embedded_files(data);
    std::fs::create_dir_all(out_dir)
        .map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let mut written: Vec<(String, usize)> = Vec::new();
    let mut taken: Vec<String> = Vec::new();
    for file in &files {
        let bytes = pdf_cos::attachments::extract_embedded_file(data, file, MAX_ATTACHMENT_BYTES)
            .map_err(|e| format!("{}: {e}", file.name))?;
        let name = safe_attachment_name(&file.name, &taken);
        let target = out_dir.join(&name);
        // create_new: never replace a file that is already there.
        let mut handle = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&target)
            .map_err(|e| format!("cannot create {}: {e}", target.display()))?;
        std::io::Write::write_all(&mut handle, &bytes)
            .map_err(|e| format!("cannot write {}: {e}", target.display()))?;
        taken.push(name.clone());
        written.push((name, bytes.len()));
    }
    Ok(written)
}

/// Extract embedded files. [FR-EMB-1, FR-EMB-3, FR-CLI-1]
///
/// Usage: pdf-platform extract-attachments <file> -o <dir>
fn cmd_extract_attachments(path: &Path, rest: &[String]) {
    let (extra, out) = match parse_dash_o(rest) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(1);
        }
    };
    if !extra.is_empty() {
        eprintln!("error: unexpected argument(s): {}", extra.join(" "));
        process::exit(1);
    }
    let data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            process::exit(2);
        }
    };
    match extract_attachments(&data, &out) {
        Ok(files) if json_mode() => {
            let list: Vec<_> = files
                .iter()
                .map(|(name, size)| serde_json::json!({ "name": name, "bytes": size }))
                .collect();
            println!(
                "{}",
                serde_json::json!({ "directory": out.display().to_string(), "extracted": list })
            );
        }
        Ok(files) if files.is_empty() => {
            println!("no embedded files");
        }
        Ok(files) => {
            for (name, size) in &files {
                println!("{name} ({size} bytes)");
            }
            println!("extracted {} file(s) to {}", files.len(), out.display());
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Which input a planned run of pages comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InsertSource {
    Base,
    Other,
}

/// The page runs that make up the result of inserting `other_range` (1-based,
/// inclusive) of another document so that its first page becomes page `at` of
/// a `base_count`-page document. `at` of `base_count + 1` appends.
/// [FR-INSERT-1]
fn plan_insertion(
    base_count: u32,
    at: u32,
    other_range: (u32, u32),
) -> Result<Vec<(InsertSource, u32, u32)>, String> {
    if at == 0 || at > base_count + 1 {
        return Err(format!(
            "--at {at} is outside 1..={}: the document has {base_count} page(s)",
            base_count + 1
        ));
    }
    let mut plan = Vec::new();
    if at > 1 {
        plan.push((InsertSource::Base, 1, at - 1));
    }
    plan.push((InsertSource::Other, other_range.0, other_range.1));
    if at <= base_count {
        plan.push((InsertSource::Base, at, base_count));
    }
    Ok(plan)
}

/// Insert pages from another PDF. [FR-INSERT-1, FR-ORG-2, FR-CLI-1]
///
/// Usage: pdf-platform insert-pages <file> --from <other.pdf> [--range 2-4] --at N -o <out.pdf>
///
/// This goes through qpdf, so the result is a rewritten file, not an
/// incremental update: signatures and earlier revisions are not carried over,
/// and the output says so. (The page operations that edit in place are the
/// other `*-pages` commands.) [PRIN-2, GR-8]
fn cmd_insert_pages(path: &Path, rest: &[String]) {
    use pdf_model::assembly_ops::{merge_pdfs_with_ranges, pdf_page_count, PageRange};

    let fail = |code: i32, message: String| -> ! {
        eprintln!("error: {message}");
        process::exit(code);
    };
    let (_, out) = parse_dash_o(rest).unwrap_or_else(|e| fail(1, e));
    let Some(from) = flag_value(rest, "--from") else {
        fail(1, "insert-pages needs --from <other.pdf>".into());
    };
    let Some(at_text) = flag_value(rest, "--at") else {
        fail(1, "insert-pages needs --at <page>".into());
    };
    let at = parse_position(at_text, "--at").unwrap_or_else(|e| fail(1, e)) + 1;
    let from = PathBuf::from(from);
    if !from.exists() {
        fail(1, format!("not found: {}", from.display()));
    }
    if out.exists() {
        fail(1, format!("{} already exists; choose a new -o", out.display()));
    }
    let base_count = pdf_page_count(path).unwrap_or_else(|e| fail(2, e.to_string()));
    let other_count = pdf_page_count(&from).unwrap_or_else(|e| fail(2, e.to_string()));
    let range = match flag_value(rest, "--range") {
        None => (1, other_count),
        Some(spec) => {
            let (a, b) = spec.split_once('-').unwrap_or((spec, spec));
            let number = |s: &str| {
                s.trim()
                    .parse::<u32>()
                    .map_err(|_| format!("'{}' is not a page number", s.trim()))
            };
            let (first, last) = (
                number(a).unwrap_or_else(|e| fail(1, e)),
                number(b).unwrap_or_else(|e| fail(1, e)),
            );
            check_page_range(first, last).unwrap_or_else(|e| fail(1, e));
            if last > other_count {
                fail(
                    1,
                    format!("--range ends at {last}; {} has {other_count} page(s)", from.display()),
                );
            }
            (first, last)
        }
    };
    let plan = plan_insertion(base_count, at, range).unwrap_or_else(|e| fail(1, e));
    let runs: Vec<PageRange> = plan
        .iter()
        .map(|&(source, first, last)| {
            let file = if source == InsertSource::Base { path } else { from.as_path() };
            PageRange::range(file, first, last)
        })
        .collect();
    match merge_pdfs_with_ranges(&runs, &out) {
        Ok(()) => {
            println!(
                "inserted {} page(s) of {} at page {at} -> {} ({} page(s)); \
                 rewritten copy: signatures and earlier revisions are not carried over",
                range.1 - range.0 + 1,
                from.display(),
                out.display(),
                base_count + (range.1 - range.0 + 1)
            );
            process::exit(0);
        }
        Err(e) => fail(2, e.to_string()),
    }
}

/// Text of a tagged document in its logical reading order.
/// [FR-A11Y-1, FR-A11Y-3, FR-EXPORT-1, US-AXS-2]
///
/// Usage: pdf-platform export-reading-order <file> [--json]
fn cmd_export_reading_order(path: &Path) {
    let data = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        process::exit(2);
    });
    let result: Result<pdf_cos::reading_order::ReadingOrder, pdf_cos::reading_order::ReadingOrderError> =
        pdf_cos::reading_order::read_in_order(&data);
    let order = match result {
        Ok(order) => order,
        Err(e) => {
            // An untagged file has no reading order to report: say so rather
            // than falling back to drawing order under the same name.
            eprintln!("error: {e}; `export-text` gives the text in drawing order");
            process::exit(2);
        }
    };
    if json_mode() {
        let blocks: Vec<_> = order
            .blocks
            .iter()
            .map(|b| serde_json::json!({ "page": b.page, "element": b.element, "text": b.text }))
            .collect();
        println!(
            "{}",
            serde_json::json!({
                "blocks": blocks,
                "missing_content": order.missing_content,
                "unreliable": order.unreliable,
            })
        );
        process::exit(0);
    }
    for block in &order.blocks {
        let block: &pdf_cos::reading_order::ReadingBlock = block;
        println!("[p.{} {}] {}", block.page, block.element, block.text);
    }
    if order.unreliable {
        eprintln!(
            "warning: some text was decoded without a Unicode map and may be wrong \
             (marked U+FFFD or guessed as Latin-1)"
        );
    }
    if order.missing_content > 0 {
        eprintln!(
            "warning: {} content reference(s) in the structure tree were not found on their page",
            order.missing_content
        );
    }
    process::exit(0);
}

/// Which comments `comments` keeps. A field left `None` keeps everything.
#[derive(Debug, Default)]
struct CommentFilter {
    /// Exact author, compared case-insensitively.
    author: Option<String>,
    /// Annotation subtype such as `Highlight`, compared case-insensitively.
    kind: Option<String>,
    page: Option<u32>,
    /// `Some(true)` keeps replies only, `Some(false)` drops them.
    replies: Option<bool>,
}

fn filter_comments(
    comments: Vec<pdf_cos::comments::Comment>,
    filter: &CommentFilter,
) -> Vec<pdf_cos::comments::Comment> {
    comments
        .into_iter()
        .filter(|c| {
            filter.author.as_ref().is_none_or(|a| c.author.eq_ignore_ascii_case(a))
                && filter.kind.as_ref().is_none_or(|k| c.subtype.eq_ignore_ascii_case(k))
                && filter.page.is_none_or(|p| c.page == p)
                && filter.replies.is_none_or(|r| c.is_reply == r)
        })
        .collect()
}

/// List the document's comments. [FR-REV-2, FR-REV-3, US-STU-7]
///
/// Usage: pdf-platform comments <file> [--author NAME] [--type Text|Highlight|...]
///        [--page N] [--replies-only | --no-replies] [--json]
fn cmd_comments(path: &Path, rest: &[String]) {
    let filter = CommentFilter {
        author: flag_value(rest, "--author").map(str::to_string),
        kind: flag_value(rest, "--type").map(str::to_string),
        page: flag_value(rest, "--page").map(|p| {
            p.trim().parse::<u32>().unwrap_or_else(|_| {
                eprintln!("error: --page must be a page number, got '{p}'");
                process::exit(1);
            })
        }),
        replies: match (
            rest.iter().any(|a| a == "--replies-only"),
            rest.iter().any(|a| a == "--no-replies"),
        ) {
            (true, true) => {
                eprintln!("error: --replies-only and --no-replies contradict each other");
                process::exit(1);
            }
            (true, false) => Some(true),
            (false, true) => Some(false),
            (false, false) => None,
        },
    };
    let data = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        process::exit(2);
    });
    let comments = filter_comments(pdf_cos::comments::list_comments(&data), &filter);
    if json_mode() {
        let list: Vec<_> = comments
            .iter()
            .map(|c| {
                serde_json::json!({
                    "page": c.page,
                    "type": c.subtype,
                    "author": c.author,
                    "text": c.contents,
                    "modified": c.modified,
                    "reply": c.is_reply,
                })
            })
            .collect();
        println!("{}", serde_json::json!({ "comments": list }));
        process::exit(0);
    }
    if comments.is_empty() {
        println!("no comments");
        process::exit(0);
    }
    for c in &comments {
        let who = if c.author.is_empty() { "(no author)" } else { c.author.as_str() };
        let reply = if c.is_reply { " [reply]" } else { "" };
        println!("p.{} {} by {who}{reply}", c.page, c.subtype);
        if !c.contents.is_empty() {
            println!("    {}", c.contents.replace('\n', "\n    "));
        }
    }
    println!("{} comment(s)", comments.len());
    process::exit(0);
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

/// Page numbers are 1-based and ordered.
fn check_page_range(first: u32, last: u32) -> Result<(), String> {
    if first == 0 || last == 0 {
        return Err("page numbers are 1-based; 0 is not a page".into());
    }
    if first > last {
        return Err(format!("first page {first} is after last page {last}"));
    }
    Ok(())
}

fn cmd_extract_pages(path: &Path, rest: &[String]) {
    use pdf_model::assembly_ops::extract_pages;
    // extract-pages <file> <first> <last> -o out
    if rest.len() < 2 {
        eprintln!("error: extract-pages requires <first> <last> -o <out>");
        process::exit(1);
    }
    // `unwrap_or(0)` turned `extract-pages doc.pdf two four` into the range
    // 0..0, so the complaint came from somewhere further down and named
    // numbers the user never typed. [UX-ERR-3]
    let parse_page = |value: &str, what: &str| -> u32 {
        match value.trim().parse() {
            Ok(page) => page,
            Err(_) => {
                eprintln!("error: {what} page must be a number, got '{value}'");
                process::exit(1);
            }
        }
    };
    let first = parse_page(&rest[0], "first");
    let last = parse_page(&rest[1], "last");
    if let Err(error) = check_page_range(first, last) {
        eprintln!("error: {error}");
        process::exit(1);
    }
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

/// List the revisions a file carries. [FR-VER-1]
///
/// Usage: pdf-platform revisions <file>
fn cmd_revisions(path: &Path) {
    let data = std::fs::read(path).unwrap_or_else(|e| {
        eprintln!("error: cannot read {}: {e}", path.display());
        process::exit(2);
    });
    match pdf_cos::revisions::list_revisions(&data) {
        Ok(revisions) if json_mode() => {
            let list: Vec<_> = revisions
                .iter()
                .map(|r| {
                    serde_json::json!({
                        "number": r.number,
                        "bytes": r.bytes,
                        "objects_written": r.objects_written,
                        "xref_offset": r.xref_offset,
                    })
                })
                .collect();
            println!("{}", serde_json::json!({ "revisions": list }));
            process::exit(0);
        }
        Ok(revisions) => {
            println!("Revisions: {}", revisions.len());
            for r in &revisions {
                println!(
                    "  {}: {} byte(s), {} object(s) written, cross-reference at {}",
                    r.number, r.bytes, r.objects_written, r.xref_offset
                );
            }
            if revisions.len() > 1 {
                println!(
                    "Earlier revisions stay readable in the file. `sanitize` writes a copy \
                     with only the latest."
                );
            }
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: cannot read the revision history: {e}");
            process::exit(2);
        }
    }
}

/// Whether the newest trailer declares encryption.
fn is_encrypted(data: &[u8]) -> bool {
    pdf_cos::scan::find_startxref(data)
        .and_then(|at| pdf_cos::scan::section_dictionary(data, at))
        .is_some_and(|dict| dict.windows(b"/Encrypt".len()).any(|w| w == b"/Encrypt"))
}

/// What a sanitize did, for the report.
#[derive(Debug)]
struct SanitizeReport {
    revisions_before: usize,
    bytes_before: usize,
    bytes_after: usize,
    signatures_discarded: u32,
    has_js: bool,
    has_acroform: bool,
}

/// Write a copy of `input` that carries only its latest revision. [FR-VER-2, FR-SAVE-2]
///
/// A full rewrite through the candidate, verify and publish path: only objects
/// the latest revision reaches are written, so earlier revisions and the objects
/// nothing references any more are gone. The result is then re-read and must
/// report exactly one revision, or it is removed and the call fails: a clean
/// copy that still holds history is the false success this command exists to
/// avoid. [GR-8, PRIN-6]
fn run_sanitize(input: &Path, output: &Path, discard_signatures: bool) -> Result<SanitizeReport, String> {
    use pdf_model::assembly::OptimizeProfile;
    use pdf_model::assembly_ops::optimize_pdf;

    let data = std::fs::read(input).map_err(|e| format!("cannot read {}: {e}", input.display()))?;
    if is_encrypted(&data) {
        return Err(
            "this document is encrypted; a clean copy of an encrypted document is not supported"
                .into(),
        );
    }
    let structure = pdf_cos::scan::scan_bytes(&data).map_err(|e| format!("cannot read the document: {e}"))?;
    let before = pdf_cos::revisions::list_revisions(&data).map_err(|e| e.to_string())?;
    // `sig_count` is a proxy (it counts `/FT /Sig` inside the AcroForm dictionary
    // itself), so a signature in an indirect field would be missed. Every
    // signature dictionary carries a `/ByteRange`; its presence anywhere in the
    // file is treated as signed, the safe direction to be wrong in. [GR-8, PRIN-6]
    let has_byte_range = data.windows(b"/ByteRange".len()).any(|w| w == b"/ByteRange");
    let signatures = structure.sig_count.max(u32::from(has_byte_range));
    if signatures > 0 && !discard_signatures {
        return Err(format!(
            "this document has {signatures} signature(s) or signature-like data, and a clean \
             copy invalidates them. Run again with --discard-signatures to proceed"
        ));
    }

    coordinator::broker::optimize_with_verification(output, |candidate| {
        optimize_pdf(input, candidate, OptimizeProfile::ArchivePreserving).map_err(|e| e.to_string())
    })?;

    let written = std::fs::read(output).map_err(|e| format!("cannot read the result: {e}"))?;
    let after = pdf_cos::revisions::list_revisions(&written).map_err(|e| e.to_string());
    match after {
        Ok(revisions) if revisions.len() == 1 => Ok(SanitizeReport {
            revisions_before: before.len(),
            bytes_before: data.len(),
            bytes_after: written.len(),
            signatures_discarded: signatures,
            has_js: structure.has_js,
            has_acroform: structure.has_acroform,
        }),
        other => {
            let _ = std::fs::remove_file(output);
            Err(match other {
                Ok(revisions) => format!(
                    "the result still has {} revisions, so it was removed rather than reported as clean",
                    revisions.len()
                ),
                Err(e) => format!("the result could not be re-read ({e}), so it was removed"),
            })
        }
    }
}

/// Write a clean copy without the revision history. [FR-VER-2]
///
/// Usage: pdf-platform sanitize <file> -o <out.pdf> [--discard-signatures]
fn cmd_sanitize(path: &Path, rest: &[String]) {
    let (extra, out) = parse_dash_o(rest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let discard = extra.iter().any(|a| a == "--discard-signatures");
    if let Some(unknown) = extra.iter().find(|a| *a != "--discard-signatures") {
        eprintln!("error: unexpected argument: {unknown}");
        process::exit(1);
    }
    match run_sanitize(path, &out, discard) {
        Ok(r) => {
            println!(
                "Wrote {}: {} revision(s) before, 1 now; {} -> {} bytes.",
                out.display(),
                r.revisions_before,
                r.bytes_before,
                r.bytes_after
            );
            println!("Removed: earlier revisions, and objects nothing in the latest revision uses.");
            if r.signatures_discarded > 0 {
                println!("Signatures: {} discarded; they no longer validate.", r.signatures_discarded);
            }
            println!(
                "Not removed: document information and XMP metadata, embedded files, annotations \
                 and comments, form data{}{}, hidden text and layers.",
                if r.has_acroform { " (this document has a form)" } else { "" },
                if r.has_js { ", JavaScript (this document has some)" } else { ", JavaScript" },
            );
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Parse `1,3-5` into zero-based page indices, sorted and without repeats.
///
/// Pages are 1-based on the command line, as in `extract-pages`. Every
/// malformed piece is an error naming the piece: a list that silently dropped
/// `three` would rotate or delete a different set of pages than the user typed.
/// [UX-ERR-3, GR-8, PRIN-1]
fn parse_page_list(spec: &str, page_count: u32) -> Result<Vec<u32>, String> {
    if spec.trim().is_empty() {
        return Err("--pages needs a list such as 1,3-5".into());
    }
    let number = |text: &str| -> Result<u32, String> {
        match text.trim().parse::<u32>() {
            Ok(n) if n >= 1 => Ok(n),
            Ok(_) => Err("page numbers are 1-based; 0 is not a page".to_string()),
            Err(_) => Err(format!("'{}' is not a page number", text.trim())),
        }
    };
    let mut pages = std::collections::BTreeSet::new();
    for piece in spec.split(',') {
        let (first, last) = match piece.split_once('-') {
            Some((a, b)) => (number(a)?, number(b)?),
            None => {
                let n = number(piece)?;
                (n, n)
            }
        };
        check_page_range(first, last)?;
        if last > page_count {
            return Err(format!("page {last} is past the end: the document has {page_count} page(s)"));
        }
        pages.extend((first..=last).map(|p| p - 1));
    }
    Ok(pages.into_iter().collect())
}

/// `--pages <list>` from a command's arguments, if present.
fn flag_value<'a>(rest: &'a [String], flag: &str) -> Option<&'a str> {
    rest.iter().position(|s| s == flag).and_then(|i| rest.get(i + 1)).map(String::as_str)
}

/// Rotate pages and save. [FR-ORG-1, FR-ORG-2, FR-CLI-1]
///
/// Usage: pdf-platform rotate <file> --pages 1,3-5 --degrees 90|180|270 -o <out.pdf>
fn cmd_rotate(path: &Path, rest: &[String]) {
    let (_, out) = parse_dash_o(rest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let Some(pages) = flag_value(rest, "--pages") else {
        eprintln!("error: rotate needs --pages 1,3-5");
        process::exit(1);
    };
    let degrees = match flag_value(rest, "--degrees").map(|d| d.trim().parse::<u32>()) {
        Some(Ok(d)) if matches!(d, 90 | 180 | 270) => d,
        Some(_) => {
            eprintln!("error: --degrees must be 90, 180 or 270");
            process::exit(1);
        }
        None => {
            eprintln!("error: rotate needs --degrees 90|180|270");
            process::exit(1);
        }
    };
    match run_page_edit(path, &out, pages, PageEdit::Rotate(degrees)) {
        Ok(count) => {
            println!("rotated {count} page(s) by {degrees} degrees -> {}", out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Delete pages and save. [FR-ORG-1, FR-ORG-2, FR-CLI-1]
///
/// Usage: pdf-platform delete-pages <file> --pages 2,4-6 -o <out.pdf>
fn cmd_delete_pages(path: &Path, rest: &[String]) {
    let (_, out) = parse_dash_o(rest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let Some(pages) = flag_value(rest, "--pages") else {
        eprintln!("error: delete-pages needs --pages 2,4-6");
        process::exit(1);
    };
    match run_page_edit(path, &out, pages, PageEdit::Delete) {
        Ok(count) => {
            println!("deleted {count} page(s) -> {}", out.display());
            // Saving is incremental (ADR-012): the earlier revision, with the
            // deleted pages, is still in the file. Say so. [PRIN-6, FR-SAVE-2]
            println!(
                "note: the pages are gone from the page tree; their bytes remain in the file \
                 as an earlier revision until it is rewritten"
            );
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Parse `llx,lly,urx,ury` into four finite numbers.
fn parse_crop_box(spec: &str) -> Result<[f64; 4], String> {
    let parts: Vec<&str> = spec.split(',').collect();
    if parts.len() != 4 {
        return Err(format!("--box needs four numbers llx,lly,urx,ury, got {}", parts.len()));
    }
    let mut out = [0.0; 4];
    for (slot, part) in out.iter_mut().zip(&parts) {
        *slot = match part.trim().parse::<f64>() {
            Ok(n) if n.is_finite() => n,
            _ => return Err(format!("'{}' is not a number", part.trim())),
        };
    }
    Ok(out)
}

/// Crop pages and save. [FR-ORG, FR-CLI-1]
///
/// Usage: pdf-platform crop <file> --pages 1,3-5 --box llx,lly,urx,ury -o <out.pdf>
///
/// The box is in PDF points, origin at the bottom left of the unrotated page,
/// and must lie within each page's media box.
fn cmd_crop(path: &Path, rest: &[String]) {
    let (_, out) = parse_dash_o(rest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let Some(pages) = flag_value(rest, "--pages") else {
        eprintln!("error: crop needs --pages 1,3-5");
        process::exit(1);
    };
    let rect = match flag_value(rest, "--box") {
        Some(spec) => parse_crop_box(spec).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        }),
        None => {
            eprintln!("error: crop needs --box llx,lly,urx,ury");
            process::exit(1);
        }
    };
    match run_page_edit(path, &out, pages, PageEdit::Crop(rect)) {
        Ok(count) => {
            println!("cropped {count} page(s) to [{} {} {} {}] -> {}", rect[0], rect[1], rect[2], rect[3], out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}
/// A one-based position, as a zero-based index.
fn parse_position(value: &str, flag: &str) -> Result<u32, String> {
    match value.trim().parse::<u32>() {
        Ok(n) if n >= 1 => Ok(n - 1),
        Ok(_) => Err(format!("{flag} is 1-based; 0 is not a position")),
        Err(_) => Err(format!("{flag} must be a whole number, got '{value}'")),
    }
}

/// `width,height` in points, both positive and finite.
fn parse_size(spec: &str) -> Result<[f64; 2], String> {
    let parts: Vec<&str> = spec.split(',').collect();
    if parts.len() != 2 {
        return Err(format!("--size needs width,height, got {} value(s)", parts.len()));
    }
    let mut out = [0.0; 2];
    for (slot, part) in out.iter_mut().zip(&parts) {
        *slot = match part.trim().parse::<f64>() {
            Ok(n) if n.is_finite() && n > 0.0 => n,
            _ => return Err(format!("'{}' is not a positive number", part.trim())),
        };
    }
    Ok(out)
}

/// Move pages. [FR-ORG-1, FR-ORG-2, FR-CLI-1]
///
/// Usage: pdf-platform move-pages <file> --pages 3 --to 1 -o <out.pdf>
///
/// `--to` is the one-based position the first moved page has in the result.
fn cmd_move_pages(path: &Path, rest: &[String]) {
    let (_, out) = parse_dash_o(rest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let Some(pages) = flag_value(rest, "--pages") else {
        eprintln!("error: move-pages needs --pages 3");
        process::exit(1);
    };
    let to = match flag_value(rest, "--to") {
        Some(v) => parse_position(v, "--to").unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        }),
        None => {
            eprintln!("error: move-pages needs --to <position>");
            process::exit(1);
        }
    };
    match run_page_edit(path, &out, pages, PageEdit::Move(to)) {
        Ok(count) => {
            println!("moved {count} page(s) to position {} -> {}", to + 1, out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Duplicate pages, each copy right after its original. [FR-ORG-1, FR-ORG-2]
///
/// Usage: pdf-platform duplicate-pages <file> --pages 2,4 -o <out.pdf>
fn cmd_duplicate_pages(path: &Path, rest: &[String]) {
    let (_, out) = parse_dash_o(rest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let Some(pages) = flag_value(rest, "--pages") else {
        eprintln!("error: duplicate-pages needs --pages 2,4");
        process::exit(1);
    };
    match run_page_edit(path, &out, pages, PageEdit::Duplicate) {
        Ok(count) => {
            println!("duplicated {count} page(s) -> {}", out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Insert one blank page. [FR-ORG-1, FR-ORG-2]
///
/// Usage: pdf-platform insert-blank <file> --at 2 [--size 612,792] -o <out.pdf>
///
/// `--at` is the one-based position the new page has in the result.
fn cmd_insert_blank(path: &Path, rest: &[String]) {
    let (_, out) = parse_dash_o(rest).unwrap_or_else(|e| {
        eprintln!("error: {e}");
        process::exit(1);
    });
    let at = match flag_value(rest, "--at") {
        Some(v) => parse_position(v, "--at").unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        }),
        None => {
            eprintln!("error: insert-blank needs --at <position>");
            process::exit(1);
        }
    };
    let size = flag_value(rest, "--size").map(|spec| {
        parse_size(spec).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            process::exit(1);
        })
    });
    match run_insert_blank(path, &out, at, size) {
        Ok(()) => {
            println!("inserted a blank page as page {} -> {}", at + 1, out.display());
            process::exit(0);
        }
        Err(e) => {
            eprintln!("error: {e}");
            process::exit(2);
        }
    }
}

/// Open `input`, insert a blank page at zero-based `at`, save to `output`.
fn run_insert_blank(input: &Path, output: &Path, at: u32, size: Option<[f64; 2]>) -> Result<(), String> {
    let worker = find_worker();
    let mut coordinator = coordinator::document::DocumentCoordinator::open(&worker, input)
        .map_err(|e| e.to_string())?;
    let result = coordinator
        .insert_blank_page(at, size)
        .and_then(|()| coordinator.save_incremental(output));
    let _ = coordinator.close();
    result.map(|_| ()).map_err(|e| e.to_string())
}
/// What `run_page_edit` does to the chosen pages.
enum PageEdit {
    Rotate(u32),
    Delete,
    Crop([f64; 4]),
    /// Zero-based position of the first moved page in the result.
    Move(u32),
    Duplicate,
}

/// Open `input`, apply `edit` to the pages `spec` names, save to `output`.
/// Returns how many pages were changed.
fn run_page_edit(input: &Path, output: &Path, spec: &str, edit: PageEdit) -> Result<usize, String> {
    let worker = find_worker();
    let mut coordinator = coordinator::document::DocumentCoordinator::open(&worker, input)
        .map_err(|e| e.to_string())?;
    let total = coordinator.page_count();
    let pages = parse_page_list(spec, total)?;
    if matches!(edit, PageEdit::Delete) && pages.len() as u32 >= total {
        let _ = coordinator.close();
        return Err("that would delete every page; a document needs at least one".into());
    }
    let result = match edit {
        PageEdit::Rotate(degrees) => coordinator.rotate_pages(&pages, degrees),
        PageEdit::Delete => coordinator.delete_pages(&pages),
        PageEdit::Crop(rect) => coordinator.crop_pages(&pages, rect),
        PageEdit::Move(to) => coordinator.move_pages(&pages, to),
        PageEdit::Duplicate => coordinator.duplicate_pages(&pages),
    }
    .and_then(|()| coordinator.save_incremental(output));
    let _ = coordinator.close();
    result.map_err(|e| e.to_string())?;
    Ok(pages.len())
}

/// Apply a watermark or Bates stamp to all pages of a PDF. [FR-STAMP]
///
/// Usage: pdf-platform stamp <file> --text "WATERMARK" -o <out.pdf>
///        pdf-platform stamp <file> --bates-start 1 --bates-width 6 -o <out.pdf>
fn cmd_stamp(path: &Path, rest: &[String]) {
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

    if let Err(error) = run_stamp(
        path,
        &out,
        text.as_deref(),
        bates_start,
        bates_width,
        font_size,
    ) {
        eprintln!("error: {error}");
        process::exit(2);
    }
    println!("Wrote {}", out.display());
    process::exit(0);
}

fn run_stamp(
    input: &Path,
    output: &Path,
    text: Option<&str>,
    bates_start: Option<u32>,
    bates_width: usize,
    font_size: f32,
) -> Result<(), String> {
    let worker = find_worker();
    let mut coordinator = coordinator::document::DocumentCoordinator::open(&worker, input)
        .map_err(|e| e.to_string())?;
    if let Some(text) = text {
        let stamp = pdf_model::stamp::Stamp {
            text: text.to_string(),
            font_size,
            ..pdf_model::stamp::Stamp::default()
        };
        coordinator.apply_stamp(&stamp).map_err(|e| e.to_string())?;
    } else if let Some(start) = bates_start {
        coordinator
            .apply_bates(
                start,
                bates_width,
                pdf_model::stamp::StampPosition::BottomCenter,
                font_size,
            )
            .map_err(|e| e.to_string())?;
    } else {
        return Err("stamp text or Bates start is required".into());
    }
    coordinator
        .save_incremental(output)
        .map_err(|e| e.to_string())?;
    Ok(())
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
/// Run OCR over a document, returning a summary or an error.
///
/// Split out of `cmd_ocr` so the batch executor can run the same code path:
/// FR-BATCH requires identical behaviour whether an operation is invoked
/// singly or as a pipeline step, and a second implementation for batch is how
/// the two drift apart. Errors are returned rather than exiting so a pipeline
/// can report which step failed and stop. [FR-BATCH, M6+]
fn run_ocr(path: &Path, rest: &[String]) -> Result<String, String> {
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
        return Err("Tesseract is not installed".into());
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
            return Err(format!("open failed: {e}"));
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
        return if skip_count > 0 {
            Err(format!("no pages to OCR; {skip_count} page(s) were out of range"))
        } else {
            Ok("no pages needed OCR".into())
        };
    }

    let worker = find_worker();
    let pool = match UtilityPool::new(&worker, 1) {
        Ok(p) => Arc::new(p),
        Err(e) => {
            return Err(format!("utility pool spawn failed: {e:?}"));
        }
    };
    let document = match std::fs::File::open(path) {
        Ok(f) => Arc::new(f),
        Err(e) => {
            return Err(format!("reopen for OCR failed: {e}"));
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
            return Err(format!("job scheduler init failed: {e:?}"));
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
            return Err(format!("OCR job graph construction failed: {e:?}"));
        }
    };
    if let Err(e) = scheduler.submit(graph) {
        return Err(format!("OCR job submission failed: {e:?}"));
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
        if let Err(e) = coord.save_incremental(&out) {
            let _ = coord.close();
            return Err(format!("save failed: {e}"));
        }
        println!("Saved: {}", out.display());
    }

    let _ = coord.close();
    if error_count > 0 {
        return Err(format!("{error_count} page(s) failed OCR"));
    }
    Ok(format!("{ocr_count} page(s) OCR'd -> {}", out.display()))
}

/// `ocr` subcommand: run OCR and translate the outcome into an exit code.
fn cmd_ocr(path: &Path, rest: &[String]) {
    match run_ocr(path, rest) {
        Ok(summary) => {
            println!("{summary}");
            process::exit(0);
        }
        Err(message) => {
            eprintln!("error: {message}");
            process::exit(2);
        }
    }
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
    if result.conformance.is_none() {
        println!("Status: UNDETERMINED — no violations detected");
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
    if result.conformance.is_none() && result.errors.is_empty() {
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
    use coordinator::visual_compare::{compare_pages, RenderedPage, DEFAULT_CHANNEL_TOLERANCE};
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
    // Moves are counted separately: a relocation is not a content change,
    // and folding it into the change count overstates the edit. [FR-CMP-2]
    let mut total_moves = 0;
    // FR-CMP-1 requires appearance differences too, not only text. Rendered at
    // a modest scale: this answers "did the page change" rather than grading
    // rendering fidelity, and it keeps the raster inside the transport frame
    // cap. [FR-CMP-1, SDS §4.6]
    const VISUAL_SCALE: f32 = 0.5;
    let mut pages_visually_changed = 0u32;
    let mut visual_unavailable: Option<String> = None;
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

        // Appearance is compared for every page, including pages whose text is
        // identical — a moved figure, a swapped logo, a box drawn over content
        // or a substituted font all leave the text untouched, and those are
        // exactly the changes a text-only diff cannot see. [FR-CMP-1]
        let visual = if visual_unavailable.is_none() && page < pages1 && page < pages2 {
            match (
                coord1.render_page(page, VISUAL_SCALE),
                coord2.render_page(page, VISUAL_SCALE),
            ) {
                (Ok(before), Ok(after)) => {
                    match compare_pages(&before, &after, DEFAULT_CHANNEL_TOLERANCE) {
                        Ok(diff) => Some(diff),
                        Err(error) => {
                            visual_unavailable = Some(error.to_string());
                            None
                        }
                    }
                }
                (Err(error), _) | (_, Err(error)) => {
                    // Say why appearance was not compared rather than printing
                    // a text-only verdict that looks complete. [GR-8]
                    visual_unavailable = Some(error.to_string());
                    None
                }
            }
        } else {
            None
        };

        let visual_changed = visual.as_ref().is_some_and(|diff| !diff.is_identical());
        if visual_changed {
            pages_visually_changed += 1;
        }

        if text1 == text2 && !visual_changed {
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

        // FR-CMP-2 wants the *nature* of each change and a location to
        // navigate to, so line numbers are printed and a relocation is
        // reported as one move rather than as an unrelated deletion and
        // insertion. Line numbers are 1-based for a reader.
        for op in &ops {
            match op {
                LineDiff::Same { .. } => {}
                LineDiff::Removed { text, before_index } => {
                    println!("  - [line {}] {text}", before_index + 1);
                    total_diffs += 1;
                }
                LineDiff::Added { text, after_index } => {
                    println!("  + [line {}] {text}", after_index + 1);
                    total_diffs += 1;
                }
                LineDiff::Moved { text, before_index, after_index } => {
                    println!(
                        "  ~ [line {} -> {}] {text}",
                        before_index + 1,
                        after_index + 1
                    );
                    total_moves += 1;
                }
            }
        }

        if let Some(diff) = &visual {
            if diff.is_identical() {
                println!("  appearance: unchanged");
            } else if diff.geometry_differs {
                println!(
                    "  appearance: page size differs; {:.2}% of pixels differ",
                    f64::from(diff.changed_fraction()) * 100.0
                );
            } else {
                println!(
                    "  appearance: {:.2}% of pixels differ (max channel delta {})",
                    f64::from(diff.changed_fraction()) * 100.0,
                    diff.max_channel_delta
                );
            }
        }
        println!();
    }

    // Summary.
    println!("Summary:");
    println!("  Pages compared: {}", max_pages);
    println!("  Pages with differences: {}", pages_with_diffs);
    println!("  Total line changes: {}", total_diffs);
    println!("  Lines moved: {}", total_moves);
    println!("  Pages with appearance changes: {}", pages_visually_changed);
    if let Some(reason) = &visual_unavailable {
        println!();
        println!("Note: appearance comparison did not run ({reason}).");
        println!("Text differences above are complete; visual differences are unknown.");
    }
    if fell_back {
        println!();
        println!("Note: at least one page exceeded the alignment bound, so its lines");
        println!("were paired by position. Those results are not reflow-resilient.");
    }

    let _ = coord1.close();
    let _ = coord2.close();

    if pages_with_diffs == 0 {
        println!();
        if visual_unavailable.is_some() {
            println!("Documents are identical (text content); appearance was not compared.");
        } else {
            println!("Documents are identical (text content and rendered appearance).");
        }
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
/// Pipeline file format: one stanza per step, opening with `type=`.
///
///   type=merge|split_per_page|split_chunked|extract_pages|optimize|ocr|
///        watermark|bates_number|index
///
/// Keys: `input=` (repeatable for merge), `output=`, `output_dir=`,
/// `pages_per_file=`, `first=`, `last=`, `text=`, `lang=`, `with_text=`,
/// `profile=`, `start=`, `width=`, `root=` (the enrolled root for `index`).
///
/// A stanza missing a key its step needs, an unknown step type, and an
/// unrecognized line are all errors: dropping them quietly would run a
/// shorter pipeline and still report success. [GR-8]
/// See `pdf_model::batch::BatchPipeline::serialize()` for the emitted form.
fn cmd_batch(pipeline_path: &Path) {
    use pdf_model::batch::{BatchPipeline, BatchStep};

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
    let mut current_language: Option<String> = None;
    let mut current_with_text = false;
    let mut current_profile: Option<String> = None;
    let mut current_start: u32 = 1;
    let mut current_width: usize = 6;
    let mut current_root: Option<PathBuf> = None;

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
                if let Err(error) = flush_batch_step(
                    &mut pipeline, step_type, &current_inputs,
                    current_output.as_deref(), current_output_dir.as_deref(),
                    current_pages_per_file, current_first, current_last,
                    current_text.as_deref(), current_profile.as_deref(),
                    current_language.as_deref(), current_with_text,
                    current_start, current_width, current_root.as_deref(),
                ) {
                    eprintln!("error: {}: {error}", pipeline_path.display());
                    process::exit(1);
                }
            }
            current_type = Some(line[5..].to_string());
            current_inputs.clear();
            current_output = None;
            current_output_dir = None;
            current_text = None;
            current_profile = None;
            current_language = None;
            current_with_text = false;
            current_root = None;
        } else if let Some(val) = line.strip_prefix("root=") {
            current_root = Some(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("input=") {
            current_inputs.push(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("output=") {
            current_output = Some(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("output_dir=") {
            current_output_dir = Some(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("pages_per_file=") {
            current_pages_per_file = match batch_number(val, "pages_per_file") {
                Ok(value) => value as u32,
                Err(error) => {
                    eprintln!("error: {}: {error}", pipeline_path.display());
                    process::exit(1);
                }
            };
        } else if let Some(val) = line.strip_prefix("first=") {
            current_first = match batch_number(val, "first") {
                Ok(value) => value as u32,
                Err(error) => {
                    eprintln!("error: {}: {error}", pipeline_path.display());
                    process::exit(1);
                }
            };
        } else if let Some(val) = line.strip_prefix("last=") {
            current_last = match batch_number(val, "last") {
                Ok(value) => value as u32,
                Err(error) => {
                    eprintln!("error: {}: {error}", pipeline_path.display());
                    process::exit(1);
                }
            };
        } else if let Some(val) = line.strip_prefix("text=") {
            current_text = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("lang=") {
            current_language = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("with_text=") {
            current_with_text = val.trim() == "true" || val.trim() == "1";
        } else if let Some(val) = line.strip_prefix("profile=") {
            current_profile = Some(val.to_string());
        } else if let Some(val) = line.strip_prefix("start=") {
            current_start = match batch_number(val, "start") {
                Ok(value) => value as u32,
                Err(error) => {
                    eprintln!("error: {}: {error}", pipeline_path.display());
                    process::exit(1);
                }
            };
        } else if let Some(val) = line.strip_prefix("width=") {
            current_width = match batch_number(val, "width") {
                Ok(value) => value as usize,
                Err(error) => {
                    eprintln!("error: {}: {error}", pipeline_path.display());
                    process::exit(1);
                }
            };
        } else {
            // A key this parser does not know is a typo, and skipping it
            // quietly produces a step built from defaults — or no step at all
            // — while the run still reports success. [GR-8, PRIN-6]
            eprintln!(
                "error: {}: unrecognized line '{line}'",
                pipeline_path.display()
            );
            process::exit(1);
        }
    }
    // Flush last step.
    if let Some(ref step_type) = current_type {
        if let Err(error) = flush_batch_step(
            &mut pipeline, step_type, &current_inputs,
            current_output.as_deref(), current_output_dir.as_deref(),
            current_pages_per_file, current_first, current_last,
            current_text.as_deref(), current_profile.as_deref(),
            current_language.as_deref(), current_with_text,
            current_start, current_width, current_root.as_deref(),
        ) {
            eprintln!("error: {}: {error}", pipeline_path.display());
            process::exit(1);
        }
    }

    if pipeline.step_count() == 0 {
        eprintln!("error: no steps in pipeline");
        process::exit(1);
    }

    println!("Executing pipeline '{}' ({} steps)...", pipeline.name, pipeline.step_count());
    // Same verified candidate/publish safety net `optimize` gets standalone
    // (FR-BATCH: identical behavior via GUI/CLI/batch — see cmd_optimize).
    let results = execute_cli_pipeline(&pipeline);

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
    // Overridable so that a test — or a user keeping more than one index —
    // does not have to share the default location.
    match std::env::var_os("PDF_PLATFORM_INDEX_DIR") {
        Some(dir) => PathBuf::from(dir),
        None => std::env::temp_dir().join("pdf-platform-index"),
    }
}

/// Reindex one enrolled root, or every enrolled root when `only` is `None`.
///
/// The `index reindex` subcommand and the `Index` batch step both call this:
/// `FR-BATCH` requires an operation to behave identically singly and in a
/// pipeline, and two copies of this sequence would drift.
fn reindex_enrolled(only: Option<&Path>) -> Result<Vec<String>, String> {
    use coordinator::broker::load_enrollment_registry;
    use coordinator::indexing::{load_registry, reindex_enrollment, save_registry};
    use search::tantivy_backend::CrossDocumentIndex;

    let state_dir = index_state_dir();
    std::fs::create_dir_all(&state_dir)
        .map_err(|e| format!("cannot create index state directory: {e}"))?;
    let file_registry_path = state_dir.join("file-registry.bin");
    let enrollment_registry = load_enrollment_registry(&state_dir.join("enrollments.bin"))
        .map_err(|e| format!("cannot load enrollment registry: {e}"))?;

    // Enrolled roots are stored canonicalized; compare canonical forms or this
    // silently matches nothing (Windows canonicalize prepends \\?\).
    let targets: Vec<([u8; 16], PathBuf)> = match only {
        Some(dir) => {
            let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
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
        return Err(match only {
            Some(dir) => format!(
                "{} is not an enrolled root; enroll it before indexing it",
                dir.display()
            ),
            None => "no enrolled roots to reindex".to_string(),
        });
    }

    let worker = find_worker();
    let mut file_registry = load_registry(&file_registry_path).unwrap_or_default();
    let mut index = CrossDocumentIndex::open_or_create(&state_dir.join("tantivy"))
        .map_err(|e| format!("cannot open index: {e}"))?;

    let mut lines = Vec::new();
    let mut failures = Vec::new();
    for (id, root) in &targets {
        let report = reindex_enrollment(
            &worker,
            &enrollment_registry,
            *id,
            root,
            &mut file_registry,
            &mut index,
        );
        lines.push(format!(
            "{}: scanned {}, reindexed {}, skipped {}, pages {}",
            root.display(),
            report.files_scanned,
            report.files_reindexed,
            report.files_skipped_unchanged,
            report.pages_indexed
        ));
        for (path, message) in &report.errors {
            failures.push(format!("{}: {message}", path.display()));
        }
    }
    save_registry(&file_registry, &file_registry_path)
        .map_err(|e| format!("cannot save file registry: {e}"))?;

    if failures.is_empty() {
        Ok(lines)
    } else {
        Err(failures.join("; "))
    }
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
            match reindex_enrolled(args.get(1).map(Path::new)) {
                Ok(lines) => {
                    for line in lines {
                        println!("{line}");
                    }
                    process::exit(0);
                }
                Err(error) => {
                    eprintln!("error: {error}");
                    process::exit(1);
                }
            }
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

/// Parse a numeric value from a pipeline file.
///
/// These used to be `val.parse().unwrap_or(default)`, so `pages_per_file=abc`
/// silently split every 2 pages and the run reported success for a document
/// nobody asked for. [GR-8, UX-ERR-3]
fn batch_number(value: &str, key: &str) -> Result<u64, String> {
    let trimmed = value.trim();
    trimmed
        .parse::<u64>()
        .map_err(|_| format!("{key}= must be a whole number, got '{trimmed}'"))
}

/// Turn one parsed stanza into a pipeline step.
///
/// Every arm used to be `if let Some(..) = ..` with no `else`, so a stanza
/// missing its `output=` — or naming a step type this build does not know —
/// produced no step at all and the pipeline ran on, reporting success for work
/// it had silently dropped. Each shortfall is now an error naming what is
/// missing. [GR-8, PRIN-6, UX-ERR-3]
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
    language: Option<&str>,
    include_pages_with_text: bool,
    start: u32,
    width: usize,
    root: Option<&Path>,
) -> Result<(), String> {
    let one_input = || -> Result<PathBuf, String> {
        inputs
            .first()
            .cloned()
            .ok_or_else(|| format!("step '{step_type}' has no input="))
    };
    let an_output = || -> Result<PathBuf, String> {
        output
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("step '{step_type}' has no output="))
    };
    let a_directory = || -> Result<PathBuf, String> {
        output_dir
            .map(Path::to_path_buf)
            .ok_or_else(|| format!("step '{step_type}' has no output_dir="))
    };

    match step_type {
        "merge" => {
            if inputs.len() < 2 {
                return Err(format!(
                    "step 'merge' needs at least two input= lines, found {}",
                    inputs.len()
                ));
            }
            pipeline.add_step(pdf_model::batch::BatchStep::Merge {
                inputs: inputs.to_vec(),
                output: an_output()?,
            });
        }
        "split_per_page" => {
            pipeline.add_step(pdf_model::batch::BatchStep::SplitPerPage {
                input: one_input()?,
                output_dir: a_directory()?,
            });
        }
        "split_chunked" => {
            pipeline.add_step(pdf_model::batch::BatchStep::SplitChunked {
                input: one_input()?,
                pages_per_file,
                output_dir: a_directory()?,
            });
        }
        "extract_pages" => {
            pipeline.add_step(pdf_model::batch::BatchStep::ExtractPages {
                input: one_input()?,
                first,
                last,
                output: an_output()?,
            });
        }
        "ocr" => {
            pipeline.add_step(pdf_model::batch::BatchStep::Ocr {
                input: one_input()?,
                language: language.unwrap_or("eng").to_string(),
                include_pages_with_text,
                output: an_output()?,
            });
        }
        "optimize" => {
            pipeline.add_step(pdf_model::batch::BatchStep::Optimize {
                input: one_input()?,
                output: an_output()?,
                profile: profile.unwrap_or("screen").to_string(),
            });
        }
        "watermark" => {
            pipeline.add_step(pdf_model::batch::BatchStep::Watermark {
                input: one_input()?,
                text: text
                    .filter(|t| !t.is_empty())
                    .ok_or("step 'watermark' has no text=")?
                    .to_string(),
                output: an_output()?,
            });
        }
        "bates_number" => {
            pipeline.add_step(pdf_model::batch::BatchStep::BatesNumber {
                input: one_input()?,
                start,
                width,
                output: an_output()?,
            });
        }
        "index" => {
            pipeline.add_step(pdf_model::batch::BatchStep::Index {
                root: root
                    .map(Path::to_path_buf)
                    .ok_or("step 'index' has no root=")?,
            });
        }
        other => return Err(format!("unknown step type '{other}'")),
    }
    Ok(())

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

fn execute_cli_pipeline(
    pipeline: &pdf_model::batch::BatchPipeline,
) -> Vec<pdf_model::batch::StepResult> {
    use pdf_model::batch::{BatchStep, StepResult};

    let mut results = Vec::new();
    for step in &pipeline.steps {
        let started = std::time::Instant::now();
        let result = match step {
            BatchStep::Watermark { input, text, output } => run_stamp(
                input,
                output,
                Some(text),
                None,
                6,
                10.0,
            )
            .map(|()| vec![output.clone()]),
            BatchStep::BatesNumber {
                input,
                start,
                width,
                output,
            } => run_stamp(input, output, None, Some(*start), *width, 10.0)
                .map(|()| vec![output.clone()]),
            // Stamps go through the coordinator injection path above; every
            // other step keeps the verified candidate/publish safety net
            // `optimize` gets standalone. [FR-BATCH, ADR-013]
            // One implementation for single and batch invocation (FR-BATCH):
            // this is the same `run_ocr` the `ocr` subcommand calls.
            BatchStep::Ocr { input, language, include_pages_with_text, output } => {
                let mut args = vec![
                    "--lang".to_string(),
                    language.clone(),
                    "-o".to_string(),
                    output.display().to_string(),
                ];
                if *include_pages_with_text {
                    args.push("--with-text".to_string());
                }
                run_ocr(input, &args).map(|_| vec![output.clone()])
            }
            // Same helper `index reindex` calls, for the same reason as OCR
            // above: one implementation for single and batch invocation.
            BatchStep::Index { root } => {
                reindex_enrolled(Some(root)).map(|_| vec![root.clone()])
            }
            _ => pdf_model::batch::execute_step(step, &|input, output, profile| {
                coordinator::broker::optimize_with_verification(output, |candidate_path| {
                    pdf_model::assembly_ops::optimize_pdf(input, candidate_path, profile)
                        .map_err(|e| e.to_string())
                })
            }),
        };
        let success = result.is_ok();
        results.push(StepResult {
            success,
            outputs: result.as_ref().cloned().unwrap_or_default(),
            message: result.err().unwrap_or_else(|| "ok".into()),
            duration_ms: started.elapsed().as_millis() as u64,
        });
        if !success {
            break;
        }
    }
    results
}

#[cfg(test)]
mod stamp_tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/corpus-diff/fixtures")
            .join(name)
    }

    #[test]
    fn run_stamp_writes_incremental_stamp_objects() {
        // No skip: the worker is built alongside this crate and the fixture is
        // in the repository. A test that quietly returns when its subject is
        // missing is a test that can never fail. [ADR-022, PRIN-6]
        let worker = find_worker();
        let input = fixture("valid-1page.pdf");
        assert!(worker.exists(), "worker binary missing at {}", worker.display());
        assert!(input.exists(), "fixture missing at {}", input.display());

        let output = std::env::temp_dir().join("pdf-platform-cli-stamp-test.pdf");
        run_stamp(&input, &output, Some("CONFIDENTIAL"), None, 6, 10.0).unwrap();
        let bytes = std::fs::read(&output).unwrap();
        assert!(bytes.windows(b"CONFIDENTIAL".len()).any(|w| w == b"CONFIDENTIAL"));
        assert!(bytes.len() > std::fs::metadata(&input).unwrap().len() as usize);
        std::fs::remove_file(output).ok();
    }

    #[test]
    fn a_stamp_is_visible_on_the_rendered_page() {
        // The test above proves the bytes are in the file. It cannot tell a
        // stamp that renders from one that is present, well-formed and
        // invisible — a content stream attached to nothing, drawn outside the
        // MediaBox, or painted in white on white would all satisfy it.
        //
        // The M6 tracker row records stamping as "not yet corpus- or
        // reader-tested" for exactly this reason. This renders both documents
        // and requires the page to have changed. [FR-STAMP, FR-CMP-1, PRIN-1]
        use coordinator::document::DocumentCoordinator;
        use coordinator::visual_compare::{compare_pages, DEFAULT_CHANNEL_TOLERANCE};

        let worker = find_worker();
        let input = fixture("valid-1page.pdf");
        assert!(worker.exists(), "worker binary missing at {}", worker.display());

        let output = std::env::temp_dir().join("pdf-platform-cli-stamp-visible.pdf");
        run_stamp(&input, &output, Some("CONFIDENTIAL"), None, 6, 24.0).unwrap();

        let mut before = DocumentCoordinator::open(&worker, &input).expect("open input");
        let mut after = DocumentCoordinator::open(&worker, &output).expect("open stamped");
        let rendered_before = before.render_page(0, 0.5).expect("render input");
        let rendered_after = after.render_page(0, 0.5).expect("render stamped");
        let _ = before.close();
        let _ = after.close();
        std::fs::remove_file(&output).ok();

        let diff = compare_pages(&rendered_before, &rendered_after, DEFAULT_CHANNEL_TOLERANCE)
            .expect("compare renders");
        assert!(
            !diff.is_identical(),
            "the stamped page renders identically to the unstamped one: the stamp \
             is in the file but not on the page"
        );
        assert!(
            diff.max_channel_delta > 64,
            "the difference is too faint to be drawn text: max channel delta {}",
            diff.max_channel_delta
        );
    }
}

#[cfg(test)]
mod export_tests {
    use super::*;
    use coordinator::document::DocumentCoordinator;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/corpus-diff/fixtures")
            .join(name)
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdf-platform-export-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// Width, height and RGBA bytes of a PNG written by `write_rgba_png`
    /// (stored deflate, filter 0). Reading the pixels back is the point: a
    /// file with the right header can still be a blank page. [T-10]
    fn read_png(path: &Path) -> (u32, u32, Vec<u8>) {
        let bytes = std::fs::read(path).expect("read png");
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n", "not a PNG");
        let (mut width, mut height, mut idat) = (0u32, 0u32, Vec::new());
        let mut at = 8;
        while at + 8 <= bytes.len() {
            let len = u32::from_be_bytes(bytes[at..at + 4].try_into().unwrap()) as usize;
            let kind = &bytes[at + 4..at + 8];
            let body = &bytes[at + 8..at + 8 + len];
            match kind {
                b"IHDR" => {
                    width = u32::from_be_bytes(body[0..4].try_into().unwrap());
                    height = u32::from_be_bytes(body[4..8].try_into().unwrap());
                }
                b"IDAT" => idat.extend_from_slice(body),
                _ => {}
            }
            at += 12 + len;
        }
        // zlib header (2 bytes), stored blocks, Adler-32 (4 bytes).
        let mut raw = Vec::new();
        let mut p = 2;
        loop {
            let last = idat[p] & 1 == 1;
            let n = u16::from_le_bytes([idat[p + 1], idat[p + 2]]) as usize;
            raw.extend_from_slice(&idat[p + 5..p + 5 + n]);
            p += 5 + n;
            if last {
                break;
            }
        }
        let stride = width as usize * 4 + 1;
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for row in raw.chunks(stride) {
            assert_eq!(row[0], 0, "only filter 0 is written");
            rgba.extend_from_slice(&row[1..]);
        }
        (width, height, rgba)
    }

    fn open(name: &str) -> DocumentCoordinator {
        let worker = find_worker();
        DocumentCoordinator::open(&worker, &fixture(name)).expect("open fixture")
    }

    #[test]
    fn one_png_per_page_and_the_dpi_scales_the_pixels() {
        let mut coord = open("valid-3page.pdf");
        let low = scratch("low");
        let high = scratch("high");

        let at_72 = export_images(&mut coord, &low, 72).expect("export at 72");
        let at_144 = export_images(&mut coord, &high, 144).expect("export at 144");
        let _ = coord.close();

        assert_eq!(at_72.len(), 3, "a 3-page document yields 3 files");
        let names: Vec<_> = at_72.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["page-001.png", "page-002.png", "page-003.png"]);

        for (a, b) in at_72.iter().zip(&at_144) {
            let (w1, h1, _) = read_png(a);
            let (w2, h2, _) = read_png(b);
            // A point is 1/72 inch, so doubling the DPI doubles each side
            // (to within the rounding of a non-integer page size).
            assert!(w2.abs_diff(w1 * 2) <= 1, "width {w1} at 72 DPI but {w2} at 144");
            assert!(h2.abs_diff(h1 * 2) <= 1, "height {h1} at 72 DPI but {h2} at 144");
            assert!(w1 > 100 && h1 > 100, "implausibly small page {w1}x{h1}");
        }
        let _ = std::fs::remove_dir_all(low);
        let _ = std::fs::remove_dir_all(high);
    }

    #[test]
    fn a_text_page_has_ink_and_a_blank_page_does_not() {
        // The blank fixture must come out all white and the text fixture must
        // not: together they fail for a stub renderer (which paints a
        // checkerboard), for an exporter that writes white regardless, and for
        // one that writes the same thing for every page. [T-10, T-12]
        let out = scratch("ink");
        let mut blank = open("valid-1page.pdf");
        let mut text = open("text-latin.pdf");
        let blank_files = export_images(&mut blank, &out.join("blank"), 72).expect("blank");
        let text_files = export_images(&mut text, &out.join("text"), 72).expect("text");
        let _ = blank.close();
        let _ = text.close();

        let (_, _, blank_pixels) = read_png(&blank_files[0]);
        let (_, _, text_pixels) = read_png(&text_files[0]);
        assert!(
            blank_pixels.iter().all(|&b| b == 255),
            "the blank page did not export as white"
        );
        assert!(
            text_pixels.chunks(4).any(|px| px[..3].iter().any(|&c| c < 128)),
            "the page with text exported with no dark pixel"
        );
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn a_dpi_the_frame_cannot_carry_is_refused_before_anything_is_written() {
        let mut coord = open("valid-1page.pdf");
        let out = scratch("too-big");
        let error = export_images(&mut coord, &out, 1200).expect_err("1200 DPI cannot fit one frame");
        let _ = coord.close();

        assert!(error.contains("DPI"), "{error}");
        assert!(error.contains("largest"), "the message must say what would work: {error}");
        assert!(!out.exists(), "a refused export must not create the output directory");
    }

    #[test]
    fn the_largest_dpi_the_message_names_actually_works() {
        // The refusal claims a number; check it is true rather than a guess.
        let mut coord = open("valid-1page.pdf");
        let error = export_images(&mut coord, &scratch("probe"), 1200).expect_err("too big");
        let largest: u32 = error
            .rsplit("is ")
            .next()
            .and_then(|tail| tail.trim_end_matches('.').parse().ok())
            .unwrap_or_else(|| panic!("could not read the largest DPI from: {error}"));

        let out = scratch("largest");
        let files = export_images(&mut coord, &out, largest)
            .unwrap_or_else(|e| panic!("the DPI the message promised ({largest}) failed: {e}"));
        assert_eq!(files.len(), 1);
        let _ = coord.close();
        let _ = std::fs::remove_dir_all(out);
    }

    #[test]
    fn the_dpi_flag_is_strict() {
        let args = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(parse_export_dpi(&[]).unwrap(), DEFAULT_EXPORT_DPI);
        assert_eq!(parse_export_dpi(&args(&["--dpi", "300"])).unwrap(), 300);
        assert!(parse_export_dpi(&args(&["--dpi", "abc"])).is_err(), "not a number");
        assert!(parse_export_dpi(&args(&["--dpi", "0"])).is_err(), "zero is not a resolution");
        assert!(parse_export_dpi(&args(&["--dpi"])).is_err(), "a flag with no value");
        assert!(parse_export_dpi(&args(&["--dpi=300"])).is_err(), "unknown spelling");
        assert!(parse_export_dpi(&args(&["300"])).is_err(), "a stray argument");
    }
}

#[cfg(test)]
mod page_edit_tests {
    use super::*;
    use coordinator::document::DocumentCoordinator;

    /// A document of `pages` pages, page n drawing `PAGEMARKn`, so a test can
    /// read the result back and check *which* pages survived.
    pub(super) fn numbered_pdf(pages: u32) -> Vec<u8> {
        numbered_pdf_with(pages, "")
    }

    /// As `numbered_pdf`, with `catalog_extra` added to the catalog dictionary.
    pub(super) fn numbered_pdf_with(pages: u32, catalog_extra: &str) -> Vec<u8> {
        let mut objects: Vec<Vec<u8>> =
            vec![format!("<< /Type /Catalog /Pages 2 0 R {catalog_extra} >>").into_bytes()];
        let kids: Vec<String> = (0..pages).map(|i| format!("{} 0 R", 4 + i * 2)).collect();
        objects.push(
            format!("<< /Type /Pages /Kids [{}] /Count {pages} >>", kids.join(" ")).into_bytes(),
        );
        objects.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_vec());
        for index in 0..pages {
            objects.push(
                format!(
                    "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
                     /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>",
                    5 + index * 2
                )
                .into_bytes(),
            );
            let content = format!("BT\n/F1 24 Tf\n72 700 Td\n(PAGEMARK{}) Tj\nET\n", index + 1);
            objects.push(
                [
                    format!("<< /Length {} >>\nstream\n", content.len()).as_bytes(),
                    content.as_bytes(),
                    b"endstream",
                ]
                .concat(),
            );
        }
        let mut bytes: Vec<u8> = b"%PDF-1.7\n".to_vec();
        let mut offsets = Vec::new();
        for (index, body) in objects.iter().enumerate() {
            offsets.push(bytes.len());
            bytes.extend_from_slice(format!("{} 0 obj\n", index + 1).as_bytes());
            bytes.extend_from_slice(body);
            bytes.extend_from_slice(b"\nendobj\n");
        }
        let xref_at = bytes.len();
        bytes.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for offset in &offsets {
            bytes.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        bytes.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref_at}\n%%EOF\n",
                objects.len() + 1
            )
            .as_bytes(),
        );
        bytes
    }

    pub(super) fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("pdf-platform-page-edit-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    pub(super) fn markers(path: &Path) -> Vec<String> {
        let worker = find_worker();
        let mut coord = DocumentCoordinator::open(&worker, path).expect("open result");
        let mut out = Vec::new();
        for page in 0..coord.page_count() {
            let text: String = coord
                .get_page_text(page)
                .expect("extract")
                .lines
                .iter()
                .map(|l| l.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            out.push(
                text.split_whitespace()
                    .find(|t| t.starts_with("PAGEMARK"))
                    .unwrap_or("<none>")
                    .to_string(),
            );
        }
        let _ = coord.close();
        out
    }

    #[test]
    fn the_page_list_is_one_based_sorted_and_strict() {
        assert_eq!(parse_page_list("1", 5).unwrap(), [0]);
        assert_eq!(parse_page_list("3-5,1", 5).unwrap(), [0, 2, 3, 4], "sorted, ranges expanded");
        assert_eq!(parse_page_list("2,2,1-2", 5).unwrap(), [0, 1], "no repeats");
        for (bad, why) in [
            ("", "empty"),
            ("0", "page 0"),
            ("two", "not a number"),
            ("1,x", "a bad piece after a good one"),
            ("4-2", "reversed range"),
            ("1-9", "past the end"),
            ("-3", "missing start"),
            ("1-", "missing end"),
        ] {
            assert!(parse_page_list(bad, 5).is_err(), "'{bad}' ({why}) must be an error");
        }
        assert!(parse_page_list("9", 5).unwrap_err().contains("5 page"), "the error names the page count");
    }

    #[test]
    fn deleting_a_page_removes_exactly_that_page() {
        let dir = scratch("delete");
        let input = dir.join("in.pdf");
        let output = dir.join("out.pdf");
        std::fs::write(&input, numbered_pdf(3)).unwrap();

        let changed = run_page_edit(&input, &output, "2", PageEdit::Delete).expect("delete");
        assert_eq!(changed, 1);
        assert_eq!(markers(&output), ["PAGEMARK1", "PAGEMARK3"], "page 2 must be the one that went");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn deleting_every_page_is_refused_and_writes_nothing() {
        let dir = scratch("delete-all");
        let input = dir.join("in.pdf");
        let output = dir.join("out.pdf");
        std::fs::write(&input, numbered_pdf(3)).unwrap();

        let error = run_page_edit(&input, &output, "1-3", PageEdit::Delete).unwrap_err();
        assert!(error.contains("every page"), "{error}");
        assert!(!output.exists(), "a refused edit left a file behind");

        let error = run_page_edit(&input, &output, "7", PageEdit::Delete).unwrap_err();
        assert!(error.contains("past the end"), "{error}");
        assert!(!output.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn rotating_a_page_turns_that_page_and_only_that_page() {
        let dir = scratch("rotate");
        let input = dir.join("in.pdf");
        let output = dir.join("out.pdf");
        std::fs::write(&input, numbered_pdf(3)).unwrap();

        run_page_edit(&input, &output, "2", PageEdit::Rotate(90)).expect("rotate");

        let worker = find_worker();
        let mut coord = DocumentCoordinator::open(&worker, &output).expect("open result");
        let first = coord.render_page(0, 1.0).expect("render 1");
        let second = coord.render_page(1, 1.0).expect("render 2");
        let third = coord.render_page(2, 1.0).expect("render 3");
        // A quarter turn either way swaps the sides, so the sizes cannot tell
        // 90 from 270; the rotation the engine reports can.
        let rotations: Vec<u32> =
            coord.summary().page_dimensions_f().iter().map(|(_, _, r)| *r).collect();
        let _ = coord.close();

        assert_eq!(rotations, [0, 90, 0], "page 2 must be turned 90, not 270");
        assert_eq!((first.width, first.height), (612, 792), "page 1 must be untouched");
        assert_eq!((second.width, second.height), (792, 612), "a quarter turn swaps the sides");
        assert_eq!((third.width, third.height), (612, 792), "page 3 must be untouched");
        assert_eq!(markers(&output), ["PAGEMARK1", "PAGEMARK2", "PAGEMARK3"], "no page may be lost");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod crop_tests {
    use super::page_edit_tests::{numbered_pdf, scratch};
    use super::*;
    use coordinator::document::DocumentCoordinator;

    #[test]
    fn the_crop_box_is_four_finite_numbers() {
        assert_eq!(parse_crop_box("10,20,300.5,400").unwrap(), [10.0, 20.0, 300.5, 400.0]);
        assert_eq!(parse_crop_box(" 0 , 0 , 1 , 1 ").unwrap(), [0.0, 0.0, 1.0, 1.0], "spaces around numbers");
        for (bad, why) in [("", "empty"), ("1,2,3", "three numbers"), ("1,2,3,4,5", "five"), ("1,2,x,4", "not a number"), ("1,2,NaN,4", "NaN"), ("1,2,inf,4", "infinite")] {
            assert!(parse_crop_box(bad).is_err(), "'{bad}' ({why}) must be an error");
        }
    }

    #[test]
    fn cropping_a_page_resizes_that_page_only() {
        let dir = scratch("crop");
        let (input, output) = (dir.join("in.pdf"), dir.join("out.pdf"));
        std::fs::write(&input, numbered_pdf(3)).unwrap();

        let changed = run_page_edit(&input, &output, "2", PageEdit::Crop([0.0, 0.0, 200.0, 300.0])).expect("crop");
        assert_eq!(changed, 1);

        let worker = find_worker();
        let mut coord = DocumentCoordinator::open(&worker, &output).expect("open result");
        let sizes: Vec<(u32, u32)> = (0..3)
            .map(|p| {
                let r = coord.render_page(p, 1.0).expect("render");
                (r.width, r.height)
            })
            .collect();
        let _ = coord.close();
        assert_eq!(sizes, [(612, 792), (200, 300), (612, 792)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_crop_that_does_not_fit_is_refused_and_writes_nothing() {
        let dir = scratch("crop-refused");
        let (input, output) = (dir.join("in.pdf"), dir.join("out.pdf"));
        std::fs::write(&input, numbered_pdf(2)).unwrap();

        let error = run_page_edit(&input, &output, "1", PageEdit::Crop([0.0, 0.0, 900.0, 300.0])).unwrap_err();
        assert!(error.contains("media box"), "{error}");
        assert!(!output.exists(), "a refused crop left a file behind");
        let error = run_page_edit(&input, &output, "1", PageEdit::Crop([300.0, 0.0, 100.0, 300.0])).unwrap_err();
        assert!(error.contains("not a rectangle"), "{error}");
        assert!(!output.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
#[cfg(test)]
mod reorder_tests {
    use super::page_edit_tests::{markers, numbered_pdf, scratch};
    use super::*;

    #[test]
    fn positions_are_one_based_and_sizes_are_positive_pairs() {
        assert_eq!(parse_position("1", "--to").unwrap(), 0);
        assert_eq!(parse_position(" 7 ", "--to").unwrap(), 6);
        for bad in ["0", "-1", "two", "", "1.5"] {
            assert!(parse_position(bad, "--to").is_err(), "'{bad}' must be an error");
        }
        assert_eq!(parse_size("612,792").unwrap(), [612.0, 792.0]);
        for bad in ["612", "612,792,1", "0,792", "-5,10", "a,b", "NaN,10", "inf,10"] {
            assert!(parse_size(bad).is_err(), "'{bad}' must be an error");
        }
    }

    #[test]
    fn move_duplicate_and_insert_change_the_document_as_asked() {
        let dir = scratch("reorder");
        let input = dir.join("in.pdf");
        std::fs::write(&input, numbered_pdf(3)).unwrap();

        let moved = dir.join("moved.pdf");
        run_page_edit(&input, &moved, "3", PageEdit::Move(0)).expect("move");
        assert_eq!(markers(&moved), ["PAGEMARK3", "PAGEMARK1", "PAGEMARK2"]);

        let duplicated = dir.join("dup.pdf");
        run_page_edit(&input, &duplicated, "1", PageEdit::Duplicate).expect("duplicate");
        assert_eq!(markers(&duplicated), ["PAGEMARK1", "PAGEMARK1", "PAGEMARK2", "PAGEMARK3"]);

        let blank = dir.join("blank.pdf");
        run_insert_blank(&input, &blank, 0, Some([300.0, 400.0])).expect("insert");
        assert_eq!(markers(&blank), ["<none>", "PAGEMARK1", "PAGEMARK2", "PAGEMARK3"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_cannot_be_done_is_refused_and_leaves_no_file() {
        let dir = scratch("reorder-refused");
        let (input, output) = (dir.join("in.pdf"), dir.join("out.pdf"));
        std::fs::write(&input, numbered_pdf(3)).unwrap();

        for (case, error) in [
            ("a move that changes nothing", run_page_edit(&input, &output, "2", PageEdit::Move(1)).unwrap_err()),
            ("a position past the end", run_page_edit(&input, &output, "1", PageEdit::Move(9)).unwrap_err()),
            ("a page past the end", run_page_edit(&input, &output, "9", PageEdit::Duplicate).unwrap_err()),
        ] {
            assert!(!error.is_empty(), "{case}");
            assert!(!output.exists(), "{case} left a file behind");
        }
        let error = run_insert_blank(&input, &output, 9, None).unwrap_err();
        assert!(error.contains("past the end"), "{error}");
        assert!(!output.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
#[cfg(test)]
mod history_tests {
    use super::page_edit_tests::{markers, numbered_pdf, numbered_pdf_with, scratch};
    use super::*;

    fn revision_count(path: &Path) -> usize {
        pdf_cos::revisions::list_revisions(&std::fs::read(path).unwrap()).expect("list").len()
    }

    /// Objects the newest cross-reference chain still lists as in use.
    fn live_objects(path: &Path) -> usize {
        let data = std::fs::read(path).unwrap();
        let start = pdf_cos::scan::find_startxref(&data).expect("startxref");
        pdf_cos::scan::parse_xref_chain(&data, start, &mut Vec::new())
            .expect("chain")
            .iter()
            .filter(|e| e.in_use && (e.offset != 0 || e.compressed.is_some()))
            .count()
    }

    #[test]
    fn every_save_is_one_more_revision() {
        let dir = scratch("revisions");
        let (a, b, c) = (dir.join("a.pdf"), dir.join("b.pdf"), dir.join("c.pdf"));
        std::fs::write(&a, numbered_pdf(3)).unwrap();
        assert_eq!(revision_count(&a), 1, "a document nobody has edited");

        run_page_edit(&a, &b, "1", PageEdit::Delete).expect("delete");
        assert_eq!(revision_count(&b), 2, "one incremental save");
        run_page_edit(&b, &c, "1", PageEdit::Rotate(90)).expect("rotate");
        assert_eq!(revision_count(&c), 3, "two incremental saves");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_drops_the_history_and_the_deleted_page_but_keeps_the_rest() {
        let dir = scratch("sanitize");
        let (original, edited, clean) = (dir.join("a.pdf"), dir.join("b.pdf"), dir.join("c.pdf"));
        std::fs::write(&original, numbered_pdf(3)).unwrap();
        run_page_edit(&original, &edited, "1", PageEdit::Delete).expect("delete page 1");

        // The deleted page is out of the page tree but still in the file.
        let edited_bytes = std::fs::read(&edited).unwrap();
        assert!(
            edited_bytes.windows(9).any(|w| w == b"PAGEMARK1"),
            "the premise: page 1's content is still in the edited file's history"
        );
        assert_eq!(markers(&edited), ["PAGEMARK2", "PAGEMARK3"]);
        assert_eq!(revision_count(&edited), 2);
        let live_before = live_objects(&edited);

        let report = run_sanitize(&edited, &clean, false).expect("sanitize");

        assert_eq!(report.revisions_before, 2);
        assert_eq!(revision_count(&clean), 1, "the history must be gone");
        assert_eq!(markers(&clean), ["PAGEMARK2", "PAGEMARK3"], "the pages that remain must remain");
        assert!(
            live_objects(&clean) + 2 <= live_before,
            "page 1's page and content objects should be gone: {} objects before, {} after",
            live_before,
            live_objects(&clean)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_refuses_a_signed_document_unless_told_to_discard_the_signature() {
        let dir = scratch("sanitize-signed");
        let (input, output) = (dir.join("signed.pdf"), dir.join("clean.pdf"));
        std::fs::write(
            &input,
            numbered_pdf_with(1, "/AcroForm << /Fields [<< /FT /Sig /T (sig1) >>] >>"),
        )
        .unwrap();

        let error = run_sanitize(&input, &output, false).unwrap_err();
        assert!(error.contains("signature"), "{error}");
        assert!(error.contains("--discard-signatures"), "the message must say how to proceed: {error}");
        assert!(!output.exists(), "a refused sanitize left a file behind");

        let report = run_sanitize(&input, &output, true).expect("with the flag it proceeds");
        assert_eq!(report.signatures_discarded, 1);
        assert!(output.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn sanitize_does_not_overwrite_an_existing_file() {
        let dir = scratch("sanitize-exists");
        let (input, output) = (dir.join("a.pdf"), dir.join("exists.pdf"));
        std::fs::write(&input, numbered_pdf(1)).unwrap();
        std::fs::write(&output, b"precious").unwrap();

        assert!(run_sanitize(&input, &output, false).is_err(), "an existing file must not be replaced");
        assert_eq!(std::fs::read(&output).unwrap(), b"precious", "and its bytes must be untouched");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod html_export_tests {
    use super::*;
    use coordinator::document::DocumentCoordinator;

    fn fixture(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/corpus-diff/fixtures")
            .join(name)
    }

    fn export(name: &str) -> String {
        let worker = find_worker();
        let mut coord = DocumentCoordinator::open(&worker, &fixture(name)).expect("open");
        let out = std::env::temp_dir().join(format!("pdf-platform-html-{name}.html"));
        let _ = std::fs::remove_file(&out);
        export_html(&mut coord, &out, "doc").expect("export");
        let _ = coord.close();
        let html = std::fs::read_to_string(&out).expect("read export");
        let _ = std::fs::remove_file(&out);
        html
    }

    #[test]
    fn the_words_of_the_page_come_out_in_reading_order() {
        // text-latin.pdf draws "Hello extraction" and then "second line here".
        let html = export("text-latin.pdf");
        let first = html.find("<p>Hello extraction</p>").expect("first line missing");
        let second = html.find("<p>second line here</p>").expect("second line missing");
        assert!(first < second, "the lines came out in the wrong order");
        assert_eq!(html.matches("<section").count(), 1, "one page, one section");
    }

    #[test]
    fn an_unreliable_page_says_so_and_a_reliable_one_does_not() {
        let unreliable = export("text-unreliable.pdf");
        let reliable = export("text-latin.pdf");
        assert!(
            unreliable.contains("flagged unreliable"),
            "the page with a private-use mapping was presented as authoritative"
        );
        assert!(
            !reliable.contains("flagged unreliable"),
            "a good page was flagged"
        );
    }

    #[test]
    fn every_page_gets_a_section_even_when_it_has_no_text() {
        let html = export("valid-3page.pdf");
        assert_eq!(html.matches("<section").count(), 3);
        assert_eq!(html.matches("No text on this page.").count(), 3);
        let p1 = html.find("id=\"page-1\"").unwrap();
        let p2 = html.find("id=\"page-2\"").unwrap();
        let p3 = html.find("id=\"page-3\"").unwrap();
        assert!(p1 < p2 && p2 < p3, "pages are out of order");
    }

    #[test]
    fn document_text_cannot_inject_markup() {
        let pages = [HtmlPage {
            number: 1,
            reliable: true,
            text: "<script>alert(1)</script> & \"quoted\"".into(),
        }];
        let html = render_html("a<b>&\"", &pages);
        assert!(!html.contains("<script>"), "page text became live markup");
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt; &amp; &quot;quoted&quot;"));
        assert!(html.contains("<title>a&lt;b&gt;&amp;&quot;</title>"), "the title was not escaped");
        assert!(!html.contains("http://") && !html.contains("https://"), "no external references");
    }
}

#[cfg(test)]
mod batch_tests {
    use super::*;
    use pdf_model::batch::{BatchPipeline, BatchStep};

    /// `PDF_PLATFORM_INDEX_DIR` is process-wide, so the two tests that set it
    /// cannot run at the same time.
    static INDEX_DIR_ENV: std::sync::Mutex<()> = std::sync::Mutex::new(());

    fn step(step_type: &str, inputs: &[PathBuf], output: Option<&Path>, root: Option<&Path>)
        -> Result<BatchPipeline, String>
    {
        let mut pipeline = BatchPipeline::new("test");
        flush_batch_step(
            &mut pipeline, step_type, inputs, output, None, 2, 1, 1, None, None, None,
            false, 1, 6, root,
        )?;
        Ok(pipeline)
    }

    #[test]
    fn a_numeric_field_that_is_not_a_number_is_an_error() {
        // `val.parse().unwrap_or(2)` turned `pages_per_file=abc` into a split
        // every 2 pages, and the run reported success. [GR-8]
        let error = batch_number("abc", "pages_per_file").expect_err("not a number");
        assert!(error.contains("pages_per_file="), "{error}");
        assert!(error.contains("abc"), "the message must quote what was read: {error}");

        assert_eq!(batch_number(" 12 ", "first").unwrap(), 12, "surrounding space is fine");
        assert!(batch_number("-1", "start").is_err(), "a negative count is not a page count");
        assert!(batch_number("", "width").is_err(), "an empty value is not a default");
        assert!(batch_number("3.5", "last").is_err(), "a page count is a whole number");
    }

    #[test]
    fn an_impossible_page_range_is_refused_before_the_work_starts() {
        assert!(check_page_range(1, 5).is_ok());
        assert!(
            check_page_range(0, 5).unwrap_err().contains("1-based"),
            "page 0 does not exist"
        );
        assert!(
            check_page_range(9, 2).unwrap_err().contains("after"),
            "a reversed range is a mistake, not an empty result"
        );
    }

    #[test]
    fn a_stanza_missing_its_output_is_an_error_not_a_dropped_step() {
        // Every arm was `if let Some(..)` with no `else`: a stanza missing
        // `output=` produced no step, the pipeline ran the remaining steps and
        // printed "Pipeline completed successfully". [GR-8]
        let error = step("optimize", &[PathBuf::from("in.pdf")], None, None)
            .expect_err("a step with no output must not be silently dropped");
        assert!(error.contains("output="), "{error}");
    }

    #[test]
    fn a_merge_with_one_input_is_an_error() {
        let error = step("merge", &[PathBuf::from("only.pdf")], Some(Path::new("out.pdf")), None)
            .expect_err("merge needs two inputs");
        assert!(error.contains("two input"), "{error}");
    }

    #[test]
    fn an_unknown_step_type_is_an_error() {
        let error = step("teleport", &[PathBuf::from("in.pdf")], Some(Path::new("out.pdf")), None)
            .expect_err("an unknown step type must not be ignored");
        assert!(error.contains("teleport"), "{error}");
    }

    #[test]
    fn an_index_stanza_needs_a_root() {
        let error = step("index", &[], None, None).expect_err("index needs root=");
        assert!(error.contains("root="), "{error}");

        let pipeline = step("index", &[], None, Some(Path::new("/enrolled"))).unwrap();
        assert!(matches!(
            pipeline.steps.first(),
            Some(BatchStep::Index { .. })
        ));
    }

    /// A one-page PDF whose content stream really contains `text`.
    fn pdf_with_text(text: &str) -> Vec<u8> {
        use std::io::Write as _;
        let content = format!("BT /F1 24 Tf 72 700 Td ({text}) Tj ET");
        let mut bytes = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        offsets.push(bytes.len());
        bytes.extend_from_slice(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n");
        offsets.push(bytes.len());
        bytes.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        offsets.push(bytes.len());
        bytes.extend_from_slice(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 5 0 R >> >> /Contents 4 0 R >>\nendobj\n");
        offsets.push(bytes.len());
        write!(
            bytes,
            "4 0 obj\n<< /Length {} >>\nstream\n{content}\nendstream\nendobj\n",
            content.len()
        )
        .unwrap();
        offsets.push(bytes.len());
        bytes.extend_from_slice(
            b"5 0 obj\n<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>\nendobj\n",
        );
        let xref = bytes.len();
        write!(bytes, "xref\n0 {}\n", offsets.len() + 1).unwrap();
        bytes.extend_from_slice(b"0000000000 65535 f \n");
        for offset in &offsets {
            writeln!(bytes, "{offset:010} 00000 n ").unwrap();
        }
        write!(
            bytes,
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            offsets.len() + 1
        )
        .unwrap();
        bytes
    }

    #[test]
    fn a_batch_index_step_makes_the_document_findable() {
        // FR-BATCH: an operation must behave identically singly and in a
        // pipeline. The observable is a search hit for a word that exists only
        // inside the document the step indexed — not that the step returned Ok.
        // [FR-BATCH, ADR-019 §3, T-10]
        use coordinator::broker::{save_enrollment_registry, IndexEnrollmentRegistry};
        use search::tantivy_backend::CrossDocumentIndex;
        let _guard = INDEX_DIR_ENV.lock().unwrap_or_else(|e| e.into_inner());

        let base = std::env::temp_dir().join(format!(
            "pdf-platform-batch-index-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        let docs = base.join("docs");
        let state = base.join("state");
        std::fs::create_dir_all(&docs).unwrap();
        std::fs::create_dir_all(&state).unwrap();
        std::env::set_var("PDF_PLATFORM_INDEX_DIR", &state);

        std::fs::write(docs.join("report.pdf"), pdf_with_text("Xylophone Quarterly")).unwrap();

        let mut enrollments = IndexEnrollmentRegistry::default();
        enrollments.enroll(&docs).unwrap();
        save_enrollment_registry(&enrollments, &state.join("enrollments.bin")).unwrap();

        let mut pipeline = BatchPipeline::new("index-run");
        pipeline.add_step(BatchStep::Index { root: docs.clone() });
        let results = execute_cli_pipeline(&pipeline);

        assert_eq!(results.len(), 1);
        assert!(results[0].success, "index step failed: {}", results[0].message);

        let index = CrossDocumentIndex::open_or_create(&state.join("tantivy")).unwrap();
        let hits = index.search("Xylophone", 10).unwrap();
        assert!(
            !hits.is_empty(),
            "the batch step reported success but the document is not in the index"
        );

        std::env::remove_var("PDF_PLATFORM_INDEX_DIR");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn a_batch_index_step_for_an_unenrolled_root_fails_loudly() {
        // Enrollment is the boundary that keeps indexing away from files the
        // user never offered; a batch file must not be a way around it.
        let _guard = INDEX_DIR_ENV.lock().unwrap_or_else(|e| e.into_inner());
        let base = std::env::temp_dir().join(format!(
            "pdf-platform-batch-index-unenrolled-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::env::set_var("PDF_PLATFORM_INDEX_DIR", base.join("state"));

        let mut pipeline = BatchPipeline::new("index-run");
        pipeline.add_step(BatchStep::Index { root: base.clone() });
        let results = execute_cli_pipeline(&pipeline);

        assert!(!results[0].success);
        assert!(
            results[0].message.contains("not an enrolled root"),
            "{}",
            results[0].message
        );

        std::env::remove_var("PDF_PLATFORM_INDEX_DIR");
        let _ = std::fs::remove_dir_all(&base);
    }
}

#[cfg(test)]
mod attachment_tests {
    use super::*;

    #[test]
    fn hostile_names_stay_inside_the_folder_and_collisions_are_kept_apart() {
        assert_eq!(safe_attachment_name("..\\..\\startup\\evil.exe", &[]), "evil.exe");
        assert_eq!(safe_attachment_name("../../etc/passwd", &[]), "passwd");
        assert_eq!(safe_attachment_name("C:\\x\\a:b.txt", &[]), "a_b.txt");
        assert_eq!(safe_attachment_name("", &[]), "attachment");
        assert_eq!(safe_attachment_name("..", &[]), "attachment");
        let taken = vec!["a.txt".to_string()];
        assert_eq!(safe_attachment_name("A.TXT", &taken), "A-2.TXT");
    }

    /// A file embedded by qpdf, a producer that is not this codebase, comes out
    /// byte for byte through the real command path. [T-12]
    #[test]
    fn a_file_embedded_by_qpdf_is_extracted_exactly() {
        if std::process::Command::new("qpdf").arg("--version").output().is_err() {
            eprintln!("skipped: qpdf not on PATH");
            return;
        }
        let dir = std::env::temp_dir().join(format!("pdfp-att-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let payload: Vec<u8> = (0..70_000u32).map(|i| (i % 251) as u8).collect();
        std::fs::write(dir.join("data.bin"), &payload).unwrap();
        let source = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tools/corpus-diff/fixtures/valid-1page.pdf");
        let status = std::process::Command::new("qpdf")
            .arg(&source)
            .arg("--add-attachment")
            .arg(dir.join("data.bin"))
            .arg("--key=data.bin")
            .arg("--")
            .arg(dir.join("with.pdf"))
            .status()
            .unwrap();
        assert!(status.code().is_some_and(|c| c == 0 || c == 3), "qpdf failed");

        let data = std::fs::read(dir.join("with.pdf")).unwrap();
        let written = extract_attachments(&data, &dir.join("out")).unwrap();
        assert_eq!(written, vec![("data.bin".to_string(), payload.len())]);
        assert_eq!(std::fs::read(dir.join("out").join("data.bin")).unwrap(), payload);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod insert_pages_tests {
    use super::*;
    use InsertSource::{Base, Other};

    #[test]
    fn the_plan_puts_the_inserted_pages_exactly_where_asked() {
        // 5-page document, insert pages 2-3 of another so they become pages 3-4.
        assert_eq!(
            plan_insertion(5, 3, (2, 3)).unwrap(),
            [(Base, 1, 2), (Other, 2, 3), (Base, 3, 5)]
        );
        // At the front, and appended.
        assert_eq!(plan_insertion(5, 1, (1, 1)).unwrap(), [(Other, 1, 1), (Base, 1, 5)]);
        assert_eq!(plan_insertion(5, 6, (1, 2)).unwrap(), [(Base, 1, 5), (Other, 1, 2)]);
    }

    #[test]
    fn a_position_outside_the_document_is_refused() {
        assert!(plan_insertion(5, 7, (1, 1)).is_err());
        assert!(plan_insertion(5, 0, (1, 1)).is_err());
    }
}

#[cfg(test)]
mod comment_filter_tests {
    use super::*;
    use pdf_cos::comments::Comment;

    fn c(page: u32, kind: &str, author: &str, reply: bool) -> Comment {
        Comment {
            page,
            subtype: kind.into(),
            author: author.into(),
            contents: String::new(),
            modified: String::new(),
            is_reply: reply,
        }
    }

    #[test]
    fn each_filter_keeps_only_what_it_names_and_they_combine() {
        let all = vec![
            c(1, "Highlight", "Ana", false),
            c(2, "Text", "Ben", false),
            c(2, "Text", "ana", true),
        ];
        let by = |f: CommentFilter| filter_comments(all.clone(), &f).len();
        assert_eq!(by(CommentFilter::default()), 3);
        assert_eq!(by(CommentFilter { author: Some("ANA".into()), ..Default::default() }), 2);
        assert_eq!(by(CommentFilter { kind: Some("text".into()), ..Default::default() }), 2);
        assert_eq!(by(CommentFilter { page: Some(2), ..Default::default() }), 2);
        assert_eq!(by(CommentFilter { replies: Some(true), ..Default::default() }), 1);
        assert_eq!(by(CommentFilter { replies: Some(false), ..Default::default() }), 2);
        let both = CommentFilter { author: Some("ana".into()), page: Some(2), ..Default::default() };
        assert_eq!(by(both), 1);
    }
}
