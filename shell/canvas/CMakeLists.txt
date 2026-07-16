# Canvas: GPU compositor (QOpenGLWidget) + software fallback. [ADR-007, SDS §6.4]

option(PDF_PLATFORM_USE_OPENGL "Use QOpenGLWidget for tile compositing" ON)

add_library(shell-canvas STATIC
    canvas.cc
    canvas.h
)
target_include_directories(shell-canvas PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}"
    "${CMAKE_SOURCE_DIR}/panels"
    "${CMAKE_SOURCE_DIR}/bridge"
)
target_link_libraries(shell-canvas PUBLIC
    shell-bridge
    shell-panels
    Qt6::Widgets
    Qt6::OpenGL
    Qt6::OpenGLWidgets
)
if(PDF_PLATFORM_USE_OPENGL)
    target_compile_definitions(shell-canvas PUBLIC PDF_PLATFORM_USE_OPENGL=1)
else()
    target_compile_definitions(shell-canvas PUBLIC PDF_PLATFORM_USE_OPENGL=0)
endif()
