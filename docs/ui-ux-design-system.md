# UI/UX Design System

**Product:** Open-source professional PDF platform (working name: *the Platform*)
**Document class:** Canonical UX specification. Governs the product's appearance, behavior, and interaction model for a 10+ year horizon.
**Companion documents (authoritative; not restated here):** *Engineering Constitution* (`[ADR-001 … ADR-030]`), *System Design Specification* (`[SDS §N]`), *Product Requirements Document* (`[PRD]`, cited as requirement IDs such as `FR-VIEW-1`, `UX-KEY-2`, `PRIN-4`). Where this document specifies *how the product looks and behaves*, it governs. Where a behavior is mandated by the PRD, this document specifies its concrete presentation; where an implementation mechanism is fixed by the ADR/SDS, this document does not contradict it.
**Audience:** Product Designers, Frontend/Qt Engineers, QA Engineers, Accessibility Specialists, Plugin Developers, Documentation Writers.

---

## Document Conventions (Normative)

Requirement key words **MUST / MUST NOT / SHALL**, **SHOULD / SHOULD NOT**, **MAY / OPTIONAL** follow RFC 2119 / RFC 8174. A release that violates a MUST is non-conformant. Deviation from a SHOULD REQUIRES a documented, reviewed rationale.

Each specified rule carries a stable identifier of the form `DS-<area>-<n>` (design-system). Identifiers are permanent; a withdrawn rule is marked *Withdrawn*, never reused.

**Normative** text defines conformance. **Informative** text (marked *Informative*) explains or motivates.

**[UX Decision]** marks a UX decision first made in this document (as opposed to one inherited from the PRD or the ADR/SDS). These are the items most warranting design-leadership ratification.

**Determinism standard (DS-CONV-1, Normative).** Every interaction, dimension, color, timing, and state in this document MUST be specified precisely enough that two independent designers, given this document, produce interfaces that are indistinguishable in behavior and near-indistinguishable in appearance. Where a value is intentionally a range, the range and its selection rule MUST be stated. "Looks good," "as appropriate," and similar non-testable phrasing are prohibited in normative text.

**Token discipline (DS-CONV-2, Normative).** No component specification may hard-code a raw pixel, color, duration, or font value that is available as a design token (§12). Components reference tokens; tokens hold values. This is the mechanism by which density modes, themes, and DPI scaling remain consistent (§2, §12). When a component spec below cites a concrete number, that number is the *token's value at the reference density (Comfortable) and reference scale (100%)*, and the component MUST consume it through the token, not the literal.

**Units (DS-CONV-3, Normative).** All spatial values are expressed in **density-independent points (dp)** unless explicitly stated in physical pixels (px). 1 dp = 1 physical px at 100% display scale; dp values scale with display scale and DPI (§2.15, `UX-DPI-1`). Type sizes are in points (pt) mapped to dp. Durations are in milliseconds (ms).

**Relationship to the interface-stability contract (DS-CONV-4, Normative).** This document *is* the concrete expression of `PRIN-4` and the interface-behavior profile of `ENT-UI` / `[ADR-030]`. Any change to a shortcut, a menu location, a default behavior, or a focus order defined here is a change to the stability contract and is governed by §14 and the versioning rules of `[ADR-030]`. Designers MUST treat the values here as contractual, not advisory.

---

# 1. Design Philosophy

*Normative. These principles are the tie-breakers for every subsequent decision. Where two rules tension, the earlier-numbered principle wins, and the resolution MUST be recorded.*

The philosophy restates, at the level of *interface behavior*, the product principles of `[PRD §4]`. It does not replace them; it operationalizes them for designers and front-end engineers.

## 1.1 Familiarity over novelty (DS-PHIL-1)
The interface MUST reproduce the mental model, spatial layout, command vocabulary, and default shortcuts an experienced Adobe Acrobat Pro user already possesses, and MUST invest novelty only where the incumbent is measurably deficient (`PRIN-4`, `PRD §5.2`). *Design consequence:* when choosing between a familiar-but-imperfect arrangement and a novel-but-"better" one, the designer MUST choose the familiar arrangement unless the novel one clears the bar in DS-PHIL-9 (measured-deficiency exception). Novelty for differentiation is prohibited; differentiation comes from speed, reliability, and honesty, not from re-teaching the user.

## 1.2 Speed over decoration (DS-PHIL-2)
Every visual element MUST justify its cost against the performance budgets of `[PRD §14]` and `[ADR-023]`. Decoration that costs frame time, input latency, or memory without a functional purpose is prohibited. *Design consequence:* shadows, blurs, translucency, and animation are used only where they communicate structure or state (§2.4, §11). The default aesthetic is calm, flat, and legible, not ornamented. A visual effect MUST be removable by the reduced-motion and high-contrast paths without loss of function (§9).

## 1.3 Predictability / determinism (DS-PHIL-3)
The same input in the same context MUST always produce the same result and the same visible feedback. There MUST be no hidden mode whose existence the user cannot perceive, no action whose outcome depends on invisible state, and no control that behaves differently based on timing the user cannot observe. *Design consequence:* every mode has a persistent, visible indicator (§4.1, §5.4); every destructive action has a consistent confirmation pattern (§6.10); every asynchronous action has a consistent progress and completion signal (§8, §11).

## 1.4 Accessibility first (DS-PHIL-4)
Accessibility is a precondition of "done," not a later pass (`PRIN-8`, `NFR-A11Y`). A component that is not keyboard-operable and screen-reader-correct is not shippable. *Design consequence:* every component spec in §6 includes an Accessibility clause that is normative and gating; a visual design that cannot meet §9 MUST be redesigned, not exempted.

## 1.5 Keyboard first (DS-PHIL-5)
Every function MUST be operable from the keyboard alone, and the keyboard path MUST be efficient for expert users, not merely possible (`UX-KEY-1`). *Design consequence:* the design defines focus order, shortcuts, and keyboard affordances *before* it defines pointer conveniences. Mouse, pen, and touch are additive to a complete keyboard model, never the only way to reach a function.

## 1.6 Progressive disclosure (DS-PHIL-6)
Complexity MUST be revealed in proportion to the user's engagement with a task. Common actions are always visible; advanced options are one predictable step away; rare options live in inspectors and dialogs. *Design consequence:* the default surface (§3) shows the professional's daily tools without clutter; depth is reachable through consistent disclosure patterns (property panels, "more" affordances, advanced dialog sections) that never relocate the common case to make room for the rare one (DS-PHIL-1).

## 1.7 Professional workflows (DS-PHIL-7)
The interface MUST optimize for repeated, expert, high-volume use, not for first-run delight at the expense of daily efficiency. *Design consequence:* step counts for common workflows MUST meet or beat the incumbent (`UX-INT-1`, `MET-PROD-2`); modality that interrupts flow is minimized; batch and keyboard paths are first-class (§7). First-run guidance (§7.1, §8 empty states) MUST be dismissible and MUST never impose recurring friction on experts.

## 1.8 Zero-surprise UX (DS-PHIL-8)
The product MUST never surprise the user with a change to their content, their settings, their interface, or their data (`PRIN-2`, `PRIN-4`, `PRIN-6`). *Design consequence:* nothing that modifies the document happens without a visible, undoable, attributable action (§7); the interface layout and shortcuts do not change under the user without opt-in (§3.19, §14); errors and tolerated conditions are disclosed honestly (§8); no dark patterns, no upsell, no attention-engineering (DS-PHIL-10).

## 1.9 Measured-deficiency exception (DS-PHIL-9)
A departure from incumbent familiarity (DS-PHIL-1) is permitted only when all of the following hold, and the justification MUST be recorded as a `[UX Decision]`: (a) the incumbent behavior is measurably deficient (step count, error rate, latency, discoverability failure, or accessibility failure) with evidence; (b) the replacement is demonstrably better on that measure without regressing others; and (c) the prior behavior remains available via an opt-in compatibility path where feasible (`ENT-UI`, `[ADR-030]`). Absent all three, familiarity wins.

## 1.10 Anti-dark-pattern mandate (DS-PHIL-10) [UX Decision]
The interface MUST NOT employ any pattern designed to manipulate the user against their interest: no confirm-shaming, no disguised ads, no false urgency, no obstructed cancellation, no pre-checked consent, no nagging, no attention-maximizing engagement mechanics, no account or cloud coercion (`VIS-1`, `VIS-2`, `PRD §5.8`). *Rationale (Informative):* the product exists in part as a reaction to incumbent commercial practices; the interface is the primary surface where such practices would appear, so the prohibition is stated here as a first-order design law. Consent prompts (§8, §10) MUST present balanced, equally-weighted choices with the privacy-preserving option never disadvantaged in prominence.

## 1.11 Precedence (DS-PHIL-11, Normative)
When principles conflict: **safety and integrity of the user's data** (DS-PHIL-8's non-destruction aspect, from `PRIN-2`) and **accessibility** (DS-PHIL-4) are never traded away. Below those, the order is: predictability (DS-PHIL-3) → keyboard/operability (DS-PHIL-5) → familiarity (DS-PHIL-1) → progressive disclosure (DS-PHIL-6) → speed (DS-PHIL-2, as a gate not a look) → visual refinement. Interface stability (`PRIN-4`) overrides all except data safety and accessibility, and may be overridden only by explicit user/administrator opt-in.

---

# 2. Visual Language

*Normative unless marked. All concrete values are the reference-density (Comfortable), reference-scale (100%), light-theme values, expressed through tokens (§12). Other densities, scales, and themes derive by the rules in this section and §12.*

## 2.1 Design ethos (Informative)
The visual language is **quiet, dense, and legible**: a professional instrument that recedes so the document dominates. It draws from the restraint of a modern IDE and the density of a professional creative tool, not from consumer-app exuberance. Color is used sparingly and semantically; the neutral palette carries the interface; the document is the only place saturated color appears by default.

## 2.2 Typography

**DS-TYPE-1 (UI typeface).** The interface MUST use the host platform's system UI typeface by default (Segoe UI Variable / the current Windows system font on Windows; SF Pro / system on macOS; the distribution's default UI font, targeting Inter or the system's equivalent, on Linux). *Rationale:* native familiarity and rendering quality (`UX-CONS-2`). A bundled fallback (Inter) MUST be available where the platform font is absent. **[UX Decision]** Inter is the canonical cross-platform fallback and the design reference metric.

**DS-TYPE-2 (Monospace typeface).** Monospaced contexts (object inspector, structure view, CLI-equivalent output, hex/technical fields) MUST use the platform monospace font (Cascadia Code / Consolas; SF Mono; the distribution default or JetBrains Mono as fallback).

**DS-TYPE-3 (Type scale).** The UI type scale is defined as tokens (§12.1) and MUST be used exclusively; arbitrary sizes are prohibited. Reference values (Comfortable, 100%):

| Token | Size / Line-height (dp) | Weight | Use |
|---|---|---|---|
| `type.caption` | 11 / 16 | Regular | Secondary labels, status bar, metadata |
| `type.body` | 13 / 20 | Regular | Default UI text, list items, menu items |
| `type.body-strong` | 13 / 20 | Semibold | Emphasis, selected labels |
| `type.subtitle` | 15 / 22 | Semibold | Panel titles, section headers |
| `type.title` | 20 / 28 | Semibold | Dialog titles |
| `type.display` | 28 / 36 | Semibold | Empty-state headlines only |

**DS-TYPE-4.** Body UI text MUST NOT render below 11 dp effective size at any density mode. **DS-TYPE-5.** The interface MUST respect the OS "large text"/text-scaling setting, scaling UI type by the OS factor up to at least 200% without truncation or overlap (`UX-A11Y-3`, §9). **DS-TYPE-6.** Font smoothing/hinting MUST follow platform conventions; the interface MUST NOT override subpixel/grayscale AA choices the OS makes. **DS-TYPE-7.** Numeric fields displaying tabular data (measurements, page numbers, coordinates) SHOULD use tabular (monospaced-figure) number rendering where the font supports it, for column stability.

**DS-TYPE-8 (Truncation).** Text that cannot fit MUST truncate with an ellipsis at the end (or middle, for file paths) and MUST expose the full text via tooltip (§6.19) and to assistive technology. Truncation MUST NOT drop the semantically important end of a string without middle-ellipsis (paths, IDs).

## 2.3 Spacing and grid

**DS-SPACE-1 (Base unit).** The spacing system is built on a **4 dp base grid**. All margins, paddings, and gaps MUST be multiples of 4 dp, expressed as spacing tokens (§12.2): `space.0=0`, `space.1=2` (half-step, exceptional use), `space.2=4`, `space.3=8`, `space.4=12`, `space.5=16`, `space.6=20`, `space.7=24`, `space.8=32`, `space.9=40`, `space.10=48`. The 2 dp half-step is permitted only for icon-to-label gaps and hairline-adjacent alignment.

**DS-SPACE-2 (Component internal padding).** Default control horizontal padding is `space.4` (12 dp) at Comfortable; vertical rhythm derives from the control height tokens (§2.14). **DS-SPACE-3 (Panel padding).** Panel content insets are `space.5` (16 dp) at Comfortable, `space.4` (12) at Compact, `space.6` (20) at Spacious. **DS-SPACE-4 (Grid alignment).** Related controls MUST align to a shared 4 dp grid line; label columns in property grids MUST share a common measure (§6.7).

**DS-SPACE-5 (Layout grid).** The application shell uses a **fixed-plus-fluid** column model (§3): side panels have token-defined default and min/max widths; the canvas is fluid and absorbs remaining space. There is no fixed 12-column marketing grid; this is an application, not a page.

## 2.4 Elevation and shadow

**DS-ELEV-1.** Elevation communicates stacking and transience, not decoration (DS-PHIL-2). The system defines discrete elevation levels as tokens (§12.5):

| Token | Level | Use | Shadow (light) |
|---|---|---|---|
| `elev.0` | Base | Canvas, docked panels, inline surfaces | none; 1 dp hairline border where separation is needed |
| `elev.1` | Raised | Cards, list hover raise (optional) | y=1, blur=2, 8% |
| `elev.2` | Overlay | Dropdowns, popovers, tooltips, context menus | y=2, blur=8, 14% |
| `elev.3` | Dialog | Modal dialogs, floating panels | y=8, blur=24, 20% |
| `elev.4` | Transient-top | Drag ghosts, active drag-over | y=12, blur=32, 24% |

**DS-ELEV-2.** Docked, tiled surfaces MUST use `elev.0` with hairline borders, not shadows, to keep the dense interface flat and fast (DS-PHIL-2). Shadows appear only on genuinely floating surfaces (`elev.2`+). **DS-ELEV-3.** In High-Contrast themes, shadows MUST be replaced by solid borders of the appropriate semantic border token (§9.3); elevation MUST remain perceivable without relying on shadow. **DS-ELEV-4.** Shadow parameters are tokens; components MUST NOT specify bespoke shadows.

## 2.5 Border and radius

**DS-RADIUS-1.** Corner radii are tokens (§12.3): `radius.none=0`, `radius.sm=3`, `radius.md=5`, `radius.lg=8`, `radius.round=9999`. Controls (buttons, inputs, chips) use `radius.sm`; containers (cards, dialogs, panels' floating form) use `radius.md`; large surfaces and images use `radius.lg`; fully round only for avatars and status dots. **DS-RADIUS-2.** Radius MUST be consistent within a control family; mixing radii within one composite control is prohibited. **DS-BORDER-1.** Borders are 1 dp by default (`border.width.hairline`), rendered crisply at all DPI (§2.15). A 2 dp border (`border.width.emphasis`) is reserved for focus (§4.2) and selected-emphasis states.

## 2.6 Iconography

**DS-ICON-1 (Grid & style).** Icons MUST be drawn on a **16 dp** base grid (primary UI size) with **20 dp** and **24 dp** variants, using a consistent **1.5 dp stroke** at 16 dp (scaling proportionally), **outlined** style (not filled) as the default, with filled variants reserved for selected/active toggle states. Corners follow a 1 dp icon-internal radius for optical consistency. **[UX Decision]** Outline-default/filled-active is the canonical icon state model.

**DS-ICON-2 (Sizes).** Icon size tokens: `icon.sm=16`, `icon.md=20`, `icon.lg=24`, `icon.xl=32` (empty states only). Toolbar icons are `icon.md` (20) at Comfortable, `icon.sm` (16) at Compact. **DS-ICON-3 (Alignment).** Icons MUST be optically centered within their touch/click target, not merely geometrically centered. **DS-ICON-4 (Color).** Icons inherit the current text/semantic color by default (monochrome). Multicolor icons are prohibited in the chrome except for brand/file-type/status semantics where color is meaningful (e.g., signature-valid green check). **DS-ICON-5 (Metaphor stability).** Icon metaphors for actions that exist in Acrobat MUST preserve the recognizable metaphor (DS-PHIL-1); a redesigned glyph MUST retain the same conceptual referent. **DS-ICON-6 (Rendering).** Icons MUST be vector, hinted or snapped to the pixel grid at 16/20/24 to stay crisp; MUST render correctly in all themes by using currentColor/semantic tokens; MUST have a non-color-dependent meaning (§9.5).

**DS-ICON-7 (Accessibility of icons).** Icon-only controls MUST have an accessible name and a tooltip (§6.19, §9). A meaning conveyed by an icon MUST also be available as text or state to assistive technology.

## 2.7 Illustrations

**DS-ILLUS-1.** Illustrations appear only in empty states (§8.1), first-run, and the about surface. They MUST be restrained, monochrome-or-duotone using palette tokens, and MUST NOT be whimsical to the point of undermining the professional tone (DS-PHIL-7). **DS-ILLUS-2.** Every illustration MUST be decorative-only (marked as such to assistive tech) or carry an accessible description if informative. **DS-ILLUS-3.** Illustrations MUST have High-Contrast and dark-theme variants or be tinted via tokens so they remain legible (§9).

## 2.8 Color system — architecture

**DS-COLOR-1 (Layered model).** Color is defined in three layers (§12.9): (1) a **primitive palette** (raw hues and neutrals, never referenced directly by components); (2) **semantic tokens** (e.g., `color.bg.canvas`, `color.text.primary`, `color.accent.default`, `color.status.danger`) that components reference; (3) **component tokens** where a component needs a specific role. Components MUST reference layer 2 or 3, never layer 1 (DS-CONV-2). Themes are produced by remapping layers 1→2; components are theme-agnostic.

**DS-COLOR-2 (Neutrals carry the UI).** The chrome MUST be built from a neutral gray ramp; saturated accent color is reserved for (a) the primary action affordance, (b) selection, (c) focus, and (d) semantic status. Large fields of saturated color in the chrome are prohibited (DS-PHIL-2). **DS-COLOR-3 (Single accent).** There is one accent hue, used for primary actions, selection highlight, and active states, defined as `color.accent.*`. **[UX Decision]** The reference accent is a blue at a hue that remains distinguishable under the common color-vision deficiencies when paired with the mandated non-color cues (§9.5); the exact primitive is fixed in §12.9.

## 2.9 Semantic colors

**DS-COLOR-4.** Semantic status colors are tokens with fixed roles, each paired with a mandatory non-color cue (icon/shape/text) per §9.5:

| Semantic token | Role | Non-color cue |
|---|---|---|
| `color.status.success` | Valid signature, passed validation, completed | check glyph |
| `color.status.warning` | Caution, indeterminate, needs attention | triangle glyph |
| `color.status.danger` | Error, invalid signature, failed, destructive | octagon/x glyph |
| `color.status.info` | Neutral information, tips | i glyph |
| `color.status.progress` | In-progress, running | spinner/bar |

**DS-COLOR-5.** Semantic colors MUST meet contrast requirements against their backgrounds in all themes (§9.2). **DS-COLOR-6.** A semantic color MUST NOT be used decoratively; if content is green it means success somewhere in the system's vocabulary. This consistency is contractual (DS-PHIL-3).

## 2.10 Document-vs-chrome color separation (DS-COLOR-7) [UX Decision]
The document canvas is a **color sanctuary**: the chrome MUST NOT tint, overlay, or color-cast the rendered document except for explicit, user-invoked view aids (§7, `FR-VIEW-6`). Selection, annotation, and measurement overlays drawn *over* the document use defined overlay tokens (§5) with sufficient contrast against arbitrary document content (§5.7). This separation guarantees the user always sees true document color (`PRIN-2`, prepress/architect personas).

## 2.11 Dark mode

**DS-DARK-1.** A dark theme MUST be provided and MUST be a full re-map of semantic tokens, not an inverted screenshot. **DS-DARK-2.** Dark theme MUST follow the platform preference by default and be overridable in settings (`UX-VIS-1`). **DS-DARK-3.** In dark theme, elevation is communicated by *surface lightening* (higher surfaces are lighter) in addition to (reduced) shadow, per token (§12.9). **DS-DARK-4.** The document canvas background in dark theme MUST default to the true document color (typically the page's own white), NOT a darkened page, unless the user explicitly enables a night/inverted reading aid (`FR-VIEW-6`, DS-COLOR-7). The area *around* pages (the pasteboard) MAY be dark. **DS-DARK-5.** Contrast ratios (§9.2) MUST be met independently in dark theme; dark is not exempt.

## 2.12 Light mode
**DS-LIGHT-1.** Light theme is the reference theme in which all tokens' primary values are specified. **DS-LIGHT-2.** The pasteboard (area around pages) in light theme is a neutral mid-gray (`color.bg.pasteboard`) chosen so that white pages read as white by simultaneous contrast and page edges are visible without heavy shadow (§5.2).

## 2.13 High-contrast
**DS-HC-1.** A High-Contrast theme MUST be provided and MUST follow the OS high-contrast setting where the platform exposes one (Windows Contrast Themes especially) (`UX-A11Y-3`, §9.3). **DS-HC-2.** In High-Contrast, all meaning conveyed by subtle color or shadow MUST be re-expressed with borders, explicit fills, and system-defined high-contrast palette roles. **DS-HC-3.** Focus indicators (§4.2) MUST be even more prominent in High-Contrast (≥3 dp, system highlight color). **DS-HC-4.** Non-essential decorative color and imagery MUST be suppressed in High-Contrast.

## 2.14 Density modes

**DS-DENSITY-1.** The interface MUST provide three density modes as a user setting: **Compact**, **Comfortable** (default), and **Spacious**. Density is a global multiplier set applied through tokens; components MUST derive their metrics from density-aware tokens (DS-CONV-2), not fixed values. **[UX Decision]** Three named densities with Comfortable as default.

**DS-DENSITY-2 (Control heights).** Reference primary control heights by density:

| Element | Compact | Comfortable | Spacious |
|---|---|---|---|
| Toolbar height | 32 | 40 | 48 |
| Menu/list item height | 24 | 28 | 34 |
| Button height (default) | 26 | 30 | 36 |
| Input height | 26 | 30 | 36 |
| Row height (tables/trees) | 24 | 28 | 32 |
| Tab height | 30 | 36 | 42 |

**DS-DENSITY-3.** Density MUST NOT change *which* elements are present or *where* they are (that would violate DS-PHIL-1/3); it changes only sizing and spacing. **DS-DENSITY-4.** Touch-primary environments (§4.11) SHOULD default to Spacious to meet touch-target minimums (§9.8) but MUST remain user-overridable. **DS-DENSITY-5.** Density changes MUST apply immediately and MUST persist per user (§3.19).

## 2.15 Window chrome and High-DPI

**DS-CHROME-1.** The application MUST use native window chrome (title bar, window controls, resize borders, snap behavior) per platform, integrating with platform window management (`UX-CONS-2`, `UX-MULTI`). Custom-drawn title bars MAY be used only if they preserve every native behavior (snap, double-click-to-maximize, system menu, accessibility) and the platform encourages it; when in doubt, native chrome wins (DS-PHIL-1). **DS-CHROME-2.** The menu bar placement MUST follow platform convention: in-window on Windows/Linux; system menu bar on macOS (§3.2). **DS-CHROME-3 (High-DPI).** All interface elements MUST render crisply at any display scale including fractional scaling, and MUST update within one frame when the window moves to a display of different scale (`UX-DPI-1/2`, `CMP-XPLAT`). Hairline borders MUST remain exactly 1 physical px where the design intends a hairline, snapping to the device pixel grid. **DS-CHROME-4.** Icons and raster assets MUST be provided or rendered at the target scale; no upscaled blur is permitted (`FR-VIEW-5`). **DS-CHROME-5.** Mixed-DPI multi-monitor moves MUST not misplace popups, drag ghosts, or hit targets (`UX-MULTI-1`).

## 2.16 Motion principles (summary; full system §11)
**DS-MOTION-1.** Motion MUST be functional (communicating change, continuity, or causality), fast, and interruptible (§11). **DS-MOTION-2.** Motion MUST honor the OS reduced-motion setting by replacing movement with instant or cross-fade transitions (§9.6, §11.2). **DS-MOTION-3.** No interface animation may delay the user's ability to act; animations are non-blocking and skippable (DS-PHIL-2).

---

# 3. Layout System

*Normative.* This section defines the application shell and every panel and surface it contains. Panel *behavior* (docking, resize, persistence) is specified once here (§3.14–§3.19) and inherited by each panel. Panel *content* is specified per panel and, for reusable widgets within them, in §6.

## 3.1 Application shell — anatomy

**DS-SHELL-1.** The shell MUST be composed of these regions, in this spatial order (top to bottom, then within the middle band left to right):

1. **Window chrome** (native, §2.15) — title bar with document title and window controls.
2. **Menu bar** (§3.2) — top of window (Windows/Linux) or system bar (macOS).
3. **Quick Access Toolbar (QAT)** (§3.5) — a compact, user-customizable strip; by default co-located with the toolbar row.
4. **Primary toolbar / command surface** (§3.4, with optional Ribbon mode §3.3) — the main tool and action surface, mode-sensitive to the active tool context.
5. **Document tab strip** (§3.11) — when more than one document is open, or always if the user pins it.
6. **Middle band** — left navigation rail + left panel group (§3.6–§3.8) · **canvas** (§5) · right panel group (properties/comments/inspector, §3.9–§3.10).
7. **Status bar** (§3.13) — bottom, full width.

**DS-SHELL-2.** The canvas MUST always be present and MUST be the largest region; panels are subordinate and collapsible (DS-PHIL-6). **DS-SHELL-3.** The shell layout (which panels are open, their sizes, docking) MUST persist per user and per — [UX Decision] — *workspace* (§3.18). **DS-SHELL-4.** No region except the canvas may be non-collapsible; the user MUST be able to reach a "just the document" state (all chrome minimized) via a single command (default `F8` toggles panels; full-screen reading is `Ctrl/Cmd+L` presentation-adjacent — see §7). **DS-SHELL-5 (Performance).** Shell layout changes (open/close/resize panel) MUST render within 1 frame at the target budget and MUST NOT trigger a document re-render (`SDS §6.6`).

## 3.2 Menu bar

**Purpose.** The complete, discoverable command taxonomy; the authoritative home of every command (DS-PHIL-6, discoverability `UX-DISC-1`).

**DS-MENU-1.** A full menu bar MUST exist with the top-level structure (order fixed for familiarity, DS-PHIL-1): **File, Edit, View, Document, Tools, Comment, Forms, Sign, Advanced, Window, Help.** *(Rationale/mapping to Acrobat is Informative; the taxonomy is chosen to map recognizably onto Acrobat's while being cleaner.)* **[UX Decision]** This top-level menu taxonomy is contractual (§14).
**DS-MENU-2.** Every command in the product MUST appear in exactly one primary menu location (a single canonical home), and MAY additionally appear in context menus, toolbar, and command search. **DS-MENU-3.** Menu items MUST show their keyboard shortcut, an enabled/disabled state reflecting context, and a leading icon where the command has one (consistency with toolbar). **DS-MENU-4.** Destructive items MUST be visually distinguished (danger semantic, §2.9) and MUST NOT be adjacent to their common non-destructive sibling in a way that invites misclick (§6.10). **DS-MENU-5.** Menus MUST be fully keyboard navigable (§6.2) and screen-reader correct (menu/menuitem roles, §9). **DS-MENU-6 (macOS).** On macOS the menu bar MUST be the system menu bar with platform-standard app menu, and Windows/Linux in-window menu MUST be reachable via `Alt` with mnemonics.

## 3.3 Ribbon mode (optional)

**DS-RIBBON-1.** The product MUST offer two command-surface modes, user-selectable: **Toolbar mode** (default, §3.4) and **Ribbon mode** (optional). **[UX Decision]** Ribbon is opt-in, not default; the default is the lighter toolbar to serve speed and familiarity for Acrobat users, while Ribbon serves users migrating from Office-style expectations and discoverability.
**DS-RIBBON-2.** In Ribbon mode, commands MUST be grouped into contextual tabs mirroring the menu taxonomy (§3.2); no command may exist in the Ribbon that lacks a menu home (DS-MENU-2). **DS-RIBBON-3.** Switching modes MUST NOT change command availability, only presentation (DS-PHIL-3), and MUST be reversible without data or state loss. **DS-RIBBON-4.** Ribbon MUST be collapsible to a single row and MUST honor density (§2.14). **DS-RIBBON-5.** Ribbon and Toolbar modes MUST both meet all accessibility and keyboard requirements equally.

## 3.4 Primary toolbar / command surface

**Purpose.** Fast access to the most-used tools and actions for the current context (DS-PHIL-7).

**DS-TOOLBAR-1.** The toolbar MUST present the common global actions (open, save, print, undo/redo, search) plus a **contextual tool group** that reflects the active tool mode (e.g., selecting the Comment toolset surfaces annotation tools). Context changes MUST be announced (§9) and visually obvious (DS-PHIL-3). **DS-TOOLBAR-2.** Toolbar contents MUST be user-customizable (add/remove/reorder) within defined groups, and customization MUST persist and be resettable to default (§3.19). **DS-TOOLBAR-3.** Overflow: when the toolbar exceeds available width, excess items MUST collapse into a clearly-marked overflow menu (never silently disappear) (DS-PHIL-8). **DS-TOOLBAR-4.** Each toolbar control follows the Button/Toggle/Split-button specs (§6.1) including tooltip, disabled, and active states. **DS-TOOLBAR-5.** Toolbar height and icon size follow density tokens (§2.14, §2.6). **DS-TOOLBAR-6 (Keyboard).** The toolbar MUST be reachable via a documented shortcut (default `F10` moves focus to the toolbar/command surface) and navigable by arrow keys with roving tabindex (§6.2, §9).

## 3.5 Quick Access Toolbar (QAT)

**Purpose.** A tiny, always-visible, fully user-curated set of the user's chosen commands (expert efficiency, DS-PHIL-7).

**DS-QAT-1.** The QAT MUST allow the user to pin any command to a compact strip that is always visible regardless of tool context. **DS-QAT-2.** Default QAT contents: Save, Undo, Redo, Print (mirroring a familiar minimal set); user-editable. **DS-QAT-3.** QAT position MUST be user-selectable between the title-bar-adjacent position and above/below the toolbar, per platform capability. **DS-QAT-4.** QAT customization persists per user and is part of the workspace (§3.18).

## 3.6 Left navigation rail

**Purpose.** Switch which left panel is shown (thumbnails, bookmarks, layers, attachments, signatures, search results), analogous to Acrobat's navigation pane buttons.

**DS-RAIL-1.** A vertical icon rail MUST list the available left panels; activating an item shows that panel in the left panel group (§3.7) and toggles it if already active. **DS-RAIL-2.** The rail MUST show which panel is active (selected/active toggle state, §6.1) and MUST be keyboard navigable (roving tabindex, §6.2). **DS-RAIL-3.** Rail items MUST have tooltips and accessible names (§9). **DS-RAIL-4.** The set of rail items is context-sensitive: panels irrelevant to the current document (e.g., Signatures when none exist) MAY be de-emphasized but MUST remain discoverable (a document with no signatures still shows a Signatures panel explaining none exist — honesty, DS-PHIL-8, §8.1 empty state). **DS-RAIL-5.** The rail itself MUST be collapsible; collapsing it does not lose the panels (they remain reachable via View menu and shortcuts).

## 3.7 Left panel group (navigation panels)

The left panel group hosts one visible panel at a time (tabbed/stacked per user preference). All panels share the docking/resize/persistence behavior of §3.14–§3.19.

### 3.7.1 Thumbnail panel (DS-THUMB-*)
**Purpose.** Visual page navigation and page-level organization (`FR-THUMB`, `FR-ORG`).
**DS-THUMB-1.** MUST show page thumbnails in a scrollable, virtualized list/grid; the current page MUST be indicated (selection + "current" marker) and kept in view as the canvas scrolls. **DS-THUMB-2.** Clicking/activating a thumbnail navigates the canvas to that page; keyboard arrows move selection; Enter/Space navigates. **DS-THUMB-3.** MUST support multi-select (Shift range, Ctrl/Cmd toggle) and drag-to-reorder with a clear insertion indicator (§5 drag rules), issuing undoable page operations (`FR-ORG-1`). **DS-THUMB-4.** Thumbnails MUST render progressively (loading state §3-loading) without blocking; MUST update on document change within a bounded time (`FR-THUMB-3`, `SDS §8.6`). **DS-THUMB-5.** MUST expose page number labels; MAY show page-changed/edited badges. **DS-THUMB-6 (a11y).** Each thumbnail MUST have an accessible name ("Page N of M", plus state); the list MUST be a proper list with selection semantics (§9.11 accessible tables/lists apply). **DS-THUMB-7 (performance).** Thumbnail generation is background-prioritized below visible canvas (`NFR-PERF-2`); scrolling the thumbnail panel MUST remain smooth via virtualization.

### 3.7.2 Bookmark / outline panel (DS-BOOK-*)
**Purpose.** Navigate and edit the document outline (`FR-BOOK`).
**DS-BOOK-1.** MUST present the outline as a Tree View (§6.4) with expand/collapse, honoring the document's structure. **DS-BOOK-2.** Activating a bookmark navigates to its destination (`FR-BOOK-1`); the item corresponding to the current view SHOULD be highlighted. **DS-BOOK-3.** MUST support create, rename (inline edit, §6.20), reorder, re-nest (drag with clear parent/insertion indication), set-destination, and delete — all undoable (`FR-BOOK-2`). **DS-BOOK-4.** Editing affordances MUST be discoverable via context menu and keyboard (F2 rename, etc., §6.4). **DS-BOOK-5 (a11y).** Full tree semantics (roles, level, expanded state) to assistive tech (§9).

### 3.7.3 Layers panel (DS-LAYER-*)
**Purpose.** View/toggle optional content (`FR-LAYER`).
**DS-LAYER-1.** MUST list optional-content groups with visibility toggles (checkbox semantics, §6.5). **DS-LAYER-2.** Toggling visibility MUST update the canvas immediately and MUST be persistable as an undoable document change when the user saves (`FR-LAYER-2`). **DS-LAYER-3.** Locked/default states MUST be indicated honestly (DS-PHIL-8). **DS-LAYER-4 (a11y).** Tree/list with checkbox state to AT.

### 3.7.4 Attachments panel (DS-ATTACH-*)
**Purpose.** Access embedded files (`FR-EMB`).
**DS-ATTACH-1.** MUST list embedded files with name, type, size, date; MUST allow open (consented/brokered, §8 permission) and extract (`FR-EMB-1`). **DS-ATTACH-2.** Add/remove attachments MUST be undoable (`FR-EMB-2`). **DS-ATTACH-3.** Opening MUST route through safe-handling consent (§8, `NFR-SEC`); the panel MUST never auto-open an attachment (DS-PHIL-8). **DS-ATTACH-4.** Structured data (e-invoice) MUST be extractable (`FR-EMB-3`).

### 3.7.5 Signatures panel (DS-SIGP-*)
**Purpose.** Show signature status and details (`FR-SIG`).
**DS-SIGP-1.** MUST list signatures with plain-language status (valid / invalid / indeterminate) using semantic color + mandatory non-color glyph (§2.9, §9.5) — never color alone. **DS-SIGP-2.** Each entry expands to show signer, time, coverage, and post-signing changes with a permitted/not-permitted determination (`FR-SIG-2`). **DS-SIGP-3.** MUST NOT present indeterminate as valid (`FR-SIG-1`, DS-PHIL-8). **DS-SIGP-4.** Empty state: "This document is not signed" (honest, §8.1). **DS-SIGP-5 (a11y).** Status MUST be conveyed textually to AT, not by color.

### 3.7.6 Search results panel (DS-SEARCHP-*)
**Purpose.** Present in-document (and cross-document, `FR-SRCH-IDX`) results (`FR-SRCH`).
**DS-SEARCHP-1.** Results MUST appear progressively (loading state) with first result fast (`MET-PERF-5`), each showing page and a text snippet with the match emphasized. **DS-SEARCHP-2.** Activating a result navigates and highlights in-canvas (crisp highlight, §5, `FR-SRCH-3`). **DS-SEARCHP-3.** MUST show total count and current position; MUST support next/previous via keyboard (F3 / Shift+F3 default) and buttons. **DS-SEARCHP-4.** Scope controls (current doc, bookmarks, comments, indexed folders) MUST be explicit (`FR-SRCH-4`). **DS-SEARCHP-5 (a11y).** Result list is a navigable list; count and position announced (§9).

## 3.8 (reserved for additional left panels contributed by plugins — see §10)

## 3.9 Properties panel

**Purpose.** Context-sensitive properties of the current selection (annotation, field, page, object) (`FR-ANNOT-5`, forms, etc.).
**DS-PROP-1.** MUST reflect the current selection's editable properties using the Properties Grid component (§6.7); empty selection → empty state ("Select an object to see its properties", §8.1). **DS-PROP-2.** Edits MUST apply live to the selection and MUST be undoable (DS-PHIL-8). **DS-PROP-3.** Multi-selection MUST show shared properties with mixed-value indication (§6.7). **DS-PROP-4.** The panel MUST update within one frame of selection change (`SDS §5`). **DS-PROP-5 (a11y).** Each property is a labeled control with correct role; grouping announced (§9).

## 3.10 Comments panel & Inspector panel

### 3.10.1 Comments panel (DS-COMMENT-*)
**Purpose.** List, filter, navigate, and manage comments/annotations and review status (`FR-REV`).
**DS-COMMENT-1.** MUST list comments with author, type, page, timestamp, status, and text/preview; threaded replies MUST nest visibly (§6.4 tree-like or indented thread). **DS-COMMENT-2.** MUST support filter (author/type/status/page) and sort; filter state persists per session. **DS-COMMENT-3.** Activating a comment navigates to and selects its annotation (bi-directional selection sync with canvas). **DS-COMMENT-4.** Reply, set-status (accepted/rejected/completed), edit, delete MUST be available via keyboard and context menu, all undoable. **DS-COMMENT-5.** Export/summary action present (`FR-REV-3`). **DS-COMMENT-6 (a11y).** Thread structure and status conveyed textually; navigation keyboard-complete.

### 3.10.2 Inspector panel (DS-INSPECT-*)
**Purpose.** Advanced/technical inspection of document structure, metadata, and diagnostics (`FR-DIAG`, advanced users, DS-PHIL-6).
**DS-INSPECT-1.** MUST present metadata (editable where allowed), document structure/revisions, and the diagnostics/leniency report (repairs, unsupported constructs, scripts/rich-media/XFA presence) in clearly separated sections (`FR-DIAG-1`). **DS-INSPECT-2.** MUST be read-only-safe by default: inspection MUST NOT modify the document; edits (e.g., metadata) are explicit and undoable. **DS-INSPECT-3.** Content prepared for sharing (a diagnostic export) MUST be user-reviewable and MUST NOT include document content the user did not choose (`FR-DIAG-3`, `NFR-PRIV-3`). **DS-INSPECT-4.** This panel is advanced; it MUST NOT be in the default-visible set for casual users but MUST be discoverable (Advanced menu, command search). **DS-INSPECT-5 (a11y).** Technical tables follow accessible-table rules (§9.11).

## 3.11 Document tabs

**Purpose.** Manage multiple open documents (`UX-MULTI-2`).
**DS-TAB-1.** When more than one document is open, a tab strip MUST appear (or always, if the user pins it, §3.19). Each tab shows the document name, a dirty indicator (§7.2), and a close affordance. **DS-TAB-2.** Tabs MUST support: switch (click / `Ctrl/Cmd+Tab` cycles, `Ctrl/Cmd+1..9` jumps, platform-consistent), reorder (drag), close (`Ctrl/Cmd+W`), and tear-off into a new window (§3.12). **DS-TAB-3.** A dirty document's tab MUST show the unsaved indicator and MUST prompt on close (§7.2). **DS-TAB-4.** Overflow tabs MUST collapse into a navigable list, never disappear (DS-PHIL-8). **DS-TAB-5 (middle-click / affordances).** Middle-click closes a tab (where platform-conventional). **DS-TAB-6 (a11y).** Tabs use tablist/tab semantics with selected state; dirty state conveyed textually (§9).

## 3.12 Split view & multiple windows

**DS-SPLIT-1.** The canvas MUST support a split view showing two views of the *same* document (e.g., different pages/zoom) or *different* documents side by side, horizontally or vertically (`UX-MULTI-2`, engineer/architect compare workflows). **DS-SPLIT-2.** Split panes are independently scrollable/zoomable; a divider (Splitter §6.22) adjusts ratio; a control collapses back to single view. **DS-SPLIT-3.** The product MUST support multiple top-level windows, each hosting one or more document tabs, for multi-monitor use (`UX-MULTI-2`). **DS-SPLIT-4.** Tearing a tab out MUST create a new window preserving document state (view, selection, undo) (`SDS` document independence). **DS-SPLIT-5.** Window and split state SHOULD be restorable across sessions (`UX-MULTI-3`). **DS-SPLIT-6 (a11y).** Each pane is a distinct navigable region with a label; focus movement between panes is keyboard-defined (§4).

## 3.13 Status bar

**Purpose.** Persistent, low-attention display of view state and background activity.
**DS-STATUS-1.** MUST show, at minimum: current page / total, zoom level (editable, §6.3 combo), view layout control, and a background-activity area (progress for indexing/OCR/thumbnails/save). **DS-STATUS-2.** Background activity MUST be shown non-intrusively here with the ability to expand for detail and to cancel where cancellable (`NFR-RESP-2`, §11 progress). **DS-STATUS-3.** Document-condition indicators (repaired-on-open, contains scripts/rich media, is signed, is redaction-pending) MUST surface here or in a consistent adjacent area with click-through to detail (honesty, DS-PHIL-8, `FR-DIAG`). **DS-STATUS-4.** The status bar MUST be accessible (labeled regions; live-region for activity announcements throttled per §9). **DS-STATUS-5.** Status bar MUST NOT flash or animate distractingly (DS-PHIL-2); progress uses the calm indicators of §11.

## 3.14 Docking model

**DS-DOCK-1.** Panels MUST be dockable to the left, right, and bottom of the shell; the canvas is the fixed center (`UX-NAV-3`). Top is reserved for command surfaces (§3.2–§3.5). **DS-DOCK-2.** A panel may be: docked (tiled in a group), floating (§3.16), or tabbed with sibling panels in the same dock zone. **DS-DOCK-3.** Drag-to-dock MUST show explicit **dock target guides** (drop zones) and a live preview of the resulting layout before drop (DS-PHIL-3; no guessing). **DS-DOCK-4.** Docking MUST be undoable via "reset layout" and MUST never lose a panel (a mis-dropped panel is recoverable). **DS-DOCK-5 (a11y).** Docking MUST have a keyboard-accessible alternative (a "move panel to…" command menu) so layout is not mouse-only (§9, DS-PHIL-5).

## 3.15 Resizable panels

**DS-RESIZE-1.** Dock edges MUST expose a Splitter (§6.22) for resizing; panels have token-defined default, min, and max widths/heights. **DS-RESIZE-2.** Resizing MUST be live (content reflows during drag) unless performance requires a deferred mode; at target budget, live resize MUST hold frame cadence (`NFR-PERF-1`). **DS-RESIZE-3.** Double-clicking a splitter MUST reset the panel to its default size. **DS-RESIZE-4.** Keyboard resize MUST be available when the splitter is focused (arrow keys, defined step = `space.3`) (§9). **DS-RESIZE-5.** Resizing a panel MUST NOT re-render the document (`SDS §6.6`); only layout recomputes.

## 3.16 Floating panels

**DS-FLOAT-1.** Any panel MAY be undocked into a floating window at `elev.3` (§2.4). Floating panels MUST stay above the main window but MUST NOT be always-on-top over other applications unless the user pins them. **DS-FLOAT-2.** Floating panels MUST be movable, resizable, and closable, and MUST remember position per workspace (§3.18). **DS-FLOAT-3.** On multi-monitor, floating panels MUST render correctly at the target monitor's DPI (§2.15). **DS-FLOAT-4 (a11y).** A floating panel is a dialog-adjacent surface: focusable, labeled, escapable back to the main window (§9.10 does not trap non-modally).

## 3.17 Collapse behavior

**DS-COLLAPSE-1.** Every panel group MUST collapse to an edge affordance (a thin rail or handle) that clearly indicates it can be reopened (never fully hidden with no way back, DS-PHIL-8). **DS-COLLAPSE-2.** Collapsing/expanding MUST animate per §11 panel transition (≤150 ms, reduced-motion → instant) and MUST not shift canvas content unexpectedly (preserve scroll/zoom focus point). **DS-COLLAPSE-3.** A single command MUST toggle all panels for a distraction-free reading state (§3.1 `F8`).

## 3.18 Workspaces [UX Decision]

**DS-WORKSPACE-1.** The product MUST support named **workspaces**: saved arrangements of panels, densities, and command-surface mode, switchable by the user (e.g., "Review", "Forms", "Prepress", "Reading"). **DS-WORKSPACE-2.** The product MUST ship a small set of default workspaces mapped to major personas (§6 of PRD) and MUST allow user-defined ones. **DS-WORKSPACE-3.** Switching a workspace MUST be immediate, non-destructive to documents, and reversible. **DS-WORKSPACE-4.** Workspaces are part of the interface-stability contract insofar as *default* workspaces MUST remain available across releases per §14. *Rationale (Informative):* workspaces let one product serve casual readers and prepress operators without either group paying for the other's complexity (DS-PHIL-6/7).

## 3.19 Persistence rules

**DS-PERSIST-1.** The following MUST persist per user across sessions: panel layout and sizes, open/closed panels, active workspace, density, theme, command-surface mode, toolbar/QAT customizations, shortcut customizations, window and (SHOULD) split/multi-window geometry, and recent-view preferences (last zoom/layout defaults). **DS-PERSIST-2.** Per-document view state (last page, zoom, scroll) SHOULD persist and be restored on reopen where feasible (reading continuity), stored in per-user app state, never written into the document (DS-PHIL-8, `NFR-PRIV`). **DS-PERSIST-3.** Persistence MUST be per-user in shared environments (`ENT-SHARE-1`). **DS-PERSIST-4.** A "reset to defaults" MUST exist for layout, toolbar, and shortcuts, each independently. **DS-PERSIST-5.** Enforced enterprise policy (`ENT-POL`, `ENT-UI`) MUST override and MAY lock any of these; locked settings MUST be shown as administrator-controlled, not silently reverted (DS-PHIL-8, `ENT-POL-3`).

---

# 4. Navigation

*Normative.* This section defines how focus, input, and viewport movement behave across all input modalities. Component-level keyboard behavior is in §6; this section defines the global model those components inherit.

## 4.1 Global interaction & mode model

**DS-NAV-1 (Visible mode).** Every persistent mode (active tool, e.g., Select / Hand / Comment / Redact / Measure) MUST have a continuously visible indicator: the active tool control shows its active state (§6.1), the cursor reflects the mode (§4.9), and the status bar or toolbar names the mode (DS-PHIL-3). There MUST be no invisible mode. **DS-NAV-2 (Escape to default).** `Esc` MUST always provide a predictable retreat: cancel the in-progress action, else close the transient surface, else deselect, else return to the default Select tool. The retreat order MUST be deterministic and identical everywhere (DS-PHIL-3). **DS-NAV-3 (One default tool).** The Select tool is the resting state; every specialized tool has an obvious one-action return to Select (`Esc` or clicking Select). **DS-NAV-4 (Sticky vs momentary tools).** A tool is "sticky" (stays active for repeated use) or "momentary" (reverts after one use) per a documented per-tool default; the user MAY toggle stickiness (e.g., hold-to-keep). The default per tool MUST match Acrobat expectations where one exists (DS-PHIL-1). **[UX Decision]** Tool stickiness is user-configurable with Acrobat-matching defaults.

## 4.2 Focus rules

**DS-FOCUS-1 (Always visible).** Keyboard focus MUST be visibly indicated at all times when navigating by keyboard, using a **2 dp focus ring** in `color.focus` with sufficient contrast in every theme (≥3:1 against adjacent colors, §9.4), plus a subtle offset so it is visible on both light and dark controls. In High-Contrast, ≥3 dp system highlight (§2.13). **DS-FOCUS-2 (Single focus).** Exactly one element has focus at a time within a window; focus MUST never be lost to nowhere — if a focused element is removed, focus moves to a defined neighbor (parent/next sibling) (DS-PHIL-3, §9). **DS-FOCUS-3 (Focus not stolen).** Background activity, notifications, progress, or async completion MUST NOT steal focus (DS-PHIL-8, §8, §11). Only an explicit user action moves focus. Modal dialogs take focus because the user invoked them. **DS-FOCUS-4 (Restore).** Closing a transient surface (menu, popover, dialog) MUST return focus to the invoking control (§6, §9.10). **DS-FOCUS-5 (Focus regions).** The shell is divided into focus regions (command surface, left rail, active panel, canvas, status bar). `F6` / `Shift+F6` MUST cycle forward/backward through visible regions; within a region, `Tab` and arrows move per component (§6.2). **[UX Decision]** `F6` region cycling is the canonical cross-region keyboard model.

## 4.3 Tab order

**DS-TAB-ORDER-1.** Tab order MUST follow logical reading/visual order within a region (left-to-right, top-to-bottom; mirrored in RTL, §9). **DS-TAB-ORDER-2.** Tab order MUST be deterministic and stable across releases (part of the stability contract, §14); QA MUST have a documented expected tab order per surface (§13). **DS-TAB-ORDER-3.** Only interactive elements are in the Tab sequence; static text is reachable by screen reader but not Tab. **DS-TAB-ORDER-4.** Composite widgets (toolbars, lists, trees, tab strips, radio groups) are a single Tab stop with internal arrow navigation (roving tabindex) — not N tab stops (§6.2, §9). **DS-TAB-ORDER-5.** Disabled controls are skipped by Tab but MAY be discoverable by screen-reader review with their disabled state announced (§6, §9).

## 4.4 Navigation history (view history)

**DS-HIST-1.** The product MUST maintain per-view navigation history (visited pages/destinations/zoom states) supporting Previous View / Next View, invocable in one action (default `Alt+Left` / `Alt+Right`; also available as buttons) (`FR-NAV-3`, a specific fix for incumbent regressions). **DS-HIST-2.** "Previous View" MUST restore page, scroll position, and zoom as they were (DS-PHIL-3). **DS-HIST-3.** History MUST be per-document-view (each split pane/window has its own). **DS-HIST-4.** History depth is effectively unbounded within a session; MUST not grow memory without bound (`NFR-MEM-1`) — store lightweight view descriptors, not renders. **DS-HIST-5 (a11y).** History navigation announces the new location (page N) to AT (§9 live region, throttled).

## 4.5 Breadcrumbs

**DS-CRUMB-1.** Where hierarchical context exists (e.g., a portfolio's file path, a nested bookmark location, an accessibility-tag tree location during remediation), a breadcrumb MUST show the path and allow jumping to any ancestor. **DS-CRUMB-2.** Breadcrumbs are not used for ordinary page navigation (pages are not hierarchical); they appear only in genuinely hierarchical surfaces to avoid false affordance (DS-PHIL-3). **DS-CRUMB-3 (a11y).** Breadcrumb is a navigable list of links with current-item marked (§9).

## 4.6 Zoom behavior

**DS-ZOOM-1 (Levels).** Zoom MUST support fit-width, fit-page, fit-visible, actual-size (100%), and arbitrary zoom, plus preset stops (25, 50, 75, 100, 125, 150, 200, 400, 800, 1600%) reachable via the zoom control (§6.3 in status bar) and shortcuts (`Ctrl/Cmd +` / `-` / `0` for fit, defaults matching Acrobat) (`FR-NAV-2`). **DS-ZOOM-2 (Anchor).** Zoom MUST anchor to a sensible focal point: pointer position for wheel/pinch zoom, selection or viewport center for keyboard zoom (DS-PHIL-3). **DS-ZOOM-3 (Smoothness).** Interactive zoom MUST remain smooth at frame cadence (`MET-PERF-4`), using progressive refinement (crisp tiles follow the gesture, `SDS §6.7`); the user MUST never wait for rasterization to continue zooming. **DS-ZOOM-4 (Bounds).** Min/max zoom are defined; attempting to exceed shows a subtle bounded-limit cue (no error). **DS-ZOOM-5 (Marquee zoom).** A marquee-zoom tool MUST let the user drag a rectangle to zoom to a region (Acrobat parity). **DS-ZOOM-6 (a11y).** Current zoom MUST be reported in the editable zoom field and to AT; zoom MUST be operable entirely by keyboard.

## 4.7 Scrolling behavior

**DS-SCROLL-1 (Layouts).** Scrolling MUST honor the active page layout (single, continuous, facing, facing-continuous) (`FR-NAV-1`). Continuous scrolling MUST be smooth and MUST use prefetch so newly-revealed pages are ready (`SDS §6.9`), showing a page-background placeholder rather than blank on the rare miss (never a white flash, DS-PHIL-3). **DS-SCROLL-2 (Inertia).** Trackpad/touch scrolling MUST have platform-appropriate inertia; mouse-wheel scrolling MUST move a consistent, configurable amount per notch. **DS-SCROLL-3 (Page snapping).** In single-page and facing (non-continuous) layouts, scrolling past a page boundary MUST snap to the next/previous page predictably. **DS-SCROLL-4 (Keyboard).** `Space`/`Shift+Space` page down/up; `PageDn`/`PageUp`; arrow keys scroll by a step; `Home`/`End` to first/last page; all deterministic (DS-PHIL-3). **DS-SCROLL-5 (Position preservation).** Layout/zoom changes preserve the focal content (the thing at viewport center stays near center) (DS-PHIL-3). **DS-SCROLL-6 (Performance).** Scrolling holds frame cadence at p95 on the large-document reference (`MET-PERF-3`).

## 4.8 Trackpad and mouse wheel

**DS-POINT-1 (Wheel).** Vertical wheel scrolls; `Ctrl/Cmd+wheel` zooms (anchored at pointer, DS-ZOOM-2); `Shift+wheel` scrolls horizontally where content exceeds width. **DS-POINT-2 (Precision trackpad).** Two-finger scroll pans; pinch zooms (anchored at gesture centroid); these MUST follow platform gesture conventions (`UX-MOUSE-2`). **DS-POINT-3 (Momentum).** Momentum scrolling MUST be honored from the OS; the app MUST NOT reimplement conflicting inertia. **DS-POINT-4.** Horizontal two-finger swipe MAY navigate pages in single-page layout where platform-conventional; MUST be consistent and not conflict with scroll.

## 4.9 Cursors

**DS-CURSOR-1.** The cursor MUST always reflect the current tool and the target under it (DS-PHIL-3): arrow (select), open/closed hand (pan/drag-pan), I-beam (text select), crosshair (draw/measure/redact region), resize arrows (handles), rotate cursor (rotation handle), not-allowed (invalid drop/target), zoom (marquee zoom), text-insert (form/text edit). **DS-CURSOR-2.** Cursor changes MUST be immediate and unambiguous. **DS-CURSOR-3.** Custom cursors MUST have high-DPI variants (§2.15) and MUST respect OS cursor size/accessibility settings. **DS-CURSOR-4.** A busy cursor MUST appear only for genuinely blocking waits (rare; most work is async, §11) and MUST never be the sole progress indicator (§11).

## 4.10 Touch gestures

**DS-TOUCH-1.** On touch-capable devices the product SHOULD support: tap (activate/select), double-tap (zoom to content/toggle fit), drag (pan/scroll), two-finger pinch (zoom, anchored), two-finger rotate (view rotate where enabled), long-press (context menu, with a clear press-and-hold affordance and haptic where available) (`UX-TOUCH-1`). **DS-TOUCH-2.** Touch targets MUST meet §9.8 minimums (≥44×44 dp effective); touch-primary contexts SHOULD default to Spacious density (DS-DENSITY-4). **DS-TOUCH-3.** Touch gestures MUST NOT be the only path to any function (`UX-TOUCH-2`, DS-PHIL-5). **DS-TOUCH-4.** Gesture conflicts MUST resolve deterministically (e.g., pan vs. select) based on the active tool and a defined threshold (movement > `space.3` within timeout = drag) (DS-PHIL-3). **DS-TOUCH-5.** Text selection by touch MUST use platform-standard selection handles.

## 4.11 Pen / stylus gestures

**DS-PEN-1.** Pen input MUST support low-latency inking for ink annotation with pressure and (where available) tilt (`FR-ANNOT-7`, `UX-PEN-1`). Inking latency MUST meet the interactive budget; the wet-ink stroke MUST track the pen closely (§11 performance). **DS-PEN-2.** Palm rejection MUST be honored via the platform's pen APIs so resting a hand does not create stray marks (DS-PHIL-8). **DS-PEN-3.** Pen barrel-button and eraser-end MUST map to defined actions (context/erase) where the hardware exposes them; defaults documented. **DS-PEN-4.** Pen hover (where supported) MAY show a tool preview cursor. **DS-PEN-5.** Pen MUST NOT be required for any function (DS-PHIL-5); it augments annotation. **DS-PEN-6.** Pressure→width and smoothing curves for ink are defined tokens so two builds ink identically (DS-CONV-1); default smoothing is documented and adjustable.

---

# 5. Canvas Design

*Normative.* The canvas is the document viewport and the product's center (DS-SHELL-2). It renders the document (via the pipeline of `SDS §6`) and hosts all direct-manipulation overlays. **DS-COLOR-7 (color sanctuary) governs throughout: overlays are drawn over, never into, the document, and the document's true colors are never altered by chrome.**

## 5.1 Page rendering presentation
**DS-CANVAS-1.** Pages MUST render at full fidelity per `FR-VIEW`; the canvas presentation layer adds only: pasteboard background, page shadow/edge, page spacing, and overlays (selection, annotations-in-progress, guides, measurement). **DS-CANVAS-2.** The canvas MUST show a placeholder page frame with a loading shimmer (§8-loading, §11 skeleton) for not-yet-rendered pages, never blank white (DS-PHIL-3, `SDS §6.9`). **DS-CANVAS-3.** Progressive refinement MUST be visually smooth: a low-res page appears first, sharpening to crisp without a jarring swap (`SDS §6.10`, §11). **DS-CANVAS-4.** Text-selection and annotation overlays MUST be drawn from geometry so they are crisp at any zoom independent of page raster (`SDS §6.4`, `FR-SRCH-3`).

## 5.2 Page shadows, spacing, pasteboard
**DS-CANVAS-5.** Each page MUST have a subtle drop shadow (`elev.1`-class, tuned for the pasteboard) OR, in flat/high-contrast/reduced settings, a 1 dp page-edge border, so page boundaries are always perceivable (§2.4, §9.3). **DS-CANVAS-6.** Inter-page spacing in continuous layouts is a token (`canvas.page.gap`, default `space.6`=20 dp at 100%); facing spreads have a defined center gutter. **DS-CANVAS-7.** The pasteboard color is `color.bg.pasteboard` (mid-gray light; darker in dark theme) chosen for page-edge legibility and reduced eye strain (§2.12). **DS-CANVAS-8.** Page shadow/edge MUST scale correctly with zoom and DPI and MUST NOT bleed into or tint page content (DS-COLOR-7).

## 5.3 Selection (objects, annotations, text)
**DS-SEL-1 (Visual).** Selected objects/annotations MUST show a bounding box in `color.accent` with a 1.5 dp stroke and defined handles (§5.4). Text selection MUST show a translucent `color.selection` highlight (defined alpha) that remains legible over arbitrary content (§5.7). **DS-SEL-2 (Single vs multi).** Single selection shows full handles; multi-selection shows a combined bounding box with a distinct multi-select treatment (§5.6). **DS-SEL-3 (Hit tolerance).** Selection hit targets MUST include a tolerance band (≥ `space.2`) so thin objects are selectable (accessibility & precision). **DS-SEL-4 (Keyboard).** Selection MUST be achievable and adjustable by keyboard (Tab through objects on a page where a tool exposes that; arrow-nudge selected; Shift+arrow extend) (§9). **DS-SEL-5 (Sync).** Canvas selection and panel selection (comments/properties) MUST stay in sync bidirectionally (DS-PHIL-3). **DS-SEL-6 (Announce).** Selection changes MUST be announced to AT with the object's accessible name/type (§9).

## 5.4 Handles, bounding boxes, rotation handles
**DS-HANDLE-1.** Resize handles are square, `handle.size` (default 8 dp, min 10 dp effective touch via tolerance), placed at 4 corners + 4 edge midpoints; they MUST be visible over any background (white fill + accent stroke, or inverse in dark) (§5.7). **DS-HANDLE-2.** A rotation handle (where rotation applies) appears above the top-center handle at a defined offset, with a rotate cursor (§4.9). **DS-HANDLE-3.** Handles MUST have a hover/active state and a ≥44 dp effective touch target via tolerance even when drawn small (§9.8). **DS-HANDLE-4.** During resize, the bounding box updates live; holding the aspect modifier (Shift) constrains ratio; Alt resizes from center — Acrobat-consistent defaults (DS-PHIL-1). **DS-HANDLE-5 (Keyboard).** Resize/rotate MUST have keyboard equivalents (e.g., properties panel numeric entry, or modifier+arrow) (§9). **DS-HANDLE-6.** Handle geometry and hit tolerance are tokens (DS-CONV-1).

## 5.5 Guides & snapping
**DS-GUIDE-1.** The canvas MUST offer optional rulers and draggable guides for precise placement (measurement/prepress/forms). Guides are view aids, not document content (DS-COLOR-7, DS-PHIL-8) unless the user explicitly adds document-level guides where supported. **DS-SNAP-1.** Snapping MUST be available to: page edges, margins, other objects' edges/centers, guides, and a configurable grid. Snapping MUST show a snap indicator (a highlighted line/point) at the moment of snap (DS-PHIL-3). **DS-SNAP-2.** Snapping MUST be toggleable (modifier to temporarily disable while dragging, default holding a documented key). **DS-SNAP-3.** Snap thresholds are tokens; identical across builds (DS-CONV-1). **DS-SNAP-4 (a11y).** Precise placement MUST also be achievable numerically (properties grid) for users who cannot drag (§9).

## 5.6 Multiple selection
**DS-MULTI-1.** Multi-select via marquee (drag over empty canvas with a selection tool), Shift+click (add/remove), Ctrl/Cmd+A (select all applicable on page/context). **DS-MULTI-2.** The combined bounding box shows aggregate handles; transforms apply to all selected proportionally. **DS-MULTI-3.** Mixed-type multi-selection shows only shared editable properties (§6.7 mixed values). **DS-MULTI-4.** Marquee rectangle uses `color.accent` stroke + translucent fill; partial-vs-full-enclosure selection rule is documented (default: intersect selects) and consistent (DS-PHIL-3).

## 5.7 Overlay contrast guarantee [UX Decision]
**DS-OVERLAY-1.** All canvas overlays (selection, handles, marquee, guides, snap indicators, measurement, in-progress annotations, search highlight) MUST remain perceivable over arbitrary document content — including content the same color as a naive overlay — by using one of: a dual-tone stroke (light core + dark halo, or vice versa), a defined blend that guarantees contrast, or an outline+fill combination. Overlays MUST NOT rely on a single color that could match document content. *Rationale:* documents contain every possible color; overlays that vanish over matching content violate DS-PHIL-3. This is a hard requirement verified in QA against adversarial-color test pages (§13).

## 5.8 Crop interaction
**DS-CROP-1.** The crop tool MUST show a draggable crop rectangle with handles over the page, darkening (view-only) the area outside the crop for preview (DS-COLOR-7: this is a preview overlay, not a document change until applied). **DS-CROP-2.** Crop MUST display numeric margins/box dimensions live and allow numeric entry (`FR-CROP-1`, precision). **DS-CROP-3.** Crop MUST clarify (honesty, DS-PHIL-8, `FR-CROP-1`) whether it hides (reversible) or removes content, defaulting to reversible; removal is an explicit choice. **DS-CROP-4.** Apply is undoable; the preview state before apply is fully cancelable (`Esc`). **DS-CROP-5 (a11y).** Crop fully operable via numeric fields and keyboard.

## 5.9 Measurement overlays
**DS-MEASURE-1.** Measurement tools (distance, perimeter, area) MUST draw the measurement geometry over the page with the live value shown adjacent, formatted to the configured scale/precision/units (`FR-MEAS`). **DS-MEASURE-2.** In-progress measurement shows each vertex and the running value; snapping (§5.5) aids precision; `Esc` cancels, `Enter`/double-click completes. **DS-MEASURE-3.** Completed measurements MAY be committed as annotations carrying their value (`FR-MEAS-3`), rendered with the overlay-contrast guarantee (§5.7). **DS-MEASURE-4.** Scale calibration UI MUST be explicit and its current value always visible while measuring (DS-PHIL-3). **DS-MEASURE-5.** Values MUST remain correct under zoom/rotation (`FR-MEAS-2`). **DS-MEASURE-6 (a11y).** Measurement values MUST be readable as text to AT, not conveyed only by on-canvas drawing.

## 5.10 Canvas accessibility
**DS-CANVAS-A11Y-1.** The canvas MUST expose the document's accessible content (tagged reading order where present) to assistive technology as a navigable structure, not as an opaque image (`FR-A11Y-1`, §9.12). **DS-CANVAS-A11Y-2.** Keyboard users MUST be able to move through page content, annotations, form fields, and links in a defined order and act on them (§9.12). **DS-CANVAS-A11Y-3.** The current focus within the document MUST be visually indicated on-canvas (a focus ring around the focused field/annotation/link) consistent with §4.2. **DS-CANVAS-A11Y-4.** Where the document is an untagged scan, the canvas MUST honestly convey that structure is unavailable and offer OCR/remediation paths (§8.1, DS-PHIL-8).

## 5.11 Canvas performance & animation
**DS-CANVAS-PERF-1.** All canvas interactions (scroll, zoom, pan, select, draw, ink) MUST meet the interaction budgets of `MET-PERF-*` at p95/p99. **DS-CANVAS-PERF-2.** Overlay drawing MUST be cheap and MUST NOT trigger document re-rasterization (geometry layer, `SDS §6.4`). **DS-CANVAS-ANIM-1.** Canvas animations (zoom, page transition, refinement) follow §11 and MUST be interruptible by continued input (DS-MOTION-3). **DS-CANVAS-ANIM-2.** No decorative canvas animation exists; motion here is strictly functional (DS-PHIL-2).

---

# 6. Component Library

*Normative.* Every reusable component is specified here with a fixed clause structure so two designers converge (DS-CONV-1). Unless a component overrides it, **all components inherit these baseline rules (DS-COMP-BASE):**

- **Sizing/spacing:** from density tokens (§2.14) and spacing tokens (§2.3); never literal.
- **Focus:** the §4.2 focus ring; visible whenever keyboard-focused.
- **Disabled state:** reduced opacity per `state.disabled.opacity` token (default 0.38 foreground) + removal from Tab order + `aria-disabled`/platform-equivalent; disabled controls MUST still be discoverable by screen-reader review and MUST expose *why* when non-obvious (tooltip on hover/focus). Disabled MUST NOT be conveyed by color alone (§9.5).
- **Loading state:** where a control triggers async work, it shows an inline progress affordance (§11) and becomes non-interactive without losing focus.
- **Error state:** where a control can be invalid, it shows the error treatment (§6.16 inputs) with text + icon (not color alone) and an accessible error association.
- **Empty state:** container components define an empty state (§8.1 patterns).
- **Hit target:** effective interactive target ≥ §9.8 minimum even if the visual is smaller (via padding/tolerance).
- **Animation:** state transitions ≤ `motion.fast` (§11), reduced-motion → instant.
- **Theming:** via semantic tokens; correct in light/dark/high-contrast.
- **RTL:** mirrored layout where the locale is RTL (§9).
- **Accessible name/role/state:** every component maps to the correct platform accessibility role with name, value, and states exposed (§9).

Only deviations and specifics are stated per component below.

## 6.1 Buttons (DS-BTN-*)
**Purpose.** Trigger an action. **Variants:** Primary (one per surface max; `color.accent` fill), Secondary (neutral fill/outline), Subtle/Ghost (no fill until hover), Destructive (danger semantic), Icon-only, Split button (§6.1.4), Toggle button (§6.1.5). **States:** rest, hover, active/pressed, focus, disabled, loading, (toggle:) selected. **Sizing:** height per density (§2.14); min width `space.10`(48) for text buttons; icon-only is square at control height. **Spacing:** label horizontal padding `space.4`; icon-to-label gap `space.2`(right-padded). 
**Behavior:** activates on release within bounds; `Esc`/pointer-leave-before-release cancels press. Primary MUST be the safe/expected default action; a destructive action MUST NOT be the default/primary unless the entire dialog is about that destruction (§6.10). 
**Keyboard:** `Enter`/`Space` activate when focused; a dialog default button activates on `Enter` from anywhere in the dialog unless focus is in a multiline field (§6.10). 
**Mouse/Pen/Touch:** standard press; touch target ≥44 dp. 
**Accessibility:** role button; name from label or, for icon-only, from tooltip/aria-label; loading announces "busy"; toggle exposes pressed state. 
**Loading:** replace label with inline spinner + keep width stable (no layout shift); control non-interactive, focus retained. 
**Performance:** visual state change ≤1 frame; no action may block UI (async per §11). 
**Animation:** hover/press are token color/elevation changes over `motion.fast`; no bounce.

### 6.1.4 Split button
Primary action + attached chevron opening a menu of related actions. Chevron is a separate focus-stop within the composite (roving); menu follows §6.2. Last-used action MAY become the primary (if so, it MUST be visible which, DS-PHIL-3). 
### 6.1.5 Toggle button
Two-state (or multi-state segmented). Selected uses filled-icon + accent treatment + `aria-pressed`; state MUST be perceivable without color (§9.5, e.g., filled vs outline icon).

## 6.2 Menus (DS-MENUC-*)
**Purpose.** A list of commands/options (menu bar menus §3.2, context menus §6.9, overflow menus). **Variants:** menu bar dropdown, context menu, submenu, split-button menu, overflow menu. **States (item):** rest, hover, focus (roving), disabled, checked/radio, with-submenu, danger. 
**Behavior:** opens on click/activation at `elev.2`; positions to stay on-screen (flip/nudge); one submenu chain open at a time; hovering a sibling closes other submenus after a small intent delay (`motion.hover-intent` ~120 ms) to avoid diagonal-travel loss (DS-PHIL-3). Selecting an item performs the command and closes the whole chain (unless a documented "stay open" multi-toggle menu). 
**Keyboard:** open focuses first (or checked) item; Up/Down move (wrap), Right opens submenu / Left closes, `Enter`/`Space` activate, typeahead selects by first letters, `Esc` closes one level and returns focus to invoker (§4.2). Mnemonics (underlined letters) on menu-bar menus (Alt path). 
**Mouse/Touch/Pen:** click to open, click item to activate; touch uses tap; long-press opens context menus (§4.10). 
**Accessibility:** menu/menuitem/menuitemcheckbox/menuitemradio roles; submenu expanded state; disabled announced; checked state announced (not color). 
**Empty:** a menu MUST NOT be empty; if all items are contextually unavailable, show a single disabled explanatory item (DS-PHIL-8). 
**Performance:** open ≤1 frame after intent; large menus virtualize. 
**Animation:** fade/scale-in over `motion.fast` from the invoker anchor; reduced-motion → instant.

## 6.3 Dropdowns & combo boxes (DS-COMBO-*)
**Purpose.** Choose one value from a list (dropdown) or choose/type a value (editable combo, e.g., zoom). **Variants:** select (non-editable), editable combo, searchable combo (with filter). **States:** rest, focus, open, disabled, invalid, loading (async options). 
**Behavior:** click/`Alt+Down` opens the list popover (§6.2 positioning); selection updates the field and closes; editable combo accepts typed values validated on commit (invalid → §6.16 error). Searchable combo filters as typed with a visible "no matches" empty state. 
**Keyboard:** closed + focus: Up/Down change value or open (documented, consistent); open: Up/Down move highlight, `Enter` commit, `Esc` cancel to prior value, typeahead. Editable: text editing keys plus list navigation. 
**Accessibility:** combobox pattern with listbox popup; expanded state; active option; typed-value validation announced. 
**Performance:** large option lists virtualize; async options show loading in the popover, never freeze. 
**Animation:** popover per §6.2.

## 6.4 Lists & Tree Views (DS-LIST-*, DS-TREE-*)
**Purpose.** Present ordered items (list) or hierarchy (tree) — bookmarks, comments, layers, attachments, file lists. **Variants:** single-select, multi-select, checkbox list/tree, editable (inline rename), reorderable (drag). **States (item):** rest, hover, selected, focused (roving), disabled, expanded/collapsed (tree), editing, drag-source, drop-target. 
**Behavior:** single Tab stop; arrow keys move focus (roving tabindex); selection model per variant. Tree: Right/Left expand/collapse or move to child/parent; `*` expands all under node (optional). Multi-select: Shift range, Ctrl/Cmd toggle. Reorder: drag with insertion-line indicator; keyboard reorder via modifier+arrow (§9). Inline rename: F2/slow-double-click enters edit (§6.20), `Enter` commit, `Esc` cancel. 
**Mouse/Touch/Pen:** click select, double-click default action, drag reorder, long-press context (touch). 
**Accessibility:** list/listbox or tree/treeitem roles with level, position-in-set, size-of-set, expanded, selected, checked; selection and expansion announced. 
**Empty/Loading:** defined empty state (§8.1) and progressive/virtualized loading; virtualization MUST preserve keyboard navigation and AT semantics for off-screen items conceptually. 
**Performance:** virtualized; smooth scroll at large item counts (`NFR-PERF`). 
**Animation:** expand/collapse ≤ `motion.fast`; reduced-motion → instant; drag indicator immediate.

## 6.5 Checkboxes & Radio buttons (DS-CHK-*, DS-RADIO-*)
**Purpose.** Boolean (checkbox), mutually-exclusive choice (radio group). **States:** unchecked, checked, indeterminate (checkbox only), focus, disabled, invalid. 
**Behavior:** `Space` toggles checkbox; radio group is one Tab stop, arrows move+select within group (roving), `Space` selects focused. Indeterminate is display-only (set programmatically), resolves to checked on user toggle. 
**Accessibility:** checkbox/radio roles; checked/mixed state; group has an accessible group label; state never by color alone (glyph presence conveys, §9.5). **Sizing:** control glyph 16 dp (Comfortable) with ≥44 dp target via label+padding; label is part of the target. **Animation:** check mark draws over `motion.fast` (reduced-motion → instant).

## 6.6 Tables (DS-TABLE-*)
**Purpose.** Tabular data (comment lists in table mode, form-field lists, batch job results, inspector data). **Variants:** read-only, sortable, selectable rows, editable cells, with row actions. **States:** row (rest/hover/selected/focused), cell (editing/invalid), column (sorted asc/desc), loading, empty. 
**Behavior:** column headers sort on activation (tri-state: asc/desc/none) with a visible sort indicator (glyph, not color). Rows selectable (single/multi). Cell edit (where editable) via `Enter`/F2, commit `Enter`, cancel `Esc`. Column resize via header splitters; reorder via header drag (where enabled). 
**Keyboard:** grid navigation — arrows move cell focus, `Home`/`End` row extremes, `Ctrl+Home/End` grid extremes, `Enter` edit/activate, selection keys as lists. Header is keyboard-focusable for sort. 
**Accessibility:** full accessible-table semantics (§9.11): row/column headers associated, sort state, selection, row/col counts; editable cells expose editing state. 
**Empty/Loading:** skeleton rows on load (§11); explicit empty state (§8.1). 
**Performance:** virtualized rows; sort/filter on large sets shows progress if non-instant, never freezes. 
**Animation:** row insert/remove ≤ `motion.fast`; sort re-order may cross-fade (reduced-motion → instant).

## 6.7 Properties grid (DS-PGRID-*)
**Purpose.** Edit an object's properties as label/value pairs (properties panel §3.9, annotation/field/page properties). **Variants:** flat, grouped (collapsible sections), with mixed-values (multi-select). **States:** per-field (rest/focus/editing/invalid/disabled), group (expanded/collapsed), value (normal/mixed/inherited/overridden). 
**Behavior:** label column fixed measure; value column hosts the correct editor per property type (input, combo, checkbox, color picker §6.24, numeric with units). Mixed value (multi-select) shows a defined "Mixed" placeholder; editing sets all. Live-apply with undo (DS-PROP-2). Numeric fields support units, steppers, and drag-to-scrub (optional, with keyboard alt). 
**Keyboard:** `Tab`/arrows move between fields; each editor keyboard-complete; group headers toggle with `Enter`/`Space`. 
**Accessibility:** each row is a labeled control; groups are labeled regions; mixed/inherited states conveyed textually. 
**Performance:** selection change → panel populate ≤1 frame (`SDS §5`). 
**Animation:** group collapse ≤ `motion.fast`.

## 6.8 Dialogs & Alerts (DS-DIALOG-*, DS-ALERT-*)
**Purpose.** Focused task (dialog) or decision/confirmation (alert). **Variants:** modal dialog, non-modal dialog (rare; prefer panels), alert (message + 1–3 actions), confirmation (destructive §6.10), wizard (multi-step). **States:** open, loading (async content/validation), error (inline within), disabled-primary (until valid). 
**Behavior:** modal dims and blocks the parent (scrim at defined alpha) at `elev.3`; focus moves into the dialog and is trapped until dismissed (§9.10); `Esc` cancels (equiv. to Cancel) unless an unsaved-change guard applies (then confirm); default button on `Enter` (§6.1). Dialogs are as small as the task allows (DS-PHIL-6) and MUST NOT nest more than one level except wizards. Buttons are right-aligned (platform order for OK/Cancel respected per OS convention — [UX Decision] follow platform button order). 
**Keyboard:** full operability; `Tab` cycles within; default/cancel mapped; wizard Next/Back mapped. 
**Accessibility:** dialog/alertdialog role; labelled by its title; described by its body; focus trap + restore on close (§9.10); alerts announce on open. 
**Loading/Error:** async validation shows inline progress on the primary; validation errors appear inline near the field (§6.16), never as a nested alert if avoidable. 
**Performance:** open ≤1 frame; content that must load shows skeleton (§11). 
**Animation:** scale+fade in from 98%→100% over `motion.base`; scrim fades; reduced-motion → instant. Dialogs MUST NOT animate on a slow curve that delays interaction.

## 6.9 Context menus (DS-CTX-*)
**Purpose.** Contextual actions for the object/region under the pointer or focus. **Behavior:** invoked by right-click, `Menu` key/`Shift+F10` (keyboard, at focused element), or long-press (touch). Contents are the applicable subset of commands for the target, each with its canonical shortcut shown; ordering stable and grouped (DS-PHIL-3). Follows §6.2 menu behavior. **Accessibility:** keyboard-invokable at the focused element (not pointer-only), correct roles, focus restore. **Determinism:** the same target always yields the same menu (DS-PHIL-3).

## 6.10 Confirmations & destructive-action pattern (DS-CONFIRM-*) [UX Decision]
**DS-CONFIRM-1.** A destructive, irreversible, or hard-to-reverse action (delete pages, flatten, sanitize/clean-save that drops history/signatures, discard unsaved changes, remove signature) MUST use a confirmation with: a clear title naming the action, a body stating exactly what will be lost/changed (honesty, DS-PHIL-8, `PRIN-6`), a **non-default** destructive primary styled with danger semantic, and a clearly available Cancel that is the safe default focus. **DS-CONFIRM-2.** Confirmations MUST NOT use confirm-shaming or unbalanced emphasis (DS-PHIL-10). **DS-CONFIRM-3.** Where an action is undoable, the product SHOULD prefer performing it with an Undo affordance (snackbar §6.13) over a blocking confirmation (fewer interruptions, DS-PHIL-7), reserving confirmations for the genuinely irreversible. **DS-CONFIRM-4.** "Don't ask again" MAY be offered only for reversible actions and MUST be re-enablable in settings.

## 6.11 Notifications (DS-NOTIF-*)
**Purpose.** Inform about background/async outcomes without stealing focus (DS-FOCUS-3). **Variants:** inline (within a panel/region), toast/snackbar (§6.13), status-bar activity (§3.13), notification center (a reviewable list of recent non-modal messages). **States:** info/success/warning/danger (semantic + glyph, §9.5), with optional action, dismissible. 
**Behavior:** notifications appear in a consistent location (bottom-trailing for toasts), never over the user's active work target, never focus-stealing, auto-dismiss for transient info (with pause-on-hover/focus), persistent for warnings/errors until acknowledged. A history is available (notification center) so nothing important is missed if a toast times out (DS-PHIL-8). 
**Accessibility:** non-focus-stealing announcement via polite live region (info) or assertive (errors) with throttling (§9); action reachable by keyboard from the notification center. 
**Animation:** slide+fade in over `motion.base`; reduced-motion → instant fade.

## 6.12 (merged into 6.11)

## 6.13 Snackbars / Undo toasts (DS-SNACK-*)
**Purpose.** Confirm a completed action and offer immediate Undo (DS-CONFIRM-3). **Behavior:** brief message + Undo action; auto-dismiss after a defined duration (`motion.snack.timeout`, default 6 s; longer for higher-consequence, pause on hover/focus); Undo triggers the same undo as `Ctrl/Cmd+Z`. Only one snackbar at a time; new replaces with a queue for rapid actions. **Accessibility:** polite announcement; Undo reachable by keyboard (a documented shortcut focuses the snackbar action while present, e.g., the toast is in the focus-forward order); never the only path to undo (menu/shortcut always exist). **Animation:** as §6.11.

## 6.14 Progress indicators (DS-PROG-*) — see also §11
**Purpose.** Communicate ongoing work. **Variants:** indeterminate spinner, determinate bar (with %), inline (in control/status bar), blocking (rare, modal). **Behavior:** show for any operation exceeding the perceptibility threshold (`motion.progress.appear`, ~200 ms — avoid flashing for fast ops by delaying appearance); determinate where progress is known, else indeterminate; always cancelable where the operation is (`NFR-RESP-2`) with a visible Cancel; on completion show a brief success or silently remove (per §11). **Accessibility:** progressbar role with value where determinate; status announced at throttled intervals, not continuously (§9). **Animation:** smooth, non-distracting; indeterminate motion respects reduced-motion by switching to a low-key pulsing or static "working…" text.

## 6.15 Tooltips (DS-TIP-*)
**Purpose.** Reveal a control's name/description and shortcut on hover/focus. **Behavior:** appear after a hover delay (`motion.tip.delay`, default 500 ms) or immediately on keyboard focus (for icon-only controls, so keyboard users get the name); dismiss on leave/blur/`Esc`; never obscure the control they describe; contain concise text + the shortcut. Rich tooltips (with a small diagram) MAY be used for complex tools but MUST remain non-interactive and dismissible. **Accessibility:** tooltip content MUST also be available as the control's accessible name/description (not tooltip-only), since AT users may not trigger hover; icon-only controls MUST have accessible names regardless of tooltip. **Animation:** quick fade `motion.fast`; reduced-motion → instant. **Performance:** tooltips MUST not cause layout reflow of the underlying UI.

## 6.16 Text inputs (DS-INPUT-*)
**Purpose.** Single- and multi-line text entry, numeric entry. **Variants:** text, multiline, numeric (with optional stepper + units), password/secret (for credentials), search (§6.18). **States:** rest, focus, filled, placeholder-shown, disabled, read-only, invalid, loading (async validation). 
**Behavior:** standard text editing (platform key bindings, selection, clipboard, undo within field); numeric supports steppers (buttons + Up/Down keys), min/max/step, and unit display; validation on commit and/or live with clear rules; invalid shows an error message (text + danger glyph, §9.5) associated with the field. Placeholder is not a label (a persistent label is required for accessibility, §9). 
**Keyboard:** all editing keys; `Esc` reverts to last-committed where the field supports revert; Up/Down for numeric. 
**Accessibility:** textbox/spinbutton roles; associated visible label; error via described-by + invalid state; required indicated textually. 
**Animation:** focus/border transitions `motion.fast`. **Performance:** live validation MUST NOT block typing (async validation off the input path).

## 6.17 (numeric covered in 6.16)

## 6.18 Search box (DS-SEARCHBOX-*)
**Purpose.** Enter search queries (in-document find §3.7.6, command search §6.25, list filters). **Behavior:** leading search glyph; clear (×) affordance when non-empty; optional scope/options control; live results or on-Enter per context (in-doc find shows first hit fast, `MET-PERF-5`); recent/suggested queries MAY appear as a popover. `Esc` clears then closes. **Keyboard:** focus via documented shortcut (`Ctrl/Cmd+F` find; command search `Ctrl/Cmd+Shift+P` or platform norm); Up/Down navigate suggestions/results; `Enter` next result; `Shift+Enter`/F3 prev. **Accessibility:** searchbox role; result count and current position announced (throttled); clear button labeled. **Animation:** suggestion popover per §6.2.

## 6.19 (tooltips = 6.15)

## 6.20 Inline edit (DS-INLINE-*)
**Purpose.** Edit a label in place (bookmark/layer/field names, tab rename where allowed). **Behavior:** enter via F2 or slow-double-click; the label becomes a text input sized to content; `Enter` commits (validated), `Esc` cancels to prior; commit on blur (documented) ; invalid keeps editing with error. **Accessibility:** editing state announced; the same accessible name updates on commit. **Animation:** none beyond focus; instant.

## 6.21 Tabs & Accordions (DS-TABS-*, DS-ACC-*)
**Tabs purpose.** Switch between peer views in the same space (document tabs §3.11 are a specialized instance; also property sub-tabs, dialog sections). **Behavior:** one tablist = one Tab stop; arrows move selection (roving), `Enter`/`Space` if manual activation, or auto-activate on focus per a documented per-use choice (default: automatic for lightweight, manual for expensive content). Overflow scrolls/collapses. **Accessibility:** tablist/tab/tabpanel with selected + controls relationships. 
**Accordions purpose.** Expand/collapse stacked sections (property groups, settings). **Behavior:** header toggles section; multiple-open vs single-open is a documented per-use choice; state persists where meaningful. **Accessibility:** button headers with expanded state controlling their region. **Animation:** expand/collapse height animation ≤ `motion.base`, reduced-motion → instant.

## 6.22 Splitters (DS-SPLITTER-*)
**Purpose.** Resize adjacent regions (§3.15, split view §3.12). **Behavior:** a grabbable divider (hit target ≥ `space.3` even if drawn as a hairline); drag resizes live; double-click resets to default; hover shows resize cursor. **Keyboard:** focusable; arrows resize by `space.3` step, `Home`/`End` to min/max, `Enter` optional collapse toggle. **Accessibility:** separator role with orientation and value; keyboard operable (DS-DOCK-5). **Animation:** none during drag (1:1); collapse animates ≤ `motion.fast`.

## 6.23 Cards (DS-CARD-*)
**Purpose.** Group related content as a unit (recent-file entries, plugin entries, empty-state containers, settings groups). **Behavior:** `elev.0` with hairline or `elev.1` if raised; may be actionable (whole-card click) or a container; actionable cards behave as buttons (role, keyboard). **Accessibility:** if actionable, single focus stop with accessible name; if container, its interactive children are the stops. **Animation:** hover raise (if interactive) `motion.fast`.

## 6.24 Color picker (DS-COLORPICK-*)
**Purpose.** Choose annotation/markup colors and (advanced) other color values. **Variants:** swatch palette (default, curated set + recents), full picker (HSV/RGB/Hex, and where relevant CMYK/spot for prepress — advanced), eyedropper (pick from canvas). **States:** rest, open, with-selection, invalid hex. **Behavior:** palette first (fast common case, DS-PHIL-6); "more" reveals the full picker; recents remembered; eyedropper samples true document color (DS-COLOR-7 aware — samples the document, informative to the user). Alpha where applicable. **Keyboard:** palette is a grid (arrow-navigable, roving); full picker fields keyboard-editable; hex input validated. **Accessibility:** each swatch has an accessible name (color name + value), selected state; the picker is fully keyboard-operable; color is never the only info (value text shown). **Animation:** popover per §6.2.

## 6.25 Command search / command palette (DS-CMDP-*) [UX Decision]
**Purpose.** Find and run any command by name; the discoverability backstop that lets the product keep a stable layout while remaining searchable (DS-PHIL-1/6, `UX-DISC-2`). **Behavior:** invoked by shortcut (default `Ctrl/Cmd+Shift+P`, or a "Find action" affordance); type to fuzzy-match commands, showing each match's name, category, and shortcut; `Enter` runs the focused command; recent/frequent commands surface first. It MUST cover every command (DS-MENU-2). **Keyboard:** fully keyboard-driven; Up/Down navigate, `Enter` run, `Esc` close (focus restore). **Accessibility:** combobox+listbox pattern; results and selection announced; running a command returns focus sensibly. **Performance:** results update within a frame of typing on the full command set. **Animation:** popover per §6.2; no delay.

## 6.26 File picker & Recent files (DS-FILE-*, DS-RECENT-*)
**File picker purpose.** Open/save files. **Behavior:** use the **native OS file dialog** by default (familiarity, platform integration, accessibility for free, DS-PHIL-1, `UX-CONS-2`); a custom in-app browser MAY supplement for special flows (e.g., combine-files ordering) but MUST NOT replace the native dialog as the default. Save flows MUST clearly distinguish Save (incremental, default) vs Save As / Save a Copy (§7.2, §7.3) and MUST disclose destructive rewrite implications (DS-PHIL-8, `FR-SAVE-2`). 
**Recent files purpose.** Fast reopen (`UX-DISC-1`). **Behavior:** a list/grid of recent documents with name, path (middle-truncated §2.2), last-opened, and thumbnail where available; pin/unpin; remove-from-list; clear-list (privacy, `NFR-PRIV-4`). Recents are per-user, local (DS-PHIL-8). **Accessibility:** list semantics; each entry named with document + last-opened; keyboard-openable; remove/pin reachable. **Empty state:** first-run "No recent documents — Open a file to begin" with an Open affordance (§8.1). **Animation:** none required beyond list norms.

## 6.27 Component baseline compliance (DS-COMP-QA)
Every component above MUST pass the §13 Design QA checklist for its category before it is considered shippable; a component missing any inherited baseline clause (DS-COMP-BASE) is non-conformant regardless of visual completeness.

---

# 7. PDF Editing UX (Workflow Interaction Flows)

*Normative.* Each workflow specifies its complete interaction flow: entry, steps, feedback, completion, undo, error, and accessibility. Flows reference components (§6), canvas (§5), and error design (§8). All document mutations obey the non-destruction and honesty principles (DS-PHIL-8, `PRIN-2`, `PRIN-6`) and are undoable (§7.6) and attributable.

## 7.1 Opening a document (DS-FLOW-OPEN)
**Entry.** File ▸ Open (`Ctrl/Cmd+O`), drag-file-onto-window, OS "open with", recent-files (§6.26), or CLI hand-off.
**Flow.** (1) Native file picker (or direct for drag/recent). (2) The window shows the document frame with a loading skeleton (§5.1, §11) immediately; the first visible page appears within the first-page budget (`MET-PERF-2`), progressively refining (§5.1). (3) If the file is encrypted, a password prompt (modal, §6.8) appears before content; wrong password re-prompts with a clear, non-accusatory error (§8, DS-PHIL-6). (4) If the file was damaged and repaired, a non-modal, honest notice appears ("This document was repaired to open. View details.") linking to the diagnostics panel (§3.10.2, `FR-DIAG-1`) — never a blocking dialog for a successful repair (DS-PHIL-7). (5) If the file contains XFA / scripts / rich media, a status-bar indicator surfaces it (§3.13, honesty) without interrupting.
**Completion.** Document interactive; view state restored if previously opened (DS-PERSIST-2); focus placed in the canvas.
**Accessibility.** Load progress announced (throttled); on ready, the canvas region is focusable and the document title announced; a scanned/untagged document announces limited structure with a path to OCR/remediation (§5.10, §8.1).
**Performance.** Cold-start and first-page budgets (`MET-PERF-1/2`) gate this flow.

## 7.2 Saving & incremental save (DS-FLOW-SAVE)
**Entry.** File ▸ Save (`Ctrl/Cmd+S`), or auto-prompt on close-if-dirty.
**Flow.** (1) Save is **incremental by default** (`FR-SAVE-1`, `[ADR-012]`): fast, size-independent (`MET-PERF-6`), preserving untouched bytes and existing signatures. (2) A brief success confirmation (status bar + dirty indicator clears); no modal on success. (3) If the document is unsaved-new, Save behaves as Save As (native dialog). (4) If saving would break an existing signature because of the pending change, the product MUST disclose this and require explicit confirmation before proceeding (§6.10, `FR-SIG-4`, DS-PHIL-8).
**Dirty indicator.** The tab (§3.11) and title show an unsaved-changes marker whenever the document differs from disk (DS-PHIL-3).
**Undo relationship.** Saving does not clear undo history (the user can still undo past a save within the session; recovery journal per `SDS §10`).
**Accessibility.** Save result announced ("Saved"); errors (§8) announced assertively with cause and remedy.
**Error.** Disk full / permission denied / file locked → §8 patterns with a retry and a "Save As elsewhere" path; work is never lost (the in-memory document and recovery journal remain, `SDS §10`).

## 7.3 Save As / Save a Copy / clean rewrite (DS-FLOW-SAVEAS)
**Entry.** File ▸ Save As… / Save a Copy… / Save Optimized/Clean…
**Flow.** (1) Native save dialog for destination. (2) **Save As** creates a new file and continues editing it; **Save a Copy** writes a copy and keeps editing the original (the distinction MUST be explicit in labels, DS-PHIL-3). (3) A **clean/optimized rewrite** (full rewrite, garbage-collect, optionally flatten history/sanitize) MUST first show a **pre-flight disclosure** enumerating exactly what will be lost or changed (history, signatures, tags, quality) and require confirmation (§6.10, `FR-SAVE-2`, `FR-OPT-2/3`, DS-PHIL-8). 
**Accessibility.** The pre-flight disclosure is an accessible dialog whose losses are read as a list (§9.10). 
**Undo.** The rewrite of the current document (if it replaces it) is a significant operation; where it produces a new file, the original on disk is untouched.

## 7.4 Recovery (DS-FLOW-RECOVER)
**Entry.** Automatic on next launch after an unclean exit (`FR-REC`, `SDS §10.2`).
**Flow.** (1) A **recovery surface** lists each document with unsaved work, showing an itemized summary of the changes to be restored (named command groups + timestamps) and a per-document Restore / Discard choice (DS-PHIL-8: itemized, honest). (2) Restored documents open dirty (unsaved), exactly as before the interruption. (3) Discarding requires no confirmation beyond the explicit choice, but the recovery data is only deleted after the user acts (nothing lost by ignoring the prompt).
**Accessibility.** The recovery list is a table (§9.11) with per-row actions; screen-reader users hear document name + change summary + choices.
**Determinism.** The same journal always reconstructs the same state (`SDS §10.2`).

## 7.5 (reserved)

## 7.6 Undo / Redo (DS-FLOW-UNDO)
**Entry.** `Ctrl/Cmd+Z` / `Ctrl/Cmd+Shift+Z` (redo; also `Ctrl+Y` on Windows), Edit menu, QAT.
**Flow.** Every document-modifying action is undoable/redoable with the action **named** in the menu and (where used) the snackbar ("Undo Delete Pages") (`FR-UNDO`, `UX-UNDO-1`). Undo depth is effectively unlimited within a session (`UX-UNDO-2`) and persists for recovery (`FR-UNDO-2`). Grouped operations undo as one unit (DS-PHIL-3). Actions that cannot be undone (explicit destructive rewrite/sanitize) are labeled as such *before* commit (§6.10, `UX-UNDO-3`) and are NOT silently placed on the undo stack as if reversible.
**Feedback.** The undo/redo affordances name the next action; disabled when none.
**Accessibility.** The named action is announced on undo/redo ("Undone: Delete Pages").
**Performance.** Undo/redo of a typical action ≤1 frame perceived; large-group undo shows progress if non-instant.

## 7.7 Annotations — general (DS-FLOW-ANNOT)
**Entry.** Comment toolset (toolbar/menu/`workspace: Review`), or specific tool shortcuts.
**Flow (common).** (1) Select an annotation tool → cursor + mode indicator update (§4.1, §4.9); the tool is sticky/momentary per its default (§4.1). (2) Create on canvas per tool (below). (3) The new annotation is selected, its properties available in the Properties panel (§3.9) and via a compact on-canvas mini-toolbar (appearing near the selection, non-occluding). (4) Default properties (color, etc.) persist per tool for the next use (DS-PHIL-7). (5) Every create/edit/move/delete is undoable and writes a complete portable appearance (`FR-ANNOT-2`). 
**Accessibility.** Every annotation tool MUST be usable by keyboard: place/size via keyboard (e.g., create-at-focus then adjust via properties), and the resulting annotation is reachable and editable from the Comments panel (§3.10.1) and canvas keyboard traversal (§5.10). Author identity, type, and content are exposed to AT. 
**Error.** Attempting to annotate a region where it's not permitted (e.g., a signed doc where change is disallowed) surfaces an honest explanation (§8, `FR-SIG-4`).

### 7.7.1 Highlight / text markup (DS-FLOW-HILITE)
Select the text-markup tool → I-beam cursor; drag over text (or select text then apply) → highlight/underline/strikeout attaches to the text region (`FR-ANNOT-3`), tracking content. Keyboard: select text via keyboard (§5.10) then apply markup shortcut. Color per current default (§6.24). Undoable.

### 7.7.2 Ink / freehand (DS-FLOW-INK)
Select ink tool → crosshair/pen cursor; draw with mouse/pen (pressure/tilt where available, low latency, palm rejection, §4.11, `FR-ANNOT-7`); stroke smoothing per token (§4.11 DS-PEN-6). Multiple strokes group into one ink annotation until tool change/commit. Eraser (pen eraser or tool) removes strokes. Keyboard alternative: not primary for freehand, but ink annotations are selectable/movable/deletable by keyboard.

### 7.7.3 Shapes (DS-FLOW-SHAPE)
Line/arrow/rectangle/ellipse/polygon/polyline. Drag to create; Shift constrains (square/circle/45°); polygon/polyline click-to-add-vertex, double-click/`Enter` to finish, `Esc` cancels. Snapping (§5.5) aids precision. Properties (stroke, fill, opacity, arrowheads) in Properties panel. Keyboard: create default-size at focus then adjust numerically.

### 7.7.4 Stamps, notes, free text, callouts (DS-FLOW-STAMP)
Place a note (sticky) → a marker on canvas + editable text in a popup/panel. Free text → a text box drawn on the page, edited inline (§6.20-like). Stamp → choose from a stamp set, place, size. Callout → text box + leader line to a point. All undoable, appearance written.

## 7.8 Comments & review (DS-FLOW-REVIEW)
**Entry.** Comments panel (§3.10.1), `Review` workspace.
**Flow.** Navigate comments (filter/sort), reply (threaded), set status (accepted/rejected/completed), edit/delete (own; others per policy), and export a summary (`FR-REV`). Canvas ↔ panel selection stays in sync (§5.3). Aggregating comments from multiple copies (`FR-REV-5`) presents a reconciled list identifying source. 
**Accessibility.** Thread structure, author, status conveyed textually; all actions keyboard+context-menu reachable. 
**Determinism.** Status and threading render identically across sessions and for other reviewers (portable data, `PRIN-7`).

## 7.9 Text editing (DS-FLOW-TEXTEDIT) *(later phase; V3)*
**Entry.** Edit ▸ Edit Text & Images, or the Edit tool.
**Flow.** (1) Entering edit mode reveals editable text blocks with subtle boundaries (view overlay, not document change). (2) Click into a block → text caret; edit with standard text keys; layout is preserved as far as the content model allows (`PRIN-2`). (3) **Honesty rule (critical, DS-PHIL-8, `PRIN-6`):** if the embedded font subset lacks a needed glyph, the product MUST surface "cannot edit safely — font subset incomplete" and offer explicit substitution-with-embedding as a choice, NEVER a silent swap. (4) All edits undoable and incremental-save-clean (no whole-document transcode). 
**Accessibility.** Edit mode announces itself; caret and edits exposed to AT; the honesty prompt is an accessible dialog. 
**Error.** Unsupported edit → honest explanation, no partial corruption.

## 7.10 Image / object editing (DS-FLOW-IMGEDIT) *(V2)*
Select an image/object (§5.3) → move/resize (handles §5.4, aspect constrain)/rotate/replace/delete via canvas + properties. Replace opens a picker; the new image is embedded (size disclosure via optimize, §7.15). Undoable, non-destructive to the rest of the page.

## 7.11 Forms (DS-FLOW-FORMS)
**Fill flow.** Tab moves between fields in the document's tab order (`FR-FORM-2`); each field type behaves per its control (text, checkbox, radio, combo, list, button, signature); required fields indicated; validation/calculation via the JS forms subset runs and updates dependent fields (`FR-JS-1`), with any unsupported script behavior skipped honestly and logged (§3.10.2, `FR-JS-3`, never a fake result). Appearances regenerate so other readers see the values (`FR-FORM-1`). 
**Flatten.** Explicit, undoable, discloses loss of interactivity (§6.10, `FR-FORM-4`). 
**Authoring (V3).** Create/place/configure/order fields with a fields panel + properties; tab-order editor. 
**Accessibility (critical).** Forms MUST be fillable accessibly: each field has a label/name/description and correct order to AT; validation errors are announced and associated (`FR-FORM-7`, `US-AXS-3`, §9). A JS-present indicator and disable control are available (`FR-JS-4`). 
**Determinism.** The same form computes the same results as reference readers for the supported subset (`MET-*`).

## 7.12 OCR (DS-FLOW-OCR)
**Entry.** Tools ▸ Recognize Text (single/batch), or on-open suggestion for a scanned/untagged document (§5.10, §8.1) — suggested, never automatic (DS-PHIL-8).
**Flow.** (1) Choose language(s), pages (including the ability to OCR pages that already contain some text — fixing the incumbent refusal, `FR-OCR-3`), and options (preprocessing, output/archival). (2) Progress shown (determinate where possible), cancelable (§6.14). (3) On completion, an **invisible, correctly-registered text layer** is added without altering appearance (`FR-OCR-1`); the page becomes searchable/selectable. (4) Low-confidence results are flagged, not silently inserted as truth (`FR-OCR-4`, DS-PHIL-6). 
**Accessibility.** Progress and completion announced; the new text layer makes the document accessible where it was an image; confidence flags exposed. 
**Error.** OCR failure (§8) explains and leaves the document unchanged (non-destructive).

## 7.13 Redaction (DS-FLOW-REDACT)
**Entry.** Tools ▸ Redact, `Redact`/legal workspace.
**Flow.** (1) **Mark** regions (drag) and/or **mark by search** (redact all occurrences of a term across selected pages/whole doc, `FR-RED-5`); marks are a distinct, clearly-styled overlay (not yet applied). (2) Review marks (a list + canvas). (3) **Apply** → the product removes underlying content (text, vector, image), covered annotations, and associated recoverable data (`FR-RED-1/2`), then runs a **verification pass** confirming non-recoverability and can produce a report (`FR-RED-3`). (4) The apply step is explicit and, because it is irreversible removal, uses the destructive-confirmation pattern (§6.10) with clear disclosure; it MUST NOT be completable as a cosmetic-only operation (`FR-RED-4`, no black-box-only path). (5) The result cannot be "applied/saved as removed" until verification passes (`SDS §3.3.1`). 
**Accessibility.** Marks list and apply flow keyboard-complete; verification result announced; report accessible. 
**Determinism & trust.** 100% verified removal on the redaction corpus is an absolute metric (`MET-FEAT-5`). This is the legal-trust workflow; its honesty is non-negotiable (DS-PHIL-8).

## 7.14 Digital signatures (DS-FLOW-SIGN)
**Validate flow.** On open (or in the Signatures panel §3.7.5), each signature shows plain-language status — valid / invalid / **indeterminate** — with semantic color + glyph (never color alone), expandable to signer, time, coverage, and permitted/illegal post-signing changes (`FR-SIG-1/2`). An indeterminate/unverifiable signature is NEVER shown as valid (DS-PHIL-8). 
**Sign flow.** (1) Choose a certificate (software store or hardware token/smart card where delivered, `FR-SIG-3`; brokered credential access, §8 consent). (2) Choose visible/invisible and (if certifying) permitted subsequent changes (`FR-SIG-5`). (3) Place the visible signature field if visible. (4) Apply → the signature is written as a permitted incremental update preserving prior signatures (`FR-SIG-4`, `[ADR-012]`); a timestamp and long-term validation data are embedded per the chosen profile. (5) Result confirmed; the Signatures panel updates. 
**Accessibility.** Certificate selection, options, and results are keyboard-complete and announced; status is textual (not color) for AT (`US-AXS`, §9). 
**Trust rule.** Validation is conservative; trust derives only from configured trust (`ENT-CERT`), and unverifiable trust yields indeterminate (`FR-SIG-1`).

## 7.15 Optimize / compress (DS-FLOW-OPTIMIZE)
**Entry.** File ▸ Optimize / Reduce Size, or batch.
**Flow.** Choose a profile (screen/print/archive-preserving/custom, `FR-OPT-4`); a **pre-flight disclosure** shows expected size reduction and any quality trade-offs (image downsampling) and what will NOT be touched (tags/signatures/metadata unless explicitly chosen) (`FR-OPT-2/3`, DS-PHIL-8). Apply is undoable (or produces a copy). 
**Accessibility.** Disclosure read as a list; controls keyboard-complete.

## 7.16 Merge / combine (DS-FLOW-MERGE)
**Entry.** File ▸ Combine Files, or drag multiple files in.
**Flow.** (1) A combine list (reorderable, §6.4) lets the user order files/pages, choose ranges, and preview. (2) Options: bookmark handling, resource de-duplication (to avoid bloat, `FR-MERGE-1`). (3) Combine → a new document; undoable within the session; bookmarks/destinations reconciled sensibly. 
**Accessibility.** The ordering list is keyboard-reorderable (§6.4, §9). 
**Determinism.** No unnecessary resource duplication (verified, `FR-MERGE-1`).

## 7.17 Split / extract (DS-FLOW-SPLIT)
Split by ranges/count/size/bookmarks (`FR-SPLIT-1`) → multiple valid files with a clear naming scheme (previewed). Extract a range → new document, optionally removing from source (explicit choice, `FR-EXTRACT-1`). Both available in batch/CLI (parity). Undoable where they modify the source.

## 7.18 Crop / rotate (DS-FLOW-CROPROTATE)
Crop per §5.8 (reversible default, numeric precision, honesty about hide vs remove). Rotate selected pages in 90° steps as an undoable document change (distinct from view rotation, `FR-ROTATE-1`); thumbnails and canvas update. Both batch/CLI-capable.

## 7.19 Compare (DS-FLOW-COMPARE)
**Entry.** Tools ▸ Compare Documents.
**Flow.** Choose two documents/versions → a side-by-side (split, §3.12) and/or an overlay view with differences enumerated in a navigable list (added/removed/changed/moved where detectable), resilient to reflow (`FR-CMP`). Navigate differences by keyboard; each difference is located in both views. 
**Accessibility.** Difference list is a navigable, labeled list; each entry describes the change textually (not color-only). 
**Determinism.** Meaningful-change detection prioritized over positional noise (`FR-CMP-3`).

## 7.20 Accessibility repair / remediation (DS-FLOW-REMEDIATE) *(V3)*
**Entry.** Tools ▸ Accessibility, `Accessibility` workspace.
**Flow.** (1) **Check** validates against the accessibility standard and lists issues with locations and guidance (`FR-A11Y-5`, `FR-STD-4`). (2) **Fix** tools let the user set/correct tags, reading order (a reading-order editor showing the sequence over the page), alternative text, table structure, and language — non-destructively and undoably (`FR-A11Y-4`, `PRIN-2`). (3) Re-check confirms conformance; the product MUST NOT declare conformance it does not meet (`FR-STD-5`, DS-PHIL-8). 
**Accessibility (meta).** The remediation tools themselves MUST be fully accessible (a screen-reader user can remediate a document, `PRIN-8`). Reading-order editing is keyboard-complete. 
**Determinism.** Remediated documents validate against recognized validators (`MET-FEAT-3`).

## 7.21 Printing (DS-FLOW-PRINT)
**Entry.** File ▸ Print (`Ctrl/Cmd+P`).
**Flow.** The **native print dialog** (DS-PHIL-1) with app-provided options: range, scaling (fit/actual/custom), duplex, print comments/annotations toggle, and (later) imposition/booklet and prepress options (`FR-PRINT`). A preview reflects options. Output matches on-screen within fidelity budget (`FR-PRINT-3`). 
**Accessibility.** Fully keyboard-operable; preview state described; native dialog carries native accessibility. 
**Error.** Printer/driver errors surfaced per §8 with retry.

---

# 8. Error, Empty, Loading & Exceptional States

*Normative.* This section defines how every non-nominal state appears. The governing law is honesty (DS-PHIL-8, `PRIN-6`): the product tells the truth about what happened, attributes cause correctly (`UX-ERR-2`), never shows false success or false "valid," and always offers the next step. **No error state may rely on color alone (§9.5); every state pairs an icon/shape + text.**

## 8.1 Empty states (DS-EMPTY-*)
**DS-EMPTY-1 (Pattern).** An empty state has: an optional restrained illustration/icon (§2.7), a `type.subtitle` headline stating the situation plainly, a `type.body` line explaining or guiding, and (where an action applies) a primary affordance. It MUST be honest and non-nagging (DS-PHIL-10). 
**DS-EMPTY-2 (Instances, each specified):** 
- **No document open** (canvas): "No document open — Open a file or drag one here" + Open button + recent files (§6.26). 
- **No recent files**: "No recent documents" (§6.26). 
- **Empty panel** (bookmarks/comments/layers/attachments/signatures): each states the truth: e.g., Signatures → "This document is not signed"; Bookmarks → "This document has no bookmarks" + (if editable) Add; Comments → "No comments yet"; Layers → "This document has no optional layers"; Attachments → "No attachments." 
- **No search results**: "No results for '<query>'" + scope hint. 
- **Untagged/scanned document** (accessibility/structure): honest note that structure is unavailable + OCR/remediation path (§5.10). 
**DS-EMPTY-3 (a11y).** Empty-state text is read to AT; the primary action is focusable.

## 8.2 Loading states (DS-LOADING-*)
**DS-LOADING-1.** Use skeletons for structured content (canvas pages §5.1, panels, tables) and spinners/bars for actions (§6.14, §11). **DS-LOADING-2.** Delay appearance by `motion.progress.appear` (~200 ms) to avoid flashing on fast operations (§11). **DS-LOADING-3.** Loading MUST NOT block interaction beyond the specific pending element (`NFR-RESP-1`); the app stays responsive. **DS-LOADING-4.** Long loads show progress + cancel where cancelable (`NFR-RESP-2`). **DS-LOADING-5 (a11y).** Loading announced politely and throttled; completion announced.

## 8.3 Offline state (DS-OFFLINE-*)
**DS-OFFLINE-1.** Because all core functionality is offline (`VIS-3`, `NFR-OFFLINE`), the absence of network MUST NOT produce any error, nag, or degradation for core work (DS-PHIL-8). **DS-OFFLINE-2.** Only an explicitly network-dependent optional feature (future) may reflect offline status, and only within that feature's surface, with a calm inline message and retry — never a global modal (§10, `FR-CLOUD`). **DS-OFFLINE-3.** The product MUST NOT display connectivity indicators that imply network is expected for normal use (DS-PHIL-10).

## 8.4 Corrupt / damaged PDF (DS-ERR-CORRUPT)
**On open with successful repair:** non-modal honest notice ("This document was repaired to open. View details.") → diagnostics panel leniency report (§3.10.2, `FR-VIEW-2`, `FR-DIAG-1`). Never block a successful repair (DS-PHIL-7). **On partial recovery:** show what rendered, mark unrecoverable pages/areas honestly (a page that cannot render shows an explicit "This page could not be displayed" placeholder, not blank, `FR-REC-3`, `SDS §10.1`), and offer a salvage-export path where possible. **On unrecoverable:** a specific diagnosis (not a generic failure), with any partial-salvage option (`SDS §10.4`). **A11y:** notices and placeholders are announced and described.

## 8.5 Worker crash (DS-ERR-WORKER)
Per `SDS §10.1`: a document-processing failure MUST be contained — the app stays usable; the affected document recovers transparently (re-rendered from intact state) with at most a brief re-render and an optional subtle notice; repeated failure on a specific page trips a circuit breaker showing that page as an explicit "cannot render" placeholder while the rest stays usable. No data loss (state is authoritative outside the worker, `SDS §10.1`). **A11y:** any user-visible notice is announced; the placeholder is described.

## 8.6 Plugin crash (DS-ERR-PLUGIN)
Per §10 and `SDS §11.5`: a plugin fault MUST be contained to the plugin — the host and documents are unaffected. The product shows a non-modal notice ("The plugin '<name>' stopped and was disabled. View details.") with a details/report path and a re-enable option; the plugin's contributed UI is cleanly retracted (§10.9). Repeated faults → the product may keep it disabled and say so (DS-PHIL-8). **A11y:** notice announced; contributed UI removal does not strand focus (§4.2).

## 8.7 Out of memory / resource pressure (DS-ERR-MEM)
Per `SDS §9.3`: the product degrades gracefully (reduced caching/quality) rather than failing (`NFR-MEM-3`), and MUST NOT crash from cache growth. If a specific operation genuinely cannot complete for lack of resources, it fails with an honest, specific message and a suggestion (e.g., close other documents, use batch/CLI for very large jobs), leaving the document intact. The UI thread is never blocked by reclamation (`SDS §9.3`). **A11y:** the condition and suggestion are announced.

## 8.8 Unsupported feature / construct (DS-ERR-UNSUPPORTED)
When a document uses something the product does not support (XFA rendering, an unsupported script behavior, an exotic construct), the product MUST disclose it honestly and specifically (status bar indicator + diagnostics detail, `FR-VIEW-7`, `FR-JS-3`, `FR-DIAG-1`), render everything it can, and never present an incomplete result as complete (DS-PHIL-8). It MUST NOT fabricate or approximate a result where fidelity matters (e.g., no fake script results). **A11y:** the presence and nature of unsupported content are exposed to AT.

## 8.9 Recovery & retry (DS-ERR-RETRY)
**DS-RETRY-1.** Every recoverable error MUST offer a clear retry and/or an alternative path; retry MUST be idempotent-safe (no duplicate side effects). **DS-RETRY-2.** Transient failures (locked file, busy device) suggest retry; systemic failures (permission, unsupported) explain the cause and the real remedy rather than a futile retry (DS-PHIL-6). **DS-RETRY-3.** No error flow may end in a dead end; there is always a next action, even if it is "save a copy elsewhere" or "view details."

## 8.10 Fatal error (DS-ERR-FATAL)
If the application itself must fail, it MUST: (1) preserve user work via the recovery journal (`SDS §10`) so nothing beyond the durability budget is lost; (2) show a single honest, non-technical-by-default message with an option to view technical detail and to save a local crash report the user can review before sharing (`NFR-PRIV-3`, no auto-transmission, DS-PHIL-10); (3) on relaunch, offer recovery (§7.4). No fatal path may silently discard work. **A11y:** the message is an accessible alert.

## 8.11 Permission denied (DS-ERR-PERM)
For OS-level permission failures (file/folder/device/credential): an honest message naming what was denied and how to grant it (path to the relevant OS setting where possible), with a retry. For document-permission (advisory PDF permissions) restrictions: the product honors them by default but discloses that they are advisory, not security (`FR-PERM-1`, `PRIN-6`), and never misrepresents them as enforcement. **A11y:** announced with cause + remedy.

## 8.12 Signature failure / indeterminate (DS-ERR-SIG)
A signature that is invalid or indeterminate MUST be shown as such, never as valid (`FR-SIG-1`, DS-PHIL-8). The Signatures panel and any inline badge use semantic color + glyph + text; expanding explains *why* in plain language (e.g., "The document was changed after signing in a way the signature does not permit," or "The signer's certificate could not be verified against configured trust"). No alarming or accusatory tone; just accurate cause and, where applicable, remedy (e.g., "add the issuer to trusted certificates"). **A11y:** status and reason textual to AT.

## 8.13 OCR failure (DS-ERR-OCR)
If OCR cannot process (unreadable image, unsupported language pack missing), the product explains specifically, leaves the document unchanged (non-destructive), and offers remedies (install language data, adjust preprocessing, try a region). Low-confidence output is flagged rather than presented as certain (`FR-OCR-4`). **A11y:** result and flags announced.

## 8.14 Validation failure (DS-ERR-VALIDATE)
For standards validation (PDF/A, PDF/UA, PDF/X) or pre-flight, failures/issues are presented as a **navigable, itemized report** with each issue's location and remediation guidance (`FR-STD-1`), not a single pass/fail with no detail. The product MUST NOT claim conformance it does not meet (`FR-STD-5`). **A11y:** the report is an accessible table/list; navigation to each issue is keyboard-complete.

## 8.15 Consistency of error presentation (DS-ERR-CONSISTENT)
**DS-ERR-CONSISTENT-1.** All error/empty/loading states MUST use the shared components (§6.11 notifications, §6.8 dialogs, §6.14 progress, §8.1 empty pattern) and the semantic + glyph + text rule (§9.5). Two different subsystems reporting the same class of problem MUST look and behave the same (DS-PHIL-3). **DS-ERR-CONSISTENT-2.** Severity mapping is fixed: *info* (non-blocking, auto-dismiss), *warning* (persistent, acknowledge), *error* (blocks the specific action, requires resolution/retry), *fatal* (§8.10). **DS-ERR-CONSISTENT-3.** Tone is factual and respectful; never blames the user; never uses alarm language for tolerated conditions (repairs, unsupported) (DS-PHIL-6).

---

# 9. Accessibility

*Normative and gating.* Accessibility is a precondition of "done" (DS-PHIL-4, `PRIN-8`, `NFR-A11Y`). Every requirement here is testable and part of per-release gating (`MET-A11Y-*`). Accessibility MUST NOT regress across releases (`NFR-A11Y-3`, part of the stability contract, §14).

## 9.1 Conformance target
**DS-A11Y-1.** The application MUST meet, at minimum, WCAG 2.2 Level AA success criteria as adapted for desktop software, and MUST meet each platform's native accessibility guidelines (Windows UIA expectations, macOS Accessibility, Linux AT-SPI) (`NFR-A11Y-2`). **DS-A11Y-2.** Where WCAG and a platform convention differ, the product MUST satisfy both intents; platform-native behavior wins for platform-standard interactions (DS-PHIL-1). **DS-A11Y-3.** Document-accessibility features (reading tagged PDFs, remediation) target PDF/UA and WCAG as applies to content (§7.20, `FR-A11Y`, `FR-STD-4`).

## 9.2 Contrast
**DS-A11Y-CONTRAST-1.** Text and meaningful non-text UI MUST meet contrast ratios: ≥4.5:1 for normal text, ≥3:1 for large text (≥18.66 dp bold or ≥24 dp) and for meaningful graphical/UI boundaries and icons that convey state, in **every theme** (light/dark/high-contrast) (`NFR-A11Y`). **DS-A11Y-CONTRAST-2.** Focus indicators meet ≥3:1 against adjacent colors (§4.2). **DS-A11Y-CONTRAST-3.** Semantic status colors (§2.9) meet contrast against their backgrounds and are verified in QA (§13). **DS-A11Y-CONTRAST-4.** Disabled controls are exempt from the text-contrast minimum but MUST remain distinguishable as disabled by more than color (§9.5).

## 9.3 High-contrast & forced-colors
**DS-A11Y-HC-1.** The product MUST honor OS high-contrast/forced-colors modes (§2.13), mapping to system palette roles and replacing shadow/subtle-color meaning with borders and explicit fills. **DS-A11Y-HC-2.** In forced-colors, no information may be lost; every state remains perceivable (§9.5). **DS-A11Y-HC-3.** Focus is ≥3 dp system highlight in high-contrast.

## 9.4 Focus visibility
**DS-A11Y-FOCUS-1.** Focus MUST always be visible during keyboard interaction (§4.2), never suppressed. **DS-A11Y-FOCUS-2.** Focus order matches visual/logical order and is stable (§4.3, part of stability contract). **DS-A11Y-FOCUS-3.** Focus is never trapped except intentionally in modals (§9.10), and is always restorable (§4.2 DS-FOCUS-4).

## 9.5 Non-color-dependence (universal rule)
**DS-A11Y-COLOR-1.** No information, state, or affordance may be conveyed by color alone. Every color-coded meaning MUST be paired with a text label, icon/shape, pattern, or position (`UX` accessibility; color-blindness). This applies to: status (§2.9), selection, validation/errors (§8), signature status (§7.14), diff results (§7.19), required fields, toggle/selected states (§6.1). **DS-A11Y-COLOR-2.** The product MUST remain fully usable under monochrome and the common color-vision deficiencies; QA verifies via simulation (§13). **DS-A11Y-COLOR-3.** This rule is a hard gate; a design that fails it is non-conformant regardless of aesthetics.

## 9.6 Reduced motion
**DS-A11Y-MOTION-1.** The product MUST honor the OS reduced-motion setting: movement-based transitions become instant or simple cross-fades; no parallax, no non-essential motion (§11.2, DS-MOTION-2). **DS-A11Y-MOTION-2.** No information may be conveyed only by motion; a moving indicator has a static equivalent (§6.14). **DS-A11Y-MOTION-3.** Reduced-motion MUST NOT reduce functionality or feedback clarity.

## 9.7 Large fonts / scaling
**DS-A11Y-SCALE-1.** The UI MUST respect OS text-scaling up to ≥200% without loss of content or function (truncation resolves via wrapping/tooltip/scroll, not disappearance) (§2.2 DS-TYPE-5). **DS-A11Y-SCALE-2.** Layouts MUST reflow gracefully; no fixed-height container may clip scaled text. **DS-A11Y-SCALE-3.** Zoom of the document is independent of UI scaling and both MUST work (§4.6).

## 9.8 Touch targets
**DS-A11Y-TOUCH-1.** Interactive targets MUST have an effective size ≥44×44 dp (via padding/tolerance even when the visual glyph is smaller) in touch contexts; pointer contexts SHOULD meet ≥24×24 dp effective with adequate spacing (§6 baseline). **DS-A11Y-TOUCH-2.** Adjacent targets MUST have enough separation to prevent mis-activation (≥ `space.2`). **DS-A11Y-TOUCH-3.** Canvas handles meet targets via tolerance (§5.4).

## 9.9 Screen reader support
**DS-A11Y-SR-1.** Every UI element MUST expose correct **role, name, value, and states** to the platform accessibility API (UIA/AX/AT-SPI) (`NFR-A11Y-1`). **DS-A11Y-SR-2.** Dynamic changes MUST be announced appropriately: polite for status/progress (throttled to avoid flooding), assertive for errors requiring attention (§6.11, §6.14). **DS-A11Y-SR-3.** Composite widgets expose their pattern correctly (menus, trees, tabs, grids, comboboxes — §6). **DS-A11Y-SR-4.** Announcements MUST be localized (`NFR-LOC`). **DS-A11Y-SR-5.** The reading of the document itself (tagged content) is exposed via the canvas accessibility model (§9.12). **DS-A11Y-SR-6.** No essential information is available only visually; nothing is screen-reader-invisible that a sighted user can act on.

## 9.10 Accessible dialogs & focus management
**DS-A11Y-DIALOG-1.** Modal dialogs use dialog/alertdialog roles, are labeled by their title and described by their body, move focus in on open (to the first field or a safe default, not the destructive button), trap focus while open, and restore focus to the invoker on close (§6.8, §4.2). **DS-A11Y-DIALOG-2.** Alerts announce on appearance. **DS-A11Y-DIALOG-3.** Non-modal surfaces (popovers, floating panels) do not trap focus and are escapable (§3.16, §6.2). **DS-A11Y-DIALOG-4.** A dialog MUST be fully operable and readable by keyboard and screen reader before it ships.

## 9.11 Accessible tables & lists
**DS-A11Y-TABLE-1.** Data tables (§6.6) expose row/column headers and their associations, sort state, selection state, and row/column counts. **DS-A11Y-TABLE-2.** Lists/trees (§6.4) expose position-in-set, set-size, level, expanded, selected, checked. **DS-A11Y-TABLE-3.** Virtualized collections MUST still convey the total set size and current position (not just the rendered window). **DS-A11Y-TABLE-4.** Editable cells expose editing state and validation.

## 9.12 Accessible canvas (document)
**DS-A11Y-CANVAS-1.** The document MUST be exposed to AT as navigable, structured content — using the tagged reading order where present — not as an opaque image (`FR-A11Y-1`, §5.10). **DS-A11Y-CANVAS-2.** Keyboard users MUST be able to traverse and act on page text, headings, lists, tables, links, annotations, and form fields in a defined order (§5.10). **DS-A11Y-CANVAS-3.** The focused document element is visually indicated on-canvas (§5.10 DS-CANVAS-A11Y-3). **DS-A11Y-CANVAS-4.** For untagged/scanned documents, the product honestly conveys the lack of structure and offers OCR/remediation (§5.10, §8.1). **DS-A11Y-CANVAS-5.** Text alternatives (alt text) in tagged documents are exposed; where absent, this is conveyed honestly. **DS-A11Y-CANVAS-6.** Form filling via AT is complete: labels, descriptions, order, required/invalid state, and value changes are all exposed (§7.11, `FR-FORM-7`).

## 9.13 Accessibility testing & governance
**DS-A11Y-TEST-1.** Each release runs automated accessibility checks + a manual audit checklist + screen-reader task testing on each platform (`MET-A11Y-*`, §13). **DS-A11Y-TEST-2.** Accessibility regressions are release-blocking (absolute metric, `MET-A11Y-1/2`). **DS-A11Y-TEST-3.** New components (§14) MUST pass the accessibility checklist before merge; the checklist is part of Design QA (§13). **DS-A11Y-TEST-4.** The accessibility of the interface is part of the stability contract: focus order, names, and announcements MUST NOT change unexpectedly across releases (`NFR-A11Y-3`, `US-AXS-5`).

---

# 10. Plugin UX

*Normative.* Plugins extend the product within explicit, user-granted, sandboxed boundaries (`PRIN-9`, `FR-PLUG`, `[ADR-014]`, `SDS §11`). The UX rule is: **plugin-contributed UI is indistinguishable in quality and behavior from first-party UI, clearly attributed as third-party, and never able to compromise host stability, security, or the user's control** (DS-PHIL-8/10).

## 10.1 Principles for plugin UX
**DS-PLUGUX-1.** Plugin UI MUST use the host's components (§6), tokens (§12), and interaction rules; a plugin MUST NOT be able to draw arbitrary chrome that violates the design system or accessibility (§9). Contributions are **declarative** (panels, toolbar items, menu items, commands, settings, batch operations) rendered by the host (`SDS §11.4`), not raw drawing into host surfaces. **DS-PLUGUX-2.** Plugin contributions MUST be clearly attributed to their plugin (name visible on hover/in settings) so the user always knows what is third-party (DS-PHIL-8). **DS-PLUGUX-3.** Plugin UI MUST meet the same accessibility bar (§9); the host provides accessible components so this is achievable by default, and a plugin that supplies inaccessible content is flagged.

## 10.2 Plugin panels
**DS-PLUG-PANEL-1.** A plugin MAY contribute a panel (left/right/bottom dock, §3.14) built from host components via its declared schema. It behaves like any panel (docking, resize, persistence, §3.14–§3.19). **DS-PLUG-PANEL-2.** The panel header shows the plugin name/attribution. **DS-PLUG-PANEL-3.** A crashing/hanging plugin's panel is cleanly retracted without stranding focus (§8.6, §4.2).

## 10.3 Toolbar & command-surface contributions
**DS-PLUG-TOOL-1.** A plugin MAY contribute toolbar items/tools and Ribbon entries into designated extension groups (not intermixed to masquerade as core), each attributed. **DS-PLUG-TOOL-2.** Contributed tools follow the tool model (§4.1: visible mode, escape, cursor) and component specs (§6.1). **DS-PLUG-TOOL-3.** Overflow and customization (§3.4) apply equally.

## 10.4 Context-menu contributions
**DS-PLUG-CTX-1.** A plugin MAY add context-menu items for relevant targets, grouped in a clearly delineated extensions section, attributed, following §6.9. They MUST NOT displace or reorder core items (DS-PHIL-1/3).

## 10.5 Commands & command search
**DS-PLUG-CMD-1.** Plugin commands MUST appear in command search (§6.25) with plugin attribution and any assigned shortcut, and MUST be bindable like core commands (§11 shortcuts) without colliding (conflicts resolved with disclosure, §12.x settings). **DS-PLUG-CMD-2.** Every plugin command has a canonical home (a menu/panel), mirroring DS-MENU-2.

## 10.6 Plugin settings
**DS-PLUG-SET-1.** Plugin settings appear in a dedicated Plugins area of the host settings, using host form components (§6), grouped per plugin, with the plugin's declared options. **DS-PLUG-SET-2.** Settings persist per user (§3.19) and are subject to enterprise policy (§12, an admin may lock/disable). **DS-PLUG-SET-3.** A plugin MUST NOT present its own out-of-band settings UI that bypasses host consistency/accessibility.

## 10.7 Permission prompts
**DS-PLUG-PERM-1.** When a plugin requests a capability (document read, annotate/modify via commands, file access, network — `SDS §11.3`), the host presents a **clear, balanced consent prompt** naming the plugin, the exact capability, and its scope, in plain language, with equally-weighted Grant/Deny (no pre-selected Grant, no dark pattern, DS-PHIL-10). **DS-PLUG-PERM-2.** Grants are visible and revocable in the Plugins settings; the user can see what each plugin can do at any time (DS-PHIL-8). **DS-PLUG-PERM-3.** A capability never requested/granted is unavailable to the plugin (`SDS §11.3`, enforced below UX). **DS-PLUG-PERM-4.** Privileged actions (network/file) are brokered and, where user-affecting, reconfirmed (§8.11 consent). **DS-PLUG-PERM-5 (a11y).** Consent prompts are accessible dialogs (§9.10); the privacy-preserving choice is never visually disadvantaged.

## 10.8 Install / update / disable
**DS-PLUG-INSTALL-1.** Installing a plugin shows its identity, version, author, and **the full set of capabilities it will require**, before installation, requiring explicit consent (DS-PHIL-8). Integrity/provenance is verified where a signed registry exists (`FR-PLUG-8`, future). **DS-PLUG-UPDATE-1.** Updates disclose what changed and any change in requested capabilities (a capability increase requires re-consent, never silent escalation, DS-PHIL-10). Updates are user/admin-controlled (§12, `ENT-POL`). **DS-PLUG-DISABLE-1.** Disabling/removing a plugin is one clear action; its contributed UI is retracted cleanly and its grants released. **DS-PLUG-DISABLE-2.** The user can disable all plugins quickly (a safe-mode) for troubleshooting (DS-PHIL-8). **DS-PLUG-INSTALL-2 (a11y).** All install/update/disable flows are accessible.

## 10.9 Plugin errors
**DS-PLUG-ERR-1.** A plugin fault is contained and reported per §8.6, attributed, with details and a report path; the host and documents are unaffected (`PRIN-9`). **DS-PLUG-ERR-2.** A plugin that repeatedly faults or exceeds quotas (CPU/memory, `SDS §11.4`) is disabled with an honest explanation and a re-enable option. **DS-PLUG-ERR-3.** A plugin MUST NOT be able to present a host-looking error that misattributes its failure to the host (attribution integrity, DS-PHIL-8).

## 10.10 Version compatibility
**DS-PLUG-VER-1.** The host communicates the extension-contract version and a plugin's targeted version; incompatibility is shown honestly with guidance (update host or plugin) rather than a silent failure (`FR-PLUG-5`, §14). **DS-PLUG-VER-2.** Deprecations are surfaced to the user/author with advance notice per the deprecation policy (§14, `US-PLG-7`). **DS-PLUG-VER-3.** A plugin built for a still-supported contract version MUST continue to work per the compatibility guarantee (`FR-PLUG-5`).

---

# 11. Animation System

*Normative.* Motion is functional, fast, interruptible, and accessible (DS-MOTION-1/2/3, DS-PHIL-2). This section fixes the timing, curves, and usage so all motion is consistent and none of it costs the user time.

## 11.1 Animation philosophy
**DS-ANIM-1.** Every animation MUST serve one of: continuity (connecting a change to its origin), causality (showing that an action had an effect), orientation (spatial relationship of appearing/disappearing surfaces), or progress (ongoing work). Decorative motion is prohibited (DS-PHIL-2). **DS-ANIM-2.** No animation may delay the user's ability to act; input during an animation interrupts and takes precedence (DS-MOTION-3). **DS-ANIM-3.** All motion has a reduced-motion equivalent (§9.6).

## 11.2 Duration & curves (tokens)
**DS-ANIM-DUR-1.** Duration tokens (§12.7): `motion.instant=0`, `motion.fast=100 ms`, `motion.base=150 ms`, `motion.slow=200 ms`. UI state changes use `fast`; surface transitions (dialogs/panels/popovers) use `base`; only larger spatial transitions may use `slow`. Nothing in the chrome exceeds 200 ms. **DS-ANIM-DUR-2.** Curve tokens: `motion.ease-out` (default for entering/most UI), `motion.ease-in-out` (movement between two on-screen states), `motion.ease-in` (exiting). Springs MAY be used only if tuned to complete within the duration budget and reduced-motion-safe. **DS-ANIM-DUR-3 (reduced motion).** With reduced motion, durations collapse to `instant` or a ≤`fast` cross-fade; no positional movement.

## 11.3 Interruptibility
**DS-ANIM-INT-1.** Animations MUST be interruptible: a new state change during an animation retargets smoothly (no queueing that delays response). **DS-ANIM-INT-2.** Scroll/zoom/pan animations yield immediately to continued input (§4, §5.11). **DS-ANIM-INT-3.** No modal wait is imposed by an animation.

## 11.4 Loading indicators
**DS-ANIM-LOAD-1.** Indeterminate spinners animate smoothly at a fixed rate (token), appear only after `motion.progress.appear` (~200 ms) to avoid flashing (§8.2), and have a reduced-motion static/"working…" equivalent. **DS-ANIM-LOAD-2.** Spinners are for waits without known progress; prefer determinate bars where progress is known.

## 11.5 Progress indicators
**DS-ANIM-PROG-1.** Determinate bars advance monotonically to actual progress (no fake progress, DS-PHIL-8) and show percentage where meaningful (§6.14). **DS-ANIM-PROG-2.** Progress that stalls MUST reflect the stall honestly (not a fake creep) and surface cause if failed (§8). **DS-ANIM-PROG-3.** Long operations show time-remaining only if it can be estimated honestly; otherwise show work done.

## 11.6 Skeleton screens
**DS-ANIM-SKEL-1.** Structured content (canvas pages §5.1, tables, panels) uses skeletons (neutral placeholder shapes) during load, with a subtle shimmer (`motion.base`-paced) that is disabled under reduced motion (becomes static). **DS-ANIM-SKEL-2.** Skeletons match the shape of the incoming content to preserve layout stability (no jarring reflow on arrival). **DS-ANIM-SKEL-3.** Skeletons appear only after the ~200 ms threshold (§8.2).

## 11.7 Page transitions (canvas)
**DS-ANIM-PAGE-1.** Page navigation in single-page/facing (non-continuous) layouts MAY use a quick, subtle transition (≤`base`) that reinforces direction; continuous scroll has no discrete transition. **DS-ANIM-PAGE-2.** Zoom uses progressive refinement (GPU-scaled preview → crisp tiles, `SDS §6.7`) presented smoothly (§5.1), interruptible (§11.3). **DS-ANIM-PAGE-3.** Reduced motion → instant page changes. **DS-ANIM-PAGE-4.** Page transitions MUST NOT delay reading; they are non-blocking and skippable by continued input.

## 11.8 Panel transitions
**DS-ANIM-PANEL-1.** Panel open/close/collapse animates size/opacity ≤`base` from the relevant edge (§3.17), preserving the canvas focal point (no content jump). **DS-ANIM-PANEL-2.** Docking preview/guides appear immediately (no delay) during drag (§3.14). **DS-ANIM-PANEL-3.** Reduced motion → instant.

## 11.9 Dialog & popover transitions
**DS-ANIM-DIALOG-1.** Dialogs scale 98%→100% + fade over `base` from center/invoker; scrim fades in; reduced motion → instant (§6.8). **DS-ANIM-DIALOG-2.** Popovers/menus/tooltips fade/scale from their anchor over `fast` (§6.2, §6.15); reduced motion → instant. **DS-ANIM-DIALOG-3.** Exit animations are ≤ entry and never delay dismissal perceptibly.

## 11.10 Motion performance
**DS-ANIM-PERF-1.** All animation MUST hold the frame budget (`MET-PERF-*`); an animation that cannot maintain cadence MUST be shortened or dropped, never allowed to stutter (DS-PHIL-2). **DS-ANIM-PERF-2.** Animations MUST NOT trigger document re-rasterization (§5.11) or block the UI thread. **DS-ANIM-PERF-3.** Motion is GPU-friendly (opacity/transform-based) where the platform allows.

---

# 12. Design Tokens

*Normative.* Tokens are the single source of truth for every visual and temporal value (DS-CONV-2). Components reference **semantic** and **component** tokens; those resolve to **primitive** values; **theme** and **density** are token remappings. This section defines the token taxonomy, naming, and the reference values. *Informative note:* concrete hex/number values below are the reference set; they are authoritative as the reference theme (light) at Comfortable density, 100% scale, and are maintained in the token source of truth alongside this document.

## 12.1 Token architecture & naming
**DS-TOK-1 (Layers).** Three layers: **primitive** (`palette.blue.600`, `size.8`), **semantic** (`color.text.primary`, `space.5`, `type.body`), **component** (`button.height`, `input.border`). Components MUST reference semantic or component tokens only (DS-CONV-2). **DS-TOK-2 (Naming).** Dot-namespaced, lowercase, category-first: `color.*`, `space.*`, `size.*`, `radius.*`, `border.*`, `elev.*`, `shadow.*`, `type.*`, `motion.*`, `icon.*`, `z.*`, plus component namespaces (`button.*`, `panel.*`, etc.). **DS-TOK-3 (No literals).** No component spec or implementation may use a raw value where a token exists; new values require a new token (§14). **DS-TOK-4 (Theming).** A theme is a complete remap of semantic→primitive; a density is a remap of size/space/type metrics; components are invariant across both.

## 12.2 Spacing tokens
`space.0=0`, `space.1=2`, `space.2=4`, `space.3=8`, `space.4=12`, `space.5=16`, `space.6=20`, `space.7=24`, `space.8=32`, `space.9=40`, `space.10=48`, `space.12=64`, `space.16=96`. (dp; base grid = 4; the `2` half-step is exceptional per DS-SPACE-1.) Density scales these via a multiplier set: Compact ×0.85 (rounded to grid), Comfortable ×1.0, Spacious ×1.2 (rounded to grid), applied to component metrics, not to the raw scale identities.

## 12.3 Radius tokens
`radius.none=0`, `radius.sm=3`, `radius.md=5`, `radius.lg=8`, `radius.round=9999`. Usage per DS-RADIUS-1.

## 12.4 Border tokens
`border.width.hairline=1`, `border.width.emphasis=2`, `border.width.hc=3` (high-contrast/focus). Color via semantic `color.border.*`. `border.style=solid` (default). Hairlines snap to device pixels (§2.15).

## 12.5 Elevation tokens
`elev.0..4` per §2.4 table, each mapping to a `shadow.*` token (light/dark variants) and, in dark theme, a surface-lightening step (`color.surface.raise.N`). High-contrast maps elevation to `border`, not shadow (DS-ELEV-3).

## 12.6 Shadow tokens
`shadow.1={y:1,blur:2,alpha:0.08}`, `shadow.2={y:2,blur:8,alpha:0.14}`, `shadow.3={y:8,blur:24,alpha:0.20}`, `shadow.4={y:12,blur:32,alpha:0.24}` (light theme; dark theme uses adjusted alphas + surface lightening). Color = `palette.black` at the given alpha (light). Components MUST NOT define bespoke shadows (DS-ELEV-4).

## 12.7 Motion tokens
Durations: `motion.instant=0`, `motion.fast=100`, `motion.base=150`, `motion.slow=200` (ms). Curves: `motion.ease-out`, `motion.ease-in`, `motion.ease-in-out` (defined cubic-bezier control points in the source of truth). Thresholds: `motion.progress.appear=200`, `motion.hover-intent=120`, `motion.tip.delay=500`, `motion.snack.timeout=6000`. Reduced-motion overrides collapse durations to `instant`/`fast` cross-fade (§11.2).

## 12.8 Icon tokens
Sizes `icon.sm=16`, `icon.md=20`, `icon.lg=24`, `icon.xl=32`; stroke `icon.stroke=1.5` (at 16, scales proportionally); style default outline, active filled (DS-ICON-1). Color inherits `color.icon.*` (currentColor by default).

## 12.9 Color tokens

### 12.9.1 Primitive palette (reference; light-theme anchors — [UX Decision] exact values)
Neutrals (the UI's backbone), gray ramp: `palette.gray.0=#FFFFFF`, `.50=#F7F8FA`, `.100=#EEF0F3`, `.200=#E2E5EA`, `.300=#CBD0D8`, `.400=#A7AEB9`, `.500=#828B99`, `.600=#5F6875`, `.700=#454C57`, `.800=#2B3138`, `.900=#1A1E24`, `palette.black=#0B0D10`.
Accent (single hue, blue): `palette.blue.50=#EAF2FF`, `.100=#D3E3FF`, `.300=#8FB8FF`, `.500=#3B82F6`, `.600=#2C6FE0`, `.700=#1F57B5`.
Semantic hues (each with a mandatory glyph, §9.5): success `palette.green.500=#2E9E5B`/`.600=#1F8049`; warning `palette.amber.500=#C8860B`/`.600=#A56E06` (chosen dark enough for contrast on light); danger `palette.red.500=#D64545`/`.600=#B5302F`; info reuses accent blue. *Rationale (Informative):* the accent blue and the semantic hues remain distinguishable under common color-vision deficiencies **when paired with the mandated non-color cues**; color alone is never relied upon (§9.5).

### 12.9.2 Semantic tokens (light theme mapping; components reference these)
Backgrounds: `color.bg.app=palette.gray.50`, `color.bg.panel=palette.gray.0`, `color.bg.canvas=palette.gray.0` (true page white is the document's, not this — this is the app surface behind panels), `color.bg.pasteboard=palette.gray.200`, `color.bg.overlay=palette.gray.0`.
Text: `color.text.primary=palette.gray.900`, `color.text.secondary=palette.gray.600`, `color.text.disabled=palette.gray.400`, `color.text.onAccent=palette.gray.0`.
Borders: `color.border.default=palette.gray.300`, `color.border.strong=palette.gray.400`, `color.border.subtle=palette.gray.200`.
Accent/selection/focus: `color.accent.default=palette.blue.600`, `color.accent.hover=palette.blue.700`, `color.accent.subtle=palette.blue.50`, `color.selection=palette.blue.500 @ alpha 0.28` (text highlight), `color.focus=palette.blue.600`.
Status: `color.status.success=palette.green.600`, `color.status.warning=palette.amber.600`, `color.status.danger=palette.red.600`, `color.status.info=palette.blue.600`, each with `.subtle` background variants.
Canvas overlays (with §5.7 dual-tone guarantee): `color.overlay.selection`, `color.overlay.handle.fill=palette.gray.0`, `color.overlay.handle.stroke=palette.blue.600`, `color.overlay.guide`, `color.overlay.snap`, `color.overlay.search=palette.amber.500 @ alpha` (with halo), each defined with a contrasting halo companion token.
State: `state.disabled.opacity=0.38`, `state.hover.overlay=palette.black @ 0.06`, `state.pressed.overlay=palette.black @ 0.12` (dark theme uses white overlays).

### 12.9.3 Dark theme (remap, reference intent)
`color.bg.app=palette.gray.900`, `color.bg.panel=palette.gray.800`, surfaces lighten with elevation (DS-DARK-3), `color.text.primary=palette.gray.50`, `color.text.secondary=palette.gray.300`, borders use lighter grays, accent shifts to a slightly lighter blue (`palette.blue.500`) for contrast, semantic hues adjusted for dark backgrounds; **canvas page stays true white unless night-reading is enabled** (DS-DARK-4); pasteboard darkens. All pairings re-verified for contrast (§9.2, DS-DARK-5).

### 12.9.4 High-contrast (forced-colors) mapping
Semantic tokens map to OS system-color roles (WindowText, Window, Highlight, HighlightText, GrayText, ButtonText, etc.); shadows→borders; focus ≥3 dp Highlight; all §9.5 non-color cues retained (§2.13, §9.3).

## 12.10 Typography tokens
Per §2.2 DS-TYPE-3 table: `type.caption`, `type.body`, `type.body-strong`, `type.subtitle`, `type.title`, `type.display`, each a composite of `{family, size, lineHeight, weight, letterSpacing}`. Family tokens: `type.family.ui` (platform system + Inter fallback), `type.family.mono` (platform mono + JetBrains Mono fallback). Sizes scale with OS text-scaling (§9.7) and density (type may shift one step at Compact/Spacious per a documented rule, but never below 11 dp, DS-TYPE-4).

## 12.11 Z-order tokens
`z.canvas=0`, `z.docked-panel=10`, `z.command-surface=20`, `z.floating-panel=100`, `z.popover=200`, `z.tooltip=250`, `z.dialog-scrim=300`, `z.dialog=310`, `z.drag-ghost=400`, `z.notification=350`. Stacking is deterministic (DS-PHIL-3); components MUST use these, not ad-hoc values.

## 12.12 Component tokens (examples; each component defines its set)
E.g., `button.height`, `button.padding.x`, `button.radius=radius.sm`, `button.gap`; `input.height`, `input.border=color.border.default`, `input.border.focus=color.focus`, `input.radius=radius.sm`; `panel.padding`, `panel.header.height`, `panel.min.width`, `panel.default.width`; `toolbar.height`, `menu.item.height`, `row.height`, `tab.height`. Component tokens resolve through semantic tokens and density multipliers (DS-TOK-1/4), so a single density/theme change updates every component coherently.

## 12.13 Token governance
**DS-TOK-GOV-1.** New tokens are added under §14 with a rationale; renaming/removing a token is a breaking change to the design contract and follows the deprecation policy (§14, `[ADR-030]`). **DS-TOK-GOV-2.** Every token has a definition in the single source of truth consumed by both the design and the Qt implementation, so design and build never diverge (DS-CONV-1/2). **DS-TOK-GOV-3.** Plugin UIs consume the same tokens (§10.1); plugins cannot introduce off-system values into host surfaces.

---

# 13. Design QA Checklist

*Normative.* A surface or component is shippable only when it passes the applicable checks. These map to `MET-*` gates and to `[ADR-022]` test strata. Each check is pass/fail (DS-CONV-1). *Informative:* this checklist is the designer/QA/engineer shared contract for "done."

## 13.1 Visual QA
- **VQA-1.** Uses only tokens (no literal colors/sizes/durations) (DS-CONV-2, §12).
- **VQA-2.** Correct in light, dark, and high-contrast themes; contrast ratios met in each (§9.2).
- **VQA-3.** Correct at Compact/Comfortable/Spacious density; no clipping/overlap (§2.14).
- **VQA-4.** Crisp at 100%/150%/200%/fractional DPI; hairlines exactly 1 px; icons pixel-crisp (§2.15).
- **VQA-5.** Correct at OS text-scaling up to 200% (reflow, no truncation loss) (§9.7).
- **VQA-6.** RTL-mirrored correctly where applicable (§9).
- **VQA-7.** Spacing on the 4 dp grid; alignment to shared grid lines (§2.3).
- **VQA-8.** Elevation/shadow per tokens; flat docked surfaces, shadows only on floating (§2.4).
- **VQA-9.** Iconography on-grid, correct size/stroke, outline/active-fill state model (§2.6).
- **VQA-10.** Document color is never tinted by chrome (DS-COLOR-7).

## 13.2 Interaction QA
- **IQA-1.** Every mode has a visible indicator; `Esc` retreat order correct (§4.1).
- **IQA-2.** All states present and correct: rest/hover/active/focus/disabled/loading/error/selected as applicable (§6 baseline).
- **IQA-3.** Deterministic: same input→same result and feedback (DS-PHIL-3).
- **IQA-4.** Undo/redo covers every document mutation; grouped actions named (§7.6).
- **IQA-5.** Destructive actions use the confirmation/disclosure pattern; no dark patterns (§6.10, DS-PHIL-10).
- **IQA-6.** Canvas overlays meet the contrast guarantee over adversarial-color pages (§5.7).
- **IQA-7.** Drag operations show insertion/target indicators and a keyboard alternative exists (§3.14, §6.4).
- **IQA-8.** Async actions never block the UI; progress + cancel present (§8.2, §11).
- **IQA-9.** Focus never stolen by background events; restored on transient close (§4.2).
- **IQA-10.** Tool stickiness and defaults match spec/Acrobat expectation (§4.1).

## 13.3 Accessibility QA
- **AQA-1.** 100% keyboard operable; documented tab/focus order matches (§4.3, §9, `MET-A11Y-2`).
- **AQA-2.** Correct role/name/value/state to the platform AT API for every element (§9.9).
- **AQA-3.** No information by color alone; verified under CVD simulation + monochrome (§9.5).
- **AQA-4.** Contrast (text, focus, UI, status) meets minimums in all themes (§9.2).
- **AQA-5.** Reduced-motion honored; no motion-only information (§9.6).
- **AQA-6.** Modal focus trap + restore; dialogs labeled/described; alerts announce (§9.10).
- **AQA-7.** Tables/lists/trees expose full semantics incl. virtualized counts (§9.11).
- **AQA-8.** Canvas exposes document structure/reading order; focused element indicated; forms fillable via AT (§9.12).
- **AQA-9.** Touch targets ≥44 dp effective in touch contexts (§9.8).
- **AQA-10.** Screen-reader task set completes on each platform (§9.13, `MET-A11Y-3`).
- **AQA-11.** No accessibility regression vs prior release (§9.13, `NFR-A11Y-3`).

## 13.4 Performance QA
- **PQA-1.** Interaction budgets met at p95/p99 (scroll/zoom/select/ink) (`MET-PERF-3/4`).
- **PQA-2.** State/animation changes ≤1 frame; animations hold cadence or are dropped (§11.10).
- **PQA-3.** No UI action triggers document re-rasterization when it shouldn't (§5.11, §3.15).
- **PQA-4.** Panels/menus/dialogs open within budget; large lists virtualized (§6).
- **PQA-5.** Loading appears only after the 200 ms threshold; no flashing (§8.2).
- **PQA-6.** Memory stable over soak; no per-interaction leak in UI layer (`MET-MEM-3`).

## 13.5 Consistency QA
- **CQA-1.** Command has exactly one canonical menu home; consistent naming/icon across menu/toolbar/search (§3.2, §6.25).
- **CQA-2.** Shared components used for shared problems (errors/empty/loading/progress) (§8.15).
- **CQA-3.** Terminology, iconography, placement consistent and matching the stability contract (§14, DS-PHIL-1).
- **CQA-4.** Same interaction pattern across subsystems (selection, context menu, inline edit, drag) (§6).
- **CQA-5.** Cross-platform behavior consistent except native conventions (`CMP-XPLAT`).
- **CQA-6.** Plugin contributions match host quality/behavior and are attributed (§10).

## 13.6 Regression QA
- **RQA-1.** Keyboard shortcuts unchanged vs the versioned shortcut set unless an approved, opt-in change (§14, `PRIN-4`).
- **RQA-2.** Menu taxonomy/locations unchanged vs contract unless approved (§3.2, §14).
- **RQA-3.** Tab/focus order unchanged unless approved (§4.3).
- **RQA-4.** Default workspaces still present and behaviorally identical (§3.18).
- **RQA-5.** Token changes reviewed for downstream impact; no unintended visual drift (§12.13).
- **RQA-6.** Accessibility semantics unchanged or improved (never regressed) (§9.13).
- **RQA-7.** Visual regression snapshots reviewed across themes/densities/DPI (§13.1).

## 13.7 QA governance
**DS-QA-1.** Every PR touching UI declares which checklist sections apply and attaches evidence. **DS-QA-2.** Absolute-gated items (accessibility AQA-1..11, destructive-pattern IQA-5, overlay contrast IQA-6, no-color-alone AQA-3) are release-blocking. **DS-QA-3.** New components (§14) MUST pass all applicable sections before first ship.

---

# 14. Future Evolution

*Normative.* This section governs how the design system changes over its 10+ year life without breaking the stability contract (DS-CONV-4, `PRIN-4`, `[ADR-030]`). *Informative preface:* the system is meant to grow by addition and careful, opt-in change — never by silent churn of the kind this product exists to avoid.

## 14.1 Adding new components
**DS-EVOLVE-1.** A new component MUST: fill a genuine, recurring need not met by composing existing components (avoid redundancy); follow the §6 clause structure (purpose/variants/states/keyboard/mouse/pen/touch/accessibility/sizing/spacing/icons/behavior/error/empty/loading/disabled/performance/animation); reference tokens only (§12); pass the full §13 checklist including accessibility; and be documented here before shipping. **DS-EVOLVE-2.** A new component MUST reuse existing interaction patterns (selection, focus, menus, drag) rather than inventing new ones for the same purpose (DS-PHIL-1/3). **DS-EVOLVE-3.** If a new pattern is unavoidable, it MUST be justified as a `[UX Decision]` with evidence (DS-PHIL-9) and, where it changes an existing behavior, provide an opt-in path.

## 14.2 Versioning
**DS-EVOLVE-VER-1.** The design system is versioned; its version is the interface-behavior-profile version referenced by `ENT-UI` and `[ADR-030]`. **DS-EVOLVE-VER-2.** Changes are classified: **additive** (new tokens/components/optional behaviors — minor), **behavioral** (changes to defaults, shortcuts, layout, focus order, taxonomy — gated, opt-in, contract-affecting), **breaking** (removal/rename of tokens, components, shortcuts, or established behaviors — major, deprecation-governed). **DS-EVOLVE-VER-3.** Behavioral and breaking changes MUST be reflected in the versioned shortcut set, menu taxonomy, and default-workspace definitions, and MUST be reviewable by administrators (`ENT-POL`, `ENT-UI`).

## 14.3 Deprecation
**DS-EVOLVE-DEP-1.** A component/token/pattern/shortcut is deprecated before removal, with: a documented replacement, a migration note (§14.5), and a minimum support window of **two release trains** (mirroring the plugin-contract policy, `[ADR-030]`, `FR-PLUG-5`). **DS-EVOLVE-DEP-2.** Deprecated shortcuts/behaviors remain available (opt-in) through the window so users and enterprises can adapt (`PRIN-4`, `ENT-UI-2`). **DS-EVOLVE-DEP-3.** Deprecation is communicated to users (where user-visible) and to plugin authors (§10.10) with advance notice.

## 14.4 Backward compatibility
**DS-EVOLVE-BC-1.** A newer design-system version MUST keep prior default shortcuts, menu locations, tab/focus order, and default workspaces available (at least via opt-in compatibility profile) for the support window; an enterprise-pinned profile MUST remain intact for its LTS track (`ENT-UI-2`, DS-CONV-4). **DS-EVOLVE-BC-2.** Token changes MUST NOT silently alter the appearance of pinned profiles; a pinned profile resolves to its version's tokens. **DS-EVOLVE-BC-3.** User customizations (toolbar/QAT/shortcuts/layout) MUST migrate without loss or be explicitly, reversibly migrated (§3.19, `CMP-BACK-2`).

## 14.5 Migration
**DS-EVOLVE-MIG-1.** Any behavioral/breaking change ships with a migration path: automatic where safe and lossless, otherwise an explicit, reversible, user-consented migration (DS-PHIL-8). **DS-EVOLVE-MIG-2.** Migrations MUST be documented for designers, engineers, QA, and plugin authors, with before/after and rationale. **DS-EVOLVE-MIG-3.** A migration MUST never discard user customization or settings silently.

## 14.6 Plugin compatibility
**DS-EVOLVE-PLUG-1.** The design tokens and declarative contribution schemas (§10) are part of the versioned contract; changes follow §14.2–14.5 and the plugin-contract versioning (`FR-PLUG-5`, `[ADR-015]`, `[ADR-030]`). **DS-EVOLVE-PLUG-2.** Additive token/component changes MUST NOT break existing plugin UIs; breaking changes are deprecation-governed with the compatibility test kit updated (`FR-PLUG-6`). **DS-EVOLVE-PLUG-3.** A plugin targeting a still-supported design-system version MUST render and behave correctly (§10.10).

## 14.7 Governance of this document
**DS-EVOLVE-GOV-1.** This document is the canonical UX specification; changes are change-controlled, preserve `DS-*` identifiers (withdrawn ones marked, never reused), and are reconciled with PRD/ADR/SDS where they intersect. **DS-EVOLVE-GOV-2.** Every `[UX Decision]` is a ratification point for design leadership; the open ones are listed in §14.8. **DS-EVOLVE-GOV-3.** No change to this document may weaken the accessibility bar (§9) or the anti-dark-pattern mandate (DS-PHIL-10); those are inviolable.

## 14.8 Open [UX Decision] items for ratification (Informative)
The following decisions introduced here warrant explicit design-leadership sign-off, as they shape the product's identity and are costly to reverse later:
1. **Command-surface duality** — Toolbar default with opt-in Ribbon (DS-RIBBON-1), and **Workspaces** (DS-WORKSPACE-1) as the mechanism for serving casual↔prepress personas from one product.
2. **Icon state model** — outline-default / filled-active (DS-ICON-1) and single-accent color discipline (DS-COLOR-3).
3. **Density model** — three named densities, Comfortable default (DS-DENSITY-1), and the density multiplier set (§12.2).
4. **Keyboard model** — `F6` region cycling (DS-FOCUS-5) and the canonical shortcut set (to be enumerated as a versioned appendix artifact; it is contractual per RQA-1).
5. **Overlay contrast guarantee** (DS-OVERLAY-1) and the **document color sanctuary** (DS-COLOR-7) as hard, testable rules.
6. **Anti-dark-pattern mandate** (DS-PHIL-10) and the balanced-consent rule (DS-PLUG-PERM-1, §8) as inviolable design law.
7. **Reference palette and token values** (§12.9) as the design source of truth.
8. **Command palette** (DS-CMDP-1) as the discoverability backstop that permits layout stability.

*Informative closing note:* the two artifacts that should accompany this document as versioned companions — because they are contractual and referenced but enumerated separately for maintainability — are (a) the **canonical keyboard-shortcut set** (mapped to Acrobat equivalents, per DS-UX-KEY and RQA-1) and (b) the **default-workspace definitions** (DS-WORKSPACE-2). Both are governed by §14 and `[ADR-030]`.

*End of UI/UX Design System (baseline). This document is maintained alongside the code and the other canonical specifications; material changes trace through §14 governance and, where they touch implementation or product intent, reconcile with the ADR, SDS, and PRD.*
