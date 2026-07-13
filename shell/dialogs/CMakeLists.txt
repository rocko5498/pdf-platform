# Modal dialogs: open, save, print, preferences, about. [SDS §13, PRD]
# ponytail: stub — M1+ implementation

add_library(shell-dialogs STATIC
    # ponytail: stub
)
target_link_libraries(shell-dialogs PUBLIC shell-bridge Qt6::Widgets)
