# Product Requirements Document (PRD)

**Product:** Open-source professional PDF platform (working name: *the Platform*)
**Document class:** Canonical product specification. Intended to govern the product for a 10+ year maintenance horizon.
**Companion documents (authoritative for implementation):** *Engineering Constitution* (ADR-001 … ADR-030); *System Design Specification* (SDS). This PRD references them as `[ADR-NNN]` and `[SDS §N]` and does not restate implementation. Where this PRD and an ADR/SDS appear to conflict on *what* the product does, this PRD governs product intent; where they concern *how*, the ADR/SDS govern.
**Audience:** Product Management, Architecture, Engineering Leads, UX Design, QA, Documentation, Contributors.

---

## Document Conventions (Normative)

This PRD uses RFC 2119 / RFC 8174 key words to indicate requirement levels:

- **MUST** / **MUST NOT** / **REQUIRED** / **SHALL** — an absolute requirement or prohibition. A release that violates a MUST is non-conformant.
- **SHOULD** / **SHOULD NOT** / **RECOMMENDED** — a strong default. Deviation REQUIRES a documented, reviewed rationale.
- **MAY** / **OPTIONAL** — a genuinely discretionary capability.

Each requirement carries a stable identifier of the form `FR-<area>-<n>` (functional), `NFR-<area>-<n>` (non-functional), `UX-<area>-<n>`, `ENT-<n>`, or `CMP-<n>`. Identifiers are permanent; a withdrawn requirement is marked *Withdrawn* rather than reused.

**Normative** text defines conformance. **Informative** text (marked *Informative*) explains, illustrates, or motivates and does not define conformance.

**[PRD Decision]** marks a product decision first made in this document (as opposed to one inherited from the vision brief or the ADR/SDS). These are the items most warranting review.

*Testability standard:* every normative requirement MUST be expressed such that a QA engineer can construct a pass/fail test or a measurable acceptance criterion from it. Requirements that specify user-visible behavior name the observable; requirements that specify quality attributes name the metric and the target (or reference the metric defined in §14).

---

# 1. Executive Summary

*Informative.*

## 1.1 Purpose

The Platform is a native, desktop-first, fully open-source application for viewing, creating, editing, reviewing, securing, and transforming PDF documents. Its objective is to replace Adobe Acrobat Pro for the substantial majority of professional workflows while remaining free of the constraints that drive dissatisfaction with incumbent products: subscription lock-in, coercive commercial practices, interface instability, performance degradation, forced cloud dependency, and opaque handling of user documents.

## 1.2 What the Platform is

The Platform is a professional PDF tool for individuals and organizations that require: faithful rendering of real-world (including malformed) documents; precise and non-destructive editing that preserves document integrity; a complete review and annotation workflow interoperable with existing tools; standards-compliant creation and validation (PDF/A, PDF/UA, PDF/X); trustworthy security features (encryption, redaction that verifiably removes content, digital signatures meeting recognized profiles); accessibility both *of* the application and *for* the documents it produces; and automation via a command-line interface and an extensible plugin ecosystem. It operates fully offline by default and treats user documents as private.

## 1.3 What the Platform is not

The Platform is not a cloud service, not a subscription product in its open-source core, not a mobile-first application, and not a general office suite. It does not require an account, does not transmit user documents or telemetry by default, and does not implement legacy or deprecated technologies (for example, full XFA authoring) where doing so would compromise its quality or security posture. These boundaries are specified normatively in §8.

## 1.4 Primary differentiators

The Platform's differentiation rests on properties that incumbents are structurally unable or unwilling to provide, established as enforced product values (§4, §5) rather than aspirations:

1. **Trust by construction** — open source, offline-first, no accounts, no default telemetry, verifiable builds.
2. **Performance as a contract** — published, measured, regression-gated performance budgets (§10, §14).
3. **Interface stability as a contract** — a familiar, Acrobat-compatible mental model whose keyboard shortcuts and core workflows do not change under users without opt-in (§11).
4. **Fidelity above features** — the product never silently degrades or transcodes a document; edits are surgical and preserve everything untouched.
5. **Honest capability boundaries** — where the product cannot act safely, it says so rather than producing an incorrect or unsafe result.

## 1.5 Platforms and editions

The Platform targets Windows, macOS, and Linux as co-equal, first-class desktop platforms. The core product is and will remain fully open source. Commercial editions or services MAY be offered later; if offered, they MUST NOT remove or degrade functionality present in the open-source core, and MUST NOT introduce the coercive practices this product exists to avoid (§4, §12).

## 1.6 Success in one sentence

The Platform succeeds when a professional who has used Adobe Acrobat Pro for years can adopt it without retraining, complete their existing workflows at equal or greater speed and reliability, trust it with confidential documents, and never encounter a commercial or interface practice that betrays that trust.

---

# 2. Product Vision

*Informative, except §2.4 which is normative.*

## 2.1 Vision statement

To be the definitive open-source PDF platform: the application that professionals, institutions, and the public reach for by default when they need to work with PDF documents correctly, privately, and without compromise — and the platform on which a durable open ecosystem of PDF tooling is built.

## 2.2 The problem the Platform addresses

The PDF is the world's format of record for finished documents: contracts, filings, scholarly articles, forms, invoices, engineering drawings, government records, and archives. The dominant professional tool for working with PDFs is expensive, increasingly disliked for its commercial and interface practices, heavy, and cloud-entangled. The open-source alternatives are individually incomplete: strong viewers without editing; annotation tools that do not write into the format; libraries without applications; platform-specific tools without cross-platform reach; and near-total absence of trustworthy open desktop signing and remediation. No open-source product today is a credible, complete replacement for Acrobat Pro across professional workflows.

Simultaneously, regulatory and social forces are enlarging the demand this product serves: accessibility mandates make document remediation legally required in growing domains; electronic-signature regulation makes standards-compliant signing legally significant; and privacy expectations make offline, non-telemetric handling of confidential documents a first-order requirement rather than a preference.

## 2.3 The opportunity

An open-source project is structurally immune to the causes of incumbent decline. It has no subscription revenue to protect, no growth metric compelling interface churn, no incentive to entangle documents with a cloud, and no reason to withhold verifiability. It can therefore make — and keep — the exact promises the market is asking for. The opportunity is to convert that structural advantage into a product of professional quality across the full workflow surface.

## 2.4 Vision-level commitments (Normative)

The following commitments are binding on all releases and supersede any conflicting convenience:

- **VIS-1.** The open-source core MUST remain capable of performing, offline and without an account, every workflow this PRD marks as in-scope and as belonging to the core.
- **VIS-2.** The Platform MUST NOT, in its open-source core, transmit user document content or personal data to any network destination except as the direct, visible, and consented result of a user action.
- **VIS-3.** The Platform MUST NOT require network connectivity to open, view, edit, save, print, or secure a local document.
- **VIS-4.** Any commercial edition or service MUST NOT remove, gate, or degrade functionality designated as core in this PRD.

## 2.5 Ten-year horizon (Informative)

This PRD assumes the product will outlive its initial contributors and its initial engine and UI technology choices. It therefore specifies product intent in terms of user-observable behavior and quality attributes, so that the product's identity survives implementation change. The roadmap (§16) is directional; the vision and principles (§2, §4) are intended to be stable for the life of the product.

---

# 3. Mission Statement

*Informative.*

**Mission:** Give everyone — individuals, professionals, and institutions — a complete, trustworthy, and permanent way to work with PDF documents that they fully control.

The mission has four load-bearing words:

- **Complete** — the Platform aims to cover the professional workflow surface, not a convenient subset. Gaps are explicit and roadmapped, not hidden.
- **Trustworthy** — correctness, security, privacy, and honesty about limitations are prerequisites, not features. The product earns trust by being verifiable, not by asking for it.
- **Permanent** — as open source with an offline-first, standards-based design, the Platform cannot be discontinued, subscription-locked, or taken from its users. Documents created or secured with it remain workable indefinitely.
- **Control** — users decide where their documents live, what leaves their machine, when the software changes, and how it behaves. The default is local, private, and stable.

---

# 4. Design Principles

*Normative. These principles constrain every requirement, feature, and release. Where a specific requirement seems to conflict with a principle, the conflict MUST be resolved and documented before release.*

The principles restate, at product level, the enforced values of `[ADR-001]` and add product-facing corollaries.

## 4.1 Correctness before capability
**PRIN-1.** A feature MUST be correct before it is complete. The Platform MUST prefer refusing an operation with a clear explanation over performing it incorrectly. Rendering, extraction, mutation, and security operations MUST be validated against real-world documents and recognized reference behavior, not only against the specification.

## 4.2 Fidelity and non-destruction
**PRIN-2.** The Platform MUST NOT alter parts of a document the user did not intend to change. Saving MUST default to a mechanism that preserves untouched content byte-for-byte and preserves existing digital signatures where the change permits `[ADR-012]`. Any operation that discards data (history, signatures, structure, quality) MUST be explicit, labeled, and preceded by a disclosure of what will be lost.

## 4.3 Trust by construction
**PRIN-3.** The Platform MUST be private and offline by default (VIS-2, VIS-3), MUST NOT collect telemetry without opt-in, MUST NOT require accounts for core functionality, and MUST be distributed in a verifiable form. Trust properties MUST be architectural and auditable rather than promised.

## 4.4 Interface stability as a contract
**PRIN-4.** The Platform MUST preserve a stable, familiar interaction model. Keyboard shortcuts and core workflow steps MUST NOT change under a user without an opt-in mechanism, and a previously offered classic behavior MUST remain available (§11, `[ADR-030]`). Interface improvements MUST target measured deficiencies, not novelty.

## 4.5 Performance as a contract
**PRIN-5.** Interactive responsiveness and resource use are product requirements with published budgets (§10, §14). A release MUST NOT regress a published budget beyond its stated tolerance. Performance is defended continuously, not restored periodically.

## 4.6 Honesty about limitations
**PRIN-6.** The Platform MUST communicate clearly when a document is damaged (and what was repaired), when a feature is unsupported (and what was skipped), when an operation cannot be performed safely, and when a security determination is indeterminate. It MUST NOT present a false success, a false "valid," or a silent substitution.

## 4.7 Interoperability
**PRIN-7.** Documents the Platform produces MUST be correctly consumable by other conformant PDF software, and the Platform MUST correctly consume documents those tools produce. Interoperability is verified against actual incumbent products, not only against the standard (§13).

## 4.8 Accessibility as a dual obligation
**PRIN-8.** The Platform MUST be operable by users of assistive technology (accessibility *of* the application) and MUST enable the creation and verification of accessible documents (accessibility *for* documents). Neither obligation is optional or deferrable to a plugin.

## 4.9 Extensibility without compromise
**PRIN-9.** The Platform MUST be extensible by third parties without allowing extensions to compromise stability, security, privacy, or the user's control. Extensions operate within explicit, user-granted boundaries (§9, `[ADR-014]`).

## 4.10 Longevity and maintainability
**PRIN-10.** Product decisions MUST favor mechanisms that a changing group of contributors can maintain for a decade: explicit behavior over cleverness, standards over bespoke formats, verifiable claims over assurances, and documented intent over tribal knowledge.

## 4.11 Precedence
**PRIN-11.** When principles tension against one another, the ordering for resolution is: **safety of the user's data and system** (PRIN-1, PRIN-2, PRIN-3) precedes **honesty** (PRIN-6) precedes **interoperability and fidelity** (PRIN-7) precedes **performance** (PRIN-5) precedes **capability and convenience**. Interface stability (PRIN-4) MUST NOT be overridden except by an explicit, opt-in user or administrator choice.

---

# 5. Product Philosophy

*Informative, except where it cross-references normative requirements.*

## 5.1 The Platform is a professional instrument

The Platform is designed as a precise instrument for people whose work depends on documents. Like a professional tool in any field, it favors predictability, precision, and the operator's retained skill over novelty and hand-holding. It assumes its users' existing expertise is an asset to be preserved (PRIN-4), not a habit to be re-educated. This is the central reason the product preserves the Acrobat mental model: for a professional, muscle memory is part of their competence, and changing it without consent is a cost imposed on them, not a gift.

## 5.2 Familiar by default, better where it counts

The product does not pursue interface novelty. It reproduces the workflows, navigation, and shortcuts that long-time Acrobat users already know, and it invests its design effort specifically where the incumbent is measurably slow, confusing, inconsistent, or missing capability. A user should feel immediately at home and then, over time, notice that specific frustrations have simply disappeared. *Informative examples:* faster startup and scrolling; a save that never corrupts; a redaction they can verify; a signature validation that explains itself; an OCR that does not refuse pages arbitrarily.

## 5.3 The document is sacred

The user's document is the product's most important asset, and it belongs to the user. The Platform treats every document as private (PRIN-3) and as something to be preserved rather than rewritten (PRIN-2). This philosophy has concrete consequences throughout the requirements: incremental save by default, verifiable redaction, explicit sanitization, no hidden history accumulation, no silent metadata leakage, and no transmission without consent.

## 5.4 Honesty is a feature

Most software hides its uncertainty. The Platform surfaces it (PRIN-6). When a file is damaged, the user is told what was repaired. When a construct is unsupported, the user is told what was skipped. When an edit cannot be made safely, the user is told why. When a signature cannot be conclusively validated, the result is "indeterminate," never a false "valid." This honesty is not a limitation to be apologized for; it is a differentiator that professionals — especially in legal, medical, financial, and government contexts — actively require.

## 5.5 Local-first, cloud-optional, never cloud-required

The Platform operates fully on the user's machine (VIS-3). It does not treat the cloud as the default location for documents, identity, or computation. Optional, clearly-bounded, self-hostable network features MAY exist in the future (§9.30, §16), always as explicit user choices and never as a dependency for core work.

## 5.6 Open by conviction, not by default

The Platform is open source because openness is the mechanism by which its trust claims become verifiable and its permanence becomes guaranteed. Openness is not merely a distribution model here; it is the enforcement mechanism for PRIN-3 and PRIN-10. The project's governance, build verifiability, and contribution processes exist to keep that enforcement real over a decade.

## 5.7 Extensible, so the community can exceed the core

The Platform cannot and should not implement every niche workflow itself. It provides an extension model (§9.28, `[ADR-014]`) so the community can build capabilities the core team would never prioritize, without those extensions endangering the properties the core guarantees. The measure of the platform's success is not only what it does, but what others can safely build on it.

## 5.8 Restraint

The Platform says no. It declines to implement deprecated technologies that would compromise it, declines to add features that would erode performance or clarity without proportionate value, and declines commercial practices that would betray its users. Restraint is how the product avoids becoming the thing its users are fleeing (§5, §15 community risks).

---

# 6. Target Users

*Informative framing; persona success criteria referenced by QA are testable via the named workflows.*

This section defines the personas the Platform serves. Each persona lists **goals**, **primary workflows**, **pain points with incumbents**, and **success criteria** (the observable conditions under which the Platform has served that persona). Personas are not mutually exclusive; a single real user may combine several. Where a persona imposes requirements, those requirements appear normatively in §9–§12 and are cross-referenced.

The personas are grouped into three bands by depth of use: **General** (occasional, breadth-seeking), **Professional** (daily, workflow-specific), and **Technical/Administrative** (automation, deployment, extension). Design priority (Informative) favors the Professional band for feature depth and the General band for first-run clarity, without sacrificing either.

## 6.1 General band

### 6.1.1 Casual user
**Goals.** Open, read, and print PDFs; fill an occasional form; combine a few files; sign a document once in a while.
**Workflows.** Open from file manager or email; read; search; print; fill and return a form; merge a scan with a cover letter.
**Pain points.** Incumbent tools are heavy, slow to start, and push accounts, subscriptions, and cloud storage for tasks that should be immediate and local.
**Success criteria.** The Platform opens near-instantly, requires no account, performs the task without upsell, and prints correctly. A casual user completes a form-fill-and-print with no training.

### 6.1.2 Student
**Goals.** Read and annotate course material; highlight and take margin notes; extract quotations with correct text; combine readings; fill and submit forms.
**Workflows.** Heavy reading with highlights and comments; copy text for citations (requiring correct extraction, not garbled ligatures); merge lecture PDFs; export notes.
**Pain points.** Copying text yields broken characters; annotations made in one tool vanish in another; large scanned readers are slow and unsearchable.
**Success criteria.** Highlights and notes persist and are portable to other readers (PRIN-7); copied text is correct (FR-SRCH, FR-A11Y export); scanned readings become searchable via OCR (FR-OCR).

### 6.1.3 Office worker
**Goals.** Produce, combine, lightly edit, and share business documents; fill and prepare forms; apply simple protection; comment on drafts.
**Workflows.** Convert and combine documents; reorder and delete pages; add headers/footers and page numbers; fill forms; comment; password-protect before emailing.
**Pain points.** Reorganizing pages is clumsy; "edit" tools re-flow and corrupt layout; protection and permissions are confusing.
**Success criteria.** Page organization is fast and reliable (FR-ORG); light edits do not damage the document (PRIN-2); protection is understandable and its limits are stated honestly (FR-SEC, PRIN-6).

## 6.2 Professional band

### 6.2.1 HR professional
**Goals.** Manage employment forms, offers, and policies; collect signatures; redact personal data; ensure accessible documents for all employees.
**Workflows.** Prepare and distribute fillable forms; collect and validate signatures; redact PII from records before sharing; produce accessible policy documents (PDF/UA).
**Pain points.** Redaction that only visually hides data (a compliance hazard); no trustworthy open signing; accessibility remediation is painful.
**Success criteria.** Redaction verifiably removes content and is provable (FR-RED, PRIN-6); signatures meet recognized profiles (FR-SIG); documents can be made and validated as accessible (FR-A11Y, FR-STD).

### 6.2.2 Accountant / finance professional
**Goals.** Work with statements, invoices, and tax forms; fill and calculate forms; combine and organize records; apply Bates numbering to document sets; archive to a durable standard.
**Workflows.** Fill forms whose embedded calculations must compute correctly; assemble and paginate document packages; apply sequential numbering; export to PDF/A for retention; handle e-invoice attachments (embedded structured data).
**Pain points.** Forms mis-calculate silently in non-Adobe tools; archival conversion is opaque; embedded invoice data is ignored.
**Success criteria.** Form calculations compute correctly for common enterprise forms (FR-JS); PDF/A export is validated (FR-STD); embedded files are accessible (FR-EMB); Bates numbering is correct across sets (FR-STAMP).

### 6.2.3 Lawyer / legal professional
**Goals.** Review, redact, Bates-number, compare, and sign; assemble discovery sets; produce court-ready filings; guarantee that redacted content is gone.
**Workflows.** High-volume redaction with certainty; Bates numbering across thousands of pages; comparing document versions to find changes; combining exhibits; validating and applying signatures; ensuring no hidden metadata or history leaks.
**Pain points.** Cosmetic redaction has caused real disclosure scandals; version comparison is slow and imprecise; hidden document history and metadata leak confidential information.
**Success criteria.** Redaction is verifiable and reportable (FR-RED); comparison reliably identifies textual and visual changes (FR-CMP); sanitization removes history and metadata on demand and discloses what remains (FR-META, FR-VER); Bates numbering is exact (FR-STAMP).

### 6.2.4 Engineer
**Goals.** Read and mark up technical drawings and specifications; measure distances and areas; work with layered drawings; manage large multi-hundred-page documents.
**Workflows.** Navigate large specs; toggle optional-content layers; take measurements at scale; add dimensioned markups; compare revisions of drawings.
**Pain points.** Large documents are slow; layer support is weak; measurement tools are imprecise or absent in affordable tools.
**Success criteria.** Large documents remain responsive (NFR-PERF, NFR-LARGE); layers are viewable and toggleable (FR-LAYER); measurement is accurate at defined scales (FR-MEAS); comparison highlights drawing changes (FR-CMP).

### 6.2.5 Architect
**Goals.** Similar to Engineer, with emphasis on layered plans, scale accuracy, sheet sets, and high-fidelity color and lineweight rendering.
**Workflows.** Review plan sets; measure at architectural scales; annotate for coordination; manage sheet order; produce PDFs for permitting and print.
**Pain points.** Rendering inaccuracies in lineweights and color; slow handling of large plan sets; imprecise measurement.
**Success criteria.** High-fidelity rendering of vector line work and color (FR-VIEW, PRIN-1); accurate measurement (FR-MEAS); responsive large-set handling (NFR-LARGE).

### 6.2.6 Researcher / academic
**Goals.** Read, annotate, extract, and cite; manage large libraries of papers; extract data and figures; ensure correct text and reference extraction.
**Workflows.** Deep reading with annotation; extracting quotations and figures; searching across a corpus of papers; exporting annotations and notes; producing accessible and archivable outputs.
**Pain points.** Text extraction is unreliable (broken ToUnicode); cross-document search is weak or cloud-bound; annotations are not portable.
**Success criteria.** Correct extraction with honest flagging when unreliable (FR-SRCH, PRIN-6); optional local cross-document search (FR-SRCH-IDX); portable annotations (PRIN-7).

### 6.2.7 Publisher / production editor
**Goals.** Prepare, review, and finalize documents for publication; manage proofs and comments; ensure correct fonts, color, and structure; produce print- and archive-ready files.
**Workflows.** Collect and reconcile reviewer comments; verify embedded fonts and color; preflight for print or archive; export to PDF/X or PDF/A.
**Pain points.** Comment reconciliation across reviewers is manual; preflight tools are expensive; font/color issues surface late.
**Success criteria.** Comment aggregation and review workflow (FR-REV); preflight and standards validation (FR-STD); reliable font/color reporting (FR-VIEW, FR-STD).

### 6.2.8 Print shop / prepress operator
**Goals.** Receive customer PDFs and produce correct printed output; impose pages; verify color, overprint, and transparency; fix or flag problem files.
**Workflows.** Preflight incoming files; impose (n-up, booklet); verify separations, overprint, and transparency flattening; print to production devices.
**Pain points.** Incoming files are inconsistent; transparency and overprint render differently across tools; imposition tools are costly.
**Success criteria.** Prepress-accurate rendering including overprint and transparency (FR-VIEW, FR-STD, later phase); imposition and production printing (FR-PRINT); preflight reporting (FR-STD).

### 6.2.9 Government / public sector
**Goals.** Produce accessible public documents (legally required); archive to durable standards; redact under public-records law; apply and validate qualified signatures; deploy at scale under policy.
**Workflows.** Remediate documents to PDF/UA; convert to PDF/A for retention; redact for disclosure; sign with qualified certificates and hardware tokens; deploy and configure across many machines under central policy; operate offline in secure environments.
**Pain points.** Accessibility remediation tools are poor; qualified signing on desktop is nearly absent in open tools; cloud dependency is unacceptable in secure environments; per-seat licensing is costly at scale.
**Success criteria.** PDF/UA remediation and validation (FR-A11Y, FR-STD); PDF/A archival (FR-STD); verifiable redaction (FR-RED); qualified/hardware signing (FR-SIG); policy-based mass deployment and offline operation (§12, VIS-3).

## 6.3 Technical / Administrative band

### 6.3.1 Enterprise IT administrator
**Goals.** Deploy, configure, update, and support the Platform across an organization under policy, with predictability and without per-seat cost or forced change.
**Workflows.** Package and deploy via standard management tooling; enforce configuration (default behaviors, disabled features, update channel) via policy; validate and stage updates; support users; guarantee no unexpected network activity.
**Pain points.** Incumbent deployment is a cost center; forced updates break plugins and workflows; licensing administration (including offboarding) is punitive; unexpected telemetry violates policy.
**Success criteria.** Standard packaging and central policy control (§12); no forced updates; LTS availability; verifiable absence of default network activity (VIS-2, §12).

### 6.3.2 Developer / automation engineer
**Goals.** Automate PDF operations in pipelines and scripts without a GUI; integrate PDF processing into build, document, and data workflows; run headless on servers.
**Workflows.** Batch convert, merge, split, OCR, optimize, redact, sign, and validate via a command-line interface; script pipelines; run in CI and on servers without a display.
**Pain points.** GUI tools are not scriptable; capable libraries are fragmented; the death of a popular CLI tool (pdftk) left a gap; server use is awkward.
**Success criteria.** A complete, stable CLI with parity for scriptable operations (FR-CLI); headless operation; machine-readable output; the same core as the GUI so results are identical (PRIN-1, `[ADR-025]`).

### 6.3.3 Plugin author
**Goals.** Extend the Platform with new capabilities and distribute them to users, safely and with a stable contract.
**Workflows.** Build extensions against a documented, versioned extension interface; declare required capabilities; contribute tools, panels, and batch operations; publish for users to install.
**Pain points.** Incumbent native-plugin SDKs are unstable, unsafe, and platform-specific; extension APIs elsewhere break frequently.
**Success criteria.** A documented, semver-stable extension contract; capability-scoped, sandboxed execution that cannot destabilize the host; a compatibility test kit (FR-PLUG, `[ADR-014]`, `[ADR-015]`).

### 6.3.4 Accessibility user (cross-cutting persona)
**Goals.** Use the application fully with assistive technology, and consume documents accessibly.
**Workflows.** Operate every workflow via keyboard and screen reader; navigate document structure; have tagged content read in correct order; fill forms accessibly.
**Pain points.** Many tools are partially or wholly inaccessible; document accessibility is inconsistent.
**Success criteria.** Full keyboard and screen-reader operability of the application (UX-A11Y, NFR-A11Y); accessible reading of tagged documents (FR-A11Y). This persona's requirements are non-deferrable (PRIN-8).

### 6.3.5 Contributor (cross-cutting persona)
*Informative.* Goals: understand, modify, and extend the product over a long horizon. Success criteria: the product's behavior is specified (this PRD), its architecture is documented (ADR/SDS), and its intent is discoverable without tribal knowledge (PRIN-10). The contributor is served primarily by documentation quality and the stability of the specifications rather than by runtime features.

## 6.4 Persona-to-requirement traceability (Informative)

Every persona's success criteria trace to normative requirements in §9–§12. QA MUST be able to construct, for each persona, an end-to-end acceptance scenario composed only of specified requirements. The user stories in §7 provide these scenarios.

---

# 7. User Stories

*Informative as narrative; each story is written so that QA can derive an acceptance test. Stories use the form "As a `<persona>`, I want `<capability>`, so that `<outcome>`," followed where useful by acceptance notes. Stories are grouped by persona band and numbered `US-<area>-<n>` for reference. Requirement traceability appears as `→ FR/NFR/UX` tags. This section is intentionally large; it is the shared scenario library for UX, QA, and documentation.*

## 7.1 Casual user

- **US-CAS-1.** As a casual user, I want to open a PDF directly from my file manager or email attachment and see the first page almost immediately, so that reading feels instant. *Acceptance:* first page visible within the cold-start + first-page budgets (§14). → NFR-START, FR-VIEW
- **US-CAS-2.** As a casual user, I want to use the application without creating an account or seeing subscription prompts, so that I can just get my task done. → VIS-1, PRIN-3
- **US-CAS-3.** As a casual user, I want to scroll, zoom, and rotate a document smoothly, so that reading is comfortable. → FR-NAV, NFR-PERF
- **US-CAS-4.** As a casual user, I want to print a document and have it match what I see, so that the printout is correct. → FR-PRINT
- **US-CAS-5.** As a casual user, I want to fill in a simple form and print or save it, so that I can return it. → FR-FORM
- **US-CAS-6.** As a casual user, I want to combine a few PDFs into one, so that I can send a single file. → FR-MERGE
- **US-CAS-7.** As a casual user, I want to rotate and delete pages from a scan, so that the document is right-side-up and clean. → FR-ROTATE, FR-ORG
- **US-CAS-8.** As a casual user, I want to sign a document by drawing or placing a signature, so that I can return it without printing. → FR-SIG (visible signature), FR-ANNOT
- **US-CAS-9.** As a casual user, I want to search within a document for a word, so that I can find a section quickly. → FR-SRCH
- **US-CAS-10.** As a casual user, I want to save a smaller version of a large PDF, so that I can email it. *Acceptance:* optimize reduces size and discloses any quality trade-off. → FR-OPT, PRIN-2
- **US-CAS-11.** As a casual user, I want the app to remember my recent files, so that I can reopen them quickly. → UX-DISC
- **US-CAS-12.** As a casual user, I want clear, plain-language messages when something goes wrong, so that I know what to do. → UX-ERR, PRIN-6

## 7.2 Student

- **US-STU-1.** As a student, I want to highlight passages and add margin notes, so that I can study effectively. → FR-ANNOT
- **US-STU-2.** As a student, I want my highlights and notes to be visible when I open the file in another reader, so that my work is portable. → PRIN-7, FR-ANNOT
- **US-STU-3.** As a student, I want to copy a quotation and have the text come out correct (no broken characters), so that my citations are accurate. → FR-SRCH, FR-A11Y-EXPORT, PRIN-6
- **US-STU-4.** As a student, I want to turn a scanned reading into searchable text, so that I can find topics. → FR-OCR
- **US-STU-5.** As a student, I want to merge weekly readings into one file, so that my materials are organized. → FR-MERGE
- **US-STU-6.** As a student, I want to extract a range of pages, so that I can share only the relevant chapter. → FR-EXTRACT
- **US-STU-7.** As a student, I want to export all my annotations as a summary, so that I can review my notes separately. → FR-REV (comment summary)
- **US-STU-8.** As a student, I want to fill and submit a form, so that I can complete administrative tasks. → FR-FORM
- **US-STU-9.** As a student, I want to read comfortably at night with adjustable view settings, so that reading is easy on the eyes. *Acceptance:* view-only appearance adjustments MUST NOT modify the document. → UX-INT, PRIN-2
- **US-STU-10.** As a student, I want to bookmark pages, so that I can return to key sections. → FR-BOOK

## 7.3 Office worker

- **US-OFF-1.** As an office worker, I want to reorder, insert, and delete pages by dragging thumbnails, so that assembling documents is fast. → FR-ORG, FR-THUMB
- **US-OFF-2.** As an office worker, I want to add page numbers, headers, and footers across a document, so that it looks professional. → FR-STAMP
- **US-OFF-3.** As an office worker, I want to combine files of different types into one PDF, so that I can produce a single deliverable. → FR-MERGE, FR-IMPORT
- **US-OFF-4.** As an office worker, I want to password-protect a document before emailing it, and to understand what that protection does and does not do, so that I use it correctly. → FR-SEC, PRIN-6
- **US-OFF-5.** As an office worker, I want to make light edits to text and images without the layout breaking, so that corrections are safe. → FR-EDIT, PRIN-2
- **US-OFF-6.** As an office worker, I want to comment on a draft and send it back, so that feedback is clear. → FR-ANNOT, FR-REV
- **US-OFF-7.** As an office worker, I want to fill a form whose fields calculate totals automatically, so that I don't compute by hand. → FR-JS
- **US-OFF-8.** As an office worker, I want to split a large document into separate files by page ranges or bookmarks, so that I can distribute sections. → FR-SPLIT
- **US-OFF-9.** As an office worker, I want to flatten a completed form so that its values can't be changed, so that it's final. → FR-FORM (flatten)
- **US-OFF-10.** As an office worker, I want to remove hidden metadata before sharing, so that I don't leak internal information. → FR-META, FR-VER (sanitize)

## 7.4 HR professional

- **US-HR-1.** As an HR professional, I want to create fillable forms from existing documents, so that employees can complete them digitally. → FR-FORM (authoring)
- **US-HR-2.** As an HR professional, I want to redact personal data so that it is verifiably removed, so that I meet privacy obligations. → FR-RED, PRIN-6
- **US-HR-3.** As an HR professional, I want to collect signatures on offer letters and validate them, so that agreements are enforceable. → FR-SIG
- **US-HR-4.** As an HR professional, I want to produce policy documents that are accessible to all employees, so that we meet accessibility obligations. → FR-A11Y, FR-STD (PDF/UA)
- **US-HR-5.** As an HR professional, I want to batch-apply a watermark ("Confidential") to a set of documents, so that sensitive material is marked. → FR-STAMP, FR-BATCH
- **US-HR-6.** As an HR professional, I want to combine an employee's records into one organized file, so that files are complete. → FR-MERGE, FR-ORG
- **US-HR-7.** As an HR professional, I want to verify that a redacted file contains no recoverable personal data before I release it, so that I can certify compliance. → FR-RED (verification report)
- **US-HR-8.** As an HR professional, I want to set permissions that discourage editing while understanding these are advisory, so that I set correct expectations. → FR-PERM, PRIN-6

## 7.5 Accountant / finance

- **US-ACC-1.** As an accountant, I want tax and expense forms with embedded calculations to compute correctly, so that filled forms are accurate. → FR-JS
- **US-ACC-2.** As an accountant, I want to assemble numbered document packages, so that records are ordered and referenceable. → FR-STAMP (Bates), FR-MERGE
- **US-ACC-3.** As an accountant, I want to archive finished records to a durable standard, so that they remain readable for years. → FR-STD (PDF/A)
- **US-ACC-4.** As an accountant, I want to access structured data embedded in e-invoices, so that I can process them. → FR-EMB
- **US-ACC-5.** As an accountant, I want to combine statements and reconcile page order, so that reports are complete. → FR-MERGE, FR-ORG
- **US-ACC-6.** As an accountant, I want to protect financial documents with encryption, so that they are confidential. → FR-SEC
- **US-ACC-7.** As an accountant, I want to extract tables of figures as text/data, so that I can analyze them. → FR-SRCH (extraction), FR-EXPORT
- **US-ACC-8.** As an accountant, I want to validate that an archived file truly conforms to the archival standard, so that retention is defensible. → FR-STD (validation)

## 7.6 Lawyer / legal

- **US-LAW-1.** As a lawyer, I want to redact privileged content and receive proof that it is gone, so that I can produce documents safely. → FR-RED
- **US-LAW-2.** As a lawyer, I want to apply Bates numbering across thousands of pages consistently, so that discovery sets are referenceable. → FR-STAMP
- **US-LAW-3.** As a lawyer, I want to compare two versions of a contract and see exactly what changed, so that I can review efficiently. → FR-CMP
- **US-LAW-4.** As a lawyer, I want to remove all hidden history and metadata from a file before filing, and be told what remains, so that I don't leak information. → FR-VER (sanitize), FR-META, PRIN-6
- **US-LAW-5.** As a lawyer, I want to assemble exhibits into a paginated, bookmarked package, so that filings are navigable. → FR-MERGE, FR-BOOK, FR-STAMP
- **US-LAW-6.** As a lawyer, I want to apply and validate digital signatures, so that documents are authenticated. → FR-SIG
- **US-LAW-7.** As a lawyer, I want to confirm that a received document's signature is valid and understand any warnings in plain language, so that I can rely on it. → FR-SIG (explainable validation), PRIN-6
- **US-LAW-8.** As a lawyer, I want to search across a large case file for terms, so that I can find relevant passages. → FR-SRCH
- **US-LAW-9.** As a lawyer, I want assurance that redaction also removed the underlying text layer and metadata, not just the visible marks, so that content cannot be recovered. → FR-RED
- **US-LAW-10.** As a lawyer, I want to process a batch of files with the same redaction and numbering rules, so that large productions are efficient. → FR-BATCH

## 7.7 Engineer / architect

- **US-ENG-1.** As an engineer, I want to open and navigate a 2,000-page specification smoothly, so that large documents are usable. → NFR-LARGE, FR-NAV
- **US-ENG-2.** As an engineer, I want to toggle drawing layers on and off, so that I can focus on relevant information. → FR-LAYER
- **US-ENG-3.** As an engineer, I want to measure distances, perimeters, and areas at a defined scale, so that I can take off quantities. → FR-MEAS
- **US-ENG-4.** As an engineer, I want to add dimensioned markups and comments, so that I can coordinate changes. → FR-ANNOT, FR-MEAS
- **US-ENG-5.** As an architect, I want accurate rendering of lineweights and colors, so that drawings are readable and correct. → FR-VIEW, PRIN-1
- **US-ENG-6.** As an architect, I want to compare two revisions of a plan and see what moved, so that I can review changes. → FR-CMP
- **US-ENG-7.** As an engineer, I want to extract a sheet or range for a subcontractor, so that I share only what's needed. → FR-EXTRACT
- **US-ENG-8.** As an architect, I want to produce a correctly imposed, print-ready plan set, so that plots are usable. → FR-PRINT
- **US-ENG-9.** As an engineer, I want measurements to remain accurate after zoom and rotation, so that I can trust readings. → FR-MEAS, FR-NAV

## 7.8 Researcher / academic

- **US-RES-1.** As a researcher, I want to annotate papers and export my annotations, so that I can synthesize literature. → FR-ANNOT, FR-REV
- **US-RES-2.** As a researcher, I want correct text extraction for quotations and references, with a warning when a document's text is unreliable, so that citations are trustworthy. → FR-SRCH, PRIN-6
- **US-RES-3.** As a researcher, I want to search across my local library of papers without uploading them anywhere, so that my corpus stays private. → FR-SRCH-IDX, PRIN-3
- **US-RES-4.** As a researcher, I want to extract figures and tables, so that I can reuse or analyze them. → FR-EXTRACT, FR-EXPORT
- **US-RES-5.** As a researcher, I want to make a scanned historical document searchable, so that I can work with it. → FR-OCR
- **US-RES-6.** As a researcher, I want to produce an accessible, archivable version of my paper, so that it meets repository requirements. → FR-STD, FR-A11Y
- **US-RES-7.** As a researcher, I want my annotations to be portable to my reference manager or other readers, so that my work isn't locked in. → PRIN-7

## 7.9 Publisher / print shop

- **US-PUB-1.** As a production editor, I want to aggregate comments from multiple reviewers into one view, so that I can reconcile feedback efficiently. → FR-REV
- **US-PUB-2.** As a production editor, I want to preflight a file for print or archive and get a clear report, so that I catch problems early. → FR-STD (preflight)
- **US-PUB-3.** As a prepress operator, I want accurate rendering of overprint, spot colors, and transparency, so that proofs match output. → FR-VIEW (prepress, later phase), FR-STD
- **US-PUB-4.** As a prepress operator, I want to impose pages (n-up, booklet) for production, so that printing is efficient. → FR-PRINT
- **US-PUB-5.** As a production editor, I want to verify that all fonts are embedded, so that the file renders correctly everywhere. → FR-STD, FR-VIEW
- **US-PUB-6.** As a prepress operator, I want to export to a print-oriented standard (PDF/X) with correct color intent, so that the file is production-ready. → FR-STD (PDF/X, later phase)
- **US-PUB-7.** As a production editor, I want to flatten transparency predictably for legacy workflows, so that output is consistent. → FR-PRINT, FR-STD

## 7.10 Government / public sector

- **US-GOV-1.** As a public-sector author, I want to remediate a document to meet accessibility standards and validate conformance, so that public documents are lawful. → FR-A11Y, FR-STD (PDF/UA)
- **US-GOV-2.** As a records officer, I want to convert documents to a validated archival standard, so that retention obligations are met. → FR-STD (PDF/A)
- **US-GOV-3.** As a disclosure officer, I want to redact under public-records law with verifiable removal, so that releases are safe. → FR-RED
- **US-GOV-4.** As a public servant, I want to sign documents with a qualified certificate on a hardware token, so that signatures are legally recognized. → FR-SIG (hardware/PAdES-LTA)
- **US-GOV-5.** As a public servant in a secure environment, I want the application to work fully offline with no external network activity, so that it meets security policy. → VIS-3, VIS-2
- **US-GOV-6.** As an agency administrator, I want to deploy and configure the Platform across many machines under central policy, so that it is manageable at scale. → §12
- **US-GOV-7.** As an agency administrator, I want a long-term-support version that receives security fixes without forcing feature or interface change, so that certified configurations remain stable. → §12, §16, PRIN-4

## 7.11 Enterprise IT administrator

- **US-ITA-1.** As an administrator, I want to deploy the Platform using standard management tooling on each platform, so that rollout is routine. → §12
- **US-ITA-2.** As an administrator, I want to enforce configuration and disable specific features via policy, so that the deployment meets our requirements. → §12
- **US-ITA-3.** As an administrator, I want to control whether and when updates are applied, so that updates don't break workflows or plugins. → §12, PRIN-4
- **US-ITA-4.** As an administrator, I want to verify that the application makes no network connections by default, so that I can certify it for restricted environments. → VIS-2, §12
- **US-ITA-5.** As an administrator, I want to offboard a departing employee without punitive licensing steps, so that administration is simple. → §12 (no per-seat lock-in in core)
- **US-ITA-6.** As an administrator, I want an auditable record of security-relevant configuration, so that I can demonstrate compliance. → §12 (auditability)
- **US-ITA-7.** As an administrator, I want to pre-configure trusted certificates for signature validation, so that users get correct trust decisions. → §12 (certificate management), FR-SIG

## 7.12 Developer / automation engineer

- **US-DEV-1.** As a developer, I want to run merge, split, OCR, optimize, redact, sign, and validate from a command line, so that I can automate document processing. → FR-CLI
- **US-DEV-2.** As a developer, I want the command line to run headless on a server with no display, so that it fits CI and backend pipelines. → FR-CLI, NFR-OFFLINE
- **US-DEV-3.** As a developer, I want machine-readable output and predictable exit codes, so that I can script reliably. → FR-CLI
- **US-DEV-4.** As a developer, I want the command line to produce results identical to the GUI, so that behavior is consistent. → FR-CLI, PRIN-1
- **US-DEV-5.** As a developer, I want to compose multi-step operations into a pipeline, so that complex jobs are repeatable. → FR-CLI, FR-BATCH
- **US-DEV-6.** As a developer, I want to inspect a document's internal structure for debugging, so that I can diagnose issues. → FR-CLI (inspect), FR-DIAG
- **US-DEV-7.** As a developer, I want to validate documents against standards in a pipeline, so that I can enforce quality gates. → FR-STD, FR-CLI

## 7.13 Plugin author

- **US-PLG-1.** As a plugin author, I want a documented, stable extension interface, so that my plugin keeps working across releases. → FR-PLUG, PRIN-9
- **US-PLG-2.** As a plugin author, I want to declare exactly what capabilities my plugin needs, so that users can trust it. → FR-PLUG (capabilities)
- **US-PLG-3.** As a plugin author, I want my plugin to run without being able to crash or hang the application, so that users are protected. → FR-PLUG (isolation), PRIN-9
- **US-PLG-4.** As a plugin author, I want to add tools, panels, and batch operations, so that I can extend real workflows. → FR-PLUG
- **US-PLG-5.** As a plugin author, I want a compatibility test kit, so that I can verify my plugin before release. → FR-PLUG
- **US-PLG-6.** As a plugin author, I want to write my plugin in more than one language, so that I can use my existing skills. → FR-PLUG (`[ADR-015]`)
- **US-PLG-7.** As a plugin author, I want clear notice when an interface I depend on is deprecated, so that I can update in time. → FR-PLUG, §12/§16 (versioning)

## 7.14 Accessibility user

- **US-AXS-1.** As a screen-reader user, I want to operate every application function via keyboard, so that I can work independently. → UX-A11Y, NFR-A11Y
- **US-AXS-2.** As a screen-reader user, I want document structure (headings, lists, tables) announced correctly for tagged documents, so that I can read efficiently. → FR-A11Y
- **US-AXS-3.** As a screen-reader user, I want to fill forms accessibly with correct field labels and order, so that I can complete tasks. → FR-A11Y, FR-FORM
- **US-AXS-4.** As a low-vision user, I want high-DPI-correct rendering and adjustable view settings that don't alter the document, so that I can read comfortably. → UX-DPI, PRIN-2
- **US-AXS-5.** As an assistive-technology user, I want focus order and announcements to remain consistent across releases, so that my learned workflow persists. → PRIN-4, UX-A11Y
- **US-AXS-6.** As a document author, I want to check and fix a document's reading order and tags, so that others can read it accessibly. → FR-A11Y (remediation, later phase)

## 7.15 Cross-cutting reliability and trust stories

- **US-TRUST-1.** As any user, I want the application to recover my unsaved work after a crash or power loss, so that I don't lose effort. *Acceptance:* no more than the durability budget of committed work is lost (§14, `[SDS §10]`). → FR-REC, FR-AUTOSAVE
- **US-TRUST-2.** As any user, I want a damaged file to still open with a clear explanation of what was repaired, so that I can keep working. → FR-VIEW (repair), FR-DIAG, PRIN-6
- **US-TRUST-3.** As any user, I want a hostile or malformed document to be unable to harm my system, so that opening files is safe. → NFR-SEC, §9 security
- **US-TRUST-4.** As any user, I want to undo any change, including after reopening, so that mistakes are reversible. → FR-UNDO
- **US-TRUST-5.** As any user, I want saving never to corrupt my document or break an existing signature, so that my files stay valid. → FR-SAVE, PRIN-2
- **US-TRUST-6.** As any user, I want to see and, if I choose, remove the document's revision history, so that I control what I share. → FR-VER, FR-META
- **US-TRUST-7.** As any user, I want the interface, shortcuts, and workflows I rely on to remain stable across updates unless I opt into changes, so that updates don't disrupt me. → PRIN-4, §11, §12
- **US-TRUST-8.** As any user, I want to confirm that the software I run matches its published source, so that I can trust it. → PRIN-3, §12 (verifiable builds)

*Informative note:* This library is representative, not exhaustive. New stories MUST trace to existing or newly added normative requirements; a story that cannot be traced indicates either a missing requirement (to be added) or an out-of-scope request (to be recorded in §8).

---

# 8. Product Scope

*Normative.* This section defines what the Platform will, will not, and may later do. Scope statements bind the roadmap (§16) and are the authority for accepting or rejecting feature requests. Scope is stated at product-capability granularity; detailed behavior is in §9.

## 8.1 In scope (core, open source)

The following capabilities are in scope for the open-source core and MUST be delivered according to the roadmap (§16). Each maps to functional requirements in §9.

**Viewing and navigation.** High-fidelity rendering of conformant and malformed PDFs; navigation (scroll, zoom, rotate, page/view history, layouts); bookmarks/outline; thumbnails; optional-content layers (view/toggle); attachments and embedded files (access); in-document search; text selection and extraction.

**Creation and assembly.** Merge, split, extract, insert, delete, reorder, rotate, crop pages; headers/footers/page numbers; watermarks; Bates numbering; image-to-PDF and basic import; optimization/compression.

**Editing.** Non-destructive editing of annotations and form values (early); image and object editing (mid); text editing with layout-preserving discipline (later). All editing MUST obey PRIN-2.

**Annotation and review.** The standard annotation set with portable appearances; comments and threaded replies; review status; comment aggregation; import/export of annotation data for interoperability; measurement tools.

**Forms.** AcroForm filling with correct appearances; the PDF-JavaScript forms subset for validation/calculation/formatting (§9.9); form authoring (later); flatten.

**Security.** Open and create standard encryption; password protection; permissions (honored as advisory, disclosed as such); verifiable redaction; metadata and history sanitization.

**Signatures.** Digital signature validation with explainable results; signing with software certificates; timestamping; long-term validation data; hardware-token/qualified signing (later). Recognized profiles (e.g., PAdES) MUST be supported for validation and creation per roadmap.

**Recognition.** OCR to produce searchable, correctly-registered text under scanned images; scanning acquisition (later).

**Standards.** PDF/A validation and export; PDF/UA validation and remediation tooling (later); PDF/X for prepress (later). Validation MUST be conformant and reportable.

**Accessibility.** Full application accessibility; accessible reading of tagged documents; document remediation tooling (later).

**Output.** Printing (basic early; imposition/prepress later); export to images, text, and HTML (early); export to office formats (later, quality-gated).

**Automation and extension.** A complete command-line interface with GUI parity for scriptable operations; batch processing and pipelines; a sandboxed, capability-scoped plugin ecosystem with a stable, versioned contract.

**Reliability and data control.** Unlimited, persistent undo; autosave via change journaling; crash and torn-save recovery; version history with sanitization control.

**Comparison and analysis.** Document comparison (visual and textual); document inspection/diagnostics.

## 8.2 Out of scope (Normative exclusions)

The following are explicitly out of scope for the foreseeable product horizon. Exclusion MAY be revisited only by an approved change to this PRD with recorded rationale.

- **OUT-1. Full XFA authoring and general XFA rendering.** XFA is deprecated in the current PDF standard and its full implementation would compromise quality and security. The Platform MUST detect XFA content and inform the user honestly (PRIN-6) rather than render it incorrectly. *(Limited handling of XFA-bearing files as ordinary PDFs where an AcroForm fallback exists MAY be supported.)* → §8.4 future may reconsider partial support only.
- **OUT-2. A cloud service as a dependency.** The core MUST NOT require any hosted service (VIS-3). Optional self-hostable services are future scope (§8.3), never core dependencies.
- **OUT-3. Mandatory accounts, identity, or subscription in the core.** (VIS-1.)
- **OUT-4. Default telemetry or document transmission.** (VIS-2.)
- **OUT-5. A general office suite** (word processor, spreadsheet, presentation authoring). The Platform imports from and exports to such formats within quality limits (§9.27) but is not a replacement for them.
- **OUT-6. Mobile-first or mobile-primary clients.** The product is desktop-first. Mobile clients, if ever pursued, are beyond this PRD's horizon and would require a separate specification.
- **OUT-7. Digital-rights-management enforcement beyond standard PDF permissions.** The Platform MUST NOT implement proprietary DRM or represent PDF permissions as stronger than they are (PRIN-6).
- **OUT-8. Cryptographic invention.** The Platform MUST use recognized, standard cryptography and profiles only; it MUST NOT devise novel cryptographic schemes.
- **OUT-9. Rendering or execution of active content beyond the specified JavaScript forms subset.** Document-level scripting for automation, file access, or UI control is out of scope and MUST NOT execute (§9.9, `[ADR-017]`).
- **OUT-10. Silent, cloud-based, or non-consensual AI processing of user documents.** Any assistive/AI feature, if ever added, MUST be optional, disclosed, local or explicitly consented, and off by default; it MUST NOT be a core dependency. *(No AI capability is committed by this PRD.)*

## 8.3 Future scope (may be pursued; not committed by this PRD)

The following are candidate future capabilities. They are neither promised nor scheduled here; each requires its own specification and MUST preserve the vision commitments (§2.4) if pursued.

- **FUT-1. Self-hostable collaboration/review service** for synced comments and shared review, always optional and user/organization-controlled.
- **FUT-2. Advanced accessibility auto-tagging** using local inference to accelerate remediation.
- **FUT-3. Prepress color management** to full PDF/X production fidelity.
- **FUT-4. Office-format export** at fidelity competitive with dedicated converters.
- **FUT-5. Optional, local, consented assistive features** (e.g., summarization or extraction aids) meeting OUT-10's constraints.
- **FUT-6. Signed plugin registry** and curated distribution.
- **FUT-7. Additional platform integrations** (e.g., OS-level document services) where they respect privacy and offline principles.

## 8.4 Scope governance (Normative)

**SCOPE-1.** A feature request MUST be classified as in-scope (§8.1), out-of-scope (§8.2), or future (§8.3) before implementation. **SCOPE-2.** Moving an item from out-of-scope or future into scope REQUIRES an approved PRD amendment recording the rationale and the vision-commitment analysis. **SCOPE-3.** No release MAY ship a capability that violates §8.2.

---

# 9. Functional Requirements

*Normative.* Requirements specify observable product behavior. Implementation is governed by the ADR/SDS and is out of scope here. Each subsystem lists requirements with identifiers; acceptance criteria are stated or reference §14 metrics. Roadmap phase is indicated as *(Phase: Mn)* referencing §16 where useful; phase indications are directional, not conformance conditions, except where a requirement is marked as applying "when delivered."

## 9.1 PDF viewing (FR-VIEW)

- **FR-VIEW-1.** The Platform MUST render PDF pages with fidelity sufficient that a professional user cannot distinguish its output from the reference rendering of the same document on a representative corpus, except for documented, tracked deviations. *Acceptance:* corpus differential rendering meets the fidelity target (§14).
- **FR-VIEW-2.** The Platform MUST open and render documents that violate the specification in common ways (broken cross-reference data, minor structural errors, recoverable corruption) whenever a reasonable rendering is possible, and MUST record and disclose what was repaired (PRIN-6, FR-DIAG).
- **FR-VIEW-3.** The Platform MUST correctly render text (including embedded, subset, and composite fonts), vector graphics, raster images, transparency, blend modes, and standard color spaces. Prepress-accurate rendering (overprint, spot color, transparency flattening preview) MUST be supported when the prepress capability is delivered.
- **FR-VIEW-4.** When a font is not embedded, the Platform MUST substitute using metrically appropriate fallbacks and MUST make substitution discoverable in diagnostics; it MUST NOT silently present substitution as original where the distinction is material (PRIN-6).
- **FR-VIEW-5.** The Platform MUST render at the display's native resolution (high-DPI correct) and MUST keep text and vector art crisp at all supported zoom levels (UX-DPI).
- **FR-VIEW-6.** View-only appearance adjustments (e.g., day/night viewing, background tint) MUST NOT modify the document (PRIN-2).
- **FR-VIEW-7.** The Platform MUST indicate when a document contains content it does not render (e.g., XFA, unsupported constructs) rather than presenting an incomplete page as complete (PRIN-6, OUT-1).

## 9.2 Navigation (FR-NAV)

- **FR-NAV-1.** The Platform MUST support single-page, continuous, and facing/spread layouts, and MUST support page rotation of the view without modifying the document.
- **FR-NAV-2.** The Platform MUST support zoom by fixed levels, fit-width, fit-page, fit-visible, and arbitrary zoom, with smooth interactive zooming (NFR-PERF).
- **FR-NAV-3.** The Platform MUST maintain a view history enabling "previous view"/"next view" navigation, and this operation MUST require no more than one action to invoke (a specific, measured improvement over incumbent regressions; §11). → UX-INT
- **FR-NAV-4.** The Platform MUST support go-to-page, first/last page, and navigation via bookmarks, thumbnails, links, and named destinations.
- **FR-NAV-5.** The Platform MUST preserve scroll position and zoom appropriately when the window is resized or the layout changes.
- **FR-NAV-6.** Navigation actions MUST remain responsive on large documents (NFR-LARGE).

## 9.3 Search (FR-SRCH)

- **FR-SRCH-1.** The Platform MUST provide in-document search returning the first result quickly (§14 latency) and all results progressively.
- **FR-SRCH-2.** Search MUST support case-insensitive and diacritic-insensitive matching by default, with options for case-sensitive and whole-word matching, and MUST correctly handle ligatures, soft hyphens, and common text-extraction irregularities.
- **FR-SRCH-3.** Search results MUST be navigable and highlighted in place, with highlights crisp at any zoom (independent of page raster).
- **FR-SRCH-4.** The Platform MUST support search across the visible document and, where present, MUST search bookmarks and comments as selectable scopes.
- **FR-SRCH-5.** Text extraction underlying search, selection, and copy MUST produce correct Unicode where the document permits, and MUST flag pages whose extraction is unreliable rather than returning silently incorrect text (PRIN-6).
- **FR-SRCH-IDX-1.** The Platform MUST provide an optional, local, user-enrolled cross-document search index over user-designated folders. It MUST NOT index content without explicit enrollment, MUST keep the index local, MUST disclose and bound its storage use, and MUST allow inspection and deletion of the index (PRIN-3).
- **FR-SRCH-6.** *(MAY)* The Platform MAY offer fuzzy or proximity search as an enhancement, provided default behavior remains exact and predictable.

## 9.4 Bookmarks / outline (FR-BOOK)

- **FR-BOOK-1.** The Platform MUST display a document's outline and navigate to destinations on activation.
- **FR-BOOK-2.** The Platform MUST support creating, renaming, reordering, nesting, and deleting bookmarks, and setting their destinations, as undoable operations (FR-UNDO).
- **FR-BOOK-3.** The Platform MUST preserve existing outline actions it does not modify (PRIN-2).

## 9.5 Thumbnails (FR-THUMB)

- **FR-THUMB-1.** The Platform MUST present page thumbnails for navigation and page organization, generated without blocking interaction (NFR-PERF).
- **FR-THUMB-2.** Thumbnails MUST support drag-based reordering and selection for page operations (FR-ORG), with clear visual feedback.
- **FR-THUMB-3.** Thumbnails MUST update to reflect document changes (e.g., rotation, edits) promptly.

## 9.6 Layers / optional content (FR-LAYER)

- **FR-LAYER-1.** The Platform MUST display optional-content groups (layers) and allow toggling their visibility.
- **FR-LAYER-2.** The Platform MUST persist layer visibility state changes to the document when the user chooses to save them, as an undoable, non-destructive operation (PRIN-2).
- **FR-LAYER-3.** The Platform MUST correctly render layer-dependent content and honor default and locked states.

## 9.7 Attachments and embedded files (FR-EMB)

- **FR-EMB-1.** The Platform MUST list embedded files and allow the user to open or extract them via brokered, consented actions (NFR-SEC).
- **FR-EMB-2.** The Platform MUST support adding and removing embedded files as undoable operations, preserving other document content (PRIN-2).
- **FR-EMB-3.** The Platform MUST make structured embedded data (e.g., e-invoice XML) accessible for extraction (US-ACC-4).
- **FR-EMB-4.** The Platform MUST treat embedded files as untrusted and MUST NOT execute them; opening MUST route through the host's safe-handling and consent path (NFR-SEC, OUT-9).

## 9.8 Forms — AcroForm (FR-FORM)

- **FR-FORM-1.** The Platform MUST allow filling AcroForm fields (text, checkbox, radio, list, combo, button, signature) and MUST regenerate correct field appearances so filled values render correctly in other conformant readers (PRIN-7).
- **FR-FORM-2.** The Platform MUST support field navigation (tab order), required-field indication, and validation feedback.
- **FR-FORM-3.** The Platform MUST support importing and exporting form data in interoperable formats for round-tripping with other tools.
- **FR-FORM-4.** The Platform MUST support flattening a form (rendering values as page content) as an explicit, undoable operation, disclosing that flattening removes interactivity (PRIN-6).
- **FR-FORM-5.** *(When delivered)* The Platform MUST support authoring form fields (creating, positioning, configuring, and ordering fields).
- **FR-FORM-6.** Form filling and authoring MUST be undoable and MUST preserve untouched document content (PRIN-2).
- **FR-FORM-7.** Forms MUST be fillable accessibly (FR-A11Y, US-AXS-3).

## 9.9 PDF JavaScript (forms subset) (FR-JS)

- **FR-JS-1.** The Platform MUST execute the document-JavaScript **forms subset** necessary for correct field validation, calculation, and formatting (including calculation ordering), so that common enterprise forms compute correct results.
- **FR-JS-2.** The Platform MUST NOT execute document JavaScript outside the forms subset; automation, file, network, and UI-control scripting MUST NOT run (OUT-9, `[ADR-017]`).
- **FR-JS-3.** When a document requests unsupported script behavior, the Platform MUST NOT emulate a false result; it MUST skip the unsupported behavior and record it in diagnostics (PRIN-6, FR-DIAG).
- **FR-JS-4.** The Platform MUST provide a per-document indicator when document JavaScript is present and MUST provide user and administrator controls to disable document JavaScript entirely (§12, UX-ERR).
- **FR-JS-5.** Script-initiated field changes MUST be treated as ordinary undoable changes (FR-UNDO) and MUST NOT bypass the document-integrity rules (PRIN-2).
- **FR-JS-6.** The Platform MUST maintain and publish a compatibility statement describing which scripted behaviors are supported (PRIN-6).

## 9.10 Annotations (FR-ANNOT)

- **FR-ANNOT-1.** The Platform MUST support the standard annotation types, including text markup (highlight, underline, strikeout, squiggly), notes, free text, ink/drawing, shapes (line, arrow, rectangle, ellipse, polygon, polyline), stamps, and callouts.
- **FR-ANNOT-2.** Every annotation the Platform writes MUST include a complete, portable visual appearance so that it renders consistently in other conformant readers (PRIN-7). The Platform MUST NOT write appearance-less annotations.
- **FR-ANNOT-3.** Text-markup annotations MUST attach to the correct text region so that they track the underlying content.
- **FR-ANNOT-4.** Annotations MUST be creatable, editable (position, appearance, properties, author, timestamp), and deletable as undoable operations (FR-UNDO).
- **FR-ANNOT-5.** The Platform MUST support annotation properties including color, opacity, line style, and author identity, and MUST support setting default properties per tool.
- **FR-ANNOT-6.** When reading a document, the Platform MUST prefer an annotation's embedded appearance over synthesizing its own, to match author intent and cross-tool rendering.
- **FR-ANNOT-7.** Ink and freehand annotation MUST feel responsive to input, including pen/stylus with pressure where available (UX-PEN).

## 9.11 Comments and review (FR-REV)

- **FR-REV-1.** The Platform MUST support comments (notes attached to annotations or the document), threaded replies, author identity, timestamps, and review status (e.g., accepted, rejected, completed).
- **FR-REV-2.** The Platform MUST provide a comment list/summary view supporting filtering (by author, type, status, page) and navigation to the commented location.
- **FR-REV-3.** The Platform MUST support exporting a comment summary as a separate document or data file (US-STU-7, US-RES-1).
- **FR-REV-4.** The Platform MUST support importing and exporting comments/annotations in an interoperable format so that reviews round-trip with other tools (PRIN-7).
- **FR-REV-5.** The Platform MUST support aggregating comments from multiple copies of the same document into a single reconciled view (US-PUB-1). *(Phase: later.)*
- **FR-REV-6.** *(Future, FUT-1)* Synced/shared review via an optional self-hostable service MAY be provided; it MUST remain optional and MUST NOT become a core dependency (VIS-3).

## 9.12 Measurement (FR-MEAS)

- **FR-MEAS-1.** The Platform MUST provide distance, perimeter, and area measurement tools operating at a user-defined scale and unit.
- **FR-MEAS-2.** Measurements MUST remain accurate under zoom and rotation and MUST display values with configurable precision.
- **FR-MEAS-3.** Measurement markups MUST be recordable as annotations with their computed values (FR-ANNOT).
- **FR-MEAS-4.** The Platform MUST read a document's defined measurement scale where present and allow the user to set or override scale where absent.

## 9.13 Page organization (FR-ORG, FR-SPLIT, FR-MERGE, FR-EXTRACT, FR-INSERT, FR-ROTATE, FR-CROP)

- **FR-ORG-1.** The Platform MUST support reordering, inserting, deleting, and duplicating pages, via thumbnails and via commands, as undoable operations preserving untouched content (PRIN-2).
- **FR-MERGE-1.** The Platform MUST merge multiple documents into one, correctly preserving content, and MUST avoid unnecessary duplication of shared resources so that merged files are not disproportionately large. Bookmarks and named destinations MUST be reconciled sensibly.
- **FR-SPLIT-1.** The Platform MUST split a document by page ranges, by page count, by file size target, and by top-level bookmarks, producing valid output files.
- **FR-EXTRACT-1.** The Platform MUST extract a page or range into a new document, optionally removing the extracted pages from the source (as an explicit choice).
- **FR-INSERT-1.** The Platform MUST insert pages from another document or from images at a chosen position.
- **FR-ROTATE-1.** The Platform MUST rotate selected pages in 90-degree increments as an undoable change to the document (distinct from view rotation, FR-NAV-1).
- **FR-CROP-1.** The Platform MUST crop pages to a defined box, MUST allow the crop to be applied to selected pages or all pages, and MUST make clear whether cropping hides or removes content. *(By default, cropping MUST be reversible/non-destructive of underlying content unless the user explicitly chooses to remove it; PRIN-2, PRIN-6.)*
- **FR-ORG-2.** All page operations MUST be available in batch and via the CLI (FR-CLI, FR-BATCH) with identical results (PRIN-1).

## 9.14 Optimization and compression (FR-OPT)

- **FR-OPT-1.** The Platform MUST reduce document size via stream re-compression, image downsampling/re-encoding, font subsetting, and removal of redundant or unreferenced objects.
- **FR-OPT-2.** Optimization MUST present the expected size reduction and MUST disclose any quality trade-offs (e.g., image downsampling) before applying (PRIN-6).
- **FR-OPT-3.** Optimization MUST NOT silently remove tags, signatures, or metadata; removing any such content MUST be an explicit, disclosed choice (PRIN-2, PRIN-6). *(Optimizing a signed document in a way that would break its signature MUST be disclosed and confirmed.)*
- **FR-OPT-4.** The Platform MUST offer preset optimization profiles (e.g., screen, print, archive-preserving) and a custom profile.
- **FR-OPT-5.** Optimization MUST be available in batch and via the CLI (PRIN-1).

## 9.15 Redaction (FR-RED)

- **FR-RED-1.** The Platform MUST perform redaction that removes the underlying content — text, vector, and image data — within the redacted region, not merely obscure it visually.
- **FR-RED-2.** Redaction MUST also remove associated recoverable data: the text underlying redacted marks, covered annotations, and relevant metadata and hidden content, as applicable.
- **FR-RED-3.** The Platform MUST provide a verification capability that confirms, after redaction, that the targeted content is not recoverable from the saved output, and MUST be able to produce a report of this verification (US-LAW-1, US-HR-7).
- **FR-RED-4.** The Platform MUST NOT allow a redaction to be considered final/saved as removed until the removal is complete; a purely cosmetic redaction path MUST NOT exist (PRIN-1, PRIN-6).
- **FR-RED-5.** Redaction MUST support marking regions and text search-based marking (redact all occurrences of a term), applied across selected pages or the whole document.
- **FR-RED-6.** Redaction MUST be available in batch and via the CLI with the same guarantees (US-LAW-10, PRIN-1).

## 9.16 Signatures and certificates (FR-SIG)

- **FR-SIG-1.** The Platform MUST validate digital signatures and present results that are explainable in plain language, distinguishing valid, invalid, and indeterminate outcomes. It MUST NOT present an indeterminate or unverifiable signature as valid (PRIN-6).
- **FR-SIG-2.** Signature validation MUST assess document integrity over the signed byte range and MUST detect changes made after signing, reporting whether such changes were permitted by the signature's constraints.
- **FR-SIG-3.** The Platform MUST support creating signatures using software certificates, and *(when delivered)* using hardware tokens/smart cards, and MUST support recognized signature profiles (e.g., PAdES levels) per roadmap, including timestamping and long-term validation data (US-GOV-4).
- **FR-SIG-4.** Signing MUST preserve the previously signed content and MUST apply new content as a permitted incremental change so that prior signatures remain valid where the change is allowed (PRIN-2, `[ADR-012]`).
- **FR-SIG-5.** The Platform MUST support visible and invisible signatures and MUST allow certification signatures that declare permitted subsequent changes.
- **FR-SIG-6.** The Platform MUST manage trusted certificates and MUST allow administrators to pre-configure trust (§12). Trust decisions MUST be based on configured trust, not on unverifiable assumptions.
- **FR-SIG-7.** Signature creation and validation MUST be available via the CLI for automation (US-DEV-1), with validation producing machine-readable results.

## 9.17 OCR and scanning (FR-OCR, FR-SCAN)

- **FR-OCR-1.** The Platform MUST recognize text in scanned/image pages and add a correctly registered, invisible text layer so that the page becomes searchable and selectable without altering its visual appearance.
- **FR-OCR-2.** OCR MUST support multiple languages and MUST allow the user to select the language(s) for recognition.
- **FR-OCR-3.** OCR MUST include image preprocessing (e.g., deskew, despeckle) to improve accuracy and MUST allow OCR of pages that already contain some text where the user requests it (addressing the incumbent limitation of refusing pages with any renderable text).
- **FR-OCR-4.** OCR MUST report low-confidence results rather than silently inserting likely-wrong text where confidence is poor (PRIN-6).
- **FR-OCR-5.** OCR MUST be available in batch and via the CLI (US-DEV-1, PRIN-1), and MUST support producing archival-standard output where requested (FR-STD).
- **FR-SCAN-1.** *(When delivered)* The Platform MUST support acquiring pages from a scanner via each platform's standard acquisition mechanism, producing appropriately compressed image pages, optionally followed by OCR.

## 9.18 Accessibility of documents — tagging, reading order (FR-A11Y)

- **FR-A11Y-1.** The Platform MUST expose a tagged document's logical structure and reading order to assistive technology so that tagged documents can be read accessibly (US-AXS-2).
- **FR-A11Y-2.** The Platform MUST provide accessible form filling with correct field labels, descriptions, and order (US-AXS-3, FR-FORM-7).
- **FR-A11Y-3.** The Platform MUST provide accessible text export/extraction that yields correct reading order for tagged documents.
- **FR-A11Y-4.** *(When delivered)* The Platform MUST provide tools to inspect, create, and correct document tags, reading order, alternative text, table structure, and other accessibility properties, in a non-destructive, undoable manner (PRIN-2), to enable remediation to recognized accessibility standards (FR-STD, PDF/UA).
- **FR-A11Y-5.** *(When delivered)* The Platform MUST support validating a document against the accessibility standard and reporting issues with locations and remediation guidance (FR-STD).

## 9.19 Standards — PDF/A, PDF/X, PDF/UA (FR-STD)

- **FR-STD-1.** The Platform MUST validate documents against supported conformance standards and MUST produce a clear, itemized, navigable report of conformance and violations.
- **FR-STD-2.** The Platform MUST convert/export documents to PDF/A (archival) with declared conformance level, embedding required resources and removing prohibited content, and MUST validate the result.
- **FR-STD-3.** *(When delivered)* The Platform MUST support PDF/X (prepress) export with correct output intent and color handling.
- **FR-STD-4.** *(When delivered)* The Platform MUST support PDF/UA (accessibility) validation and, with remediation tooling (FR-A11Y-4), production of conformant documents.
- **FR-STD-5.** Conformance claims the Platform writes MUST be accurate; the Platform MUST NOT declare a conformance level it does not meet (PRIN-6).
- **FR-STD-6.** Standards validation MUST be available via the CLI for pipeline gating (US-DEV-7).

## 9.20 Printing (FR-PRINT)

- **FR-PRINT-1.** The Platform MUST print documents using each platform's native print system and dialog, honoring paper size, orientation, duplex, and printer selection.
- **FR-PRINT-2.** The Platform MUST support scaling options (fit, actual size, custom), page-range selection, and printing of comments/annotations as options.
- **FR-PRINT-3.** Printed output MUST match the on-screen rendering within the fidelity target (PRIN-1), including correct handling of transparency for the target device.
- **FR-PRINT-4.** *(When delivered)* The Platform MUST support imposition (n-up, booklet) and prepress-oriented options (e.g., marks, overprint handling, transparency flattening) for production printing.
- **FR-PRINT-5.** Printing MUST be operable accessibly and via keyboard (UX-A11Y).

## 9.21 Batch processing (FR-BATCH)

- **FR-BATCH-1.** The Platform MUST allow applying operations (e.g., OCR, optimize, redact-by-term, stamp, convert, secure, validate) to multiple documents in one job.
- **FR-BATCH-2.** The Platform MUST allow composing multiple operations into a repeatable multi-step pipeline, savable and re-runnable.
- **FR-BATCH-3.** Batch jobs MUST report progress, allow cancellation, continue past per-file errors with a clear per-file result, and produce a summary report.
- **FR-BATCH-4.** Batch jobs MUST survive application restart where feasible (resume), consistent with reliability requirements (FR-REC).
- **FR-BATCH-5.** Every batch operation MUST be available via the CLI with identical behavior (PRIN-1).

## 9.22 Command-line interface (FR-CLI)

- **FR-CLI-1.** The Platform MUST provide a command-line interface that performs the scriptable operations of the product (at minimum: convert, merge, split, extract, rotate, crop, stamp, optimize, OCR, redact-by-term, encrypt/decrypt, sign, validate signatures, validate standards, inspect) using the same core as the GUI, producing identical results (PRIN-1).
- **FR-CLI-2.** The CLI MUST run headless without a display (US-DEV-2) and MUST be suitable for servers and CI.
- **FR-CLI-3.** The CLI MUST provide machine-readable output modes and predictable, documented exit codes (US-DEV-3).
- **FR-CLI-4.** The CLI MUST support pipelines/batch (FR-BATCH) and MUST support document inspection/diagnostics output (FR-DIAG).
- **FR-CLI-5.** The CLI MUST NOT require network access for any operation that the GUI can perform offline (VIS-3).

## 9.23 Plugin ecosystem (FR-PLUG)

- **FR-PLUG-1.** The Platform MUST provide an extension mechanism allowing third parties to add capabilities (tools, panels, batch operations, and format or engine backends where applicable) without modifying the core.
- **FR-PLUG-2.** Extensions MUST declare the capabilities they require; the user (or administrator) MUST grant capabilities explicitly; extensions MUST be unable to exceed granted capabilities (PRIN-9, `[ADR-014]`).
- **FR-PLUG-3.** Extensions MUST run in isolation such that a faulty or malicious extension cannot crash, hang, corrupt, or exfiltrate data from the host or other documents (PRIN-9). An extension's failure MUST be contained and reported.
- **FR-PLUG-4.** Extensions MUST interact with documents only through the same integrity-preserving operations as the core (all changes undoable and attributable; PRIN-2).
- **FR-PLUG-5.** The extension contract MUST be documented and versioned with a stability guarantee and a deprecation policy (US-PLG-1, US-PLG-7, §16).
- **FR-PLUG-6.** The Platform MUST provide a compatibility test kit enabling authors to verify an extension against a target contract version (US-PLG-5).
- **FR-PLUG-7.** Extensions MUST be authorable in more than one programming language via the published contract (US-PLG-6, `[ADR-015]`).
- **FR-PLUG-8.** *(Future, FUT-6)* A curated, signed registry MAY be provided; if provided, integrity and provenance of packages MUST be verifiable.

## 9.24 Document comparison (FR-CMP)

- **FR-CMP-1.** The Platform MUST compare two documents (or two versions) and identify differences in text content and in visual/page appearance.
- **FR-CMP-2.** Comparison MUST present differences navigably, indicating locations and the nature of each change (added, removed, changed, moved where detectable).
- **FR-CMP-3.** Textual comparison MUST be resilient to reflow and pagination changes to the extent feasible, prioritizing meaningful change detection over raw positional diff (US-LAW-3).
- **FR-CMP-4.** Comparison MUST be available via the CLI for automated review gating.

## 9.25 Portfolio / collection support (FR-PORT)

- **FR-PORT-1.** The Platform MUST open PDF collections/portfolios (documents that package multiple files), list their contents, and allow opening or extracting constituent files via consented, brokered actions (NFR-SEC).
- **FR-PORT-2.** The Platform MUST NOT execute any active content associated with a portfolio's presentation layer (OUT-9).
- **FR-PORT-3.** *(MAY)* The Platform MAY support creating or modifying portfolios; if provided, it MUST preserve constituent files faithfully (PRIN-2).

## 9.26 Rich media policy (FR-MEDIA)

- **FR-MEDIA-1.** The Platform MUST handle documents containing rich media (audio, video, 3D, embedded active content) safely: it MUST NOT automatically execute or play embedded active content, and MUST require explicit user consent for any playback, routed through safe handling (NFR-SEC, OUT-9).
- **FR-MEDIA-2.** The Platform MUST clearly indicate the presence of rich media and MUST allow the user to remove it (e.g., during sanitization; FR-VER).
- **FR-MEDIA-3.** *(MAY)* Basic playback of standard media MAY be supported as an explicit, consented, optional capability; it MUST NOT be a core dependency and MUST be disableable by policy (§12).

## 9.27 Import and export (FR-IMPORT, FR-EXPORT)

- **FR-IMPORT-1.** The Platform MUST create PDFs from images and MUST support importing common inputs (at minimum images; other formats as delivered) into PDF form.
- **FR-EXPORT-1.** The Platform MUST export document content to images, plain text, and HTML, preserving reading order for tagged documents where applicable.
- **FR-EXPORT-2.** *(When delivered, quality-gated)* The Platform MUST support exporting to office document formats; such export MUST disclose fidelity limitations honestly (PRIN-6) and MUST NOT claim perfect fidelity it does not achieve.
- **FR-EXPORT-3.** All export operations MUST be available in batch and via the CLI (PRIN-1).

## 9.28 Encryption, permissions, metadata (FR-SEC, FR-PERM, FR-META)

- **FR-SEC-1.** The Platform MUST open documents protected with standard encryption (given the correct password/credential) and MUST support creating documents with standard encryption using current, recognized algorithms. It MUST be able to read legacy-encrypted documents but MUST NOT create documents using algorithms known to be insecure.
- **FR-SEC-2.** The Platform MUST support user (open) and owner (permissions) passwords and MUST clearly explain the difference and the practical strength of each (PRIN-6).
- **FR-SEC-3.** *(MAY)* Certificate-based (public-key) encryption MAY be supported.
- **FR-PERM-1.** The Platform MUST allow setting permission flags (e.g., printing, copying, editing) and MUST honor them by default when consuming documents, while clearly disclosing that such permissions are advisory and not a security guarantee (PRIN-6, OUT-7).
- **FR-META-1.** The Platform MUST display and edit standard document metadata (title, author, subject, keywords, dates) as undoable changes (FR-UNDO).
- **FR-META-2.** The Platform MUST support removing hidden and identifying metadata as part of sanitization (FR-VER), and MUST disclose what categories of metadata remain after a sanitize operation (PRIN-6).

## 9.29 Version history, autosave, recovery (FR-VER, FR-AUTOSAVE, FR-REC, FR-UNDO, FR-SAVE)

- **FR-SAVE-1.** The Platform MUST, by default, save changes in a manner that preserves untouched document content and existing signatures where the change permits, and completes quickly regardless of document size (PRIN-2, §14 save latency, `[ADR-012]`).
- **FR-SAVE-2.** The Platform MUST provide an explicit "save a clean/optimized copy" operation that may rewrite the document fully, always preceded by a disclosure of what will be lost (history, signatures) (PRIN-6).
- **FR-VER-1.** The Platform MUST allow the user to view the document's revision history where present, and to understand that incremental changes are retained.
- **FR-VER-2.** The Platform MUST provide a sanitize/flatten-history operation that removes retained prior revisions and hidden data, disclosing what is removed and what remains (PRIN-6). This addresses the privacy risk that document history and hidden content can leak information (US-LAW-4, US-TRUST-6).
- **FR-UNDO-1.** The Platform MUST provide unlimited undo and redo of document changes within a session, with clearly named, grouped operations.
- **FR-UNDO-2.** Undo history MUST persist such that changes remain reversible after closing and reopening a document within a recovery context, to the extent feasible (US-TRUST-4, `[ADR-013]`).
- **FR-AUTOSAVE-1.** The Platform MUST protect unsaved work automatically such that a crash, power loss, or forced termination loses no more than the durability budget of committed changes (§14, `[SDS §10]`). Autosave MUST NOT create unmanaged copies of the document in shared or user-visible locations (privacy; `[SDS §10.3]`).
- **FR-REC-1.** After an unexpected termination, the Platform MUST offer to recover unsaved work, presenting a clear, itemized summary of what will be restored, per document.
- **FR-REC-2.** The Platform MUST recover gracefully from a save interrupted by crash or power loss such that the document remains openable as a valid prior version (never corrupted; PRIN-2, `[SDS §10.5]`).
- **FR-REC-3.** The Platform MUST contain the effect of a document that repeatedly fails to render or process, keeping the rest of the application usable and informing the user (PRIN-6, `[SDS §10.1]`).

## 9.30 Cloud integration policy (FR-CLOUD)

- **FR-CLOUD-1.** The Platform MUST function fully without any cloud integration (VIS-3).
- **FR-CLOUD-2.** The Platform MUST NOT enable any network feature by default; any such feature MUST be explicitly enabled and MUST disclose what data leaves the device (VIS-2, PRIN-3).
- **FR-CLOUD-3.** *(Future, FUT-1)* Any collaboration or sync feature MUST be optional, self-hostable, and administrator-controllable, and MUST NOT become a dependency for core work.
- **FR-CLOUD-4.** The Platform MUST allow administrators to disable all network features entirely by policy (§12).

## 9.31 Diagnostics and inspection (FR-DIAG)

- **FR-DIAG-1.** The Platform MUST provide a per-document diagnostics view reporting: repairs performed on a damaged file (leniency), unsupported constructs skipped, presence of scripts/rich media/XFA, and other honesty-relevant conditions (PRIN-6).
- **FR-DIAG-2.** The Platform MUST provide a document inspection capability (structure, revisions, metadata) sufficient for advanced users and support/bug reporting, available in the GUI (advanced) and CLI (US-DEV-6).
- **FR-DIAG-3.** Diagnostic and inspection output intended for sharing MUST allow the user to review its contents before it leaves the device, and MUST NOT include document content the user has not chosen to include (PRIN-3).

---

# 10. Non-Functional Requirements

*Normative.* Non-functional requirements define quality attributes and are testable via the metrics in §14. Budgets stated as "reference targets" are the initial published values; the authoritative, versioned budget values live with the benchmarking system (`[ADR-023]`) and §14, but a release MUST meet published budgets to be conformant (PRIN-5).

## 10.1 Performance (NFR-PERF)

- **NFR-PERF-1.** Interactive operations (scroll, zoom, page navigation, selection, annotation drawing) MUST maintain responsiveness at the display's refresh cadence under normal conditions; frame-time budgets MUST be met at the 95th and 99th percentiles, not merely on average (§14).
- **NFR-PERF-2.** The Platform MUST render the visible region with priority over off-screen work, and MUST not block interaction on background work (indexing, thumbnails, OCR, batch) (`[SDS §6, §9]`).
- **NFR-PERF-3.** Editing a page in a large document MUST NOT cause the whole document to re-process; only affected content is recomputed (`[SDS §6.6]`). *Acceptance:* editing latency is independent of total document size beyond a bounded factor.
- **NFR-PERF-4.** Search MUST return the first result within the published latency budget on large documents (§14).
- **NFR-PERF-5.** Performance budgets MUST be enforced as release gates; a regression beyond tolerance MUST block release (PRIN-5, `[ADR-023, ADR-029]`).

## 10.2 Responsiveness (NFR-RESP)

- **NFR-RESP-1.** The user interface MUST remain responsive to input at all times, including while a document is loading, rendering, or undergoing a long operation. No user action intended to be interactive may block the interface (`[SDS §7.1]`).
- **NFR-RESP-2.** Long operations MUST show progress and MUST be cancellable, with cancellation taking effect promptly (§14 cancellation latency).
- **NFR-RESP-3.** The Platform MUST provide feedback within a perceptible threshold for any action that cannot complete instantly, so the user is never left uncertain whether an action registered (UX-ERR, UX-DISC).

## 10.3 Memory (NFR-MEM)

- **NFR-MEM-1.** Memory use MUST be bounded and predictable relative to system resources; the Platform MUST NOT grow memory without limit as documents are viewed, edited, or as sessions lengthen (`[ADR-011]`). *Acceptance:* long-run soak tests show a stable steady state (§14).
- **NFR-MEM-2.** The Platform MUST handle documents substantially larger than available RAM by loading content on demand rather than requiring the whole document in memory (`[ADR-011]`, NFR-LARGE).
- **NFR-MEM-3.** Under memory pressure, the Platform MUST degrade gracefully (reduced caching, slower rendering) rather than fail, and MUST never crash due to cache growth (`[SDS §9.3]`).
- **NFR-MEM-4.** Closing a document MUST release the memory associated with it promptly (`[SDS §9.2]`).
- **NFR-MEM-5.** Per-open-page memory MUST be a measured, regression-gated figure (§14).

## 10.4 Startup (NFR-START)

- **NFR-START-1.** Application cold start to an interactive state MUST meet the published startup budget on reference hardware for each platform (§14).
- **NFR-START-2.** Opening a document and displaying its first page MUST meet the published first-page budget; the budget applies to a representative document, with large/complex documents governed by NFR-LARGE.
- **NFR-START-3.** Startup MUST perform no network activity (VIS-2) and MUST NOT be delayed by background/maintenance work.

## 10.5 Large-document handling (NFR-LARGE)

- **NFR-LARGE-1.** The Platform MUST open, navigate, search, and annotate very large documents (reference class: thousands of pages and/or scan-heavy, hundreds of megabytes) while meeting interaction budgets (§14).
- **NFR-LARGE-2.** Navigation to an arbitrary page in a large document MUST be fast and MUST NOT require processing all preceding pages (`[ADR-006]`).
- **NFR-LARGE-3.** Saving changes to a large document MUST be fast by default (incremental), independent of total document size (`[ADR-012]`, §14 save latency).

## 10.6 Reliability (NFR-REL)

- **NFR-REL-1.** The Platform MUST NOT lose user data as a result of application crash, worker failure, power loss, or interrupted save (FR-REC, FR-AUTOSAVE). The bounded acceptable loss is the durability budget (§14).
- **NFR-REL-2.** A malformed or hostile document MUST NOT crash the entire application; failure MUST be contained (`[SDS §10.1]`, FR-REC-3).
- **NFR-REL-3.** The Platform's crash rate MUST meet the published reliability target (§14); crashes MUST be diagnosable from local, user-reviewable reports (PRIN-3, FR-DIAG).
- **NFR-REL-4.** Operations that modify documents MUST be atomic with respect to failure: either the change is fully applied and saved, or the document remains at a valid prior state (`[SDS §10.5]`).

## 10.7 Availability (NFR-AVAIL)

- **NFR-AVAIL-1.** As a desktop application with no required services, the Platform's availability MUST NOT depend on any network or external service for core operation (VIS-3).
- **NFR-AVAIL-2.** Any optional service (future) MUST be designed so that its unavailability degrades only the optional feature, never core functionality (FR-CLOUD-3).

## 10.8 Security (NFR-SEC)

- **NFR-SEC-1.** The Platform MUST treat all document content as untrusted and MUST process it so that opening or interacting with a malicious document cannot compromise the user's system (`[ADR-016]`, `[SDS §12]`).
- **NFR-SEC-2.** The Platform MUST isolate document processing so that exploitation of a parsing or rendering flaw is contained and cannot access the user's files, network, or other documents beyond brokered, consented actions (`[ADR-016]`).
- **NFR-SEC-3.** The Platform MUST NOT execute active document content except the specified JavaScript forms subset, which MUST run without access to the file system, network, or host control (FR-JS-2, OUT-9).
- **NFR-SEC-4.** Privileged actions (file access, printing, clipboard, any network) MUST require explicit routing through the host and, where user-affecting, explicit consent (`[SDS §12.3]`).
- **NFR-SEC-5.** The Platform MUST support a verifiable relationship between the distributed binary and its published source, so that users and administrators can confirm authenticity (PRIN-3, §12, `[ADR-029]`).
- **NFR-SEC-6.** The Platform MUST have a published security disclosure process and MUST treat isolation/sandbox escapes as highest-severity, release-blocking issues (`[ADR-016]`).
- **NFR-SEC-7.** Extensions MUST be constrained per FR-PLUG-2/3 and MUST NOT be able to escalate beyond granted capabilities.

## 10.9 Privacy (NFR-PRIV)

- **NFR-PRIV-1.** The Platform MUST NOT transmit user documents, document content, or personal data to any network destination except as the direct, visible, consented result of a user action (VIS-2).
- **NFR-PRIV-2.** The Platform MUST NOT collect analytics or telemetry by default. Any telemetry MUST be strictly opt-in, disclosed in full, minimal, and disableable, and MUST NOT include document content (PRIN-3).
- **NFR-PRIV-3.** The Platform MUST NOT retain document content in logs, diagnostics, or temporary artifacts in a way that could leak it; diagnostic exports MUST be user-reviewable before sharing (FR-DIAG-3, `[ADR-020]`).
- **NFR-PRIV-4.** The Platform MUST give the user control over data that persists on the device (recent files, thumbnails, indexes, recovery journals), including inspection and deletion (FR-SRCH-IDX-1, FR-DIAG).
- **NFR-PRIV-5.** Administrators MUST be able to disable any network-capable feature entirely by policy (FR-CLOUD-4, §12).

## 10.10 Accessibility (NFR-A11Y)

- **NFR-A11Y-1.** The application MUST be fully operable via keyboard alone (UX-A11Y), and MUST be usable with the standard screen reader and assistive technologies on each supported platform.
- **NFR-A11Y-2.** The application MUST meet recognized accessibility guidelines for desktop software on each platform, and this conformance MUST be tested each release (§14, PRIN-8).
- **NFR-A11Y-3.** Accessibility of the application MUST NOT regress across releases; focus order, labels, and announcements are part of the interface-stability contract (PRIN-4, US-AXS-5).
- **NFR-A11Y-4.** Accessibility features MUST be part of the core and MUST NOT be delegated to extensions (PRIN-8).

## 10.11 Localization and internationalization (NFR-LOC)

- **NFR-LOC-1.** The Platform MUST be fully localizable; all user-facing text MUST be externalized for translation from the first release (`[SDS §2.1]`).
- **NFR-LOC-2.** The Platform MUST correctly handle documents and user input in all major writing systems, including left-to-right, right-to-left, and vertical scripts, and complex text shaping, for viewing, search, selection, and extraction (FR-SRCH-2).
- **NFR-LOC-3.** The Platform MUST format numbers, dates, and measurements per the user's locale where displayed by the application (measurement precision per FR-MEAS).
- **NFR-LOC-4.** Localization MUST cover the application interface; it need not translate document content. Right-to-left interface layout MUST be supported where the platform's conventions require it.

## 10.12 Scalability (NFR-SCALE)

- **NFR-SCALE-1.** The Platform MUST scale to large individual documents (NFR-LARGE) and to large batch jobs (many files) without loss of responsiveness of the interactive application (NFR-RESP).
- **NFR-SCALE-2.** The Platform MUST make effective use of available multi-core hardware for parallelizable work (rendering, batch) while respecting interactive priority (`[SDS §7]`).
- **NFR-SCALE-3.** The command-line interface MUST scale to server and pipeline use, including concurrent invocations, without a graphical environment (FR-CLI-2).

## 10.13 Maintainability (NFR-MAINT)

- **NFR-MAINT-1.** Product behavior MUST be specified (this PRD) and architecture documented (ADR/SDS) such that a new contributor can determine intended behavior without undocumented assumptions (PRIN-10).
- **NFR-MAINT-2.** User-observable behavior changes MUST be traceable to a requirement or an approved change; undocumented behavior changes are defects.
- **NFR-MAINT-3.** The Platform MUST be structured so that core capabilities can outlive specific implementation choices (engine, UI toolkit) — a product-level restatement of the architecture's replaceability goals (`[ADR-003, ADR-005]`).
- **NFR-MAINT-4.** Diagnostic and inspection capabilities (FR-DIAG) MUST be sufficient to triage field issues without telemetry (PRIN-3).

## 10.14 Compatibility (NFR-COMPAT)

- **NFR-COMPAT-1.** The Platform MUST interoperate with documents produced by major PDF software and MUST produce documents those tools consume correctly (§13, PRIN-7).
- **NFR-COMPAT-2.** The Platform MUST support the range of PDF versions in common use, including current and legacy documents, and MUST preserve forward-compatibility by not corrupting constructs it does not understand (PRIN-2, §13).
- **NFR-COMPAT-3.** The Platform MUST run on supported versions of each operating system as defined by the platform-support policy (§12), and MUST behave consistently across them for all specified functionality (differences limited to platform-native conventions).
- **NFR-COMPAT-4.** File formats and data the Platform writes (annotations, form data, exported comments, saved pipelines, extension packages) MUST maintain backward compatibility per the versioning policy (§16), so that older data remains usable.

## 10.15 Offline operation (NFR-OFFLINE)

- **NFR-OFFLINE-1.** All core functionality MUST work with no network connectivity (VIS-3). *Acceptance:* the full in-scope core workflow set (§8.1) completes on a machine with networking disabled.
- **NFR-OFFLINE-2.** The Platform MUST NOT degrade, nag, or restrict functionality based on connectivity or the absence of an account (VIS-1).
- **NFR-OFFLINE-3.** License activation, where any exists for a commercial edition, MUST support fully offline activation (§12); the open-source core MUST require no activation at all.

---

# 11. UX Requirements

*Normative.* UX requirements define interaction quality and the interface-stability contract. They constrain UX design but do not prescribe visual specifics, which are the province of design under these constraints.

## 11.1 Navigation philosophy (UX-NAV)

- **UX-NAV-1.** The Platform MUST preserve the navigation model and mental model familiar to experienced Acrobat users: document pane with page navigation, side panels for outline/thumbnails/comments/attachments/layers, a tool/command surface, and standard view controls (PRIN-4, §5.2).
- **UX-NAV-2.** Where the Platform improves on incumbent navigation, the improvement MUST address a measured deficiency (e.g., excessive clicks for common actions) and MUST NOT remove or relocate familiar capabilities without an opt-in path (PRIN-4).
- **UX-NAV-3.** Panels MUST be dockable, resizable, and hideable; layout MUST be persistent per user (`[SDS §2.11]`).

## 11.2 Interaction model (UX-INT)

- **UX-INT-1.** Common actions MUST be reachable in the fewest reasonable steps; the Platform MUST NOT increase the step count of a common workflow relative to the classic Acrobat baseline without justification (a direct response to documented incumbent regressions; §5.2, US-TRUST-7).
- **UX-INT-2.** The interaction model MUST be consistent across subsystems: selection, context menus, tool activation, property editing, and undo MUST behave uniformly (UX-CONS).
- **UX-INT-3.** Tools MUST provide clear active-state indication and an obvious way to return to the default (selection) state.
- **UX-INT-4.** Direct manipulation (drag to reorder pages, drag annotation handles, drag to select) MUST be supported wherever it is the natural interaction.

## 11.3 Keyboard-first workflows (UX-KEY)

- **UX-KEY-1.** Every function of the Platform MUST be operable via keyboard (UX-A11Y, NFR-A11Y-1).
- **UX-KEY-2.** The Platform MUST preserve familiar Acrobat keyboard shortcuts for equivalent actions wherever they exist, as part of the stability contract (PRIN-4). The default shortcut set is a versioned artifact (§11.9).
- **UX-KEY-3.** The Platform MUST allow users to view, and SHOULD allow users to customize, keyboard shortcuts; customized shortcuts MUST persist and MUST be exportable/importable for portability across machines.
- **UX-KEY-4.** Keyboard focus MUST be always visible and MUST follow a logical, stable order (NFR-A11Y-3).

## 11.4 Mouse workflows (UX-MOUSE)

- **UX-MOUSE-1.** The Platform MUST support standard mouse interactions (click, double-click, right-click context menus, drag, wheel scroll, modifier-augmented actions) consistent with each platform's conventions.
- **UX-MOUSE-2.** Wheel and trackpad gestures MUST support smooth scrolling and zoom (with modifier), consistent with platform norms.

## 11.5 Touch and pen support (UX-TOUCH, UX-PEN)

- **UX-TOUCH-1.** The Platform SHOULD support touch interaction for core navigation (scroll, pinch-zoom, tap) on touch-capable devices, without compromising the desktop mouse/keyboard experience.
- **UX-PEN-1.** The Platform SHOULD support pen/stylus input for ink annotation, including pressure sensitivity where the hardware and platform provide it, with responsive, low-latency inking (FR-ANNOT-7).
- **UX-TOUCH-2.** Touch and pen support MUST NOT be prerequisites for any functionality; all functions remain available via keyboard and mouse (UX-KEY-1).

## 11.6 Multi-monitor and window management (UX-MULTI)

- **UX-MULTI-1.** The Platform MUST behave correctly across multiple monitors, including monitors with different DPI and scaling, without blurring or misplacement (UX-DPI).
- **UX-MULTI-2.** The Platform SHOULD support viewing multiple documents and SHOULD support detaching/arranging document views to support multi-monitor workflows (e.g., comparing documents side by side).
- **UX-MULTI-3.** Window and layout state SHOULD be restorable across sessions.

## 11.7 High-DPI rendering (UX-DPI)

- **UX-DPI-1.** The Platform MUST render crisply at any display scaling, including fractional scaling, and MUST update correctly when a window moves between displays of different scale (UX-MULTI-1, FR-VIEW-5).
- **UX-DPI-2.** Text, vector graphics, UI, and annotation overlays MUST all be high-DPI correct.

## 11.8 Accessibility of the interface (UX-A11Y)

- **UX-A11Y-1.** The interface MUST expose all controls, state, and content to assistive technology with correct roles, names, values, and relationships (NFR-A11Y).
- **UX-A11Y-2.** All interactive elements MUST be keyboard reachable and operable, with visible focus (UX-KEY-4).
- **UX-A11Y-3.** The Platform MUST respect platform accessibility settings (contrast, reduced motion, text scaling) where applicable.
- **UX-A11Y-4.** Accessibility of the interface MUST be verified each release and MUST NOT regress (NFR-A11Y-3, PRIN-4).

## 11.9 Undo expectations (UX-UNDO)

- **UX-UNDO-1.** Every document-modifying action MUST be undoable and redoable (FR-UNDO), and the undo/redo affordances MUST clearly name the action to be undone/redone.
- **UX-UNDO-2.** Undo depth MUST be effectively unlimited within a session (FR-UNDO-1), and MUST persist for recovery across reopen where feasible (FR-UNDO-2).
- **UX-UNDO-3.** Actions that cannot be undone (e.g., an explicit destructive sanitize/clean-save) MUST be clearly identified as such before the user commits (PRIN-6).

## 11.10 Consistency (UX-CONS)

- **UX-CONS-1.** Terminology, iconography, interaction patterns, and command placement MUST be consistent across the application and stable across releases (PRIN-4).
- **UX-CONS-2.** The Platform MUST follow each host platform's conventions where users expect native behavior (menus, dialogs, shortcuts for platform-standard actions, file pickers), while keeping document-workflow behavior consistent across platforms (NFR-COMPAT-3).

## 11.11 Error messaging (UX-ERR)

- **UX-ERR-1.** Error and status messages MUST be specific, plain-language, and actionable; they MUST state what happened and, where possible, what the user can do (PRIN-6, US-CAS-12).
- **UX-ERR-2.** The Platform MUST distinguish between a document problem (e.g., damaged file, unsupported construct) and an application problem, and MUST attribute cause correctly.
- **UX-ERR-3.** The Platform MUST NOT present false success and MUST surface honest, non-alarming notices for tolerated conditions (e.g., "this file was repaired to open"; leniency, FR-DIAG-1).
- **UX-ERR-4.** Security-relevant prompts (consent for network, opening embedded files, running document scripts) MUST present clear, sufficient information for an informed decision (NFR-SEC-4).

## 11.12 Discoverability (UX-DISC)

- **UX-DISC-1.** Features MUST be discoverable through consistent menus, a searchable command surface, and contextual affordances, without compromising the stability of familiar locations (PRIN-4).
- **UX-DISC-2.** The Platform SHOULD provide a command search ("find a tool by name") to aid discoverability without forcing users to relearn locations.
- **UX-DISC-3.** First-run and empty states SHOULD orient new users without obstructing experienced users (persona balance, §6).
- **UX-DISC-4.** Progress, background activity, and the presence of notable document conditions (scripts, rich media, damage, unsupported content) MUST be discoverable (FR-DIAG, UX-ERR-3).

## 11.13 Visual and appearance (UX-VIS)

- **UX-VIS-1.** The Platform SHOULD support light and dark application themes and MUST respect the platform's theme preference where applicable; application theming MUST NOT alter document rendering (PRIN-2, FR-VIEW-6).
- **UX-VIS-2.** Reading-comfort options (background tint, view modes) MUST be view-only and non-destructive (FR-VIEW-6).

---

# 12. Enterprise Requirements

*Normative.* These requirements serve the enterprise IT administrator and government/public-sector personas (§6.2.9, §6.3.1) and the enterprise-adoption thesis (§2.3). They address deployment, policy, licensing, auditability, certificate and trust management, offline activation, and shared environments.

## 12.1 Policy management (ENT-POL)

- **ENT-POL-1.** The Platform MUST support central configuration of application behavior via each platform's standard policy/configuration mechanism, such that administrators can set defaults and enforce settings across many machines (US-ITA-2).
- **ENT-POL-2.** Policy MUST be able to: set default behaviors (e.g., default save mode, default units), disable specific features (e.g., all network features, document JavaScript, rich-media playback, plugin installation), pin the update channel, and pin the interface-behavior profile (§12.6, PRIN-4).
- **ENT-POL-3.** Enforced policies MUST take precedence over user settings and MUST be presented to the user as administrator-controlled (not silently failing) (UX-ERR).
- **ENT-POL-4.** Policy configuration MUST be documented and MUST be stable across releases so that certified configurations remain valid (NFR-MAINT, PRIN-4).
- **ENT-POL-5.** The Platform MUST be able to run in a fully network-disabled mode by policy, with verifiable absence of network activity (US-ITA-4, VIS-2, NFR-PRIV-5).

## 12.2 Deployment (ENT-DEP)

- **ENT-DEP-1.** The Platform MUST be deployable using each platform's standard enterprise deployment and management tooling (US-ITA-1), including silent/unattended installation.
- **ENT-DEP-2.** The Platform MUST support side-by-side or controlled coexistence sufficient for administrators to stage and validate a new version before broad rollout (US-ITA-3).
- **ENT-DEP-3.** Installation MUST NOT require an account, network connectivity, or activation for the open-source core (VIS-1, NFR-OFFLINE-3).
- **ENT-DEP-4.** The Platform MUST provide a long-term-support (LTS) track receiving security and critical fixes without forcing feature or interface change (US-GOV-7, §16, PRIN-4).
- **ENT-DEP-5.** Updates MUST be administrator-controllable: administrators MUST be able to disable automatic updates, choose when updates apply, and prevent updates that would change the pinned interface profile (US-ITA-3, PRIN-4).

## 12.3 Licensing (ENT-LIC)

- **ENT-LIC-1.** The open-source core MUST impose no per-seat licensing, activation, or account requirement, and MUST NOT include any mechanism that restricts use based on license state (VIS-1).
- **ENT-LIC-2.** Offboarding or reallocating users MUST require no license-management action for the open-source core (US-ITA-5).
- **ENT-LIC-3.** Any commercial edition MUST NOT degrade or gate core functionality (VIS-4) and MUST support fully offline activation (NFR-OFFLINE-3, ENT-OFF).
- **ENT-LIC-4.** The Platform's licensing and its third-party components' licenses MUST be documented and available for administrator review (governance; §17 references).

## 12.4 Auditability (ENT-AUD)

- **ENT-AUD-1.** The Platform MUST allow administrators to determine the effective configuration (settings and enforced policies) on a machine for audit purposes (US-ITA-6).
- **ENT-AUD-2.** The Platform MUST provide a verifiable relationship between the installed binary and its published source/release (NFR-SEC-5, PRIN-3), so that administrators can confirm authenticity and integrity.
- **ENT-AUD-3.** Security-relevant local actions (e.g., changes to trusted certificates, enabling a network feature) SHOULD be locally recordable for audit, without transmitting data externally (NFR-PRIV-1).
- **ENT-AUD-4.** The Platform MUST document its data-at-rest footprint (recent files, thumbnails, indexes, recovery journals, settings) so administrators can manage it in shared or regulated environments (NFR-PRIV-4).

## 12.5 Certificate and trust management (ENT-CERT)

- **ENT-CERT-1.** Administrators MUST be able to pre-configure trusted certificates and trust settings used for signature validation, so that users receive correct and consistent trust decisions (US-ITA-7, FR-SIG-6).
- **ENT-CERT-2.** The Platform MUST integrate with each platform's certificate/key stores and MUST support hardware tokens/smart cards for signing where delivered (FR-SIG-3).
- **ENT-CERT-3.** Trust configuration MUST be manageable centrally (ENT-POL) and MUST be auditable (ENT-AUD-1).
- **ENT-CERT-4.** The Platform MUST NOT trust certificates or validation data by default beyond recognized, configured trust; unverifiable trust MUST yield an indeterminate result, not a trusted one (FR-SIG-1, PRIN-6).

## 12.6 Interface-behavior profiles for enterprises (ENT-UI) — [PRD Decision]

- **ENT-UI-1.** The Platform MUST support pinning an interface-behavior profile (shortcuts, menu taxonomy, and workflow behaviors) by policy, so that an organization can standardize and freeze the user experience across a deployment for the life of an LTS track (PRIN-4, US-GOV-7, `[ADR-030]`).
- **ENT-UI-2.** A pinned profile MUST remain available and supported for the duration of the LTS track on which it is pinned; removal of a supported profile MUST NOT occur within that window (PRIN-4).
- **ENT-UI-3.** This capability exists to prevent the class of disruption in which an interface change is imposed on users without consent (§5.2); it is the enterprise expression of the interface-stability contract.

## 12.7 Offline activation policy (ENT-OFF)

- **ENT-OFF-1.** The open-source core MUST require no activation (NFR-OFFLINE-3, ENT-LIC-1).
- **ENT-OFF-2.** Any commercial edition's activation MUST be completable entirely offline (e.g., via file-based activation), MUST NOT require ongoing connectivity, and MUST NOT disable functionality due to connectivity loss (VIS-3, NFR-OFFLINE-1).

## 12.8 Shared and restricted environments (ENT-SHARE)

- **ENT-SHARE-1.** The Platform MUST operate correctly in shared-computer environments (e.g., multiple users, non-persistent or roaming profiles, terminal/remote sessions), keeping per-user data (settings, recent files, recovery journals) correctly separated per user (NFR-PRIV-4).
- **ENT-SHARE-2.** The Platform MUST operate in restricted environments without elevated privileges for normal use, and MUST function with no network access (VIS-3, US-GOV-5).
- **ENT-SHARE-3.** In non-persistent environments, the Platform MUST behave predictably regarding transient data (e.g., recovery journals), and administrators MUST be able to configure their location and lifecycle (NFR-PRIV-4, ENT-AUD-4).
- **ENT-SHARE-4.** The Platform SHOULD function correctly over remote-desktop/virtualized display, maintaining responsiveness and correct rendering to the extent the environment permits (NFR-RESP).

---

# 13. Compatibility Requirements

*Normative.* Compatibility is verified against actual products and standards, not only against specifications (PRIN-7, §5.2). This section defines required interoperability. Compatibility is validated by an interoperability test matrix maintained by QA (`[ADR-022]`, §14 feature-completeness/interop metric).

## 13.1 General interoperability (CMP-GEN)

- **CMP-GEN-1.** Documents the Platform produces (saved edits, annotations, form data, filled forms, signatures, standards-conformant exports) MUST be correctly consumable by the reference set of major PDF applications (§13.2), within documented, tracked limitations.
- **CMP-GEN-2.** The Platform MUST correctly consume documents produced by that reference set, including their annotations, form data, and signatures, to the extent those are standards-based.
- **CMP-GEN-3.** Where a compatibility limitation exists, it MUST be documented and, where user-visible, disclosed honestly (PRIN-6).
- **CMP-GEN-4.** Interoperability MUST be tested each release against the reference set; regressions MUST be tracked and gated per policy (`[ADR-022, ADR-029]`).

## 13.2 Reference applications (CMP-REF)

The following constitute the reference set for interoperability testing. *Informative:* the set may expand; it MUST include at least the following classes.

- **CMP-REF-1 (Adobe Acrobat/Reader).** Annotations, comments, form data (fill and round-trip), signatures (validate Acrobat-produced, and produce signatures Acrobat validates), redaction results, and standards outputs MUST interoperate with current Adobe Acrobat/Reader, within documented limits. Acrobat is the primary fidelity and interoperability reference (§5.2).
- **CMP-REF-2 (Foxit).** Interoperability of annotations, forms, and signatures with Foxit's current editor/reader MUST be tested.
- **CMP-REF-3 (PDF-XChange Editor).** Interoperability of annotations and forms SHOULD be tested (power-user reference).
- **CMP-REF-4 (Apple Preview).** Rendering fidelity and annotation/form interoperability with Preview MUST be tested (macOS default).
- **CMP-REF-5 (Chrome PDF viewer).** Rendering fidelity and form-fill interoperability with the Chrome/Chromium built-in viewer MUST be tested (ubiquitous reference; shares the same underlying engine family the Platform uses for rendering, per `[ADR-005]`).
- **CMP-REF-6 (Firefox PDF viewer / pdf.js).** Rendering fidelity and form interoperability with the Firefox built-in viewer MUST be tested.

## 13.3 Standards conformance (CMP-STD)

- **CMP-STD-1.** The Platform MUST conform to the PDF specification (ISO 32000) for the constructs it reads and writes, and MUST preserve constructs it does not interpret rather than corrupting them (PRIN-2, NFR-COMPAT-2).
- **CMP-STD-2.** Where the Platform claims conformance to an archival, prepress, or accessibility standard (PDF/A, PDF/X, PDF/UA), the output MUST validate against that standard, verified with recognized validation methods (FR-STD-5, §14).
- **CMP-STD-3.** Signature conformance MUST follow recognized profiles (e.g., PAdES) for the levels the Platform claims to support (FR-SIG-3).
- **CMP-STD-4.** The Platform MUST NOT declare a standards conformance it does not meet (PRIN-6, FR-STD-5).

## 13.4 Backward compatibility (CMP-BACK)

- **CMP-BACK-1.** The Platform MUST open and correctly handle legacy PDF documents, including older versions and older encryption (read), and MUST render and allow working with them (NFR-COMPAT-2).
- **CMP-BACK-2.** Data and artifacts the Platform itself produces (saved pipelines, exported comment data, extension packages, settings/profiles) MUST remain usable by later versions per the versioning policy (§16), or MUST be migratable with no data loss.
- **CMP-BACK-3.** The Platform MUST NOT require users to upgrade documents to a proprietary or non-standard format to use core features (PRIN-7, OUT-7).

## 13.5 Forward compatibility (CMP-FWD)

- **CMP-FWD-1.** The Platform MUST handle documents using newer specification features it does not fully support by preserving them where possible and disclosing unsupported constructs rather than corrupting or silently dropping them (PRIN-2, PRIN-6, FR-VIEW-7).
- **CMP-FWD-2.** The Platform's own forward-facing contracts (extension contract, saved-pipeline format, exported-data formats) MUST evolve under a versioning and deprecation policy that gives dependents advance notice (FR-PLUG-5, §16).

## 13.6 Cross-platform behavioral compatibility (CMP-XPLAT)

- **CMP-XPLAT-1.** For all specified functionality, the Platform MUST behave consistently across Windows, macOS, and Linux, with differences limited to platform-native conventions (menus, dialogs, standard shortcuts, file pickers) (NFR-COMPAT-3, UX-CONS-2).
- **CMP-XPLAT-2.** A document produced on one platform MUST be byte-for-byte equivalent, or functionally identical and standards-equivalent, to the same operation performed on another platform (PRIN-1). *Acceptance:* cross-platform output-equivalence tests pass (§14).

---

# 14. Success Metrics

*Normative for the existence and gating role of metrics; the specific target values are the initial published budgets and are maintained authoritatively by the benchmarking and QA systems (`[ADR-023]`), versioned with the reference hardware and corpus. A release MUST meet the then-published targets to be conformant (PRIN-5).*

*Informative note on values:* The numeric targets below are **[PRD Decision]** initial reference targets, chosen to be ambitious but plausible for a native, engine-backed application, and explicitly subject to calibration during the first milestones (per the SDS caution that early budgets require validation by a prototype). They exist so that teams have concrete goals from day one; they are not immutable.

## 14.1 Performance metrics (MET-PERF)

- **MET-PERF-1 (Cold start).** Time from launch to interactive, on reference hardware per platform. *Initial target:* ≤ 1.0 s median, ≤ 1.5 s p95. Gated per release (NFR-START-1).
- **MET-PERF-2 (First page).** Time from open request to first-page visible for a representative document. *Initial target:* ≤ 300 ms median, ≤ 600 ms p95 (NFR-START-2).
- **MET-PERF-3 (Scroll smoothness).** Frame-time under a standardized scroll of a large document. *Initial target:* meets display cadence at p95; no frame exceeds 2× the frame budget at p99 (NFR-PERF-1, NFR-LARGE-1).
- **MET-PERF-4 (Zoom responsiveness).** Interactive zoom maintains cadence at p95 (NFR-PERF-1).
- **MET-PERF-5 (Search first result).** On a large document. *Initial target:* ≤ 200 ms median for first hit (NFR-PERF-4).
- **MET-PERF-6 (Save latency, incremental).** Independent of document size. *Initial target:* ≤ 200 ms median for a typical edit set on any document size (NFR-LARGE-3, FR-SAVE-1).
- **MET-PERF-7 (Edit locality).** Editing latency does not scale with total document size beyond a bounded factor (NFR-PERF-3).
- **MET-PERF-8 (Cancellation latency).** A cancelled long operation ceases within *initial target* 200 ms (NFR-RESP-2).

## 14.2 Reliability metrics (MET-REL)

- **MET-REL-1 (Crash-free sessions).** Proportion of sessions without an application crash. *Initial target:* ≥ 99.9% across the test and dogfood population; measured without telemetry via aggregated, voluntary, local-report submission and QA fleets (NFR-REL-3, PRIN-3).
- **MET-REL-2 (Data-loss incidents).** Zero data-loss incidents beyond the durability budget under the reliability and fault-injection test suites (NFR-REL-1, FR-AUTOSAVE-1). *This target is absolute.*
- **MET-REL-3 (Durability budget).** Maximum unsaved committed work lost to crash/power-loss/interrupted-save. *Initial target:* ≤ 2 seconds (or a bounded number of committed changes), per `[SDS §10]`.
- **MET-REL-4 (Hostile-document containment).** 100% of a hostile/malformed corpus fails to crash the whole application or escape isolation (NFR-REL-2, NFR-SEC-1). *Absolute.*
- **MET-REL-5 (Corrupt-file open rate).** Proportion of a damaged-file corpus that opens with disclosed repair rather than failing. *Initial target:* meets or exceeds the reference engines' open rate on the same corpus (FR-VIEW-2).

## 14.3 Memory metrics (MET-MEM)

- **MET-MEM-1 (Per-page steady memory).** Bounded, measured figure for a representative document class; regression-gated (NFR-MEM-5).
- **MET-MEM-2 (Large-document ceiling).** A large reference document is fully usable within a defined memory ceiling on reference hardware (NFR-LARGE-1, NFR-MEM-2).
- **MET-MEM-3 (Soak stability).** Over a multi-hour open/edit/close/scroll soak, memory returns to a stable steady state with no unbounded growth (NFR-MEM-1). *Absolute (no leak).*
- **MET-MEM-4 (Release on close).** Closing a document returns its memory promptly to a defined baseline (NFR-MEM-4).

## 14.4 Feature-completeness and interoperability metrics (MET-FEAT)

- **MET-FEAT-1 (Rendering fidelity).** Differential rendering pass-rate against the reference corpus and oracle. *Initial target:* meets or exceeds the reference engine's agreement rate, with all deviations tracked (FR-VIEW-1).
- **MET-FEAT-2 (Interop matrix).** Pass-rate of the interoperability matrix against the reference application set (§13.2) for annotations, forms, and signatures. *Initial target:* ≥ 99% of matrix cells pass, remainder documented (CMP-GEN-4).
- **MET-FEAT-3 (Standards validation).** 100% of documents the Platform claims to produce as PDF/A (and, when delivered, PDF/X, PDF/UA) validate against recognized validators (FR-STD-5, CMP-STD-2). *Absolute for claims made.*
- **MET-FEAT-4 (Extraction correctness).** Text-extraction correctness on the extraction corpus (ligatures, RTL, CJK, hyphenation) meets the published target; unreliable pages are correctly flagged (FR-SRCH-5).
- **MET-FEAT-5 (Redaction completeness).** 100% of redaction test cases show non-recoverable removal under verification (FR-RED-3). *Absolute.*
- **MET-FEAT-6 (Signature validation correctness).** 100% agreement with reference validators on a signature corpus including tampered files, never producing a false "valid" (FR-SIG-1). *Absolute.*
- **MET-FEAT-7 (CLI/GUI parity).** For every operation available in both, identical output on the parity corpus (FR-CLI-1, PRIN-1). *Absolute.*
- **MET-FEAT-8 (Roadmap completeness).** Proportion of in-scope core capabilities (§8.1) delivered and passing acceptance, tracked per milestone (§16).

## 14.5 Accessibility metrics (MET-A11Y)

- **MET-A11Y-1 (Interface conformance).** The application passes the recognized desktop accessibility conformance checks on each platform, each release (NFR-A11Y-2). *Absolute (no regression).*
- **MET-A11Y-2 (Keyboard operability).** 100% of functions operable via keyboard, verified by an audit checklist (UX-KEY-1). *Absolute.*
- **MET-A11Y-3 (Screen-reader task completion).** A defined set of core tasks is completable by a screen-reader user in usability testing (NFR-A11Y-1).
- **MET-A11Y-4 (Document remediation, when delivered).** Documents remediated with the Platform validate as accessible against the accessibility standard (FR-A11Y-4/5).

## 14.6 User-productivity metrics (MET-PROD)

*Informative; measured via structured usability studies, not telemetry (PRIN-3).*
- **MET-PROD-1 (Time-on-task vs. incumbent).** For a set of representative workflows (form fill, page assembly, redaction, sign, comment reconciliation), task completion time is ≤ the incumbent's on the same hardware (§5.2).
- **MET-PROD-2 (Step count vs. incumbent).** Common workflows require no more steps than the classic incumbent baseline (UX-INT-1).
- **MET-PROD-3 (Onboarding).** An experienced Acrobat user completes the representative workflows without training (§1.6, US-CAS-1).
- **MET-PROD-4 (Error recovery).** Users successfully recover from induced errors (crash, damaged file) in usability testing without data loss (MET-REL-2).

## 14.7 Metric governance (MET-GOV)

- **MET-GOV-1.** Metric targets, reference hardware, and reference corpora MUST be versioned together so results are comparable over time (`[ADR-023]`).
- **MET-GOV-2.** Metrics designated *Absolute* above MUST NOT be traded off; a failure is release-blocking. Budgeted metrics MUST meet the published target within tolerance to release (PRIN-5).
- **MET-GOV-3.** Metric results SHOULD be published for transparency (self-application of PRIN-6).

---

# 15. Risks

*Informative analysis with normative mitigations where marked.* Risks are rated by likelihood (L/M/H) and impact (L/M/H). Each lists mitigation; where a mitigation is a binding product requirement it references the requirement.

## 15.1 Technical risks

- **RISK-T1. Rendering-fidelity gap vs. incumbents (L: M, I: H).** Real-world PDFs are diverse and often malformed; achieving reference-quality rendering is hard. *Mitigation:* build on a hardened rendering engine (`[ADR-005]`); differential testing against oracles and a large corpus (`[ADR-022]`, MET-FEAT-1); honest disclosure of deviations (PRIN-6). *Residual:* a long tail of edge cases persists; tracked, not hidden.
- **RISK-T2. Large-document performance under real workloads (L: M, I: H).** Budgets may prove optimistic on pathological documents. *Mitigation:* architecture designed for laziness and locality (`[ADR-006, ADR-007, ADR-011]`); budgets validated by early prototype (SDS M0/M1); gating (MET-PERF). *Residual:* some documents may miss budgets; degradation must remain graceful (NFR-MEM-3).
- **RISK-T3. Correctness of security-critical subsystems (redaction, signatures) (L: M, I: H).** Errors here have severe consequences. *Mitigation:* verification-by-construction (redaction verification pass, FR-RED-3; conservative signature validation, FR-SIG-1); reference-validator agreement as an absolute metric (MET-FEAT-5/6). *Residual:* dependency on correct reference behavior; mitigated by multiple oracles.
- **RISK-T4. Isolation/sandbox escapes (L: L–M, I: H).** A PDF platform is a high-value attack target. *Mitigation:* isolation architecture (`[ADR-016]`); continuous fuzzing and external audit; escapes are release-blocking (NFR-SEC-6). *Residual:* zero-day risk inherent to all such software; mitigated by defense-in-depth and rapid disclosure/response.
- **RISK-T5. Dependency on an external rendering engine's roadmap (L: M, I: M).** The chosen engine serves other masters. *Mitigation:* capability-boundary abstraction enabling engine replacement (`[ADR-005]`, NFR-MAINT-3); an alternative engine tracked from early on. *Residual:* migration cost if the engine diverges; bounded by the abstraction.
- **RISK-T6. Reproducible-build and supply-chain integrity (L: M, I: M).** Verifiability is a core promise and is technically demanding. *Mitigation:* reproducible builds and provenance as release requirements (NFR-SEC-5, `[ADR-029]`); dependency governance (`[ADR-028]`). *Residual:* ongoing engineering burden; treated as standing work.

## 15.2 Product risks

- **RISK-P1. Scope overreach / never shipping (L: M, I: H).** The full Acrobat-replacement surface is enormous. *Mitigation:* strict phasing with every milestone shippable (§16, `[SDS §14]`); scope governance (SCOPE-1..3). *Residual:* pressure to add scope; countered by restraint (§5.8).
- **RISK-P2. Under-serving a critical professional workflow (L: M, I: M).** Missing one workflow can disqualify the product for a whole persona. *Mitigation:* persona-to-requirement traceability (§6.4); user-story coverage (§7); acceptance per persona. *Residual:* niche workflows remain for plugins (§9.23).
- **RISK-P3. Feature bloat eroding clarity and performance (L: M, I: M).** The incumbent's decline path. *Mitigation:* design principles as gates (§4); performance budgets (§10, §14); restraint (§5.8). *Residual:* continuous vigilance required.
- **RISK-P4. Text-editing quality falling short of expectations (L: M, I: M).** In-place text editing is the hardest capability and users expect Acrobat-level results. *Mitigation:* deliver last, with honest limitations (FR-EDIT, PRIN-6); dedicated design effort (§16 V3). *Residual:* expectation management via honesty (PRIN-6).

## 15.3 Legal and compliance risks

- **RISK-L1. Patent exposure in codecs/algorithms (L: M, I: H).** Some PDF-adjacent technologies have patent histories. *Mitigation:* prefer unencumbered, standard technologies; dependency and licensing governance (`[ADR-028]`, ENT-LIC-4); avoid known-encumbered features or isolate them. *Residual:* legal review required for specific features; a standing obligation.
- **RISK-L2. License compatibility across components (L: M, I: M).** Mixing component licenses can create obligations. *Mitigation:* license policy and enforcement (`[ADR-028]`); documented licensing (ENT-LIC-4). *Residual:* some capable components excluded by license; accepted trade-off.
- **RISK-L3. Signature/standards legal recognition (L: L–M, I: M).** Legal recognition of signatures depends on conformance and context. *Mitigation:* adhere to recognized profiles (CMP-STD-3); never overstate validity (FR-SIG-1, PRIN-6). *Residual:* legal recognition is jurisdiction-dependent and outside the product's control; documented honestly.
- **RISK-L4. Accessibility legal obligations on outputs (L: M, I: M).** Users may rely on the product for legally-required accessible documents. *Mitigation:* standards validation (FR-STD, FR-A11Y); accurate conformance claims (FR-STD-5). *Residual:* remediation quality is a shared responsibility with the author; documented.

## 15.4 UX risks

- **RISK-U1. Alienating experienced users by changing familiar behavior (L: M, I: H).** The exact failure the product exists to avoid. *Mitigation:* interface-stability contract (PRIN-4, §11.9, ENT-UI); opt-in changes; enterprise profile pinning (ENT-UI-1). *Residual:* some evolution is necessary; managed via opt-in and versioned profiles.
- **RISK-U2. Overwhelming new/casual users with professional density (L: M, I: M).** Depth can intimidate. *Mitigation:* discoverability and first-run orientation without disrupting experts (UX-DISC-3); command search (UX-DISC-2). *Residual:* inherent tension between depth and simplicity; balanced by persona priority (§6).
- **RISK-U3. Inconsistency across platforms (L: M, I: M).** Three platforms invite divergence. *Mitigation:* cross-platform behavioral compatibility (CMP-XPLAT); consistency requirements (UX-CONS). *Residual:* native-convention differences remain by design.
- **RISK-U4. Accessibility regressions (L: M, I: H).** Accessibility can silently degrade. *Mitigation:* per-release accessibility gating as absolute (MET-A11Y-1); accessibility in the stability contract (NFR-A11Y-3). *Residual:* requires sustained investment.

## 15.5 Performance risks

- **RISK-PF1. Budgets regressing under feature pressure (L: M, I: H).** The incumbent's documented decline. *Mitigation:* budgets as release gates on dedicated hardware (MET-GOV-2, `[ADR-023]`). *Residual:* occasional release delays to meet budgets — the mechanism working as intended.
- **RISK-PF2. Benchmark validity (L: M, I: M).** Bad benchmarks give false confidence. *Mitigation:* versioned reference hardware and corpora, percentile-based budgets (MET-GOV-1, MET-PERF). *Residual:* benchmark maintenance is ongoing work.

## 15.6 Interoperability risks

- **RISK-I1. Divergent behavior of reference applications (L: M, I: M).** Targets themselves disagree and change. *Mitigation:* interop matrix tested each release (CMP-GEN-4); prefer standards where references conflict (CMP-STD-1); honesty about limits (PRIN-6). *Residual:* moving targets require continuous testing.
- **RISK-I2. Annotation/appearance interoperability defects (L: M, I: M).** A historic ecosystem pain point. *Mitigation:* always write complete portable appearances (FR-ANNOT-2). *Residual:* some consumers mis-render regardless; bounded by writing correct appearances.

## 15.7 Community and governance risks

- **RISK-C1. Contributor sustainability over a decade (L: M, I: H).** Open-source projects can lose momentum. *Mitigation:* thorough specification and documentation lowering onboarding cost (NFR-MAINT, PRIN-10); a clear extension model attracting ecosystem contributors (§9.23). *Residual:* sustained community stewardship required; a governance concern beyond this PRD.
- **RISK-C2. Mission drift toward the practices the product opposes (L: L–M, I: H).** Commercial or growth pressure could erode the trust values. *Mitigation:* vision commitments as binding (§2.4, VIS-1..4); principles as gates (§4); commercial-edition constraints (VIS-4, ENT-LIC-3). *Residual:* requires principled governance; the specification makes drift visible and reviewable.
- **RISK-C3. Fragmentation via incompatible forks or plugins (L: M, I: M).** Openness invites divergence. *Mitigation:* stable, versioned extension contract with compatibility kit (FR-PLUG-5/6); standards adherence (CMP-STD). *Residual:* forks are a right of open source; mitigated by a healthy core.
- **RISK-C4. Dependency abandonment (L: M, I: M).** A key dependency could be abandoned (the pdftk/GCJ lesson). *Mitigation:* dependency governance with named replacement seams (`[ADR-028]`, NFR-MAINT-3). *Residual:* migration cost if it occurs; bounded by abstraction.

## 15.8 Risk governance (Normative)

- **RISK-GOV-1.** Risks rated impact-H MUST have an owner and a tracked mitigation status throughout development.
- **RISK-GOV-2.** A mitigation that is a binding requirement MUST NOT be silently dropped; removing it REQUIRES a documented decision and re-assessment of the associated risk.

---

# 16. Future Roadmap

*Informative for sequencing; the phasing principle in §16.1 is normative.* This roadmap expresses product intent in versions. It aligns with, but is expressed at higher granularity than, the engineering milestones in `[SDS §14]`. Version boundaries are directional; capabilities may shift between versions as long as the phasing principle and scope governance hold.

## 16.1 Phasing principle (Normative)

- **ROAD-1.** Every released version MUST be a complete, usable, shippable application that delivers coherent value on its own; there MUST be no release that is merely infrastructure with no user-visible, acceptance-passing capability (`[SDS §14]`).
- **ROAD-2.** A capability MUST NOT ship until it meets its acceptance criteria and the relevant absolute metrics (§14); partial capabilities MUST be disclosed as such (PRIN-6).
- **ROAD-3.** Each version MUST maintain the vision commitments (§2.4), principles (§4), and the interface-stability contract (§11) established by prior versions.

## 16.2 Version 1 — "A viewer and toolkit professionals trust"

*Theme:* Establish the trust, performance, and reliability foundation with a complete, excellent viewer and the essential non-editing professional toolkit.
**Scope (Informative):**
- Best-in-class viewing, navigation, search, selection, and correct extraction (FR-VIEW, FR-NAV, FR-SRCH).
- Bookmarks, thumbnails, layers (view/toggle), attachments/embedded files (access) (FR-BOOK, FR-THUMB, FR-LAYER, FR-EMB).
- Robust open of malformed files with disclosed repair; diagnostics/inspection (FR-VIEW-2, FR-DIAG).
- Core annotation and commenting with portable appearances; review summary; annotation interop (FR-ANNOT, FR-REV).
- Page organization: merge, split, extract, insert, delete, reorder, rotate, crop (FR-ORG family).
- Open encrypted documents; basic printing; export to images/text/HTML (FR-SEC-1, FR-PRINT, FR-EXPORT-1).
- The mutation foundation: incremental save, unlimited undo, autosave, crash/torn-save recovery, version-history view and sanitize (FR-SAVE, FR-UNDO, FR-AUTOSAVE, FR-REC, FR-VER).
- Full application accessibility and accessible reading of tagged documents (NFR-A11Y, FR-A11Y-1/2/3).
- The command-line interface for the delivered scriptable operations, with GUI parity (FR-CLI).
- Established performance, reliability, memory, and interop metrics gating (§14).
**Exit definition (Informative):** an experienced Acrobat user can adopt V1 as their daily viewer/annotator/assembler/redactor-of-simple-cases with no training, trusting it never to lose or corrupt work, on all three platforms.

## 16.3 Version 2 — "A professional editor and securer"

*Theme:* Complete the professional editing, security, and forms surface; deliver the signature and redaction wedges.
**Scope (Informative):**
- Form filling with correct appearances and the JavaScript forms subset (FR-FORM, FR-JS); form data interop.
- Verifiable redaction with reporting, including batch (FR-RED).
- Digital signature validation (explainable) and software-certificate signing with timestamps and long-term validation data (FR-SIG).
- OCR with correct invisible-text-layer registration; searchable scans; batch and CLI (FR-OCR).
- Assembly finishing: watermarks, headers/footers, page numbering, Bates numbering (FR-STAMP); optimization/compression with disclosure (FR-OPT).
- PDF/A validation and export (FR-STD-1/2).
- Image and object editing (non-text) (FR-EDIT, early).
- Document comparison (visual + textual) (FR-CMP).
- Batch processing and savable pipelines (FR-BATCH).
- Metadata editing and richer sanitization (FR-META).
- Encryption creation and permissions with honest disclosure (FR-SEC, FR-PERM).
- Enterprise foundation: policy management, deployment tooling, LTS track, interface-profile pinning (§12).
**Exit definition (Informative):** V2 replaces Acrobat Pro for the majority of professional workflows in HR, finance, legal (redaction/Bates/compare/sign), and general office use.

## 16.4 Version 3 — "The complete platform"

*Theme:* Close the remaining Acrobat-Pro gaps and open the ecosystem.
**Scope (Informative):**
- In-place text editing with layout-preserving, honest-limitation discipline (FR-EDIT, PRIN-2, PRIN-6).
- Form authoring (FR-FORM-5).
- Hardware-token/qualified signing and highest signature-assurance profiles (FR-SIG-3, PAdES-LTA).
- PDF/UA validation and accessibility remediation tooling (FR-A11Y-4/5, FR-STD-4).
- Scanning acquisition (FR-SCAN).
- Prepress: imposition and PDF/X export with color intent (FR-PRINT-4, FR-STD-3).
- The public plugin ecosystem: stable versioned contract, capability model, compatibility kit, multi-language authoring (FR-PLUG).
- Office-format export at disclosed fidelity (FR-EXPORT-2).
**Exit definition (Informative):** V3 is a credible complete replacement for Acrobat Pro across professional workflows, with an ecosystem enabling capabilities beyond the core.

## 16.5 Long-term vision (beyond V3)

*Informative.* Directions that may be pursued under future-scope governance (§8.3), each requiring its own specification and preserving the vision commitments:
- Optional, self-hostable collaboration and shared-review service (FUT-1).
- Advanced local-inference accessibility auto-tagging (FUT-2).
- Full prepress color management (FUT-3).
- Optional, local, consented assistive features under strict constraints (FUT-5, OUT-10).
- Curated signed plugin registry (FUT-6).
- Deeper OS and workflow integrations respecting privacy and offline principles (FUT-7).

## 16.6 Versioning and deprecation policy (Normative)

- **ROAD-4.** User-facing contracts — the extension contract, saved-pipeline format, exported-data formats, and settings/profiles — MUST be versioned with published stability and deprecation policies; dependents MUST receive advance notice of deprecations sufficient to adapt (FR-PLUG-5, CMP-FWD-2).
- **ROAD-5.** The interface-behavior profile MUST be versioned; a supported profile MUST remain available per the stability contract and MUST NOT be removed without an explicit, publicly-reviewed decision (PRIN-4, ENT-UI-2, `[ADR-030]`).
- **ROAD-6.** Backward compatibility of the Platform's own produced data MUST be maintained or migratable without data loss (CMP-BACK-2).
- **ROAD-7.** Security fixes MUST be deliverable to supported and LTS tracks promptly and independently of feature releases (ENT-DEP-4).

---

# 17. Appendix

*Informative.*

## 17.1 Terminology and definitions (Glossary)

- **AcroForm.** The native PDF interactive-form model (fields and their on-page widgets). Distinct from XFA.
- **Annotation.** A markup or note object attached to a page (highlight, note, ink, shape, stamp, redaction mark, etc.).
- **Appearance (appearance stream).** The self-contained visual representation of an annotation or form field that determines how it renders across conformant readers. The Platform always writes complete appearances (FR-ANNOT-2).
- **Assistive technology (AT).** Software such as screen readers used by people with disabilities to operate the application and read documents.
- **Bates numbering.** Sequential identifying numbers applied across a set of pages/documents, used in legal discovery.
- **CLI.** Command-line interface; the headless, scriptable form of the Platform (FR-CLI).
- **Conformance (standards).** Meeting the requirements of a defined standard (e.g., PDF/A). The Platform validates and claims conformance only when accurate (FR-STD-5).
- **Cross-reference (xref).** The internal index a PDF uses to locate its objects; damage here is a common corruption class the Platform repairs (FR-VIEW-2).
- **Durability budget.** The maximum amount of committed but unsaved work that may be lost to a crash/power-loss/interrupted-save event; an absolute reliability target (MET-REL-3).
- **Extension / plugin.** Third-party-provided added capability running under the Platform's capability model and isolation (FR-PLUG).
- **Fidelity.** The degree to which the Platform's rendering matches the reference rendering of the same document (FR-VIEW-1, MET-FEAT-1).
- **Flatten.** To render interactive or layered content (e.g., form values, annotations, transparency) into static page content.
- **Forms subset (JavaScript).** The limited set of document-JavaScript behaviors (validation, calculation, formatting) the Platform executes; all other document scripting is excluded (FR-JS, OUT-9).
- **Incremental save / update.** Saving by appending changes while preserving prior content byte-for-byte and existing signatures where permitted; the Platform's default save (FR-SAVE-1, `[ADR-012]`).
- **Interoperability.** Correct exchange of documents and their data with other PDF software (§13, PRIN-7).
- **Leniency (repair) ledger.** The record of deviations tolerated and repairs made when opening a damaged file, disclosed to the user (FR-DIAG-1, PRIN-6).
- **Long-term support (LTS).** A release track receiving security/critical fixes over an extended period without forced feature/interface change (ENT-DEP-4).
- **Optional content group (OCG) / layer.** A named group of content whose visibility can be toggled (FR-LAYER).
- **PAdES.** Recognized profiles for PDF Advanced Electronic Signatures, including long-term-validation levels (FR-SIG-3, CMP-STD-3).
- **PDF/A, PDF/X, PDF/UA.** ISO standards for archival, prepress, and accessible PDFs respectively (FR-STD).
- **Permissions.** PDF flags requesting restrictions (print, copy, edit); advisory, not a security guarantee — disclosed as such (FR-PERM-1, PRIN-6).
- **Portfolio / collection.** A PDF that packages multiple files (FR-PORT).
- **Preflight.** Checking a document against production or archival criteria and reporting issues (FR-STD, US-PUB-2).
- **Redaction.** Permanent removal of content such that it is not recoverable; the Platform verifies removal (FR-RED).
- **Reference application set.** The specific external products against which interoperability is tested (§13.2).
- **Remediation (accessibility).** Correcting a document's tags, reading order, and alternative text to meet accessibility standards (FR-A11Y-4).
- **Sanitize.** Removing hidden data, retained history, and identifying metadata, with disclosure of what remains (FR-VER-2, FR-META-2).
- **Tagged PDF.** A PDF carrying a logical-structure tree enabling accessible reading and reliable extraction (FR-A11Y).
- **Telemetry.** Automated collection of usage/diagnostic data; off by default and opt-in only (NFR-PRIV-2).
- **Trust (signatures).** Configured basis for deciding a signature's certificate is trustworthy; unverifiable trust yields indeterminate results (FR-SIG-1, ENT-CERT-4).
- **XFA.** A deprecated XML-based forms technology; out of scope for full support, detected and disclosed (OUT-1).

## 17.2 Requirement-level key words

As defined in the Document Conventions: MUST/SHALL, MUST NOT, SHOULD, SHOULD NOT, MAY/OPTIONAL, per RFC 2119/8174.

## 17.3 Persona index

Casual user (§6.1.1); Student (§6.1.2); Office worker (§6.1.3); HR professional (§6.2.1); Accountant/finance (§6.2.2); Lawyer/legal (§6.2.3); Engineer (§6.2.4); Architect (§6.2.5); Researcher/academic (§6.2.6); Publisher/production editor (§6.2.7); Print shop/prepress (§6.2.8); Government/public sector (§6.2.9); Enterprise IT administrator (§6.3.1); Developer/automation engineer (§6.3.2); Plugin author (§6.3.3); Accessibility user (§6.3.4); Contributor (§6.3.5).

## 17.4 Requirement-area index

FR-VIEW, FR-NAV, FR-SRCH(/-IDX), FR-BOOK, FR-THUMB, FR-LAYER, FR-EMB, FR-FORM, FR-JS, FR-ANNOT, FR-REV, FR-MEAS, FR-ORG/SPLIT/MERGE/EXTRACT/INSERT/ROTATE/CROP, FR-OPT, FR-RED, FR-SIG, FR-OCR/SCAN, FR-A11Y, FR-STD, FR-PRINT, FR-BATCH, FR-CLI, FR-PLUG, FR-CMP, FR-PORT, FR-MEDIA, FR-IMPORT/EXPORT, FR-SEC/PERM/META, FR-VER/AUTOSAVE/REC/UNDO/SAVE, FR-CLOUD, FR-DIAG (§9); NFR-PERF/RESP/MEM/START/LARGE/REL/AVAIL/SEC/PRIV/A11Y/LOC/SCALE/MAINT/COMPAT/OFFLINE (§10); UX-* (§11); ENT-* (§12); CMP-* (§13); MET-* (§14).

## 17.5 References (Informative)

- Companion project documents: *Engineering Constitution* (ADR-001 … ADR-030); *System Design Specification* (SDS). Authoritative for implementation.
- ISO 32000 (PDF); ISO 19005 (PDF/A); ISO 15930 (PDF/X); ISO 14289 (PDF/UA); ETSI EN 319 142 (PAdES). Cited as the standards the Platform targets; the project maintains the specific version references in its engineering documentation.
- RFC 2119 and RFC 8174 (requirement-level key words).
- Research corpus underlying this product (competitive, format, user-research, and opportunity analyses) is maintained with the project and informs §5, §6, §8, and §15.

## 17.6 Document control (Informative)

- This PRD is a living document under change control. Material changes MUST preserve requirement identifiers (withdrawn requirements are marked, not reused) and MUST be reconciled with the ADR/SDS where they touch implementation-governing decisions.
- Where this PRD introduced new product decisions, they are marked **[PRD Decision]** and are the items most warranting stakeholder ratification: the enterprise interface-profile pinning (ENT-UI, §12.6) and the initial reference metric targets (§14).
- Open items for ratification alongside this baseline: confirmation of the initial metric targets (§14) against early-prototype measurements; confirmation of the reference-application set membership (§13.2); and confirmation of the out-of-scope exclusions (§8.2), particularly the XFA posture (OUT-1) and the AI posture (OUT-10).

*End of Product Requirements Document (baseline).*
