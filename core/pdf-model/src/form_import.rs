//! Import AcroForm fields from PDF bytes into the session model. [FR-FORM-1, SDS §14 M5]
//!
//! COS scan lives in `pdf_cos::acroform`; this module maps into [`AcroForm`]
//! and regenerates widget appearances for honesty with other readers.

use pdf_cos::acroform::{extract_acroform_fields, AcroFormScan, ScannedFormField};

use crate::form::{
    AcroForm, FieldCalculation, FieldRect, FieldType, FieldValue, FormField, ValidationRule,
};

/// Outcome of loading form fields from a PDF.
#[derive(Debug, Clone)]
pub struct FormImportResult {
    /// Populated AcroForm (may be empty if document has no fields).
    pub form: AcroForm,
    /// Number of fields imported.
    pub field_count: u32,
    /// Honesty notes from the COS scan.
    pub notes: Vec<String>,
}

/// Load AcroForm fields from PDF file bytes into a session model. [FR-FORM-1]
pub fn import_acroform_from_bytes(data: &[u8]) -> Result<FormImportResult, String> {
    let scan = extract_acroform_fields(data)?;
    Ok(scan_to_acroform(scan))
}

/// Convert a COS scan into an [`AcroForm`], regenerating appearances.
pub fn scan_to_acroform(scan: AcroFormScan) -> FormImportResult {
    let mut form = AcroForm::new();
    let mut notes = scan.notes;

    for sf in scan.fields {
        form.add_field(scanned_to_field(sf));
    }

    if !scan.calculation_order.is_empty() {
        form.calculation_order = scan.calculation_order;
    } else {
        // Fields with calculations in document order.
        form.calculation_order = form
            .fields()
            .iter()
            .filter(|(_, f)| f.calculation.is_some())
            .map(|(n, _)| n.clone())
            .collect();
    }

    form.has_javascript = form.detect_javascript() || !form.calculation_order.is_empty();
    form.javascript_enabled = true;

    if scan.need_appearances {
        notes.push("NeedAppearances=true; regenerated session appearances".into());
    }

    let n = form.regenerate_appearances();
    if n > 0 {
        notes.push(format!("regenerated appearances for {n} fields"));
    }

    let field_count = form.field_count() as u32;
    FormImportResult {
        form,
        field_count,
        notes,
    }
}

fn scanned_to_field(sf: ScannedFormField) -> FormField {
    let field_type = match sf.field_type.as_str() {
        "Tx" => FieldType::Text,
        "Btn" => {
            // Checkbox vs radio: radio often has parent kids; leaf Btn with Yes/Off is checkbox.
            FieldType::Checkbox
        }
        "Ch" => FieldType::ComboBox,
        "Sig" => FieldType::Signature,
        _ => FieldType::Text,
    };

    let rect = match sf.rect {
        Some([x0, y0, x1, y1]) => {
            let x = x0.min(x1);
            let y = y0.min(y1);
            let w = (x1 - x0).abs().max(1.0);
            let h = (y1 - y0).abs().max(1.0);
            FieldRect::new(x, y, w, h)
        }
        None => FieldRect::new(0.0, 0.0, 100.0, 20.0),
    };

    let mut field = FormField::new(sf.name.clone(), field_type, sf.page_index, rect);
    field.fully_qualified_name = sf.name;
    field.tab_order = sf.tab_order;
    field.read_only = sf.read_only;
    field.required = sf.required;
    field.widget_obj_num = sf.widget_obj_num;

    if field.required {
        field.validation.push(ValidationRule::Required);
    }

    field.value = value_for_type(field_type, &sf.value);
    field.default_value = field.value.clone();

    if let Some(expr) = sf.calculation {
        field.calculation = Some(FieldCalculation {
            expression: expr,
            dependencies: Vec::new(),
            enabled: true,
        });
        // Calculated fields are effectively read-only for direct edit.
        field.read_only = true;
    }

    field
}

fn value_for_type(ty: FieldType, raw: &str) -> FieldValue {
    if raw.is_empty() {
        return FieldValue::None;
    }
    match ty {
        FieldType::Checkbox => {
            let on = !matches!(raw, "Off" | "No" | "false" | "0" | "");
            FieldValue::Bool(on && raw != "Off")
        }
        FieldType::RadioButton | FieldType::ComboBox | FieldType::ListBox => {
            FieldValue::Choice(raw.to_string())
        }
        _ => FieldValue::Text(raw.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forms_js::run_form_calculations;

    fn form_pdf() -> Vec<u8> {
        let mut body = Vec::new();
        body.extend_from_slice(b"%PDF-1.4\n");
        let o1 = body.len();
        body.extend_from_slice(
            b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R /AcroForm 4 0 R >>\nendobj\n",
        );
        let o2 = body.len();
        body.extend_from_slice(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n");
        let o3 = body.len();
        body.extend_from_slice(
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R 6 0 R 7 0 R] >>\nendobj\n",
        );
        let o4 = body.len();
        body.extend_from_slice(
            b"4 0 obj\n<< /Fields [5 0 R 6 0 R 7 0 R] /CO [(total)] >>\nendobj\n",
        );
        let o5 = body.len();
        body.extend_from_slice(
            b"5 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (a) /V (10) /Rect [72 700 152 718] /P 3 0 R >>\nendobj\n",
        );
        let o6 = body.len();
        body.extend_from_slice(
            b"6 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (b) /V (5) /Rect [72 670 152 688] /P 3 0 R >>\nendobj\n",
        );
        let o7 = body.len();
        body.extend_from_slice(
            b"7 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (total) /V () /Rect [72 640 152 658] /P 3 0 R \
/AA << /C << /S /JavaScript /JS (AFSimple_Calculate(\"SUM\", [\"a\",\"b\"])) >> >> >>\nendobj\n",
        );
        let xref_at = body.len();
        body.extend_from_slice(b"xref\n0 8\n");
        body.extend_from_slice(b"0000000000 65535 f \n");
        for off in [o1, o2, o3, o4, o5, o6, o7] {
            body.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        body.extend_from_slice(b"trailer\n<< /Size 8 /Root 1 0 R >>\nstartxref\n");
        body.extend_from_slice(format!("{xref_at}\n").as_bytes());
        body.extend_from_slice(b"%%EOF\n");
        body
    }

    #[test]
    fn import_and_calc_from_bytes() {
        let r = import_acroform_from_bytes(&form_pdf()).expect("import");
        assert_eq!(r.field_count, 3);
        assert!(r.form.field("a").is_some());
        assert!(r.form.field("total").unwrap().appearance.is_some());

        let mut form = r.form;
        let calc = run_form_calculations(&mut form);
        assert!(calc.updated_fields.contains(&"total".to_string()));
        assert_eq!(form.field("total").unwrap().value.display(), "15");
    }
}
