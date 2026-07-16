//! Generate a minimal multi-page PDF for benchmarking. [ADR-023, SDS §14 M1]
//!
//! Usage: gen_test_pdf <num_pages> <output_path>
//!
//! Creates a valid PDF with N pages, each US Letter size, with minimal content.
//! The PDF uses classic xref tables (no compression) for maximum compatibility.

use std::env;
use std::fs::File;
use std::io::Write;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: gen_test_pdf <num_pages> <output_path>");
        std::process::exit(1);
    }

    let num_pages: u32 = args[1].parse().expect("num_pages must be a u32");
    let output_path = &args[2];

    let mut pdf = Vec::new();
    let mut offsets = Vec::new();

    // Header
    pdf.extend_from_slice(b"%PDF-1.4\n");

    // Object 1: Catalog
    offsets.push(pdf.len() as u32);
    write!(pdf, "1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n").unwrap();

    // Object 2: Pages (parent)
    offsets.push(pdf.len() as u32);
    let kids: Vec<String> = (0..num_pages)
        .map(|i| format!("{} 0 R", i + 3))
        .collect();
    write!(
        pdf,
        "2 0 obj\n<< /Type /Pages /Kids [{}] /Count {} >>\nendobj\n",
        kids.join(" "),
        num_pages
    )
    .unwrap();

    // Objects 3..N+2: Page objects
    for i in 0..num_pages {
        offsets.push(pdf.len() as u32);
        write!(
            pdf,
            "{} 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>\nendobj\n",
            i + 3
        )
        .unwrap();
    }

    // xref table
    let xref_offset = pdf.len() as u32;
    write!(pdf, "xref\n0 {}\n", num_pages + 3).unwrap();
    write!(pdf, "0000000000 65535 f \n").unwrap();
    for offset in &offsets {
        write!(pdf, "{:010} 00000 n \n", offset).unwrap();
    }

    // Trailer
    write!(
        pdf,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{}\n%%EOF\n",
        num_pages + 3,
        xref_offset
    )
    .unwrap();

    let mut file = File::create(output_path).expect("create output file");
    file.write_all(&pdf).expect("write PDF");
    eprintln!(
        "Generated {}-page PDF: {} bytes -> {}",
        num_pages,
        pdf.len(),
        output_path
    );
}
