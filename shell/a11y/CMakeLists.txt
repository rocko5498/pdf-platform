# Accessibility: QAccessible surface for chrome and canvas.
# [PRD NFR-A11Y, DS-A11Y-*, ADR-026, SDS §2.1]

add_library(shell-a11y STATIC
    a11y.cc
    a11y.h
)
target_include_directories(shell-a11y PUBLIC
    "${CMAKE_CURRENT_SOURCE_DIR}"
)
target_link_libraries(shell-a11y PUBLIC Qt6::Widgets)
