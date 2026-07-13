cmake_minimum_required(VERSION 3.25)
project(pdf-platform-shell CXX)

# Cargo-first build: Rust workspace must be built before this project.
# Shell only links the generated static lib from ffi-bridge. [ADR-004, SDS §13.3]
set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

find_package(Qt6 6.5 REQUIRED COMPONENTS Widgets OpenGL)
qt_standard_project_setup()

# ffi-bridge static lib produced by Cargo. Path set by parent build script.
if(NOT DEFINED FFI_BRIDGE_LIB)
    message(FATAL_ERROR "FFI_BRIDGE_LIB must be set to the path of the ffi-bridge static library.")
endif()

add_library(ffi-bridge STATIC IMPORTED)
set_target_properties(ffi-bridge PROPERTIES IMPORTED_LOCATION "${FFI_BRIDGE_LIB}")

# M0: only bridge, canvas, and app. Others deferred to M1+.
add_subdirectory(bridge)
add_subdirectory(canvas)
add_subdirectory(app)
