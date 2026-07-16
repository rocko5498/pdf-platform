# Accessibility Audit Checklist (Application Chrome)

**Cites:** NFR-A11Y-*, DS-A11Y-*, PRIN-8, DS §13 AQA  
**Automated gate:** `python tools/a11y_audit.py` (CI)  
**Manual gate:** once per release train with NVDA (Windows), VoiceOver (macOS), Orca (Linux)

## Absolute gates (block release)

| ID | Check | Auto | Manual |
|---|---|---|---|
| AQA-1 | Every interactive chrome control has name + role | partial | yes |
| AQA-2 | Keyboard-only: open, navigate pages, find, annotate tool select | partial | yes |
| AQA-3 | No color-alone status | — | yes |
| AQA-4 | Focus always visible | — | yes |
| AQA-5 | Focus order stable (canvas F6, docks, tools) | partial | yes |
| AQA-6 | Modal dialogs trap focus (password, find) | — | yes |
| AQA-7 | Document canvas exposed as Document role | yes (static) | yes |
| AQA-8 | Diagnostics/outline lists labeled | yes (static) | yes |
| AQA-9 | High-contrast / reduced-motion not broken | — | yes |
| AQA-10 | Screen reader announces page change | — | yes |
| AQA-11 | No essential info image-only | — | yes |

## How to run manual audit (1 hour)

1. Build shell; open a multi-page PDF.  
2. Keyboard only: Ctrl+O, PageDown, Ctrl+F, F6, tool + Enter annot.  
3. Enable OS screen reader; confirm app name, canvas status, docks.  
4. Record findings in release notes; any Absolute failure blocks release.

## CI

```
python tools/a11y_audit.py
```
