# Accessibility: AT-SPI2 (Linux), UIAutomation (Windows), NSAccessibility (macOS). [PRD NFR-A11Y]
# ponytail: stub — M5 implementation

add_library(shell-a11y STATIC
    # ponytail: stub
)
target_link_libraries(shell-a11y PUBLIC shell-bridge Qt6::Widgets)
