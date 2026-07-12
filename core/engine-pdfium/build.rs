// Links the prebuilt PDFium static/shared library from third_party/pdfium/prebuilt/.
// Full engine build runs only in CI when third_party/ changes. [SDS §13.4, ADR-028]
fn main() {
    // ponytail: stub — wire up linking to third_party/pdfium/prebuilt/ at M0 implementation
    println!("cargo:rerun-if-changed=../../third_party/pdfium/prebuilt");
}
