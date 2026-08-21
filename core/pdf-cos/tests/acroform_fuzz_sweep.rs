//! Randomised sweep over the AcroForm field extractor.
//!
//! `extract_acroform_fields` parses raw document bytes, so it sits on the same
//! untrusted-input path as the structural scanner and ADR-022 T-4 applies to it
//! identically. It had no sweep.
//!
//! Same generator and same determinism guarantees as `scan_fuzz_sweep.rs`; see
//! that file's header for why the sweep is seeded rather than random and why it
//! is not a substitute for a coverage-guided fuzzer.
//!
//! [ADR-022, T-4, SDS §12.6, GR-1, PRIN-1]

use pdf_cos::acroform::extract_acroform_fields;

/// Build a document carrying an AcroForm with two fields and a calculation.
///
/// Offsets are computed while appending rather than hardcoded: a fixture whose
/// xref is wrong fails at the first check, and every mutant then bails before
/// reaching the code under test. The liveness test at the bottom of this file
/// exists to catch exactly that, and caught it during development.
fn form_pdf() -> Vec<u8> {
    let objects: [&[u8]; 6] = [
        b"<</Type /Catalog /Pages 2 0 R /AcroForm 4 0 R>>",
        b"<</Type /Pages /Kids [3 0 R] /Count 1>>",
        b"<</Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Annots [5 0 R 6 0 R]>>",
        b"<</Fields [5 0 R 6 0 R] /CO [6 0 R]>>",
        b"<</Type /Annot /Subtype /Widget /FT /Tx /T (a) /V (12) /Rect [10 10 100 30]>>",
        b"<</Type /Annot /Subtype /Widget /FT /Tx /T (total) /Rect [10 40 100 60]>>",
    ];

    let mut out: Vec<u8> = b"%PDF-1.4
".to_vec();
    let mut offsets = Vec::with_capacity(objects.len());
    for (i, body) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj
", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"
endobj
");
    }

    let xref_at = out.len();
    out.extend_from_slice(format!("xref
0 {}
", objects.len() + 1).as_bytes());
    out.extend_from_slice(b"0000000000 65535 f 
");
    for off in &offsets {
        out.extend_from_slice(format!("{off:010} 00000 n 
").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer
<</Size {} /Root 1 0 R>>
startxref
{}
%%EOF",
            objects.len() + 1,
            xref_at
        )
            .as_bytes(),
    );
    out
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        if n == 0 { 0 } else { (self.next() % n as u64) as usize }
    }
}

fn mutate(bytes: &mut Vec<u8>, rng: &mut Rng) {
    if bytes.is_empty() {
        return;
    }
    match rng.below(6) {
        0 => {
            let i = rng.below(bytes.len());
            bytes[i] ^= 1 << rng.below(8);
        }
        1 => {
            let n = rng.below(bytes.len());
            bytes.truncate(n);
        }
        2 => {
            // Unbalance a delimiter. Dictionary and array nesting is where a
            // hand-written object parser runs off the end.
            let delims: Vec<usize> = bytes
                .iter()
                .enumerate()
                .filter(|(_, b)| matches!(*b, b'<' | b'>' | b'[' | b']' | b'(' | b')'))
                .map(|(i, _)| i)
                .collect();
            if !delims.is_empty() {
                let i = delims[rng.below(delims.len())];
                bytes[i] = b' ';
            }
        }
        3 => {
            let start = rng.below(bytes.len());
            let end = (start + 1 + rng.below(40)).min(bytes.len());
            bytes.drain(start..end);
        }
        4 => {
            let at = rng.below(bytes.len());
            let n = 1 + rng.below(12);
            let junk: Vec<u8> = (0..n).map(|_| rng.below(256) as u8).collect();
            let tail = bytes.split_off(at);
            bytes.extend_from_slice(&junk);
            bytes.extend_from_slice(&tail);
        }
        _ => {
            let digits: Vec<usize> = bytes
                .iter()
                .enumerate()
                .filter(|(_, b)| b.is_ascii_digit())
                .map(|(i, _)| i)
                .collect();
            if !digits.is_empty() {
                let i = digits[rng.below(digits.len())];
                bytes[i] = b'0' + (rng.below(10) as u8);
            }
        }
    }
}

#[test]
fn mutated_acroform_documents_never_crash_the_extractor() {
    const SEEDS: [u64; 6] = [3, 11, 2026, 0xABCD, 0x1234_5678, 777];
    const CASES_PER_SEED: usize = 400;

    for seed in SEEDS {
        let mut rng = Rng(seed);
        for case in 0..CASES_PER_SEED {
            let mut bytes = form_pdf();
            for _ in 0..=rng.below(5) {
                mutate(&mut bytes, &mut rng);
            }
            let outcome = std::panic::catch_unwind(|| {
                let _ = extract_acroform_fields(&bytes);
            });
            assert!(
                outcome.is_ok(),
                "extractor panicked on seed {seed} case {case}; bytes = {:?}",
                bytes
            );
        }
    }
}

/// Truncation sweep: every prefix must be handled without panicking.
#[test]
fn no_prefix_of_a_form_document_can_crash_the_extractor() {
    for len in 0..form_pdf().len() {
        let outcome = std::panic::catch_unwind(|| {
            let _ = extract_acroform_fields(&form_pdf()[..len]);
        });
        assert!(outcome.is_ok(), "extractor panicked on prefix of length {len}");
    }
}

/// The sweep has to reach the extractor rather than bailing at the first check.
#[test]
fn the_sweep_produces_inputs_that_reach_the_extractor() {
    let mut rng = Rng(4242);
    let (mut ok, mut err) = (0, 0);
    for _ in 0..400 {
        let mut bytes = form_pdf();
        mutate(&mut bytes, &mut rng);
        match extract_acroform_fields(&bytes) {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
    }
    assert!(ok > 0, "single mutations should sometimes still extract; got none");
    assert!(err > 0 || ok > 0, "the extractor must be reached at all");
}
