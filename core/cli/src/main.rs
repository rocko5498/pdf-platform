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
    ];

    let (cmd, path, rest) = if file_cmds.contains(&args[1].as_str()) {
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
    match optimize_pdf(path, &out, profile) {
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
