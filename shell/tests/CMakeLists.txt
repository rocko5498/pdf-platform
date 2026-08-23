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
else()
    message(STATUS "shell.canvas_tile skipped: WORKER_BIN not set or missing")
endif()

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
