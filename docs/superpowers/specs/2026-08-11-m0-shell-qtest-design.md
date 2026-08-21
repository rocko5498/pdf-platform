# M0 Cross-Platform Shell QTest Design

**Status:** Approved for implementation by the user's instruction to continue from the canonical documentation.

**Requirements:** ADR-003, ADR-022, ADR-026, ADR-029, SDS §6.4, SDS §13.3, SDS §13.6, SDS §13.7, SDS §14 M0, T-8, GR-3, GR-8.

## Problem

M0 requires the real shell-to-tile path to be proven on Windows, macOS, and Linux. The Rust bridge, IPC, and shared-memory slices run in the three-OS CI matrix, but the Qt shell is absent from CI and has only a Windows human smoke result. The shell also cannot compile unchanged on non-Windows systems because `canvas.cc` includes and calls Win32 APIs unconditionally and the application target always links Windows system libraries.

ADR-026 independently requires QTest coverage for the shell's input-to-command translation, and ADR-029 requires that suite in the PR pipeline. No QTest target currently exists.

## Scope

This slice makes the existing shell compile portably, tests the existing `CanvasWidget` input translation, and gates the shell build and QTest suite on all three CI operating systems.

It does not change the Rust/C++ FFI surface, shortcut assignments, document behavior, sandbox policy, shared-memory ownership, or PDFium provisioning. It does not claim that an actual PDF was rendered through the shell on all three operating systems; that remains a later M0 proof after this prerequisite lands.

## Design

### Platform-correct shell

Keep the current same-process shared-memory pointer path unchanged. The legacy Win32 section cleanup is inactive during the normal path because `shmem_section_` remains null. Compile the include and cleanup calls only on Windows; on other systems, clearing the local pointer is sufficient because Rust owns the mapped region.

Link `ws2_32`, `ntdll`, `userenv`, `bcrypt`, `advapi32`, `kernel32`, and `msvcrt` only when `WIN32` is true. All Qt libraries remain cross-platform targets.

### Widget input QTest

Exercise the real `CanvasWidget` rather than creating a parallel input mapper. A Qt Test executable uses `QSignalSpy` and synthetic events to verify:

- Page Down and Page Up emit `pageStepRequested(+1/-1)`.
- Ctrl+Plus and Ctrl+Minus emit `zoomStepRequested(+1/-1)`.
- Plain wheel input emits `scrollDeltaRequested` with the existing sign convention.
- Ctrl+wheel emits `zoomStepRequested` and does not emit scrolling.

The test build disables OpenGL and uses Qt's offscreen platform so it needs no display server or GPU. Tests characterize the current registry-sensitive behavior; they do not add or alter shortcuts.

### CMake and CI

Use CTest's standard `BUILD_TESTING` switch. Request Qt Test only when tests are enabled, add one shell test subdirectory, and register the executable with `add_test`.

Add a separate `shell` CI matrix for Ubuntu, Windows, and macOS. Each job:

1. checks out the repository;
2. installs the pinned Rust toolchain and Qt 6.5-or-newer development package;
3. builds `ffi-bridge` with Cargo;
4. configures CMake with the platform-native static-library path and OpenGL disabled;
5. builds the shell and runs CTest with `QT_QPA_PLATFORM=offscreen`.

The existing Rust job remains unchanged so shell failures are distinct and diagnosable.

## Failure handling and evidence

CMake continues to fail explicitly if the bridge library or generated cxx headers are absent. CI fails independently for configuration, compilation, or QTest failure on each platform. A green matrix proves that the shell compiles and its input translation behaves consistently on all three systems; it does not substitute for the later real-PDF render assertion.

The clean `main` baseline currently has one unrelated failure: `ocr_bridge::tests::tesseract_engine_detection` panics in `despeckle` on a zero-sized raster. That known baseline defect is already fixed on the separate `codex/ocr-despeckle-underflow` branch and is not modified by this slice.

## Dependency and review impact

No Rust or product dependency is added. Qt Test is part of the already-selected Qt framework. Any CI setup action must be version-pinned and used only to provision Qt; removing it restores manual/system Qt discovery without changing product code.

No security-critical FFI, confinement, crypto, redaction, or signature path changes. Normal review is sufficient for this slice.
