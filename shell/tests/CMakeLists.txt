# Widget input-to-command coverage. [ADR-026, ADR-029, T-8]

add_executable(shell-canvas-input-test
    canvas_input_test.cc
)
target_link_libraries(shell-canvas-input-test PRIVATE
    shell-canvas
    Qt6::Test
    Qt6::Widgets
)

add_test(NAME shell.canvas_input COMMAND shell-canvas-input-test)
set_tests_properties(shell.canvas_input PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen;PDF_PLATFORM_UI_REGISTRY=${CMAKE_SOURCE_DIR}/chrome/ui-registry.toml"
    ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
)

# M0 exit criterion: tile through real bridge + IPC + shmem, on every OS.
# [SDS §14 M0, ADR-022 T-5, ADR-029]
#
# Needs two runtime inputs the input test does not: the worker binary, which
# the bridge spawns from its own executable's directory, and a provisioned
# PDFium, which the worker binds. WORKER_BIN is set by the caller (CI passes
# core/target/<profile>/worker[.exe]); without it the test is not built, so a
# missing worker can never look like a passing gate.
if(DEFINED WORKER_BIN AND EXISTS "${WORKER_BIN}")
    add_executable(shell-canvas-tile-test
        canvas_tile_test.cc
    )
    target_link_libraries(shell-canvas-tile-test PRIVATE
        shell-bridge
        Qt6::Test
        Qt6::Widgets
    )
    target_compile_definitions(shell-canvas-tile-test PRIVATE
        PDF_PLATFORM_FIXTURE_DIR="${CMAKE_SOURCE_DIR}/../tools/corpus-diff/fixtures"
    )

    # The bridge spawns `worker` from the directory of the running executable.
    add_custom_command(TARGET shell-canvas-tile-test POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${WORKER_BIN}"
                "$<TARGET_FILE_DIR:shell-canvas-tile-test>"
        COMMENT "Staging worker binary beside the tile test"
    )

    add_test(NAME shell.canvas_tile COMMAND shell-canvas-tile-test)
    set_tests_properties(shell.canvas_tile PROPERTIES
        ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
        ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
        TIMEOUT 120
    )
    # Printing reaches the output: print to a PDF, reopen it with the product's
    # own engine, look at the pixels. Needs the worker for the same reason the
    # tile test does. [FR-PRINT-1, FR-PRINT-3, T-10, T-12]
    add_executable(shell-print-roundtrip-test
        print_roundtrip_test.cc
    )
    target_link_libraries(shell-print-roundtrip-test PRIVATE
        shell-print
        shell-bridge
        Qt6::Test
        Qt6::Widgets
        Qt6::PrintSupport
    )
    target_compile_definitions(shell-print-roundtrip-test PRIVATE
        PDF_PLATFORM_FIXTURE_DIR="${CMAKE_SOURCE_DIR}/../tools/corpus-diff/fixtures"
    )
    add_custom_command(TARGET shell-print-roundtrip-test POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${WORKER_BIN}"
                "$<TARGET_FILE_DIR:shell-print-roundtrip-test>"
        COMMENT "Staging worker binary beside the print round-trip test"
    )
    add_test(NAME shell.print_roundtrip COMMAND shell-print-roundtrip-test)
    set_tests_properties(shell.print_roundtrip PROPERTIES
        ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
        ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
        TIMEOUT 180
    )
else()
    message(STATUS "shell.canvas_tile skipped: WORKER_BIN not set or missing")
endif()

# Pure layout geometry: no worker, so it runs on every build. [FR-PRINT-2, T-10]
add_executable(shell-print-layout-test
    print_layout_test.cc
)
target_link_libraries(shell-print-layout-test PRIVATE
    shell-print
    Qt6::Test
    Qt6::Widgets
    Qt6::PrintSupport
)
add_test(NAME shell.print_layout COMMAND shell-print-layout-test)
set_tests_properties(shell.print_layout PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
    ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
)

# ADR-032: bindings live in ui-registry.toml, not in C++. The rebinding test is
# the one that can tell the difference. [ADR-032, T-8]
add_executable(shell-registry-binding-test
    registry_binding_test.cc
)
target_link_libraries(shell-registry-binding-test PRIVATE
    shell-chrome
    Qt6::Test
    Qt6::Widgets
)
target_compile_definitions(shell-registry-binding-test PRIVATE
    PDF_PLATFORM_UI_REGISTRY_SOURCE="${CMAKE_SOURCE_DIR}/chrome/ui-registry.toml"
)
add_test(NAME shell.registry_binding COMMAND shell-registry-binding-test)
set_tests_properties(shell.registry_binding PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
    ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
)

# ADR-032: the declared menu taxonomy must be what the application builds.
# `[[menus]]` was read by nothing until this test's subject existed. [ADR-032, DS-MENU-1, T-8]
add_executable(shell-menu-bar-test
    menu_bar_test.cc
)
target_link_libraries(shell-menu-bar-test PRIVATE
    shell-canvas
    Qt6::Test
    Qt6::Widgets
)
add_test(NAME shell.menu_bar COMMAND shell-menu-bar-test)
set_tests_properties(shell.menu_bar PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen;PDF_PLATFORM_UI_REGISTRY=${CMAKE_SOURCE_DIR}/chrome/ui-registry.toml"
    ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
)

# The thumbnail list asks only for the pages in view. [FR-THUMB-1, T-10]
add_executable(shell-thumbnail-panel-test
    thumbnail_panel_test.cc
)
target_link_libraries(shell-thumbnail-panel-test PRIVATE
    shell-panels
    Qt6::Test
    Qt6::Widgets
)
add_test(NAME shell.thumbnail_panel COMMAND shell-thumbnail-panel-test)
set_tests_properties(shell.thumbnail_panel PROPERTIES
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
    ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
)
