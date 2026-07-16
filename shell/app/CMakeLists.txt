# Application lifecycle. [SDS §13.1]
# M1: canvas + a11y + docked panels + annotation tools.

add_executable(pdf-platform WIN32
    main.cc
)
target_include_directories(pdf-platform PRIVATE
    "${CMAKE_SOURCE_DIR}/canvas"
    "${CMAKE_SOURCE_DIR}/bridge"
    "${CMAKE_SOURCE_DIR}/a11y"
    "${CMAKE_SOURCE_DIR}/panels"
)
target_link_libraries(pdf-platform PRIVATE
    shell-bridge
    shell-canvas
    shell-a11y
    shell-panels
    Qt6::Widgets
    Qt6::OpenGL
    Qt6::OpenGLWidgets
    ws2_32
    ntdll
    userenv
    bcrypt
    advapi32
    kernel32
    msvcrt
)
