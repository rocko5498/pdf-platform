# Design: where qpdf runs, and under what confinement

**Date:** 2026-08-22
**Milestone:** M6+ (blocks "Optimization pipeline safety net — NOT sandboxed")
**Status:** Design only — no code in this change. IG §2.3 requires review before
implementation, and AI-6 makes the decision itself human-gated: this changes where
untrusted document bytes are parsed.
**Cites:** ADR-008, ADR-012, ADR-016, ADR-022, SDS §2.2.6, SDS §12.2, GR-1, GR-7,
GR-8, PRIN-1, PRIN-6, AI-6, FR-OPT-4

---

## Problem

`optimize` runs qpdf, and qpdf parses the user's document. Today
`pdf_model::assembly_ops::optimize_pdf` spawns it with `Command::new(qpdf)` from
whichever process called it — the coordinator, or the CLI acting in the same role.
The tracker records this as "NOT sandboxed"; the sharper statement is that an
external parser of untrusted bytes runs **as an unconfined child of Z0, with the
user's full privileges**, while GR-1 says "no document parsing in Z0".

`coordinator::broker::optimize_with_verification` wraps that call in a
candidate/verify/publish sequence, so a crash mid-write cannot corrupt the user's
file. That is a durability property. It is not a confinement property, and the
two have been conflated in the row's wording.

## The obstacle that decides this

The obvious fix — run qpdf inside the Z1 worker, where document parsing belongs —
**cannot work**. `sandbox::confinement::LINUX_SYSCALL_DENYLIST` denies `execve`,
`fork` and `vfork`, and the macOS and Windows profiles are written to the same
intent. A confined worker cannot spawn a child process at all. Relaxing that to
let the worker exec an external binary would widen the confinement contract
specifically to admit an unaudited parser, which is the opposite of the guardrail's
purpose, and AI-6 forbids an agent from relaxing a sandbox constraint.

A second obstacle follows from the first. Z1 receives files as **inherited
descriptors**, never paths, and qpdf's interface is paths. Handing qpdf a path
means granting filesystem access to something the confinement is built to deny.

So the question is not "how do we move qpdf into the sandbox". It is "given that
qpdf cannot go there, what should happen instead".

## Options

### A. Confine qpdf where it already runs — the broker spawns it sandboxed

The broker is "sole executor of privileged ops" (SDS §2.2.6), so it is the correct
place for a privileged spawn. It would launch qpdf under a per-OS confinement
profile of its own: no network, no filesystem except the input file and the
candidate output, reduced privileges, a CPU/memory bound (GR-7).

- **For:** smallest change to the pipeline; keeps qpdf, which is a mature and
  widely-audited implementation; the candidate/verify/publish safety net is
  untouched; the confinement contract for Z1 is not widened.
- **Against:** a second confinement implementation appears, one for workers and one
  for tool children, and per-OS sandboxing of an arbitrary child is genuinely
  fiddly (AppContainer needs an ACL'd file grant; the macOS profile needs a path
  allowlist; Linux needs seccomp plus a mount view or landlock).
- **Honest note:** this is confinement of a *process we spawn*, not of a parser we
  control. If qpdf has a bug that the sandbox does not contain, the result is a
  contained crash, not a prevented parse.

### B. Replace qpdf with our own rewrite path, executed in Z1

`pdf-write` already owns serialization, and ADR-012 §3 describes full rewrite as a
first-class operation. Optimization becomes a coordinator command executed by the
worker: parse in Z1, rewrite in Z1, emit through an inherited output descriptor.

- **For:** the guardrail is satisfied by construction rather than by a second
  sandbox; no external binary; no path grants; works identically on all three OSes;
  removes a runtime dependency users must install (`optimize` currently fails
  outright without qpdf on PATH).
- **Against:** substantially more work, and it trades a mature implementation for a
  new one on the exact axis — object stream generation, stream recompression,
  resource deduplication — where qpdf's maturity is worth most. Doing this badly is
  worse than option A, because a rewrite defect silently degrades a document the
  user asked us to *improve*.

### C. Do nothing, and say so

Keep the current arrangement and mark the row as an accepted risk rather than an
open defect.

- **For:** honest; costs nothing.
- **Against:** it leaves a GR-1 breach standing, and the same reasoning that
  removed the runtime PDFium download applies here.

## Recommendation

**A now, B later, and never C silently.**

Option A closes the guardrail breach without betting the optimizer's correctness
on new code. Option B is the right end state and should be written down as such,
but it is a milestone of its own, not a step inside this one.

If A is chosen, the work is: a `broker::spawn_confined_tool` seam with per-OS
profiles, an explicit file grant for exactly the input and candidate paths, a
resource bound, and a test that the spawned tool cannot open a third file or reach
the network.

## Testing

Per ADR-022 strata:

- **T-1** The tool profile denies a path outside its grant; denies network.
- **T-5** `optimize` still produces a byte-identical result to the unconfined path
  for the corpus fixtures — confinement must not change output.
- **T-5** A tool that exceeds its CPU or memory bound is killed and reported as a
  failed optimization, never as a silent partial write (GR-8).
- The existing candidate/verify/publish tests must pass unchanged: this design
  changes *where* qpdf runs, not what happens to its output.

## Success criteria

- [ ] No untrusted document is parsed by an unconfined process.
- [ ] Z1's confinement contract is unchanged — no new syscalls allowed.
- [ ] `optimize` output is unchanged for the corpus.
- [ ] A missing or unusable qpdf is still an honest error, not a silent skip.
- [ ] The tracker row distinguishes durability (already done) from confinement.

## What an agent may not decide here

AI-6: this is sandbox and privileged-spawn work. An agent may draft the seam and
the tests; a human owner must review, and must be the one to accept whether A or B
is the direction. Nothing in this note should be implemented until that choice is
recorded.

---

*Design only. No source file is modified by this change.*
