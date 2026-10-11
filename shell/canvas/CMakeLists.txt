# Canvas: GPU compositor (QOpenGLWidget) + software fallback. [ADR-007, SDS §6.4]

# Printing lives in its own library so a test can drive it without a window.
# [FR-PRINT-1..3, SDS §2.6, ADR-003]
add_library(shell-print STATIC
    print_job.cc
    print_job.h
)
target_include_directories(shell-print PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}"
    "${CMAKE_SOURCE_DIR}/bridge"
)
target_link_libraries(shell-print PUBLIC
    shell-bridge
    Qt6::Widgets
    Qt6::PrintSupport
)

add_library(shell-canvas STATIC
    canvas.cc
    canvas.h
    view_history.cc
    view_history.h
)
target_include_directories(shell-canvas PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}"
    "${CMAKE_SOURCE_DIR}/panels"
    "${CMAKE_SOURCE_DIR}/bridge"
)
target_link_libraries(shell-canvas PUBLIC
    shell-bridge
    shell-panels
    shell-chrome
    shell-print
    Qt6::Widgets
    Qt6::PrintSupport
)
if(PDF_PLATFORM_USE_OPENGL)
    target_link_libraries(shell-canvas PUBLIC
        Qt6::OpenGL
        Qt6::OpenGLWidgets
    )
    target_compile_definitions(shell-canvas PUBLIC PDF_PLATFORM_USE_OPENGL=1)
else()
    target_compile_definitions(shell-canvas PUBLIC PDF_PLATFORM_USE_OPENGL=0)
endif()
