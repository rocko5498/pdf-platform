# Application lifecycle: QApplication init, session restore, shutdown. [SDS §13.1]
# M0: minimal entry point with canvas.

add_executable(pdf-platform WIN32
    main.cc
)
target_include_directories(pdf-platform PRIVATE
    "${CMAKE_SOURCE_DIR}/canvas"   # canvas.h
    "${CMAKE_SOURCE_DIR}/bridge"   # bridge.h
)
target_link_libraries(pdf-platform PRIVATE
    shell-bridge
    shell-canvas
    Qt6::Widgets
    # Windows system libs needed by the Rust staticlib (std networking, process spawn).
    ws2_32
    ntdll
    userenv
    bcrypt
    advapi32
    kernel32
    msvcrt
)
