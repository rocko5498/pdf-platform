//! AcroForm model: field types, values, and form filling. [FR-FORM, SDS §2.2]
//!
//! Supports filling AcroForm fields with correct appearances so that
//! filled values render correctly in other conformant readers. [FR-FORM-1]
//!
//! All mutations go through Commands (FR-FORM-6, ADR-013).

use std::collections::HashMap;
use std::io::Write;

/// AcroForm field type. [FR-FORM-1]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldType {
    /// Text input field.
    Text,
    /// Checkbox (boolean).
    Checkbox,
    /// Radio button (mutually exclusive choice).
    RadioButton,
    /// Dropdown/combo box (select from list).
    ComboBox,
    /// List box (select from list, possibly multi-select).
    ListBox,
    /// Push button (no value, triggers action).
    Button,
    /// Signature field.
    Signature,
}

/// Field value: the current value of a form field.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldValue {
    /// Text value.
    Text(String),
    /// Boolean value (checkbox).
    Bool(bool),
    /// Choice value (combo/list) — the selected option name.
    Choice(String),
    /// Multi-select values (list box).
    MultiChoice(Vec<String>),
    /// No value.
    None,
}

impl FieldValue {
    /// Convert to a display string.
    pub fn display(&self) -> String {
        match self {
            Self::Text(s) => s.clone(),
            Self::Bool(b) => if *b { "Yes".into() } else { "No".into() },
            Self::Choice(s) => s.clone(),
            Self::MultiChoice(v) => v.join(", "),
            Self::None => String::new(),
        }
    }

    /// Whether the field has a value.
    pub fn is_empty(&self) -> bool {
        match self {
            Self::Text(s) => s.is_empty(),
            Self::None => true,
            _ => false,
        }
    }
}

/// A form field option (for combo/list boxes).
#[derive(Debug, Clone, PartialEq)]
pub struct FieldOption {
    /// Export value (what gets stored in the PDF).
    pub export_value: String,
    /// Display label (shown to the user).
    pub display_label: String,
}

/// Validation rule for a field.
#[derive(Debug, Clone)]
pub enum ValidationRule {
    /// Field is required (must have a value).
    Required,
    /// Maximum text length.
    MaxLength(u32),
    /// Minimum text length.
    MinLength(u32),
}

// `Pattern(String)` and `Custom { description }` used to sit here. Nothing
// constructed either one, and `validate` matched both with an empty body — so
// a field carrying a pattern rule validated clean no matter what was typed
// into it. A rule that cannot fail is worse than no rule: it reports a
// verification that never happened. They come back with an implementation, or
// not at all. [PRIN-6, GR-8, FR-FORM-5]

/// JavaScript calculation for a field (simplified representation). [FR-JS-1]
#[derive(Debug, Clone)]
pub struct FieldCalculation {
    /// The JavaScript expression to evaluate.
    pub expression: String,
    /// Field names this calculation depends on.
    pub dependencies: Vec<String>,
    /// Whether this calculation is currently enabled.
    pub enabled: bool,
}

/// A form field definition and current state. [FR-FORM]
#[derive(Debug, Clone)]
pub struct FormField {
    /// Unique field name (PDF /T entry).
    pub name: String,
    /// Fully qualified name (parent.field).
    pub fully_qualified_name: String,
    /// Field type.
    pub field_type: FieldType,
    /// Current value.
    pub value: FieldValue,
    /// Default value.
    pub default_value: FieldValue,
    /// Display/tooltip text.
    pub tooltip: String,
    /// Whether the field is read-only.
    pub read_only: bool,
    /// Whether the field is required.
    pub required: bool,
    /// Whether the field is visible.
    pub visible: bool,
    /// Tab order index (for field navigation). [FR-FORM-2]
    pub tab_order: u32,
    /// Page index where the field's widget is located.
    pub page_index: u32,
    /// Widget rectangle in PDF user-space coordinates.
    pub rect: FieldRect,
    /// Options (for combo/list fields).
    pub options: Vec<FieldOption>,
    /// Validation rules.
    pub validation: Vec<ValidationRule>,
    /// JavaScript calculation (if any). [FR-JS-1]
    pub calculation: Option<FieldCalculation>,
    /// Font name for text rendering.
    pub font_name: Option<String>,
    /// Font size for text rendering.
    pub font_size: Option<f32>,
    /// Maximum text length (for text fields).
    pub max_length: Option<u32>,
    /// Multi-line text field.
    pub multiline: bool,
    /// Password field (masked input).
    pub password: bool,
    /// Appearance stream bytes (if generated).
    pub appearance: Option<Vec<u8>>,
    /// The PDF object number of this field's widget annotation.
    pub widget_obj_num: Option<u32>,
}

/// Rectangle in PDF user-space coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FieldRect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl FieldRect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }
}

impl FormField {
    /// Create a new form field.
    pub fn new(
        name: impl Into<String>,
        field_type: FieldType,
        page_index: u32,
        rect: FieldRect,
    ) -> Self {
        let name = name.into();
        Self {
            fully_qualified_name: name.clone(),
            name,
            field_type,
            value: FieldValue::None,
            default_value: FieldValue::None,
            tooltip: String::new(),
            read_only: false,
            required: false,
            visible: true,
            tab_order: 0,
            page_index,
            rect,
            options: Vec::new(),
            validation: Vec::new(),
            calculation: None,
            font_name: None,
            font_size: None,
            max_length: None,
            multiline: false,
            password: false,
            appearance: None,
            widget_obj_num: None,
        }
    }

    /// Set the value, returning whether it changed.
    ///
    /// Clears cached appearance so callers must regenerate `/AP` [FR-FORM-1].
    pub fn set_value(&mut self, value: FieldValue) -> bool {
        if self.value != value {
            self.value = value;
            self.appearance = None;
            true
        } else {
            false
        }
    }

    /// Validate the current value against all validation rules.
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();

        for rule in &self.validation {
            match rule {
                ValidationRule::Required => {
                    if self.value.is_empty() {
                        errors.push(format!("{} is required", self.name));
                    }
                }
                ValidationRule::MaxLength(max) => {
                    if let FieldValue::Text(ref s) = self.value {
                        if s.len() as u32 > *max {
                            errors.push(format!(
                                "{} exceeds maximum length of {}",
                                self.name, max
                            ));
                        }
                    }
                }
                ValidationRule::MinLength(min) => {
                    if let FieldValue::Text(ref s) = self.value {
                        if (s.len() as u32) < *min {
                            errors.push(format!(
                                "{} must be at least {} characters",
                                self.name, min
                            ));
                        }
                    }
                }
            }
        }

        errors
    }

    /// Whether the field is a choice field (combo/list).
    pub fn is_choice(&self) -> bool {
        matches!(self.field_type, FieldType::ComboBox | FieldType::ListBox)
    }

    /// Whether the field is a button.
    pub fn is_button(&self) -> bool {
        self.field_type == FieldType::Button
    }

    /// The PDF field type string.
    pub fn pdf_type_str(&self) -> &'static str {
        match self.field_type {
            FieldType::Text => "Tx",
            FieldType::Checkbox => "Btn",
            FieldType::RadioButton => "Btn",
            FieldType::ComboBox => "Ch",
            FieldType::ListBox => "Ch",
            FieldType::Button => "Btn",
            FieldType::Signature => "Sig",
        }
    }
}

/// AcroForm: the collection of all form fields in a document. [FR-FORM]
#[derive(Debug, Clone)]
pub struct AcroForm {
    /// Fields keyed by fully qualified name.
    fields: HashMap<String, FormField>,
    /// Field order (by tab order).
    field_order: Vec<String>,
    /// Whether the form has JavaScript calculations. [FR-JS-4]
    pub has_javascript: bool,
    /// Whether JavaScript execution is enabled (user/admin kill switch). [FR-JS-4]
    pub javascript_enabled: bool,
    /// Calculation order (field names in dependency order). [FR-JS-1]
    pub calculation_order: Vec<String>,
    /// Whether the form needs appearance regeneration.
    pub needs_appearance_regen: bool,
}

impl AcroForm {
    /// Create a new empty form.
    pub fn new() -> Self {
        Self {
            fields: HashMap::new(),
            field_order: Vec::new(),
            has_javascript: false,
            javascript_enabled: true,
            calculation_order: Vec::new(),
            needs_appearance_regen: false,
        }
    }

    /// Add a field to the form.
    pub fn add_field(&mut self, field: FormField) {
        let name = field.fully_qualified_name.clone();
        self.field_order.push(name.clone());
        self.fields.insert(name, field);
    }

    /// Get a field by name.
    pub fn field(&self, name: &str) -> Option<&FormField> {
        self.fields.get(name)
    }

    /// Get a mutable field by name.
    pub fn field_mut(&mut self, name: &str) -> Option<&mut FormField> {
        self.fields.get_mut(name)
    }

    /// Get all fields.
    pub fn fields(&self) -> &HashMap<String, FormField> {
        &self.fields
    }

    /// Get fields in tab order.
    pub fn fields_in_tab_order(&self) -> Vec<&FormField> {
        let mut fields: Vec<&FormField> = self.field_order.iter()
            .filter_map(|name| self.fields.get(name))
            .collect();
        fields.sort_by_key(|f| f.tab_order);
        fields
    }

    /// Total field count.
    pub fn field_count(&self) -> usize {
        self.fields.len()
    }

    /// Set a field value by name. Returns whether the value changed.
    pub fn set_field_value(&mut self, name: &str, value: FieldValue) -> bool {
        if let Some(field) = self.fields.get_mut(name) {
            let changed = field.set_value(value);
            if changed {
                self.needs_appearance_regen = true;
            }
            changed
        } else {
            false
        }
    }

    /// Validate all fields and return errors.
    pub fn validate_all(&self) -> Vec<String> {
        self.fields.values()
            .flat_map(|f| f.validate())
            .collect()
    }

    /// Collect all field values as a HashMap (for export). [FR-FORM-3]
    pub fn export_values(&self) -> HashMap<String, String> {
        self.fields.iter()
            .map(|(name, field)| (name.clone(), field.value.display()))
            .collect()
    }

    /// Import field values from a HashMap. [FR-FORM-3]
    /// Returns the number of fields that were changed.
    pub fn import_values(&mut self, values: &HashMap<String, String>) -> u32 {
        let mut changed = 0;
        for (name, value_str) in values {
            if let Some(field) = self.fields.get_mut(name) {
                let new_value = match field.field_type {
                    FieldType::Checkbox => {
                        FieldValue::Bool(value_str == "Yes" || value_str == "true" || value_str == "1")
                    }
                    FieldType::RadioButton => FieldValue::Choice(value_str.clone()),
                    FieldType::ComboBox | FieldType::ListBox => FieldValue::Choice(value_str.clone()),
                    _ => FieldValue::Text(value_str.clone()),
                };
                if field.set_value(new_value) {
                    changed += 1;
                    self.needs_appearance_regen = true;
                }
            }
        }
        changed
    }

    /// Run the calculation order via the forms JS **subset** evaluator. [FR-JS-1, ADR-017]
    ///
    /// Unsupported constructs are not silently emulated — see `forms_js` log
    /// when using [`crate::forms_js::run_form_calculations`]. Returns field
    /// names that were recalculated successfully.
    ///
    /// After any value change, regenerates widget appearances [FR-FORM-1].
    pub fn run_calculations(&mut self) -> Vec<String> {
        let updated = crate::forms_js::run_form_calculations(self).updated_fields;
        if self.needs_appearance_regen {
            self.regenerate_appearances();
        }
        updated
    }

    /// Regenerate `/AP` appearance streams for all fields. [FR-FORM-1]
    ///
    /// Required after fill or calculation so other readers show values (PRIN-7).
    /// Returns the number of fields whose appearance was written.
    pub fn regenerate_appearances(&mut self) -> u32 {
        let mut n = 0u32;
        for field in self.fields.values_mut() {
            crate::appearance::ensure_widget_appearance(field);
            n += 1;
        }
        self.needs_appearance_regen = false;
        n
    }

    /// Check if any field has JavaScript calculations.
    pub fn detect_javascript(&self) -> bool {
        self.fields.values().any(|f| f.calculation.is_some())
    }
}

impl Default for AcroForm {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Form flattening [FR-FORM-4, PRIN-2, PRIN-6]
// ---------------------------------------------------------------------------

/// Result of flattening a form field into page content.
#[derive(Debug, Clone)]
pub struct FlattenFieldResult {
    /// Page index this field belongs to.
    pub page_index: u32,
    /// Content stream bytes to append to the page's content stream.
    pub content_stream: Vec<u8>,
    /// Widget object number to remove from the page's /Annots.
    pub widget_obj_num: Option<u32>,
}

/// Generate flatten content streams for all fields in the form. [FR-FORM-4]
///
/// Returns a vector of `FlattenFieldResult` — one per field with a value.
/// The caller (coordinator/FFI) is responsible for:
/// 1. Appending each page's content streams to the page's `/Contents`
/// 2. Removing widget annotations from page `/Annots` arrays
/// 3. Removing or emptying the `/AcroForm` dictionary
/// 4. Creating a Command for undo
///
/// This is a **destructive** operation — the user MUST be warned (PRIN-6, DS-CONFIRM-1).
pub fn flatten_form(form: &AcroForm) -> Vec<FlattenFieldResult> {
    let mut results = Vec::new();
    for field in form.fields.values() {
        if field.value.is_empty() {
            continue;
        }
        if matches!(field.field_type, FieldType::Button | FieldType::Signature) {
            continue;
        }
        let content = generate_flatten_stream(field);
        results.push(FlattenFieldResult {
            page_index: field.page_index,
            content_stream: content,
            widget_obj_num: field.widget_obj_num,
        });
    }
    results
}

/// Generate a PDF content stream that renders a field's value in page coordinates. [FR-FORM-4]
fn generate_flatten_stream(field: &FormField) -> Vec<u8> {
    let mut buf = Vec::new();
    let x = field.rect.x;
    let y = field.rect.y;
    let h = field.rect.height;
    let font_size = field.font_size.unwrap_or(10.0).clamp(6.0, 24.0);

    match field.field_type {
        FieldType::Checkbox | FieldType::RadioButton => {
            let label = match &field.value {
                FieldValue::Bool(true) => "\u{2713}".to_string(), // checkmark
                FieldValue::Bool(false) => String::new(),
                FieldValue::Choice(s) => s.clone(),
                _ => String::new(),
            };
            if label.is_empty() {
                return buf;
            }
            write!(&mut buf, "q\n").unwrap();
            write!(&mut buf, "0 0 0 rg\n").unwrap();
            write!(&mut buf, "BT\n").unwrap();
            write!(&mut buf, "/F1 {font_size:.1} Tf\n").unwrap();
            write!(&mut buf, "{x:.1} {y:.1} Td\n").unwrap();
            let escaped = escape_pdf_str(&label);
            write!(&mut buf, "({escaped}) Tj\n").unwrap();
            write!(&mut buf, "ET\n").unwrap();
            write!(&mut buf, "Q\n").unwrap();
        }
        FieldType::ComboBox | FieldType::ListBox => {
            let display = field.value.display();
            if display.is_empty() {
                return buf;
            }
            write!(&mut buf, "q\n").unwrap();
            write!(&mut buf, "0 0 0 rg\n").unwrap();
            write!(&mut buf, "BT\n").unwrap();
            write!(&mut buf, "/F1 {font_size:.1} Tf\n").unwrap();
            write!(&mut buf, "{x:.1} {y:.1} Td\n").unwrap();
            let escaped = escape_pdf_str(&display);
            write!(&mut buf, "({escaped}) Tj\n").unwrap();
            write!(&mut buf, "ET\n").unwrap();
            write!(&mut buf, "Q\n").unwrap();
        }
        FieldType::Text => {
            let display = match &field.value {
                FieldValue::None => return buf,
                other => other.display(),
            };
            write!(&mut buf, "q\n").unwrap();
            write!(&mut buf, "0 0 0 rg\n").unwrap();
            write!(&mut buf, "BT\n").unwrap();
            write!(&mut buf, "/F1 {font_size:.1} Tf\n").unwrap();
            let baseline = (h - font_size).max(2.0);
            let text_y = y + baseline;
            write!(&mut buf, "{x:.1} {text_y:.1} Td\n").unwrap();
            let escaped = escape_pdf_str(&display);
            write!(&mut buf, "({escaped}) Tj\n").unwrap();
            write!(&mut buf, "ET\n").unwrap();
            write!(&mut buf, "Q\n").unwrap();
        }
        _ => {}
    }
    buf
}

/// Escape a string for PDF literal string syntax `(...)`.
fn escape_pdf_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '(' => out.push_str("\\("),
            ')' => out.push_str("\\)"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_field(name: &str) -> FormField {
        FormField::new(name, FieldType::Text, 0, FieldRect::new(10.0, 20.0, 200.0, 20.0))
    }

    fn checkbox_field(name: &str) -> FormField {
        FormField::new(name, FieldType::Checkbox, 0, FieldRect::new(10.0, 50.0, 20.0, 20.0))
    }

    #[test]
    fn form_field_set_value() {
        let mut field = text_field("name");
        assert!(field.set_value(FieldValue::Text("John".into())));
        assert_eq!(field.value, FieldValue::Text("John".into()));

        // Same value — no change.
        assert!(!field.set_value(FieldValue::Text("John".into())));
    }

    #[test]
    fn form_field_validation() {
        let mut field = text_field("required_field");
        field.required = true;
        field.validation.push(ValidationRule::Required);

        // Empty value — should fail.
        let errors = field.validate();
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("required"));

        // Set value — should pass.
        field.set_value(FieldValue::Text("filled".into()));
        let errors = field.validate();
        assert!(errors.is_empty());
    }

    #[test]
    fn form_max_length_validation() {
        let mut field = text_field("short");
        field.validation.push(ValidationRule::MaxLength(5));

        field.set_value(FieldValue::Text("hello".into()));
        assert!(field.validate().is_empty());

        field.set_value(FieldValue::Text("too long".into()));
        assert!(!field.validate().is_empty());
    }

    #[test]
    fn acroform_add_and_set_value() {
        let mut form = AcroForm::new();
        form.add_field(text_field("name"));
        form.add_field(checkbox_field("agree"));

        assert_eq!(form.field_count(), 2);

        assert!(form.set_field_value("name", FieldValue::Text("Alice".into())));
        assert!(form.set_field_value("agree", FieldValue::Bool(true)));
        assert!(form.needs_appearance_regen);

        // Non-existent field.
        assert!(!form.set_field_value("missing", FieldValue::Text("x".into())));
    }

    #[test]
    fn acroform_tab_order() {
        let mut form = AcroForm::new();

        let mut f1 = text_field("first");
        f1.tab_order = 2;
        let mut f2 = text_field("second");
        f2.tab_order = 1;
        let mut f3 = text_field("third");
        f3.tab_order = 3;

        form.add_field(f1);
        form.add_field(f2);
        form.add_field(f3);

        let ordered = form.fields_in_tab_order();
        assert_eq!(ordered[0].name, "second");
        assert_eq!(ordered[1].name, "first");
        assert_eq!(ordered[2].name, "third");
    }

    #[test]
    fn acroform_validate_all() {
        let mut form = AcroForm::new();
        let mut f = text_field("req");
        f.required = true;
        f.validation.push(ValidationRule::Required);
        form.add_field(f);

        let errors = form.validate_all();
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn checkbox_toggle() {
        let mut field = checkbox_field("check");
        assert!(field.set_value(FieldValue::Bool(true)));
        assert_eq!(field.value, FieldValue::Bool(true));
        assert!(!field.set_value(FieldValue::Bool(true))); // same value
    }

    #[test]
    fn combo_box_options() {
        let mut field = FormField::new("color", FieldType::ComboBox, 0, FieldRect::new(0.0, 0.0, 100.0, 20.0));
        field.options = vec![
            FieldOption { export_value: "red".into(), display_label: "Red".into() },
            FieldOption { export_value: "blue".into(), display_label: "Blue".into() },
        ];

        field.set_value(FieldValue::Choice("red".into()));
        assert_eq!(field.value, FieldValue::Choice("red".into()));
        assert!(field.is_choice());
    }

    #[test]
    fn regenerate_appearances_after_fill() {
        // [FR-FORM-1] fill marks regen needed; regenerate_appearances writes /AP.
        let mut form = AcroForm::new();
        form.add_field(text_field("name"));
        assert!(form.set_field_value("name", FieldValue::Text("Bob".into())));
        assert!(form.needs_appearance_regen);
        assert!(form.field("name").unwrap().appearance.is_none());
        let n = form.regenerate_appearances();
        assert_eq!(n, 1);
        assert!(!form.needs_appearance_regen);
        let ap = form.field("name").unwrap().appearance.as_ref().unwrap();
        let text = String::from_utf8_lossy(ap);
        assert!(text.contains("Bob"));
    }

    #[test]
    fn calculation_order() {
        let mut form = AcroForm::new();
        form.has_javascript = true;
        form.javascript_enabled = true;

        let mut a = FormField::new("a", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 100.0, 20.0));
        a.set_value(FieldValue::Text("10".into()));
        form.add_field(a);
        let mut b = FormField::new("b", FieldType::Text, 0, FieldRect::new(0.0, 0.0, 100.0, 20.0));
        b.set_value(FieldValue::Text("5".into()));
        form.add_field(b);

        let mut total_field = FormField::new(
            "total",
            FieldType::Text,
            0,
            FieldRect::new(0.0, 0.0, 100.0, 20.0),
        );
        total_field.calculation = Some(FieldCalculation {
            expression: r#"AFSimple_Calculate("SUM", ["a","b"])"#.into(),
            dependencies: vec!["a".into(), "b".into()],
            enabled: true,
        });
        form.add_field(total_field);

        let mut tax_field = FormField::new(
            "tax",
            FieldType::Text,
            0,
            FieldRect::new(0.0, 30.0, 100.0, 20.0),
        );
        tax_field.calculation = Some(FieldCalculation {
            expression: r#"getField("total") * 0.1"#.into(),
            dependencies: vec!["total".into()],
            enabled: true,
        });
        form.add_field(tax_field);

        form.calculation_order = vec!["total".into(), "tax".into()];

        // Forms JS subset evaluates in-process for tests; production path is Z1. [ADR-017]
        let recalc = form.run_calculations();
        assert!(recalc.contains(&"total".into()));
        assert!(recalc.contains(&"tax".into()));
        assert_eq!(
            form.fields().get("total").unwrap().value,
            FieldValue::Text("15".into())
        );
    }

    #[test]
    fn flatten_text_field_produces_content_stream() {
        let mut form = AcroForm::new();
        let mut f = text_field("name");
        f.set_value(FieldValue::Text("Alice".into()));
        f.widget_obj_num = Some(10);
        form.add_field(f);

        let results = flatten_form(&form);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].page_index, 0);
        assert_eq!(results[0].widget_obj_num, Some(10));
        let stream = String::from_utf8_lossy(&results[0].content_stream);
        assert!(stream.contains("Alice"), "flatten stream should contain field value");
        assert!(stream.contains("BT"), "flatten stream should have text object");
        assert!(stream.contains("ET"), "flatten stream should close text object");
    }

    #[test]
    fn flatten_checkbox_produces_checkmark() {
        let mut form = AcroForm::new();
        let mut f = FormField::new("agree", FieldType::Checkbox, 0,
            FieldRect::new(10.0, 20.0, 20.0, 20.0));
        f.set_value(FieldValue::Bool(true));
        f.widget_obj_num = Some(11);
        form.add_field(f);

        let results = flatten_form(&form);
        assert_eq!(results.len(), 1);
        let stream = String::from_utf8_lossy(&results[0].content_stream);
        assert!(stream.contains("Tj"), "should have text rendering");
    }

    #[test]
    fn flatten_empty_fields_produce_nothing() {
        let mut form = AcroForm::new();
        form.add_field(text_field("empty"));

        let results = flatten_form(&form);
        assert!(results.is_empty(), "empty fields should not produce flatten streams");
    }

    #[test]
    fn flatten_button_skipped() {
        let mut form = AcroForm::new();
        let mut f = FormField::new("submit", FieldType::Button, 0,
            FieldRect::new(0.0, 0.0, 80.0, 20.0));
        f.value = FieldValue::Text("Submit".into());
        form.add_field(f);

        let results = flatten_form(&form);
        assert!(results.is_empty(), "buttons should be skipped during flatten");
    }

    #[test]
    fn escape_pdf_str_handles_specials() {
        assert_eq!(escape_pdf_str("hello"), "hello");
        assert_eq!(escape_pdf_str("a(b)"), "a\\(b\\)");
        assert_eq!(escape_pdf_str("a\\b"), "a\\\\b");
        assert_eq!(escape_pdf_str("line\nbreak"), "line\\nbreak");
    }
}
