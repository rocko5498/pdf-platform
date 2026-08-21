# Side panels: outline, diagnostics, annotation tools, forms. [SDS A2.1, M1/M4/M5]
# Shell is view-only over protocol events; no document truth. [ADR-026, GR-2]

add_library(shell-panels STATIC
    outline_panel.cc
    outline_panel.h
    diagnostics_panel.cc
    diagnostics_panel.h
    annotation_tools.cc
    annotation_tools.h
    forms_panel.cc
    forms_panel.h
    search_panel.cc
    search_panel.h
)
target_include_directories(shell-panels PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}"
)
target_link_libraries(shell-panels PUBLIC Qt6::Widgets)
