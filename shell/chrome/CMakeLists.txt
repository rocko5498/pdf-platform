# Menu bar, toolbars, keyboard shortcuts. Loads ui-registry.toml via toml11. [ADR-032, SDS §13]
# ponytail: stub — add ChromeController.cpp at M0 implementation

add_library(shell-chrome STATIC
    # ponytail: stub — ChromeController.cpp here
)
target_link_libraries(shell-chrome PUBLIC shell-bridge Qt6::Widgets)
target_include_directories(shell-chrome PUBLIC "${CMAKE_CURRENT_SOURCE_DIR}")

# ui-registry.toml is installed alongside the binary; CI gates schema_version bumps. [ADR-032]
install(FILES ui-registry.toml DESTINATION share/pdf-platform)
