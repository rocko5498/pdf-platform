cmake_minimum_required(VERSION 3.25)
project(pdf-platform-shell CXX)

# Cargo-first build: Rust workspace must be built before this project.
# Shell only links the generated static lib from ffi-bridge. [ADR-004, SDS §13.3]
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

option(PDF_PLATFORM_USE_OPENGL "Use QOpenGLWidget for tile compositing" ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Widgets)
if(PDF_PLATFORM_USE_OPENGL)
    find_package(Qt6 6.5 REQUIRED COMPONENTS OpenGL OpenGLWidgets)
endif()
qt_standard_project_setup()

include(CTest)
if(BUILD_TESTING)
    find_package(Qt6 6.5 REQUIRED COMPONENTS Test)
endif()

if(NOT DEFINED FFI_BRIDGE_LIB)
    message(FATAL_ERROR "FFI_BRIDGE_LIB must be set to the path of the ffi-bridge static library.")
endif()

add_library(ffi-bridge STATIC IMPORTED)
set_target_properties(ffi-bridge PROPERTIES IMPORTED_LOCATION "${FFI_BRIDGE_LIB}")
if(WIN32)
    # Reported by `cargo rustc -p ffi-bridge -- --print native-static-libs`.
    target_link_libraries(ffi-bridge INTERFACE
        kernel32 ntdll userenv ws2_32 dbghelp
    )
endif()

# M1: bridge, panels, a11y, canvas (GPU), app. [SDS §14 M1]
add_subdirectory(bridge)
add_subdirectory(panels)
add_subdirectory(a11y)
add_subdirectory(canvas)
add_subdirectory(app)

if(BUILD_TESTING)
    add_subdirectory(tests)
endif()
