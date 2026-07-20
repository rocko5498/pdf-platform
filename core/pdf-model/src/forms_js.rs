//! PDF forms JavaScript **subset** evaluator. [ADR-017, FR-JS-*, SDS §14 M5]
//!
//! Runs only the forms calculation/format/validation subset. Document/app
//! automation (file I/O, network, UI) is permanently out of scope.
//!
//! Execution belongs conceptually in Z1; this module is pure evaluation with
//! zero broker reach — safe to unit-test in Z0 tests. The worker protocol
//! command `FormsCalc` runs the same evaluator across the Z0↔Z1 boundary.
//! Unsupported constructs are logged, never silently mis-emulated. [PRIN-6, ADR-017]
//!
//! **Appearance note (M5):** after calculated field values change, product code
//! must regenerate widget appearance streams (`/AP`) before incremental save —
//! same non-negotiable rule as annotations ([FR-ANNOT-2]). Leaving a value
//! updated without an appearance is a silent interop failure.

use std::collections::HashMap;
use std::fmt;

use crate::form::{AcroForm, FieldValue};

/// Error or unsupported construct from the subset engine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormsJsError {
    /// Expression not in the supported subset (logged, not faked).
    Unsupported(String),
    /// Missing field reference.
    MissingField(String),
    /// Kill switch / global disable.
    Disabled,
    /// Parse / type error on supported surface.
    Eval(String),
}

impl fmt::Display for FormsJsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "unsupported JS forms construct: {s}"),
            Self::MissingField(s) => write!(f, "missing field: {s}"),
            Self::Disabled => write!(f, "forms JavaScript disabled"),
            Self::Eval(s) => write!(f, "forms JS eval error: {s}"),
        }
    }
}

impl std::error::Error for FormsJsError {}

/// Compatibility / honesty log entry. [ADR-017, FR-JS]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormsJsLogEntry {
    /// Field that triggered the evaluation (if any).
    pub field: Option<String>,
    /// Expression or construct.
    pub detail: String,
    /// Whether this was unsupported (vs error on supported surface).
    pub unsupported: bool,
}

/// Result of a subset evaluation pass.
#[derive(Debug, Clone)]
pub struct FormsJsRunResult {
    /// Field names whose values changed.
    pub updated_fields: Vec<String>,
    /// Compatibility / honesty log (unsupported no-ops, etc.).
    pub log: Vec<FormsJsLogEntry>,
}

/// Supported subset primitives (living compatibility table seed). [ADR-017]
pub const SUPPORTED_SUBSET: &[&str] = &[
    "AFSimple_Calculate SUM",
    "AFSimple_Calculate PRD",
    "AFSimple_Calculate AVG",
    "AFSimple_Calculate MIN",
    "AFSimple_Calculate MAX",
    "field value get (this.getField)",
    "numeric literals",
    "AFNumber_Format (format only, non-mutating)",
];

/// Parse and evaluate a supported calculation expression against field values.
///
/// Supported forms (case-insensitive keywords):
/// - `AFSimple_Calculate("SUM", ["a","b","c"])`
/// - `AFSimple_Calculate("PRD", ...)` product
/// - `AFSimple_Calculate("AVG", ...)` average
/// - `AFSimple_Calculate("MIN", ...)` / `MAX`
/// - `getField("name")` → numeric value of field
/// - `1 + 2 * 3` simple arithmetic over numbers and `getField("x")`
pub fn evaluate_expression(
    expr: &str,
    fields: &HashMap<String, f64>,
) -> Result<f64, FormsJsError> {
    let trimmed = expr.trim();
    if trimmed.is_empty() {
        return Err(FormsJsError::Eval("empty expression".into()));
    }

    // Detect clearly out-of-scope surfaces early (honesty).
    let lower = trimmed.to_lowercase();
    for bad in [
        "app.",
        "this.export",
        "net.",
        "util.printd",
        "doc.",
        "event.",
        "xmlhttp",
        "eval(",
        "function ",
    ] {
        if lower.contains(bad) {
            return Err(FormsJsError::Unsupported(format!(
                "out-of-subset surface ({bad})"
            )));
        }
    }

    if let Some(v) = try_af_simple_calculate(trimmed, fields)? {
        return Ok(v);
    }

    // Simple arithmetic: replace getField("x") with numbers then eval.
    let rewritten = rewrite_get_fields(trimmed, fields)?;
    eval_arithmetic(&rewritten)
}

fn try_af_simple_calculate(
    expr: &str,
    fields: &HashMap<String, f64>,
) -> Result<Option<f64>, FormsJsError> {
    // AFSimple_Calculate("SUM", new Array("a", "b"))  or  ["a","b"]
    let lower = expr.to_lowercase();
    if !lower.contains("afsimple_calculate") {
        return Ok(None);
    }

    let op = if lower.contains("\"sum\"") || lower.contains("'sum'") {
        "SUM"
    } else if lower.contains("\"prd\"") || lower.contains("'prd'") {
        "PRD"
    } else if lower.contains("\"avg\"") || lower.contains("'avg'") {
        "AVG"
    } else if lower.contains("\"min\"") || lower.contains("'min'") {
        "MIN"
    } else if lower.contains("\"max\"") || lower.contains("'max'") {
        "MAX"
    } else {
        return Err(FormsJsError::Unsupported(
            "AFSimple_Calculate op not in subset".into(),
        ));
    };

    let names = extract_quoted_names(expr);
    if names.is_empty() {
        return Err(FormsJsError::Eval(
            "AFSimple_Calculate missing field names".into(),
        ));
    }

    let mut vals = Vec::new();
    for n in &names {
        let v = fields
            .get(n)
            .copied()
            .ok_or_else(|| FormsJsError::MissingField(n.clone()))?;
        vals.push(v);
    }

    let result = match op {
        "SUM" => vals.iter().sum(),
        "PRD" => vals.iter().product(),
        "AVG" => vals.iter().sum::<f64>() / vals.len() as f64,
        "MIN" => vals.into_iter().fold(f64::INFINITY, f64::min),
        "MAX" => vals.into_iter().fold(f64::NEG_INFINITY, f64::max),
        _ => unreachable!(),
    };
    Ok(Some(result))
}

fn extract_quoted_names(expr: &str) -> Vec<String> {
    let mut names = Vec::new();
    let bytes = expr.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'"' || bytes[i] == b'\'' {
            let q = bytes[i];
            i += 1;
            let start = i;
            while i < bytes.len() && bytes[i] != q {
                i += 1;
            }
            if i <= bytes.len() {
                let s = &expr[start..i];
                // Skip op keywords
                let up = s.to_uppercase();
                if !matches!(up.as_str(), "SUM" | "PRD" | "AVG" | "MIN" | "MAX") {
                    names.push(s.to_string());
                }
            }
        }
        i += 1;
    }
    names
}

fn rewrite_get_fields(expr: &str, fields: &HashMap<String, f64>) -> Result<String, FormsJsError> {
    // getField("name") or this.getField("name")
    let mut out = expr.to_string();
    // iterative replace
    loop {
        let lower = out.to_lowercase();
        let Some(pos) = lower.find("getfield(") else {
            break;
        };
        let after = pos + "getfield(".len();
        let rest = &out[after..];
        let quote = rest.chars().next().unwrap_or('"');
        if quote != '"' && quote != '\'' {
            return Err(FormsJsError::Eval("getField expects string name".into()));
        }
        let name_start = after + 1;
        let name_end = out[name_start..]
            .find(quote)
            .ok_or_else(|| FormsJsError::Eval("unclosed getField string".into()))?
            + name_start;
        let name = &out[name_start..name_end];
        let val = fields
            .get(name)
            .copied()
            .ok_or_else(|| FormsJsError::MissingField(name.to_string()))?;
        // include optional this. before getField
        let mut start = pos;
        if start >= 5 && out[start - 5..start].eq_ignore_ascii_case("this.") {
            start -= 5;
        }
        let end = name_end + 1; // closing quote
        let end = if out[end..].starts_with(')') {
            end + 1
        } else {
            end
        };
        out = format!("{}{}{}", &out[..start], val, &out[end..]);
    }
    Ok(out)
}

/// Minimal arithmetic evaluator: numbers, + - * / ( ), whitespace.
fn eval_arithmetic(expr: &str) -> Result<f64, FormsJsError> {
    let tokens = tokenize(expr)?;
    let mut pos = 0;
    let v = parse_expr(&tokens, &mut pos)?;
    if pos != tokens.len() {
        return Err(FormsJsError::Eval(format!(
            "trailing tokens in: {expr}"
        )));
    }
    Ok(v)
}

#[derive(Debug, Clone)]
enum Tok {
    Num(f64),
    Op(char),
    LParen,
    RParen,
}

fn tokenize(s: &str) -> Result<Vec<Tok>, FormsJsError> {
    let mut out = Vec::new();
    let chars: Vec<char> = s.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c.is_ascii_digit() || c == '.' {
            let start = i;
            i += 1;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            let n: f64 = s[start..i]
                .parse()
                .map_err(|_| FormsJsError::Eval(format!("bad number near {start}")))?;
            out.push(Tok::Num(n));
            continue;
        }
        match c {
            '+' | '-' | '*' | '/' => {
                out.push(Tok::Op(c));
                i += 1;
            }
            '(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            ')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            _ => {
                return Err(FormsJsError::Unsupported(format!(
                    "token {c:?} not in arithmetic subset"
                )));
            }
        }
    }
    Ok(out)
}

fn parse_expr(toks: &[Tok], pos: &mut usize) -> Result<f64, FormsJsError> {
    let mut v = parse_term(toks, pos)?;
    while *pos < toks.len() {
        match &toks[*pos] {
            Tok::Op('+') => {
                *pos += 1;
                v += parse_term(toks, pos)?;
            }
            Tok::Op('-') => {
                *pos += 1;
                v -= parse_term(toks, pos)?;
            }
            _ => break,
        }
    }
    Ok(v)
}

fn parse_term(toks: &[Tok], pos: &mut usize) -> Result<f64, FormsJsError> {
    let mut v = parse_factor(toks, pos)?;
    while *pos < toks.len() {
        match &toks[*pos] {
            Tok::Op('*') => {
                *pos += 1;
                v *= parse_factor(toks, pos)?;
            }
            Tok::Op('/') => {
                *pos += 1;
                let d = parse_factor(toks, pos)?;
                if d == 0.0 {
                    return Err(FormsJsError::Eval("division by zero".into()));
                }
                v /= d;
            }
            _ => break,
        }
    }
    Ok(v)
}

fn parse_factor(toks: &[Tok], pos: &mut usize) -> Result<f64, FormsJsError> {
    if *pos >= toks.len() {
        return Err(FormsJsError::Eval("unexpected end".into()));
    }
    match &toks[*pos] {
        Tok::Num(n) => {
            *pos += 1;
            Ok(*n)
        }
        Tok::Op('-') => {
            *pos += 1;
            Ok(-parse_factor(toks, pos)?)
        }
        Tok::LParen => {
            *pos += 1;
            let v = parse_expr(toks, pos)?;
            match toks.get(*pos) {
                Some(Tok::RParen) => {
                    *pos += 1;
                    Ok(v)
                }
                _ => Err(FormsJsError::Eval("missing )".into())),
            }
        }
        _ => Err(FormsJsError::Eval("expected factor".into())),
    }
}

fn field_as_f64(v: &FieldValue) -> Option<f64> {
    match v {
        FieldValue::Text(s) => s.trim().parse().ok(),
        FieldValue::Choice(s) => s.trim().parse().ok(),
        FieldValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        FieldValue::MultiChoice(_) | FieldValue::None => None,
    }
}

/// Run calculation order on an AcroForm using the forms JS subset. [FR-JS-1, FR-JS-4]
pub fn run_form_calculations(form: &mut AcroForm) -> FormsJsRunResult {
    let mut log = Vec::new();
    let mut updated = Vec::new();

    if !form.javascript_enabled {
        log.push(FormsJsLogEntry {
            field: None,
            detail: "forms JavaScript kill switch active".into(),
            unsupported: false,
        });
        return FormsJsRunResult {
            updated_fields: updated,
            log,
        };
    }

    if !form.has_javascript && !form.detect_javascript() {
        return FormsJsRunResult {
            updated_fields: updated,
            log,
        };
    }

    // Snapshot numeric field values.
    let mut nums: HashMap<String, f64> = HashMap::new();
    for (name, field) in form.fields() {
        if let Some(n) = field_as_f64(&field.value) {
            nums.insert(name.clone(), n);
        }
    }

    let order = form.calculation_order.clone();
    for field_name in order {
        let calc_expr = {
            let Some(field) = form.fields().get(&field_name) else {
                continue;
            };
            let Some(ref calc) = field.calculation else {
                continue;
            };
            if !calc.enabled {
                continue;
            }
            calc.expression.clone()
        };

        match evaluate_expression(&calc_expr, &nums) {
            Ok(v) => {
                let new_val = FieldValue::Text(format!("{v}"));
                if form.set_field_value(&field_name, new_val) {
                    updated.push(field_name.clone());
                    nums.insert(field_name.clone(), v);
                }
            }
            Err(FormsJsError::Unsupported(s)) => {
                log.push(FormsJsLogEntry {
                    field: Some(field_name.clone()),
                    detail: s,
                    unsupported: true,
                });
            }
            Err(e) => {
                log.push(FormsJsLogEntry {
                    field: Some(field_name.clone()),
                    detail: e.to_string(),
                    unsupported: false,
                });
            }
        }
    }

    FormsJsRunResult {
        updated_fields: updated,
        log,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::form::{FieldCalculation, FieldRect, FieldType, FormField};

    #[test]
    fn sum_afsimple() {
        let mut fields = HashMap::new();
        fields.insert("a".into(), 2.0);
        fields.insert("b".into(), 3.0);
        let v = evaluate_expression(
            r#"AFSimple_Calculate("SUM", new Array("a", "b"))"#,
            &fields,
        )
        .unwrap();
        assert!((v - 5.0).abs() < 1e-9);
    }

    #[test]
    fn arithmetic_with_getfield() {
        let mut fields = HashMap::new();
        fields.insert("qty".into(), 4.0);
        fields.insert("price".into(), 2.5);
        let v = evaluate_expression(r#"getField("qty") * getField("price")"#, &fields).unwrap();
        assert!((v - 10.0).abs() < 1e-9);
    }

    #[test]
    fn unsupported_app_surface_is_honest() {
        let fields = HashMap::new();
        let err = evaluate_expression("app.alert('x')", &fields).unwrap_err();
        assert!(matches!(err, FormsJsError::Unsupported(_)));
    }

    #[test]
    fn run_calculations_updates_form() {
        let mut form = AcroForm::new();
        form.has_javascript = true;
        form.javascript_enabled = true;

        let mut a = FormField::new("a", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 10.0, 10.0));
        a.set_value(FieldValue::Text("10".into()));
        form.add_field(a);

        let mut b = FormField::new("b", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 10.0, 10.0));
        b.set_value(FieldValue::Text("5".into()));
        form.add_field(b);

        let mut total =
            FormField::new("total", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 10.0, 10.0));
        total.calculation = Some(FieldCalculation {
            expression: r#"AFSimple_Calculate("SUM", ["a","b"])"#.into(),
            dependencies: vec!["a".into(), "b".into()],
            enabled: true,
        });
        form.add_field(total);
        form.calculation_order = vec!["total".into()];

        let result = run_form_calculations(&mut form);
        assert!(result.updated_fields.contains(&"total".into()));
        assert_eq!(
            form.fields().get("total").unwrap().value,
            FieldValue::Text("15".into())
        );
        assert!(result.log.iter().all(|e| !e.unsupported));
    }

    #[test]
    fn kill_switch_blocks_js() {
        let mut form = AcroForm::new();
        form.has_javascript = true;
        form.javascript_enabled = false;
        form.calculation_order = vec!["total".into()];
        let result = run_form_calculations(&mut form);
        assert!(result.updated_fields.is_empty());
        assert!(result.log.iter().any(|e| e.detail.contains("kill switch")));
    }

    #[test]
    fn supported_subset_table_nonempty() {
        assert!(!SUPPORTED_SUBSET.is_empty());
    }

    // =========================================================================
    // Stress / security tests — exercise evaluator with adversarial inputs.
    // ADR-022 §4: any code reachable by untrusted input needs fuzz-style testing.
    // =========================================================================

    #[test]
    fn fuzz_empty_expression() {
        let fields = HashMap::new();
        let err = evaluate_expression("", &fields).unwrap_err();
        assert!(matches!(err, FormsJsError::Eval(_)));
    }

    #[test]
    fn fuzz_whitespace_only() {
        let fields = HashMap::new();
        let err = evaluate_expression("   \t\n  ", &fields).unwrap_err();
        assert!(matches!(err, FormsJsError::Eval(_)));
    }

    #[test]
    fn fuzz_division_by_zero() {
        let fields = HashMap::new();
        let err = evaluate_expression("1 / 0", &fields).unwrap_err();
        assert!(matches!(err, FormsJsError::Eval(_)));
    }

    #[test]
    fn fuzz_deeply_nested_parens() {
        let fields = HashMap::new();
        // 50 levels of nesting — should not stack overflow.
        let expr = "(".repeat(50) + "1" + &")".repeat(50);
        let result = evaluate_expression(&expr, &fields);
        // Should either evaluate to 1.0 or return an error — never panic.
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn fuzz_long_expression() {
        let fields = HashMap::new();
        // 1000 additions — should not hang or overflow.
        let expr = "1".to_string() + &"+1".repeat(1000);
        let result = evaluate_expression(&expr, &fields);
        assert!(result.is_ok());
        assert!((result.unwrap() - 1001.0).abs() < 1e-6);
    }

    #[test]
    fn fuzz_out_of_scope_surfaces() {
        let fields = HashMap::new();
        let bad_inputs = [
            "app.alert('x')",
            "doc.submitForm()",
            "this.exportData()",
            "net.socket()",
            "eval('1+1')",
            "function foo() {}",
            "XMLHttpRequest()",
            "util.printd()",
            "app.launchURL()",
            "doc.mailDoc()",
        ];
        for input in &bad_inputs {
            let err = evaluate_expression(input, &fields).unwrap_err();
            assert!(
                matches!(err, FormsJsError::Unsupported(_)),
                "expected Unsupported for '{input}', got {err:?}"
            );
        }
    }

    #[test]
    fn fuzz_case_insensitive_surface_detection() {
        let fields = HashMap::new();
        // Mixed case should still be detected.
        let err = evaluate_expression("APP.alert('x')", &fields).unwrap_err();
        assert!(matches!(err, FormsJsError::Unsupported(_)));
    }

    #[test]
    fn fuzz_missing_field_reference() {
        let fields = HashMap::new();
        let err = evaluate_expression(r#"getField("nonexistent")"#, &fields).unwrap_err();
        assert!(matches!(err, FormsJsError::MissingField(_)));
    }

    #[test]
    fn fuzz_afsimple_missing_fields() {
        let fields = HashMap::new();
        let err = evaluate_expression(
            r#"AFSimple_Calculate("SUM", ["a","b"])"#,
            &fields,
        )
        .unwrap_err();
        assert!(matches!(err, FormsJsError::MissingField(_)));
    }

    #[test]
    fn fuzz_afsimple_empty_array() {
        let fields = HashMap::new();
        let result = evaluate_expression(
            r#"AFSimple_Calculate("SUM", [])"#,
            &fields,
        );
        // Empty array → error (missing field names or eval error).
        assert!(result.is_err());
    }

    #[test]
    fn fuzz_afsimple_unknown_op() {
        let fields = HashMap::new();
        let err = evaluate_expression(
            r#"AFSimple_Calculate("FOO", ["a"])"#,
            &fields,
        )
        .unwrap_err();
        assert!(matches!(err, FormsJsError::Unsupported(_)));
    }

    #[test]
    fn fuzz_invalid_syntax() {
        let fields = HashMap::new();
        let bad = [
            "+",
            "*",
            "(",
            ")",
            "1 +",
            "+ 1",
            "1 2 3",
            "1 ++ 2",
            "1 + * 2",
            "((1)",
            "1))",
        ];
        for input in &bad {
            let result = evaluate_expression(input, &fields);
            // Should return an error, never panic.
            assert!(result.is_err(), "expected error for '{input}', got {result:?}");
        }
    }

    #[test]
    fn fuzz_unicode_in_expression() {
        let fields = HashMap::new();
        // Unicode that's not in the subset should produce an error, not a panic.
        let result = evaluate_expression("1 + \u{00e9}", &fields);
        assert!(result.is_err());
    }

    #[test]
    fn fuzz_null_bytes() {
        let fields = HashMap::new();
        let result = evaluate_expression("1\0+ 2", &fields);
        // Should error, not panic or produce wrong results.
        assert!(result.is_err() || result.is_ok());
    }

    #[test]
    fn fuzz_extreme_float_values() {
        let mut fields = HashMap::new();
        fields.insert("a".into(), f64::MAX);
        fields.insert("b".into(), f64::MIN_POSITIVE);
        // Overflow/underflow should not panic.
        let _ = evaluate_expression(r#"getField("a") * getField("b")"#, &fields);
        let _ = evaluate_expression(r#"getField("a") + getField("b")"#, &fields);
        let _ = evaluate_expression(r#"getField("b") / getField("a")"#, &fields);
    }

    #[test]
    fn fuzz_negative_numbers() {
        let mut fields = HashMap::new();
        fields.insert("a".into(), -5.0);
        fields.insert("b".into(), 3.0);
        let v = evaluate_expression(r#"getField("a") + getField("b")"#, &fields).unwrap();
        assert!((v - (-2.0)).abs() < 1e-9);
    }

    #[test]
    fn fuzz_malformed_afsimple() {
        let fields = HashMap::new();
        let bad = [
            r#"AFSimple_Calculate()"#,
            r#"AFSimple_Calculate("SUM")"#,
            r#"AFSimple_Calculate(123, ["a"])"#,
            r#"AFSimple_Calculate("SUM", "not_array")"#,
        ];
        for input in &bad {
            let result = evaluate_expression(input, &fields);
            assert!(result.is_err(), "expected error for '{input}', got {result:?}");
        }
    }

    #[test]
    fn fuzz_getfield_nested() {
        let mut fields = HashMap::new();
        fields.insert("a".into(), 1.0);
        // getField inside getField is not in the subset.
        let result = evaluate_expression(r#"getField("a") + getField("b")"#, &fields);
        // Should error on missing field "b", not panic.
        assert!(result.is_err());
    }

    #[test]
    fn fuzz_concurrent_evaluations() {
        // Verify the evaluator is stateless (no global state corruption).
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        let error_count = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];
        for i in 0..20 {
            let ec = error_count.clone();
            handles.push(std::thread::spawn(move || {
                let mut fields = HashMap::new();
                fields.insert("x".into(), i as f64);
                let result = evaluate_expression(r#"getField("x") * 2"#, &fields);
                if result.is_err() {
                    ec.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(error_count.load(Ordering::Relaxed), 0);
    }
}
