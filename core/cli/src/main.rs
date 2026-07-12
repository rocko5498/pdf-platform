//! `pdf-platform` CLI entry point. [ADR-025, FR-CLI, US-DEV-6, SDS §14 M0]
//!
//! M0 scope: structural summary only. Full CLI surface at M6.

use std::{path::PathBuf, process};

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() != 2 {
        eprintln!("usage: pdf-platform <file>");
        process::exit(1);
    }

    let path = PathBuf::from(&args[1]);

    if !path.exists() {
        eprintln!("error: not found: {}", path.display());
        process::exit(1);
    }

    match coordinator::inspect::inspect(&path) {
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

fn yn(b: bool) -> &'static str {
    if b {
        "yes"
    } else {
        "no"
    }
}
