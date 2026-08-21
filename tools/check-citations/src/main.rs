//! G-1 traceability gate.
//!
//! `G-1` requires every non-trivial change to cite the requirement or decision
//! it implements, and `AI-2` declares a change that cannot cite a spec out of
//! scope. Nothing checked that a citation names an identifier that *exists*, so
//! a typo (`FR-ANNOT-9`, `ADR-0.3`) reads as traceable and passes CI.
//!
//! This gate harvests every identifier defined in the canonical documents, then
//! reports citations in source that match none of them.
//!
//! A citation is accepted when it is either an exact identifier (`FR-ANNOT-2`)
//! or a family prefix of one (`FR-ANNOT`, as used in `[FR-ANNOT-*]`), because
//! citing a whole requirement family is idiomatic throughout this repo.
//!
//! Usage:
//!     cargo run -p check-citations -- [repo_root]
//! Exit 0 = every citation resolves, 1 = unknown identifiers found.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Documents that define identifiers. Anything cited must appear in one.
const CANONICAL_DOCS: &[&str] = &[
    "docs/adr-constitution.md",
    "docs/system-design-specification.md",
    "docs/product-requirements-document.md",
    "docs/ui-ux-design-system.md",
    "IMPLEMENTATION_GUIDE.md",
    "AGENTS.md",
];

/// Prefixes that denote a citable identifier.
const PREFIXES: &[&str] = &[
    "FR", "NFR", "UX", "ENT", "CMP", "MET", "DS", "PRIN", "GR", "VIS", "ROAD", "ADR", "AI", "SCOPE",
    "OUT", "FUT", "RISK", "DEP", "T", "B", "CR", "PR", "RQA", "AQA", "IQA", "VQA", "PQA", "CQA",
];

/// Pull every `PREFIX-...` token out of `text`.
fn identifiers(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < bytes.len() {
        if !bytes[index].is_ascii_uppercase() {
            index += 1;
            continue;
        }
        // A token starts at a word boundary.
        if index > 0 && (bytes[index - 1].is_ascii_alphanumeric() || bytes[index - 1] == '-') {
            index += 1;
            continue;
        }
        let start = index;
        while index < bytes.len()
            && (bytes[index].is_ascii_uppercase()
                || bytes[index].is_ascii_digit()
                || bytes[index] == '-')
        {
            index += 1;
        }
        let token: String = bytes[start..index].iter().collect();
        let token = token.trim_end_matches(['-']).to_owned();
        if is_citation_shaped(&token) {
            found.insert(token);
        }
    }
    found
}

/// True when `token` looks like `PREFIX-SUFFIX` with a known prefix.
fn is_citation_shaped(token: &str) -> bool {
    let Some((prefix, rest)) = token.split_once('-') else {
        return false;
    };
    !rest.is_empty() && PREFIXES.contains(&prefix)
}

/// Every identifier plus every family prefix of one, e.g. `FR-ANNOT-2` also
/// contributes `FR-ANNOT`.
fn accepted_set(defined: &BTreeSet<String>) -> BTreeSet<String> {
    let mut accepted = defined.clone();
    for id in defined {
        if let Some((family, last)) = id.rsplit_once('-') {
            if last.chars().all(|c| c.is_ascii_digit()) && family.contains('-') {
                accepted.insert(family.to_owned());
            }
        }
    }
    accepted
}

/// Report citations in `text` that `accepted` does not contain.
fn unknown_citations(text: &str, accepted: &BTreeSet<String>) -> BTreeSet<String> {
    identifiers(text)
        .into_iter()
        .filter(|id| !accepted.contains(id))
        .collect()
}

/// Collect source files under `root` with one of `extensions`, skipping build
/// output and vendored trees.
fn sources(root: &Path, extensions: &[&str], found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            if matches!(name.as_ref(), "target" | "build" | "third_party" | ".git") {
                continue;
            }
            sources(&path, extensions, found);
        } else if path
            .extension()
            .is_some_and(|ext| extensions.iter().any(|e| ext == *e))
        {
            found.push(path);
        }
    }
}

fn main() -> ExitCode {
    let root = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);

    let mut defined = BTreeSet::new();
    let mut missing_docs = Vec::new();
    for doc in CANONICAL_DOCS {
        match std::fs::read_to_string(root.join(doc)) {
            Ok(text) => defined.extend(identifiers(&text)),
            Err(_) => missing_docs.push(*doc),
        }
    }
    // ADRs past 030 live one-per-file under docs/adr/ (ADR-024), so the
    // constitution alone does not define every valid ADR-NNN.
    let mut adr_files = Vec::new();
    sources(&root.join("docs").join("adr"), &["md"], &mut adr_files);
    for path in &adr_files {
        if let Ok(text) = std::fs::read_to_string(path) {
            defined.extend(identifiers(&text));
        }
    }
    if !missing_docs.is_empty() {
        println!("FAIL: canonical documents not found: {missing_docs:?}");
        return ExitCode::FAILURE;
    }
    let accepted = accepted_set(&defined);

    // tools/ is deliberately excluded: the gates there carry deliberately
    // bogus identifiers as test fixtures, which are not product citations.
    let mut files = Vec::new();
    sources(&root.join("core"), &["rs"], &mut files);
    sources(&root.join("shell"), &["cc", "h"], &mut files);
    files.sort();

    let mut total = 0;
    for path in &files {
        let Ok(text) = std::fs::read_to_string(path) else {
            continue;
        };
        let unknown = unknown_citations(&text, &accepted);
        if unknown.is_empty() {
            continue;
        }
        let shown = path.strip_prefix(&root).unwrap_or(path);
        for id in &unknown {
            println!("{}: cites {id}, which no canonical document defines", shown.display());
            total += 1;
        }
    }

    println!(
        "\n{} identifiers defined across {} canonical documents; {} source files scanned",
        defined.len(),
        CANONICAL_DOCS.len(),
        files.len()
    );
    if total > 0 {
        println!("FAIL: {total} unresolved citation(s) [G-1, AI-2]");
        return ExitCode::FAILURE;
    }
    println!("OK: every citation resolves to a defined identifier");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn harvests_identifiers_from_document_text() {
        let found = identifiers("**FR-VIEW-1.** The Platform MUST render, per ADR-005.");
        assert!(found.contains("FR-VIEW-1"), "{found:?}");
        assert!(found.contains("ADR-005"), "{found:?}");
    }

    #[test]
    fn ignores_ordinary_prose_and_screaming_constants() {
        let found = identifiers("MUST NOT render. See MAX_UTILITY_GRANTS and HTTP.");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn accepts_an_exact_identifier_that_exists() {
        let accepted = accepted_set(&set(&["FR-ANNOT-2"]));
        assert!(unknown_citations("// [FR-ANNOT-2]", &accepted).is_empty());
    }

    #[test]
    fn accepts_a_family_prefix_of_a_defined_identifier() {
        // `[FR-ANNOT]` and `[DS-ERR-*]` are idiomatic in this repo.
        let accepted = accepted_set(&set(&["FR-ANNOT-2", "DS-ERR-1"]));
        assert!(unknown_citations("// [FR-ANNOT] and [DS-ERR-*]", &accepted).is_empty());
    }

    #[test]
    fn rejects_an_identifier_no_document_defines() {
        let accepted = accepted_set(&set(&["FR-ANNOT-2"]));
        let unknown = unknown_citations("// [FR-ANNOT-9]", &accepted);
        assert!(unknown.contains("FR-ANNOT-9"), "{unknown:?}");
    }

    #[test]
    fn rejects_a_misspelled_prefix_family() {
        let accepted = accepted_set(&set(&["FR-ANNOT-2"]));
        let unknown = unknown_citations("// [FR-ANNOTATION-2]", &accepted);
        assert!(unknown.contains("FR-ANNOTATION-2"), "{unknown:?}");
    }

    #[test]
    fn a_bare_family_is_not_invented_from_a_lettered_suffix() {
        // `PRIN-2` must not license a bare `PRIN` citation: the family rule
        // only applies where a real hierarchy exists (FR-AREA-N).
        let accepted = accepted_set(&set(&["PRIN-2"]));
        assert!(!accepted.contains("PRIN"), "{accepted:?}");
    }
}
