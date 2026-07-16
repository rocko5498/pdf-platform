//! `pdf-platform` CLI entry point. [ADR-025, FR-CLI, US-DEV-6, SDS §14]
//!
//! Commands:
//!   pdf-platform <file>                         structural summary
//!   pdf-platform outline <file>                 bookmarks
//!   pdf-platform layers <file>                  optional content
//!   pdf-platform attachments <file>             embedded files
//!   pdf-platform diagnostics <file>             leniency + flags
//!   pdf-platform find <file> <query>            in-document find (M2)

use std::path::{Path, PathBuf};
use std::process;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        usage();
        process::exit(1);
    }

    let (cmd, path, rest) = if args[1] == "outline"
        || args[1] == "layers"
        || args[1] == "attachments"
        || args[1] == "diagnostics"
        || args[1] == "find"
    {
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

    // Crash recovery notice (summary path). [SDS §10.2]
    if cmd == "summary" {
        print_recovery_notice(&path);
    }

    match cmd {
        "summary" => cmd_summary(&path),
        "outline" | "layers" | "attachments" | "diagnostics" | "find" => {
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
        "usage:\n  pdf-platform <file>\n  pdf-platform outline|layers|attachments|diagnostics <file>\n  pdf-platform find <file> <query>"
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
                    "Leniency:   {} repair(s) — details on stderr",
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

fn find_worker() -> PathBuf {
    let mut dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // core/cli -> core
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
    // Same folder as this binary
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
                    println!("Query: {query:?}");
                    println!("Hits:  {total} across {} page(s)", results.len());
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
        _ => unreachable!(),
    }

    let _ = coord.close();
    process::exit(0);
}

fn compute_sidecar_path(doc_path: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let canonical = doc_path.canonicalize().unwrap_or_else(|_| doc_path.to_path_buf());
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

    let canonical_doc = doc_path.canonicalize().unwrap_or_else(|_| doc_path.to_path_buf());
    let canonical_sidecar = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.clone());
    if canonical_doc != canonical_sidecar
        && canonical_doc.to_string_lossy() != canonical_sidecar.to_string_lossy()
    {
        return Err("sidecar is for a different document".into());
    }

    let journal_text = text.lines().skip(journal_start).collect::<Vec<_>>().join("\n");
    let group_count = journal_text.lines().filter(|l| l.starts_with("GROUP:")).count();
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
