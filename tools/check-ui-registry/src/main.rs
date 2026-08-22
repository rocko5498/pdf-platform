//! ADR-032 conformance gate for the shortcut/menu registry.
//!
//! `shell/chrome/ui-registry.toml` is the interface-stability contract as an
//! artifact [ADR-032, ADR-030, DS-CONV-4, PRIN-4]. This gate enforces the
//! rules ADR-032 states unambiguously:
//!
//! * `schema_version` is an integer and not newer than this parser supports.
//! * `profile_version` is semver — the ADR-030 UI-profile identity.
//! * No two actions bind the same key (DS-PHIL-3 determinism).
//! * Every menu item is an action or a separator; its action exists in
//!   `[shortcuts]`; any shortcut it displays is one of the keys declared for
//!   that action (DS-MENU-3, DS-PHIL-3).
//! * "No shortcut binding may appear in C++ source" [ADR-032] — every key
//!   bound under `shell/` must be declared in the registry.
//!
//! **Not enforced:** ADR-032 also requires every action-id to name a Command
//! in the `protocol` crate. The shipped registry uses shell-local view actions
//! (`view.zoom_in`, `focus.canvas`, `nav.next_page`) which are not, and cannot
//! be, worker-directed protocol Commands — zero of twelve ids satisfy it.
//! Enforcing it as written would reject every entry. Resolving that needs an
//! amending ADR (ADRU-2); this gate does not silently substitute its own rule.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Highest `schema_version` this parser understands.
const SUPPORTED_SCHEMA_VERSION: i64 = 1;

/// Maximum lines scanned for a modifier guard below a `case` label group.
const CASE_BODY_LOOKAHEAD: usize = 30;

/// `QKeySequence` standard sequences, in their Windows/Linux key text.
const STANDARD_SEQUENCES: &[(&str, &str)] = &[
    ("Open", "Ctrl+O"),
    ("Close", "Ctrl+W"),
    ("Save", "Ctrl+S"),
    ("Print", "Ctrl+P"),
    ("Quit", "Ctrl+Q"),
    ("Find", "Ctrl+F"),
    ("FindNext", "F3"),
    ("FindPrevious", "Shift+F3"),
    ("Copy", "Ctrl+C"),
    ("Cut", "Ctrl+X"),
    ("Paste", "Ctrl+V"),
    ("Undo", "Ctrl+Z"),
    ("Redo", "Ctrl+Y"),
    ("SelectAll", "Ctrl+A"),
    ("ZoomIn", "Ctrl++"),
    ("ZoomOut", "Ctrl+-"),
];

/// `Qt::Key_*` spellings whose key text differs from the bare suffix.
const KEY_NAMES: &[(&str, &str)] = &[("Equal", "="), ("Plus", "+"), ("Minus", "-")];

/// Parse the registry and report every violation of the checked rules.
fn check_registry(source: &str) -> Vec<String> {
    let mut findings = Vec::new();
    let document: toml::Value = match source.parse() {
        Ok(value) => value,
        Err(error) => return vec![format!("not valid TOML: {error}")],
    };

    match document.get("schema_version").and_then(toml::Value::as_integer) {
        None => findings.push("schema_version must be an integer".to_owned()),
        Some(version) if version > SUPPORTED_SCHEMA_VERSION => findings.push(format!(
            "schema_version {version} is newer than supported {SUPPORTED_SCHEMA_VERSION}; refusing to parse"
        )),
        Some(_) => {}
    }

    let profile = document.get("profile_version").and_then(toml::Value::as_str);
    if !profile.is_some_and(is_semver) {
        findings.push(format!("profile_version must be semver, got {profile:?}"));
    }

    // One action may declare several keys (a primary plus alternates), so the
    // map is action -> set of keys rather than action -> key.
    let mut keys_for_action: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut owner_of_key: BTreeMap<&str, &str> = BTreeMap::new();

    if let Some(shortcuts) = document.get("shortcuts").and_then(toml::Value::as_table) {
        for (name, entry) in shortcuts {
            let key = entry.get("key").and_then(toml::Value::as_str);
            let action = entry.get("action").and_then(toml::Value::as_str);
            let (Some(key), Some(action)) = (key, action) else {
                findings.push(format!("shortcut {name:?} needs both key and action"));
                continue;
            };
            if let Some(previous) = owner_of_key.insert(key, name) {
                findings.push(format!(
                    "duplicate key {key:?} bound by {previous:?} and {name:?}"
                ));
            }
            keys_for_action.entry(action).or_default().insert(key);

            // `alternates` are declared bindings too: PageDown carries Down and
            // Space, zoom-in carries the shifted and keypad spellings of plus.
            // They are as much a part of the contract as the primary key, so a
            // C++ site that binds one is compliant, and a duplicate across two
            // actions is still a conflict. [ADR-032]
            let alternates = entry
                .get("alternates")
                .and_then(toml::Value::as_array)
                .map(Vec::as_slice)
                .unwrap_or_default();
            for alternate in alternates {
                let Some(alternate) = alternate.as_str() else {
                    findings.push(format!("shortcut {name:?} has a non-string alternate"));
                    continue;
                };
                if let Some(previous) = owner_of_key.insert(alternate, name) {
                    findings.push(format!(
                        "duplicate key {alternate:?} bound by {previous:?} and {name:?}"
                    ));
                }
                keys_for_action.entry(action).or_default().insert(alternate);
            }
        }
    }

    let menus = document
        .get("menus")
        .and_then(toml::Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for menu in menus {
        let id = menu.get("id").and_then(toml::Value::as_str).unwrap_or("?");
        let items = menu
            .get("items")
            .and_then(toml::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for (index, item) in items.iter().enumerate() {
            let where_ = format!("menu {id:?} item {index}");
            if item
                .get("separator")
                .and_then(toml::Value::as_bool)
                .unwrap_or(false)
            {
                continue;
            }
            let Some(action) = item.get("action").and_then(toml::Value::as_str) else {
                findings.push(format!("{where_} is neither an action nor a separator"));
                continue;
            };
            let Some(declared) = keys_for_action.get(action) else {
                findings.push(format!("{where_} action {action:?} is not in [shortcuts]"));
                continue;
            };
            if let Some(shortcut) = item.get("shortcut").and_then(toml::Value::as_str) {
                if !declared.contains(shortcut) {
                    findings.push(format!(
                        "{where_} shortcut {shortcut:?} disagrees with [shortcuts] {declared:?}"
                    ));
                }
            }
        }
    }

    findings
}

/// True when `text` is `major.minor.patch`.
fn is_semver(text: &str) -> bool {
    let parts: Vec<&str> = text.split('.').collect();
    parts.len() == 3 && parts.iter().all(|part| part.parse::<u32>().is_ok())
}

/// Extract the identifier following `needle` on `line`, if present.
fn identifier_after<'a>(line: &'a str, needle: &str) -> Option<&'a str> {
    let rest = &line[line.find(needle)? + needle.len()..];
    let end = rest
        .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    (end > 0).then(|| &rest[..end])
}

/// Apply the modifiers present in `guard` to a bare key name.
fn with_modifiers(key: &str, guard: &str) -> String {
    let key = KEY_NAMES
        .iter()
        .find(|(name, _)| *name == key)
        .map_or(key, |(_, text)| *text);
    let shifted = if guard.contains("ShiftModifier") {
        format!("Shift+{key}")
    } else {
        key.to_owned()
    };
    if guard.contains("ControlModifier") {
        format!("Ctrl+{shifted}")
    } else {
        shifted
    }
}


/// Reject any key binding written in production C++ source. [ADR-032]
///
/// `check_cxx_source` asks whether a literal is *declared*; this asks whether
/// it exists at all. Both are needed: the first catches a registry that has
/// drifted from the code, the second enforces the rule that makes drift
/// impossible. `QKeySequence::ExactMatch` and its siblings are match results,
/// not bindings, so they are allowed.
fn check_no_cxx_bindings(name: &str, source: &str) -> Vec<String> {
    const MATCH_RESULTS: &[&str] = &["ExactMatch", "PartialMatch", "NoMatch"];
    let mut findings = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if let Some(key) = identifier_after(line, "Qt::Key_") {
            findings.push(format!(
                "{name}:{}: Qt::Key_{key} in production source — bindings belong in                  ui-registry.toml, ask chrome::shortcuts().matches(action, event) [ADR-032]",
                index + 1
            ));
        }
        if let Some(sequence) = identifier_after(line, "QKeySequence::") {
            if !MATCH_RESULTS.contains(&sequence) {
                findings.push(format!(
                    "{name}:{}: QKeySequence::{sequence} in production source — the registry                      decides bindings, not Qt's standard-key table [ADR-032]",
                    index + 1
                ));
            }
        }
    }
    findings
}

/// Report C++ key bindings that the registry does not declare.
///
/// Qt switch statements put the modifier test in the body shared by a run of
/// stacked `case` labels, several lines below them, so the guard is resolved
/// per case-group rather than per line.
fn check_cxx_source(name: &str, source: &str, declared: &BTreeSet<String>) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut findings = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let trimmed = lines[index].trim();
        let is_case_label = trimmed.starts_with("case ") && trimmed.contains("Qt::Key_");

        if is_case_label {
            let start = index;
            while index < lines.len() {
                let candidate = lines[index].trim();
                if !(candidate.starts_with("case ") && candidate.contains("Qt::Key_")) {
                    break;
                }
                index += 1;
            }
            let guard = case_body_guard(&lines, index);
            for (offset, line) in lines[start..index].iter().enumerate() {
                if let Some(key) = identifier_after(line, "Qt::Key_") {
                    record(
                        &mut findings,
                        name,
                        start + offset + 1,
                        &with_modifiers(key, &guard),
                        declared,
                    );
                }
            }
            continue;
        }

        if let Some(sequence) = identifier_after(lines[index], "QKeySequence::") {
            if let Some((_, key)) = STANDARD_SEQUENCES.iter().find(|(n, _)| *n == sequence) {
                record(&mut findings, name, index + 1, key, declared);
            }
        } else if let Some(key) = identifier_after(lines[index], "Qt::Key_") {
            let text = with_modifiers(key, lines[index]);
            record(&mut findings, name, index + 1, &text, declared);
        }
        index += 1;
    }

    findings
}

/// Collect the body shared by a run of `case` labels ending at `body_start`.
fn case_body_guard(lines: &[&str], body_start: usize) -> String {
    let mut guard = String::new();
    for line in lines.iter().skip(body_start).take(CASE_BODY_LOOKAHEAD) {
        let trimmed = line.trim();
        if trimmed.starts_with("case ") || trimmed == "}" || trimmed.contains("break;") {
            break;
        }
        guard.push_str(line);
    }
    guard
}

fn record(
    findings: &mut Vec<String>,
    name: &str,
    line: usize,
    key: &str,
    declared: &BTreeSet<String>,
) {
    if !declared.contains(key) {
        findings.push(format!(
            "{name}:{line}: key {key:?} bound in C++ but absent from ui-registry.toml [ADR-032]"
        ));
    }
}

/// Walk `root` for `.cc` files, skipping build output.
fn cxx_sources(root: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let Ok(entries) = std::fs::read_dir(root) else {
        return found;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "build") {
                continue;
            }
            found.extend(cxx_sources(&path));
        } else if path.extension().is_some_and(|ext| ext == "cc") {
            found.push(path);
        }
    }
    found.sort();
    found
}

fn main() -> ExitCode {
    let root = std::env::args()
        .nth(1)
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    let registry_path = root.join("shell").join("chrome").join("ui-registry.toml");
    let Ok(source) = std::fs::read_to_string(&registry_path) else {
        println!("FAIL: no registry at {}", registry_path.display());
        return ExitCode::FAILURE;
    };

    let findings = check_registry(&source);
    let declared: BTreeSet<String> = source
        .parse::<toml::Value>()
        .ok()
        .and_then(|doc| doc.get("shortcuts").and_then(toml::Value::as_table).cloned())
        .map(|table| {
            // Primary keys and their declared alternates alike: a C++ site that
            // binds `Down` is compliant when `nav.next_page` lists it. [ADR-032]
            let mut keys: BTreeSet<String> = BTreeSet::new();
            for entry in table.values() {
                if let Some(key) = entry.get("key").and_then(toml::Value::as_str) {
                    keys.insert(key.to_owned());
                }
                let alternates = entry
                    .get("alternates")
                    .and_then(toml::Value::as_array)
                    .map(Vec::as_slice)
                    .unwrap_or_default();
                for alternate in alternates.iter().filter_map(toml::Value::as_str) {
                    keys.insert(alternate.to_owned());
                }
            }
            keys
        })
        .unwrap_or_default();

    let mut cxx_findings = Vec::new();
    for path in cxx_sources(&root.join("shell")) {
        let name = path
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // A test that presses a key simulates a user; it does not define the
        // contract, and the rebinding test ADR-032 needs must be free to press
        // a key no shipped profile declares. Production source is held to the
        // ADR's actual words: no binding may appear in C++ at all, declared or
        // not, because a declared literal still puts the binding in two places.
        if name.ends_with("_test.cc") {
            continue;
        }
        cxx_findings.extend(check_cxx_source(&name, &text, &declared));
        cxx_findings.extend(check_no_cxx_bindings(&name, &text));
    }

    for finding in &findings {
        println!("registry: {finding}");
    }
    for finding in &cxx_findings {
        println!("cxx: {finding}");
    }

    let total = findings.len() + cxx_findings.len();
    if total > 0 {
        println!("\nFAIL: {total} ADR-032 violation(s)");
        return ExitCode::FAILURE;
    }
    println!("OK: ui-registry.toml conforms to ADR-032 (checked rules)");
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
schema_version = 1
profile_version = "0.1.0"

[shortcuts]
open_document = { key = "Ctrl+O", action = "document.open" }
find          = { key = "Ctrl+F", action = "document.find" }

[[menus]]
id = "file"
title = "&File"
items = [
  { action = "document.open", title = "&Open...", shortcut = "Ctrl+O" },
  { separator = true },
]
"#;

    fn declared(keys: &[&str]) -> BTreeSet<String> {
        keys.iter().map(|k| (*k).to_owned()).collect()
    }

    #[test]
    fn accepts_a_conformant_registry() {
        assert!(check_registry(VALID).is_empty());
    }

    #[test]
    fn rejects_schema_version_newer_than_supported() {
        let source = VALID.replace("schema_version = 1", "schema_version = 99");
        assert!(check_registry(&source)
            .iter()
            .any(|f| f.contains("schema_version")));
    }

    #[test]
    fn rejects_non_semver_profile_version() {
        let source = VALID.replace(r#"profile_version = "0.1.0""#, r#"profile_version = "1""#);
        assert!(check_registry(&source)
            .iter()
            .any(|f| f.contains("profile_version")));
    }

    #[test]
    fn rejects_two_actions_bound_to_the_same_key() {
        let source = VALID.replace(r#"key = "Ctrl+F""#, r#"key = "Ctrl+O""#);
        assert!(check_registry(&source)
            .iter()
            .any(|f| f.contains("duplicate key")));
    }

    #[test]
    fn rejects_menu_action_absent_from_shortcuts_table() {
        let source = VALID.replace(r#"action = "document.open", title"#, r#"action = "document.ghost", title"#);
        assert!(check_registry(&source)
            .iter()
            .any(|f| f.contains("document.ghost")));
    }

    #[test]
    fn rejects_menu_shortcut_disagreeing_with_shortcuts_table() {
        let source = VALID.replace(r#"shortcut = "Ctrl+O""#, r#"shortcut = "Ctrl+P""#);
        assert!(check_registry(&source)
            .iter()
            .any(|f| f.contains("disagrees")));
    }

    #[test]
    fn rejects_menu_item_that_is_neither_action_nor_separator() {
        let source = VALID.replace("{ separator = true },", r#"{ title = "orphan" },"#);
        assert!(check_registry(&source)
            .iter()
            .any(|f| f.contains("neither")));
    }

    #[test]
    fn one_action_may_declare_several_keys() {
        let source = VALID.replace(
            r#"find          = { key = "Ctrl+F", action = "document.find" }"#,
            "find     = { key = \"Ctrl+F\", action = \"document.find\" }\n\
             find_alt = { key = \"F3\", action = \"document.find\" }",
        );
        assert!(check_registry(&source).is_empty());
    }

    #[test]
    fn menu_shortcut_may_name_any_declared_key_for_its_action() {
        // The menu still shows the primary key while an alternate exists.
        let source = VALID.replace(
            r#"open_document = { key = "Ctrl+O", action = "document.open" }"#,
            "open_document = { key = \"Ctrl+O\", action = \"document.open\" }\n\
             open_alt      = { key = \"Ctrl+Shift+O\", action = \"document.open\" }",
        );
        assert!(check_registry(&source).is_empty());
    }

    #[test]
    fn flags_a_shortcut_bound_in_cxx_but_absent_from_the_registry() {
        let source = "if (event->modifiers() & Qt::ControlModifier \
                      && event->key() == Qt::Key_S) { save(); }";
        let findings = check_cxx_source("canvas.cc", source, &declared(&["Ctrl+O"]));
        assert!(findings.iter().any(|f| f.contains("Ctrl+S")), "{findings:?}");
    }

    #[test]
    fn reports_the_file_and_line_of_each_cxx_binding() {
        let source = "\n\nif (event->matches(QKeySequence::Copy)) {}\n";
        let findings = check_cxx_source("canvas.cc", source, &declared(&[]));
        assert!(
            findings.iter().any(|f| f.contains("canvas.cc:3")),
            "{findings:?}"
        );
    }

    #[test]
    fn accepts_cxx_bindings_that_the_registry_declares() {
        let source = "if (event->matches(QKeySequence::Open)) {}";
        assert!(check_cxx_source("canvas.cc", source, &declared(&["Ctrl+O"])).is_empty());
    }

    #[test]
    fn stacked_case_labels_inherit_the_guard_from_their_shared_body() {
        let source = "switch (event->key()) {\n\
                      case Qt::Key_Plus:\n\
                      case Qt::Key_Equal:\n\
                          if (event->modifiers() & Qt::ControlModifier) {\n\
                              zoom(1);\n\
                              return;\n\
                          }\n\
                          break;\n\
                      }\n";
        let findings = check_cxx_source("canvas.cc", source, &declared(&[]));
        assert!(findings.iter().any(|f| f.contains("Ctrl++")), "{findings:?}");
        assert!(findings.iter().any(|f| f.contains("Ctrl+=")), "{findings:?}");
        assert!(
            !findings.iter().any(|f| f.contains(r#""+""#)),
            "{findings:?}"
        );
    }

    #[test]
    fn case_labels_with_an_unguarded_body_stay_bare_keys() {
        let source = "switch (event->key()) {\n\
                      case Qt::Key_PageDown:\n\
                      case Qt::Key_Space:\n\
                          step(1);\n\
                          return;\n\
                      }\n";
        let findings = check_cxx_source("canvas.cc", source, &declared(&[]));
        assert!(
            findings.iter().any(|f| f.contains("PageDown")),
            "{findings:?}"
        );
        assert!(findings.iter().any(|f| f.contains("Space")), "{findings:?}");
        assert!(!findings.iter().any(|f| f.contains("Ctrl+")), "{findings:?}");
    }

    #[test]
    fn production_source_may_not_bind_a_key_even_when_declared() {
        // The declared-key check would pass this; the contract still forbids it.
        let source = "if (event->key() == Qt::Key_PageDown) { next(); }";
        let findings = check_no_cxx_bindings("canvas.cc", source);
        assert_eq!(findings.len(), 1, "{findings:?}");
        assert!(findings[0].contains("ui-registry.toml"), "{findings:?}");
    }

    #[test]
    fn production_source_may_not_use_qt_standard_sequences() {
        let source = "if (event->matches(QKeySequence::Copy)) { copy(); }";
        assert_eq!(check_no_cxx_bindings("canvas.cc", source).len(), 1);
    }

    #[test]
    fn match_results_are_not_bindings() {
        let source = "if (pressed.matches(key) == QKeySequence::ExactMatch) { return true; }";
        assert!(check_no_cxx_bindings("registry.cc", source).is_empty());
    }

    #[test]
    fn registry_alternates_count_as_declared() {
        let registry = "schema_version = 1
profile_version = \"1.0.0\"
                        [shortcuts]
                        next_page = { key = \"PageDown\", action = \"nav.next\", alternates = [\"Down\"] }
";
        let findings = check_registry(registry);
        assert!(findings.is_empty(), "{findings:?}");
    }
}
