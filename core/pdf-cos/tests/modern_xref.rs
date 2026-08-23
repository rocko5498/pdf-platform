//! PDF 1.5+ cross-reference streams and object streams. [FR-VIEW-2, SDS §3.1]
//!
//! Most producers have written cross-reference *streams* rather than classic
//! tables since 2003, and put most objects inside compressed *object streams*.
//! `parse_xref_table` answered "not supported at M0" for the first and there
//! was no reader for the second, so every COS path — page objects for stamping,
//! AcroForm fields, optional content — failed on documents of that shape. The
//! engine hid it: PDFium renders and extracts text from them perfectly well, so
//! opening and reading such a file looked fine and only editing failed.
//!
//! Every fixture here is built rather than checked in, so what is being parsed
//! is visible in the test.

use std::io::Write as _;

use flate2::write::ZlibEncoder;
use flate2::Compression;
use pdf_cos::scan::{fetch_object_bytes, find_startxref, parse_xref_chain, scan_bytes};

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data).expect("compress");
    encoder.finish().expect("finish")
}

/// A document whose cross-reference section is a stream, with the catalog and
/// pages as ordinary objects.
fn xref_stream_document() -> Vec<u8> {
    let mut bytes: Vec<u8> = b"%PDF-1.5\n".to_vec();
    let mut offsets = vec![0usize; 5];

    let objects: Vec<(usize, &[u8])> = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>"),
        (2, b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>"),
    ];
    for (num, body) in &objects {
        offsets[*num] = bytes.len();
        write!(bytes, "{num} 0 obj\n").unwrap();
        bytes.extend_from_slice(body);
        bytes.extend_from_slice(b"\nendobj\n");
    }

    // Object 4 is the xref stream itself: /W [1 4 2], one entry per object.
    let xref_at = bytes.len();
    offsets[4] = xref_at;
    let mut table = Vec::new();
    // Entry 0: free.
    table.push(0u8);
    table.extend_from_slice(&0u32.to_be_bytes());
    table.extend_from_slice(&65535u16.to_be_bytes());
    for num in 1..=4usize {
        let offset = if num == 4 { xref_at } else { offsets[num] };
        table.push(1u8);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&0u16.to_be_bytes());
    }
    let compressed = deflate(&table);
    write!(
        bytes,
        "4 0 obj\n<< /Type /XRef /Size 5 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} >>\nstream\n",
        compressed.len()
    )
    .unwrap();
    bytes.extend_from_slice(&compressed);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    write!(bytes, "startxref\n{xref_at}\n%%EOF\n").unwrap();
    bytes
}

/// A document whose catalog and page live inside a compressed object stream.
fn object_stream_document() -> Vec<u8> {
    let mut bytes: Vec<u8> = b"%PDF-1.5\n".to_vec();

    // Object 4 is the object stream holding objects 1, 2 and 3.
    let payload_objects: Vec<(u32, &str)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >>"),
    ];
    let mut header = String::new();
    let mut body = String::new();
    for (num, text) in &payload_objects {
        header.push_str(&format!("{num} {} ", body.len()));
        body.push_str(text);
        body.push(' ');
    }
    let first = header.len();
    let content = format!("{header}{body}");
    let compressed = deflate(content.as_bytes());

    let objstm_at = bytes.len();
    write!(
        bytes,
        "4 0 obj\n<< /Type /ObjStm /N {} /First {first} /Filter /FlateDecode /Length {} >>\nstream\n",
        payload_objects.len(),
        compressed.len()
    )
    .unwrap();
    bytes.extend_from_slice(&compressed);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");

    // Object 5 is the xref stream: objects 1..3 are type 2, in stream 4.
    let xref_at = bytes.len();
    let mut table = Vec::new();
    table.push(0u8);
    table.extend_from_slice(&0u32.to_be_bytes());
    table.extend_from_slice(&65535u16.to_be_bytes());
    for index in 0..3u16 {
        table.push(2u8);
        table.extend_from_slice(&4u32.to_be_bytes()); // container object
        table.extend_from_slice(&index.to_be_bytes()); // index within it
    }
    for offset in [objstm_at, xref_at] {
        table.push(1u8);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&0u16.to_be_bytes());
    }
    let compressed_table = deflate(&table);
    write!(
        bytes,
        "5 0 obj\n<< /Type /XRef /Size 6 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} >>\nstream\n",
        compressed_table.len()
    )
    .unwrap();
    bytes.extend_from_slice(&compressed_table);
    bytes.extend_from_slice(b"\nendstream\nendobj\n");
    write!(bytes, "startxref\n{xref_at}\n%%EOF\n").unwrap();
    bytes
}

#[test]
fn a_cross_reference_stream_resolves_objects() {
    let data = xref_stream_document();
    let mut leniency = Vec::new();
    let start = find_startxref(&data).expect("startxref");
    let xref = parse_xref_chain(&data, start, &mut leniency).expect("xref stream must parse");
    let catalog = fetch_object_bytes(&data, &xref, 1).expect("catalog");
    assert!(
        String::from_utf8_lossy(&catalog).contains("/Type /Catalog"),
        "resolved {:?}",
        String::from_utf8_lossy(&catalog)
    );

    let page = fetch_object_bytes(&data, &xref, 3).expect("page");
    assert!(
        String::from_utf8_lossy(&page).contains("/MediaBox [0 0 612 792]"),
        "resolved {:?}",
        String::from_utf8_lossy(&page)
    );
}

#[test]
fn an_object_inside_an_object_stream_resolves() {
    let data = object_stream_document();
    let mut leniency = Vec::new();
    let start = find_startxref(&data).expect("startxref");
    let xref = parse_xref_chain(&data, start, &mut leniency).expect("xref stream must parse");

    let page = fetch_object_bytes(&data, &xref, 3).expect("a compressed object must resolve");
    let text = String::from_utf8_lossy(&page);
    assert!(
        text.contains("/Type /Page") && text.contains("200 100"),
        "resolved {text:?}"
    );

    let catalog = fetch_object_bytes(&data, &xref, 1).expect("catalog");
    assert!(
        String::from_utf8_lossy(&catalog).contains("/Type /Catalog"),
        "the first object in the stream must not be confused with the second"
    );
}

#[test]
fn flate_decode_reads_a_zlib_stream() {
    // `/FlateDecode` is zlib (RFC 1950), not bare deflate. The decoder used
    // `DeflateDecoder`, so every well-formed xref stream failed to decompress —
    // the same class of defect as the missing zlib header that made OCR
    // recognise nothing. Both fixtures above are zlib-framed, so this is really
    // asserted by them; keeping it explicit names the thing that was wrong.
    let data = xref_stream_document();
    assert!(
        data.windows(2).any(|w| w == [0x78, 0x9c] || w == [0x78, 0x01]),
        "the fixture must carry a zlib header, or it proves nothing"
    );
}

/// A document whose catalog, page tree, page and AcroForm all live inside an
/// object stream — the ordinary shape of a modern filled-in form.
fn modern_form_document() -> Vec<u8> {
    let mut bytes: Vec<u8> = b"%PDF-1.6
".to_vec();

    let payload: Vec<(u32, &str)> = vec![
        (1, "<< /Type /Catalog /Pages 2 0 R /AcroForm << /Fields [6 0 R] >> >>"),
        (2, "<< /Type /Pages /Kids [3 0 R] /Count 1 >>"),
        (3, "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>"),
        (6, "<< /FT /Tx /T (full_name) /V (Ada Lovelace) >>"),
    ];
    let mut header = String::new();
    let mut body = String::new();
    for (num, text) in &payload {
        header.push_str(&format!("{num} {} ", body.len()));
        body.push_str(text);
        body.push(' ');
    }
    let first = header.len();
    let compressed = deflate(format!("{header}{body}").as_bytes());

    let objstm_at = bytes.len();
    write!(
        bytes,
        "4 0 obj
<< /Type /ObjStm /N {} /First {first} /Filter /FlateDecode /Length {} >>
stream
",
        payload.len(),
        compressed.len()
    )
    .unwrap();
    bytes.extend_from_slice(&compressed);
    bytes.extend_from_slice(b"
endstream
endobj
");

    // Objects 1, 2, 3 and 6 are type 2 in stream 4; 4 and 5 are ordinary.
    let xref_at = bytes.len();
    let mut table = Vec::new();
    let mut push_type2 = |table: &mut Vec<u8>, index: u16| {
        table.push(2u8);
        table.extend_from_slice(&4u32.to_be_bytes());
        table.extend_from_slice(&index.to_be_bytes());
    };
    let mut push_type1 = |table: &mut Vec<u8>, offset: usize| {
        table.push(1u8);
        table.extend_from_slice(&(offset as u32).to_be_bytes());
        table.extend_from_slice(&0u16.to_be_bytes());
    };
    // 0 free
    table.push(0u8);
    table.extend_from_slice(&0u32.to_be_bytes());
    table.extend_from_slice(&65535u16.to_be_bytes());
    push_type2(&mut table, 0); // 1 catalog
    push_type2(&mut table, 1); // 2 pages
    push_type2(&mut table, 2); // 3 page
    push_type1(&mut table, objstm_at); // 4 object stream
    push_type1(&mut table, xref_at); // 5 xref stream
    push_type2(&mut table, 3); // 6 field

    let compressed_table = deflate(&table);
    write!(
        bytes,
        "5 0 obj
<< /Type /XRef /Size 7 /W [1 4 2] /Root 1 0 R /Filter /FlateDecode /Length {} >>
stream
",
        compressed_table.len()
    )
    .unwrap();
    bytes.extend_from_slice(&compressed_table);
    bytes.extend_from_slice(b"
endstream
endobj
");
    write!(bytes, "startxref
{xref_at}
%%EOF
").unwrap();
    bytes
}

#[test]
fn a_modern_document_reports_its_pages_and_its_form() {
    // Resolving compressed objects only in `fetch_object_bytes` was not enough:
    // `scan_bytes` builds the summary the whole product opens with, and it read
    // objects the old way. A modern PDF therefore reported **no pages and no
    // form** — worse than an error, because it looks like an answer.
    // [FR-VIEW-2, FR-FORM-1, GR-8]
    let data = modern_form_document();
    let summary = scan_bytes(&data).expect("a modern document must open");

    assert_eq!(summary.page_count, 1, "the page tree is inside the object stream");
    assert!(summary.has_acroform, "the AcroForm is inside the object stream");
}

#[test]
fn a_modern_documents_form_fields_are_readable() {
    let data = modern_form_document();
    let scan = pdf_cos::acroform::extract_acroform_fields(&data).expect("scan");

    assert_eq!(scan.fields.len(), 1, "notes: {:?}", scan.notes);
    assert_eq!(scan.fields[0].name, "full_name");
    assert_eq!(scan.fields[0].value, "Ada Lovelace");
}
