//! Deterministic randomised sweep over the structural scanner.
//!
//! ADR-022 T-4: "Any code reachable by untrusted document bytes is fuzz-targeted;
//! new parsers add a fuzz target." No fuzz target existed anywhere in the
//! workspace — `forms_js` carries a handful of hand-written fixed cases named
//! `fuzz_*`, which is not a sweep — while `pdf_cos::scan` is the first thing a
//! hostile file reaches, inside the Z1 worker.
//!
//! That surface had a live panic: `find_startxref` sliced `tail[0..9]` out of a
//! shorter slice, aborting the worker on any document under nine bytes. It was
//! found by the prefix sweep in `leniency_corpus.rs`. This file generalises that
//! from a handful of shapes to a structure-aware mutation sweep.
//!
//! **Deterministic on purpose.** A fixed seed set means a CI failure reproduces
//! exactly and the offending bytes print, so a crash becomes a named fixture
//! rather than a flake. It is not a substitute for a real coverage-guided
//! fuzzer under `cargo-fuzz`, which needs a nightly toolchain and belongs in its
//! own change; it is the stratum that can run in the existing gate today.
//!
//! No dependency is added: the generator is a nine-line xorshift.
//!
//! [ADR-022, T-4, SDS §12.6, GR-1, GR-8, PRIN-1]

use pdf_cos::scan::scan_bytes;

const VALID: &[u8] = b"%PDF-1.4\n\
1 0 obj\n\
<</Type /Catalog /Pages 2 0 R>>\n\
endobj\n\
2 0 obj\n\
<</Type /Pages /Kids [3 0 R] /Count 1>>\n\
endobj\n\
3 0 obj\n\
<</Type /Page /Parent 2 0 R /MediaBox [0 0 612 792]>>\n\
endobj\n\
xref\n\
0 4\n\
0000000000 65535 f \n\
0000000009 00000 n \n\
0000000056 00000 n \n\
0000000111 00000 n \n\
trailer\n\
<</Size 4 /Root 1 0 R>>\n\
startxref\n\
180\n\
%%EOF";

/// xorshift64*. Deterministic, no dependency, good enough to pick offsets.
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
        if n == 0 {
            0
        } else {
            (self.next() % n as u64) as usize
        }
    }
}

/// Apply one structure-aware mutation. The interesting defects in a PDF parser
/// live in the offsets and counts it reads out of the file, so the mutations are
/// weighted toward those rather than uniform byte noise.
fn mutate(bytes: &mut Vec<u8>, rng: &mut Rng) {
    if bytes.is_empty() {
        return;
    }
    match rng.below(7) {
        0 => {
            // Flip one byte.
            let i = rng.below(bytes.len());
            bytes[i] ^= 1 << rng.below(8);
        }
        1 => {
            // Truncate.
            let n = rng.below(bytes.len());
            bytes.truncate(n);
        }
        2 => {
            // Corrupt a digit, which is how offsets and counts are encoded.
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
        3 => {
            // Splice out a span.
            let start = rng.below(bytes.len());
            let end = (start + 1 + rng.below(32)).min(bytes.len());
            bytes.drain(start..end);
        }
        4 => {
            // Insert noise.
            let at = rng.below(bytes.len());
            let n = 1 + rng.below(16);
            let junk: Vec<u8> = (0..n).map(|_| rng.below(256) as u8).collect();
            let tail = bytes.split_off(at);
            bytes.extend_from_slice(&junk);
            bytes.extend_from_slice(&tail);
        }
        5 => {
            // Duplicate a span — exercises repeated keywords and sections.
            let start = rng.below(bytes.len());
            let end = (start + 1 + rng.below(48)).min(bytes.len());
            let span = bytes[start..end].to_vec();
            let at = rng.below(bytes.len());
            let tail = bytes.split_off(at);
            bytes.extend_from_slice(&span);
            bytes.extend_from_slice(&tail);
        }
        _ => {
            // Zero a run, which produces the degenerate lengths that break
            // unchecked arithmetic.
            let start = rng.below(bytes.len());
            let end = (start + 1 + rng.below(24)).min(bytes.len());
            for b in &mut bytes[start..end] {
                *b = 0;
            }
        }
    }
}

/// Every mutant must return `Ok` or `Err`. A panic is a crash on untrusted input
/// and aborts the sandboxed worker, which the coordinator can only report as
/// "transport disconnected". [PRIN-1, T-4, GR-1]
#[test]
fn mutated_documents_never_crash_the_scanner() {
    const SEEDS: [u64; 8] = [1, 7, 42, 1337, 0xDEAD_BEEF, 0x5EED, 99_991, 0xF00D_F00D];
    const CASES_PER_SEED: usize = 400;
    const MUTATIONS_PER_CASE: usize = 6;

    let mut checked = 0usize;
    for seed in SEEDS {
        let mut rng = Rng(seed);
        for case in 0..CASES_PER_SEED {
            let mut bytes = VALID.to_vec();
            for _ in 0..=rng.below(MUTATIONS_PER_CASE) {
                mutate(&mut bytes, &mut rng);
            }

            // Any panic here fails the test with the seed and case printed, so
            // the exact input is reproducible and can become a named fixture.
            let outcome = std::panic::catch_unwind(|| {
                let _ = scan_bytes(&bytes);
            });
            assert!(
                outcome.is_ok(),
                "scanner panicked on seed {seed} case {case}; bytes = {:?}",
                bytes
            );
            checked += 1;
        }
    }

    assert_eq!(checked, SEEDS.len() * CASES_PER_SEED);
}

/// The generator has to actually reach the parser rather than producing inputs
/// that all bail at the first check. Without this, the sweep above could pass by
/// never testing anything. [AI-7 — a test that cannot fail proves nothing]
#[test]
fn the_sweep_produces_inputs_that_reach_the_parser() {
    let mut rng = Rng(2026);
    let mut parsed_ok = 0;
    let mut rejected = 0;

    for _ in 0..400 {
        let mut bytes = VALID.to_vec();
        mutate(&mut bytes, &mut rng);
        match scan_bytes(&bytes) {
            Ok(_) => parsed_ok += 1,
            Err(_) => rejected += 1,
        }
    }

    assert!(
        parsed_ok > 0,
        "single mutations should sometimes still parse; got none in 400"
    );
    assert!(
        rejected > 0,
        "single mutations should sometimes be rejected; got none in 400"
    );
}
