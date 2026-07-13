# Sole C++ counterpart to the cxx bridge. [ADR-004, GR-3]
# Two-reviewer rule: any change here requires one FFI-surface owner. [ADR-027]

# Find the cxx-generated headers from the Cargo build output.
# The headers live in core/target/debug/build/ffi-bridge-*/out/cxxbridge/include/
file(GLOB FFI_BRIDGE_BUILD_DIRS
    "${CMAKE_SOURCE_DIR}/../core/target/debug/build/ffi-bridge-*/out"
)
# Use the most recent one.
list(SORT FFI_BRIDGE_BUILD_DIRS)
list(REVERSE FFI_BRIDGE_BUILD_DIRS)
list(GET FFI_BRIDGE_BUILD_DIRS 0 FFI_BRIDGE_OUT_DIR)

if(NOT EXISTS "${FFI_BRIDGE_OUT_DIR}/cxxbridge/include/rust/cxx.h")
    message(FATAL_ERROR
        "cxx-generated headers not found. Build the Rust workspace first:\n"
        "  cd core && cargo build -p ffi-bridge\n"
        "Then re-run CMake."
    )
endif()

add_library(shell-bridge STATIC
    # M0: header-only bridge (inline functions in bridge.h)
)
target_include_directories(shell-bridge PUBLIC
    "${FFI_BRIDGE_OUT_DIR}/cxxbridge/include"  # rust/cxx.h + ffi-bridge/src/lib.rs.h
    "${CMAKE_CURRENT_SOURCE_DIR}"              # bridge.h
)
target_link_libraries(shell-bridge PUBLIC ffi-bridge Qt6::Widgets)
