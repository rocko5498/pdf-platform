//! Import AcroForm fields from PDF bytes into the session model. [FR-FORM-1, SDS §14 M5]
//!
//! COS scan lives in `pdf_cos::acroform`; this module maps into [`AcroForm`]
//! and regenerates widget appearances for honesty with other readers.

use pdf_cos::acroform::{extract_acroform_fields, AcroFormScan, ScannedFormField};

use crate::form::{
    AcroForm, FieldCalculation, FieldOption, FieldRect, FieldType, FieldValue, FormField,
    ValidationRule,
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
            // PDF /Ff bit 16 (0x8000) = radio button; otherwise checkbox.
            // [FR-FORM-1, ADR-017]
            if (sf.flags & 0x8000) != 0 {
                FieldType::RadioButton
            } else {
                FieldType::Checkbox
            }
        }
        "Ch" => {
            // PDF /Ff bit 18 (0x20000) = list box; otherwise combo box.
            if (sf.flags & 0x20000) != 0 {
                FieldType::ListBox
            } else {
                FieldType::ComboBox
            }
        }
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

    // Map COS `/Opt` options to model options for combo/list fields.
    if field.is_choice() && !sf.options.is_empty() {
        field.options = sf.options.into_iter()
            .map(|(export, display)| FieldOption {
                export_value: export,
                display_label: display,
            })
            .collect();
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

    #[test]
    fn radio_button_detected_from_flags() {
        use pdf_cos::acroform::ScannedFormField;

        // Simulate a Btn field with radio flag (bit 16 = 0x8000).
        let sf = ScannedFormField {
            name: "choice".into(),
            field_type: "Btn".into(),
            value: "Option1".into(),
            rect: Some([72.0, 700.0, 152.0, 718.0]),
            page_index: 0,
            calculation: None,
            read_only: false,
            required: false,
            flags: 0x8000, // radio flag set
            options: Vec::new(),
            widget_obj_num: Some(10),
            tab_order: 1,
        };
        let field = scanned_to_field(sf);
        assert_eq!(field.field_type, FieldType::RadioButton,
            "Btn with radio flag should be RadioButton");
        assert_eq!(field.value, FieldValue::Choice("Option1".into()));
    }

    #[test]
    fn checkbox_detected_from_flags() {
        use pdf_cos::acroform::ScannedFormField;

        // Btn without radio flag = checkbox.
        let sf = ScannedFormField {
            name: "agree".into(),
            field_type: "Btn".into(),
            value: "Yes".into(),
            rect: Some([72.0, 700.0, 92.0, 718.0]),
            page_index: 0,
            calculation: None,
            read_only: false,
            required: false,
            flags: 0, // no radio flag
            options: Vec::new(),
            widget_obj_num: Some(11),
            tab_order: 2,
        };
        let field = scanned_to_field(sf);
        assert_eq!(field.field_type, FieldType::Checkbox,
            "Btn without radio flag should be Checkbox");
        assert_eq!(field.value, FieldValue::Bool(true));
    }

    #[test]
    fn list_box_detected_from_flags() {
        use pdf_cos::acroform::ScannedFormField;

        // Ch with list-box flag (bit 18 = 0x20000).
        let sf = ScannedFormField {
            name: "items".into(),
            field_type: "Ch".into(),
            value: "Option1".into(),
            rect: Some([72.0, 700.0, 200.0, 718.0]),
            page_index: 0,
            calculation: None,
            read_only: false,
            required: false,
            flags: 0x20000, // list box flag
            options: Vec::new(),
            widget_obj_num: Some(12),
            tab_order: 3,
        };
        let field = scanned_to_field(sf);
        assert_eq!(field.field_type, FieldType::ListBox,
            "Ch with list-box flag should be ListBox");
    }

    // =========================================================================
    // Enterprise form corpus tests — real-world patterns. [SDS §14 M5 exit]
    // =========================================================================

    /// Build a PDF with a tax-form-style calculation chain:
    /// income, deductions → taxable_income → tax_rate → tax.
    fn tax_form_pdf() -> Vec<u8> {
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
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Annots [5 0 R 6 0 R 7 0 R 8 0 R] >>\nendobj\n",
        );
        let o4 = body.len();
        body.extend_from_slice(
            b"4 0 obj\n<< /Fields [5 0 R 6 0 R 7 0 R 8 0 R] /CO [(taxable_income) (tax)] >>\nendobj\n",
        );
        // income = 100000
        let o5 = body.len();
        body.extend_from_slice(
            b"5 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (income) /V (100000) \
             /Rect [72 700 200 718] /P 3 0 R >>\nendobj\n",
        );
        // deductions = 20000
        let o6 = body.len();
        body.extend_from_slice(
            b"6 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (deductions) /V (20000) \
             /Rect [72 670 200 688] /P 3 0 R >>\nendobj\n",
        );
        // taxable_income = income - deductions
        let o7 = body.len();
        body.extend_from_slice(
            b"7 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (taxable_income) /V () \
             /Rect [72 640 200 658] /P 3 0 R \
             /AA << /C << /S /JavaScript /JS (getField(\"income\") - getField(\"deductions\")) >> >> >>\nendobj\n",
        );
        // tax = taxable_income * 0.25
        let o8 = body.len();
        body.extend_from_slice(
            b"8 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (tax) /V () \
             /Rect [72 610 200 628] /P 3 0 R \
             /AA << /C << /S /JavaScript /JS (getField(\"taxable_income\") * 0.25) >> >> >>\nendobj\n",
        );
        let xref_at = body.len();
        body.extend_from_slice(b"xref\n0 9\n");
        body.extend_from_slice(b"0000000000 65535 f \n");
        for off in [o1, o2, o3, o4, o5, o6, o7, o8] {
            body.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        body.extend_from_slice(b"trailer\n<< /Size 9 /Root 1 0 R >>\nstartxref\n");
        body.extend_from_slice(format!("{xref_at}\n").as_bytes());
        body.extend_from_slice(b"%%EOF\n");
        body
    }

    /// Build a PDF with an invoice-style form: qty, unit_price → line_total, tax_rate → tax, total.
    fn invoice_form_pdf() -> Vec<u8> {
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
            b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] \
             /Annots [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] >>\nendobj\n",
        );
        let o4 = body.len();
        body.extend_from_slice(
            b"4 0 obj\n<< /Fields [5 0 R 6 0 R 7 0 R 8 0 R 9 0 R] /CO [(line_total) (tax) (total)] >>\nendobj\n",
        );
        let o5 = body.len();
        body.extend_from_slice(
            b"5 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (qty) /V (10) \
             /Rect [72 700 150 718] /P 3 0 R >>\nendobj\n",
        );
        let o6 = body.len();
        body.extend_from_slice(
            b"6 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (unit_price) /V (25.50) \
             /Rect [72 670 150 688] /P 3 0 R >>\nendobj\n",
        );
        let o7 = body.len();
        body.extend_from_slice(
            b"7 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (line_total) /V () \
             /Rect [72 640 150 658] /P 3 0 R \
             /AA << /C << /S /JavaScript /JS (getField(\"qty\") * getField(\"unit_price\")) >> >> >>\nendobj\n",
        );
        let o8 = body.len();
        body.extend_from_slice(
            b"8 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (tax) /V () \
             /Rect [72 610 150 628] /P 3 0 R \
             /AA << /C << /S /JavaScript /JS (getField(\"line_total\") * 0.1) >> >> >>\nendobj\n",
        );
        let o9 = body.len();
        body.extend_from_slice(
            b"9 0 obj\n<< /Type /Annot /Subtype /Widget /FT /Tx /T (total) /V () \
             /Rect [72 580 150 598] /P 3 0 R \
             /AA << /C << /S /JavaScript /JS (getField(\"line_total\") + getField(\"tax\")) >> >> >>\nendobj\n",
        );
        let xref_at = body.len();
        body.extend_from_slice(b"xref\n0 10\n");
        body.extend_from_slice(b"0000000000 65535 f \n");
        for off in [o1, o2, o3, o4, o5, o6, o7, o8, o9] {
            body.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
        }
        body.extend_from_slice(b"trailer\n<< /Size 10 /Root 1 0 R >>\nstartxref\n");
        body.extend_from_slice(format!("{xref_at}\n").as_bytes());
        body.extend_from_slice(b"%%EOF\n");
        body
    }

    #[test]
    fn enterprise_tax_form_computes_correctly() {
        // [SDS §14 M5 exit] Tax form: income - deductions = taxable, taxable * rate = tax.
        let r = import_acroform_from_bytes(&tax_form_pdf()).expect("import");
        assert_eq!(r.field_count, 4);
        let mut form = r.form;

        // Run calculations in dependency order.
        form.calculation_order = vec!["taxable_income".into(), "tax".into()];
        let result = run_form_calculations(&mut form);
        assert!(result.updated_fields.contains(&"taxable_income".to_string()));
        assert!(result.updated_fields.contains(&"tax".to_string()));

        // Verify: 100000 - 20000 = 80000, 80000 * 0.25 = 20000.
        assert_eq!(form.field("taxable_income").unwrap().value.display(), "80000");
        assert_eq!(form.field("tax").unwrap().value.display(), "20000");
    }

    #[test]
    fn enterprise_invoice_computes_correctly() {
        // [SDS §14 M5 exit] Invoice: qty * price = line, line * 0.1 = tax, line + tax = total.
        let r = import_acroform_from_bytes(&invoice_form_pdf()).expect("import");
        assert_eq!(r.field_count, 5);
        let mut form = r.form;

        form.calculation_order = vec!["line_total".into(), "tax".into(), "total".into()];
        let result = run_form_calculations(&mut form);
        assert!(result.updated_fields.contains(&"line_total".to_string()));
        assert!(result.updated_fields.contains(&"tax".into()));
        assert!(result.updated_fields.contains(&"total".to_string()));

        // Verify: 10 * 25.50 = 255.0, 255.0 * 0.1 = 25.5, 255.0 + 25.5 = 280.5.
        let line = form.field("line_total").unwrap().value.display().parse::<f64>().unwrap();
        let tax = form.field("tax").unwrap().value.display().parse::<f64>().unwrap();
        let total = form.field("total").unwrap().value.display().parse::<f64>().unwrap();
        assert!((line - 255.0).abs() < 0.01, "line_total should be 255.0, got {line}");
        assert!((tax - 25.5).abs() < 0.01, "tax should be 25.5, got {tax}");
        assert!((total - 280.5).abs() < 0.01, "total should be 280.5, got {total}");
    }

    #[test]
    fn enterprise_form_appearances_regenerated() {
        // [FR-FORM-1] After calculation, appearances must be regenerated.
        let r = import_acroform_from_bytes(&tax_form_pdf()).expect("import");
        let mut form = r.form;
        form.calculation_order = vec!["taxable_income".into(), "tax".into()];
        form.run_calculations();

        // Every calculated field should have an appearance.
        for name in &["taxable_income", "tax"] {
            let field = form.field(name).unwrap();
            assert!(field.appearance.is_some(),
                "field {name} should have appearance after calculation");
        }
    }
}
