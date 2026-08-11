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
    ENVIRONMENT "QT_QPA_PLATFORM=offscreen"
    ENVIRONMENT_MODIFICATION "PATH=path_list_prepend:$<TARGET_FILE_DIR:Qt6::Core>"
)
