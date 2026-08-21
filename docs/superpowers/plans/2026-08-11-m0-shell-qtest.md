# M0 Cross-Platform Shell QTest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the existing Qt shell compile portably and gate its input-to-command QTest suite on Windows, macOS, and Linux.

**Architecture:** Delete the unused Win32-only shared-memory cleanup branch while retaining Rust ownership of the normal mapped pointer, conditionally link Windows system libraries, and test the real `CanvasWidget` signals with Qt Test. A separate three-OS CI job builds Cargo's `ffi-bridge`, builds the shell with OpenGL disabled, and runs CTest offscreen.

**Tech Stack:** C++20, Qt 6.10.3 Widgets/Test in CI (6.8.3 local compatibility proof), CMake/CTest 3.25+, Rust/Cargo, GitHub Actions.

## Global Constraints

- Preserve the sole Rust/C++ boundary; do not change `shell/bridge`, `core/ffi-bridge`, or any FFI signature (`GR-3`, ADR-004, ADR-027).
- Preserve all existing input mappings and signals; this slice characterizes them but does not revise the ADR-032 shortcut registry.
- Preserve Rust ownership of the same-process shared-memory pointer (`ADR-011`, SDS §6.3).
- Add no Rust or product dependency. Qt Test is part of the accepted Qt framework (`ADR-003`, ADR-028).
- Do not claim the three-OS real-PDF render criterion is closed; this is its portable build/test prerequisite (`GR-8`, SDS §14 M0).
- The unrelated clean-main baseline panic in `ocr_bridge::tests::tesseract_engine_detection` remains out of scope.

---

### Task 1: Remove the dead Windows-only shell path

**Files:**
- Modify: `shell/CMakeLists.txt`
- Modify: `shell/canvas/canvas.h`
- Modify: `shell/canvas/canvas.cc`
- Modify: `shell/app/CMakeLists.txt`
- Modify: `shell/panels/CMakeLists.txt`
- Modify: `shell/panels/search_panel.cc`

**Interfaces:**
- Consumes: `TileResultFFI::shmem_handle`, already documented as a same-process mapped pointer owned by Rust.
- Produces: the unchanged `MainWindow::mapShmem(qintptr)` behavior on all operating systems.

- [x] **Step 1: Prove the legacy section handle is never populated**

Run:

```powershell
rg -n "shmem_section_|CreateFileMapping|MapViewOfFile" shell
```

Expected: `shmem_section_` is initialized to null and only checked/cleared; no assignment gives it a live section handle.

- [x] **Step 2: Delete the unused cleanup state and Win32 calls**

In `canvas.h`, keep only the mapped pointer:

```cpp
void* shmem_mapping_ = nullptr;  // Borrowed pointer owned by Rust SharedRegion.
```

In `canvas.cc`, remove the unconditional `<windows.h>` include and every `shmem_section_`/`UnmapViewOfFile`/`CloseHandle` block. The destructor, `mapShmem`, and `clearDocumentUi` only clear the borrowed pointer; `mapShmem` remains:

```cpp
void MainWindow::mapShmem(qintptr handle) {
    // Same-process FFI: Rust SharedRegion owns the mapped view. [ADR-011, SDS §6.3]
    shmem_mapping_ = handle == 0 ? nullptr : reinterpret_cast<void*>(handle);
}
```

- [x] **Step 3: Make system-library linkage platform-specific**

Attach Rust's native libraries to the imported target so every consumer inherits them:

```cmake
if(WIN32)
    target_link_libraries(ffi-bridge INTERFACE
        kernel32 ntdll userenv ws2_32 dbghelp
    )
endif()
```

- [x] **Step 4: Restore omitted shell sources exposed by the real build**

The first QTest link proved `search_panel.cc/.h` were omitted from `shell-panels`; adding them exposed one uncompiled call to nonexistent `QLineEdit::setPlaceholder`. Add both sources to the target and use Qt's `setPlaceholderText` API.

- [x] **Step 5: Verify the source no longer has unconditional Win32 dependencies**

Run:

```powershell
rg -n "windows\.h|shmem_section_|UnmapViewOfFile|CloseHandle" shell/canvas shell/app
git diff --check
```

Expected: the search returns no matches and `git diff --check` exits 0.

### Task 2: Add the required CanvasWidget QTest suite

**Files:**
- Modify: `shell/CMakeLists.txt`
- Create: `shell/tests/CMakeLists.txt`
- Create: `shell/tests/canvas_input_test.cc`

**Interfaces:**
- Consumes: `CanvasWidget::pageStepRequested`, `zoomStepRequested`, and `scrollDeltaRequested`.
- Produces: CTest target `shell-canvas-input-test` and test name `shell.canvas_input`.

- [x] **Step 1: Write the QTest before registering it**

Create `canvas_input_test.cc` with one `QObject` test class. Use `QTEST_MAIN`, `QSignalSpy`, and the real `CanvasWidget`. The test cases must assert exact signal counts and arguments:

```cpp
#include "canvas.h"

#include <QSignalSpy>
#include <QTest>
#include <QWheelEvent>

using pdf_platform::CanvasWidget;

class CanvasInputTest : public QObject {
    Q_OBJECT

private slots:
    void page_keys_emit_steps();
    void control_zoom_keys_emit_steps();
    void wheel_routes_scroll_and_zoom();
};

void CanvasInputTest::page_keys_emit_steps() {
    CanvasWidget canvas;
    QSignalSpy spy(&canvas, &CanvasWidget::pageStepRequested);
    QTest::keyClick(&canvas, Qt::Key_PageDown);
    QTest::keyClick(&canvas, Qt::Key_PageUp);
    QCOMPARE(spy.count(), 2);
    QCOMPARE(spy.at(0).at(0).toInt(), +1);
    QCOMPARE(spy.at(1).at(0).toInt(), -1);
}

void CanvasInputTest::control_zoom_keys_emit_steps() {
    CanvasWidget canvas;
    QSignalSpy spy(&canvas, &CanvasWidget::zoomStepRequested);
    QTest::keyClick(&canvas, Qt::Key_Plus, Qt::ControlModifier);
    QTest::keyClick(&canvas, Qt::Key_Minus, Qt::ControlModifier);
    QCOMPARE(spy.count(), 2);
    QCOMPARE(spy.at(0).at(0).toInt(), +1);
    QCOMPARE(spy.at(1).at(0).toInt(), -1);
}

void CanvasInputTest::wheel_routes_scroll_and_zoom() {
    CanvasWidget canvas;
    QSignalSpy scroll(&canvas, &CanvasWidget::scrollDeltaRequested);
    QSignalSpy zoom(&canvas, &CanvasWidget::zoomStepRequested);

    QWheelEvent plain(QPointF(10, 10), QPointF(10, 10), QPoint(), QPoint(0, 120),
                      Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(&canvas, &plain);
    QCOMPARE(scroll.count(), 1);
    QCOMPARE(scroll.at(0).at(0).toInt(), -120);
    QCOMPARE(zoom.count(), 0);

    QWheelEvent controlled(QPointF(10, 10), QPointF(10, 10), QPoint(), QPoint(0, 120),
                           Qt::NoButton, Qt::ControlModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(&canvas, &controlled);
    QCOMPARE(scroll.count(), 1);
    QCOMPARE(zoom.count(), 1);
    QCOMPARE(zoom.at(0).at(0).toInt(), +1);
}

QTEST_MAIN(CanvasInputTest)
#include "canvas_input_test.moc"
```

- [x] **Step 2: Run CTest discovery and observe the missing test**

Run after Qt is available:

```powershell
ctest --test-dir build/shell -N
```

Expected before registration: `shell.canvas_input` is absent.

- [x] **Step 3: Register Qt Test through standard CTest controls**

In `shell/CMakeLists.txt`, add `include(CTest)`, request `Test` only under `BUILD_TESTING`, and add `tests` after production targets:

```cmake
find_package(Qt6 6.5 REQUIRED COMPONENTS Widgets OpenGL OpenGLWidgets)
include(CTest)
if(BUILD_TESTING)
    find_package(Qt6 6.5 REQUIRED COMPONENTS Test)
endif()

# Existing subdirectories...
if(BUILD_TESTING)
    add_subdirectory(tests)
endif()
```

Create `shell/tests/CMakeLists.txt`:

```cmake
add_executable(shell-canvas-input-test canvas_input_test.cc)
target_link_libraries(shell-canvas-input-test PRIVATE shell-canvas Qt6::Test Qt6::Widgets)
add_test(NAME shell.canvas_input COMMAND shell-canvas-input-test)
set_tests_properties(shell.canvas_input PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
    ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
)
```

- [x] **Step 4: Build and run the focused test**

Run:

```powershell
cmake --build build/shell --config Release --target shell-canvas-input-test
ctest --test-dir build/shell -C Release --output-on-failure -R shell.canvas_input
```

Expected: one test passes.

### Task 3: Gate the portable shell in three-OS CI

**Files:**
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: Cargo target output `ffi_bridge.lib` on Windows and `libffi_bridge.a` on Unix.
- Produces: required check `shell (<os>)` for Ubuntu, Windows, and macOS.

- [x] **Step 1: Confirm the current workflow never builds the shell**

Run:

```powershell
rg -n "cmake|ctest|install-qt|shell:" .github/workflows/ci.yml
```

Expected: no Qt install, CMake configure/build, or CTest command exists; `shell:` appears only as a command interpreter selector.

- [x] **Step 2: Add the shell matrix job**

Add a sibling job using the immutable commit for official release `install-qt-action v4.3.1`:

```yaml
  shell:
    name: shell (${{ matrix.os }})
    runs-on: ${{ matrix.os }}
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]

    steps:
      - name: Checkout
        uses: actions/checkout@11d5960a326750d5838078e36cf38b85af677262 # v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@4360b52568e2003a75bf9bc1d59f33a8e3fc893c
        with:
          toolchain: 1.97.1

      - name: Install Qt
        uses: jurplel/install-qt-action@48d3ad6db93f3627c8ee7a0454bc6f3744f7e730 # v4.3.1
        with:
          version: 6.10.3
          cache: true

      - name: Build FFI bridge
        working-directory: core
        run: cargo build -p ffi-bridge

      - name: Locate FFI bridge
        id: ffi
        shell: pwsh
        run: |
          $path = if ($IsWindows) {
            Join-Path $env:GITHUB_WORKSPACE 'core/target/debug/ffi_bridge.lib'
          } else {
            Join-Path $env:GITHUB_WORKSPACE 'core/target/debug/libffi_bridge.a'
          }
          if (-not (Test-Path -LiteralPath $path)) { throw "Missing FFI bridge: $path" }
          "path=$path" >> $env:GITHUB_OUTPUT

      - name: Configure shell
        run: >-
          cmake -S shell -B build/shell
          -DFFI_BRIDGE_LIB=${{ steps.ffi.outputs.path }}
          -DPDF_PLATFORM_USE_OPENGL=OFF
          -DBUILD_TESTING=ON

      - name: Build shell
        run: cmake --build build/shell --config Release

      - name: Test shell
        run: ctest --test-dir build/shell -C Release --output-on-failure
```

- [x] **Step 3: Validate workflow syntax and local diff hygiene**

Run:

```powershell
python -c "import pathlib, yaml; yaml.safe_load(pathlib.Path('.github/workflows/ci.yml').read_text())"
git diff --check
```

If PyYAML is unavailable, use Ruby's standard YAML parser already present on GitHub runners or review with `gh workflow view ci.yml --yaml` after pushing; do not add a project dependency solely for YAML parsing.

- [x] **Step 4: Run local verifications available on this machine**

Run:

```powershell
cargo build -p ffi-bridge
cargo test -p ffi-bridge
git diff --check
```

If Qt remains unavailable locally, state that the CMake/QTest proof is pending the CI matrix; do not claim it passed.

- [x] **Step 5: Commit the implementation**

Create a signed commit:

```text
test(shell): add cross-platform QTest gate

Cites: ADR-003, ADR-022, ADR-026, ADR-029, SDS §13.3,
SDS §13.6, SDS §13.7, SDS §14 M0, T-8, GR-3, GR-8
```

### Task 4: Verify scope and record honest M0 status

**Files:**
- Modify only if evidence changes: `docs/milestone-exit-tracker.md`

**Interfaces:**
- Consumes: three completed `shell (<os>)` CI checks.
- Produces: an evidence-backed tracker row; no M0 completion claim.

- [x] **Step 1: Run the complete focused verification**

Run locally:

```powershell
cargo test -p ffi-bridge
git diff --check
git status --short
```

Run through CI on all three operating systems:

```text
shell (ubuntu-latest)
shell (windows-latest)
shell (macos-latest)
```

Expected: all three configure, compile, and pass `shell.canvas_input`.

- [x] **Step 2: Preserve the remaining M0 gap**

Update the tracker only after the matrix supplies evidence. Record “shell builds + QTest on three OSes” separately from “real PDF rendered end-to-end”; leave the latter partial until a real PDF is opened and a tile is asserted through the shell on all three systems.

- [x] **Step 3: Review the final diff against exclusions**

Run:

```powershell
git diff origin/main...HEAD --name-only
git diff origin/main...HEAD -- core/ffi-bridge shell/bridge core/sandbox
```

Expected final exception: one unused platform import is removed from `core/ffi-bridge`, and one Unix-only test binding is made mutable under `core/sandbox`; both are isolated, behavior-neutral commits requiring their respective human owners. `shell/bridge` remains unchanged.
