# Menu bar, toolbars, keyboard shortcuts. Loads ui-registry.toml via toml11. [ADR-032, SDS §13]
#
# The registry is the single source of truth for bindings: ADR-032 forbids a
# shortcut appearing in C++ source, so widgets ask this library whether an event
# matches an action id. Menus are not built here yet.

add_library(shell-chrome STATIC
    registry.cc
    registry.h
)
target_link_libraries(shell-chrome PUBLIC shell-bridge Qt6::Widgets)
target_include_directories(shell-chrome PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")
# toml11 v4.4.0, vendored single header (MIT). [ADR-028 §3, ADR-032]
target_include_directories(shell-chrome PRIVATE "${CMAKE_SOURCE_DIR}/../third_party/toml11")
# Last-resort registry path for a developer build running from the build tree.
target_compile_definitions(shell-chrome PRIVATE
    PDF_PLATFORM_UI_REGISTRY_SOURCE="${CMAKE_CURRENT_SOURCE_DIR}/ui-registry.toml"
)

# ui-registry.toml is installed alongside the binary; CI gates schema_version bumps. [ADR-032]
install(FILES ui-registry.toml DESTINATION share/pdf-platform)
