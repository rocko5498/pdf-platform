# GPU compositor: maps shmem TileHandles and composites into QOpenGLWidget. [ADR-007, SDS §6]
# M0: software paint from shmem RGBA8 buffer.

add_library(shell-canvas STATIC
    canvas.cc
)
target_link_libraries(shell-canvas PUBLIC shell-bridge Qt6::Widgets Qt6::OpenGL)
