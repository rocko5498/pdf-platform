# Forms JavaScript Compatibility Table (Living)

**Cites:** ADR-017, FR-JS-*, SDS §14 M5  
**Engine:** `pdf_model::forms_js`  
**Principle:** unsupported constructs **no-op + log**, never silent wrong numbers.

## Supported (subset)

| Construct | Notes |
|---|---|
| `AFSimple_Calculate("SUM", [...])` | Sum of named fields |
| `AFSimple_Calculate("PRD", [...])` | Product |
| `AFSimple_Calculate("AVG", [...])` | Average |
| `AFSimple_Calculate("MIN"/"MAX", [...])` | Min / max |
| `getField("name")` / `this.getField("name")` | Numeric field get |
| `+ - * / ( )` arithmetic | Over numbers and getField |
| Kill switch | `AcroForm.javascript_enabled = false` |

## Explicitly unsupported (logged)

| Construct | Behavior |
|---|---|
| `app.*` | Unsupported |
| `doc.*` / file I/O | Unsupported |
| Network / `XMLHttp` | Unsupported |
| Arbitrary `function` / `eval` | Unsupported |
| Full AFNumber_Keystroke side effects | Not emulated |

## Wire path (M5)

| Layer | API |
|---|---|
| Pure eval | `pdf_model::forms_js::evaluate_expression` / `run_form_calculations` |
| Widget `/AP` | `generate_widget_appearance` / `build_widget_pdf_objects` / `AcroForm::regenerate_appearances` |
| COS import | `pdf_cos::acroform::extract_acroform_fields` → `pdf_model::form_import::import_acroform_from_bytes` |
| Protocol | `Command::FormsCalc` → `WorkerEvent::FormsCalcResult` |
| Session | `WorkerSession::forms_calc(expr, fields, enabled)` |
| FFI / shell | open imports fields; `list/set/calc/seed/reload` + JS kill switch |
| CLI demo | `pdf-platform forms-calc-demo` (local AcroForm + AP regen) |
| Shell panel | Forms dock: seed demo, edit, Apply, Calc, JS on/off (`Ctrl+G` = calc) |

**Appearance:** after values change, regenerate widget `/AP` before save (same honesty rule as annotations).

**COS import:** on open, classic-xref documents with `/AcroForm` leaf widgets are loaded into the session form (name, type, value, rect, calc JS). Nested Kids walked with a depth bound. Compressed xref / full field-tree edge cases surface as honesty notes, not silent success (PRIN-6).

## How to extend

1. Add parser path in `forms_js.rs` with unit tests.  
2. Document here.  
3. Prefer Z1 worker execution for untrusted expressions (now on wire via `FormsCalc`).
