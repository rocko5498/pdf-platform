// Canvas widget: displays rendered tiles from shared memory. [ADR-007, SDS §6.4]
// M0: software paint from shmem RGBA8 buffer. GPU texture upload deferred.

#pragma once

#include <QImage>
#include <QMainWindow>
#include <QWidget>

namespace pdf_platform {

/// Minimal canvas widget that paints a tile from shared memory.
class CanvasWidget : public QWidget {
    Q_OBJECT

public:
    explicit CanvasWidget(QWidget* parent = nullptr);

    /// Display a tile from the shared memory buffer.
    /// @param buffer  Pointer to the RGBA8 pixel data.
    /// @param width   Tile width in pixels.
    /// @param height  Tile height in pixels.
    void setTile(const uint8_t* buffer, uint32_t width, uint32_t height);

    /// Clear the canvas.
    void clear();

protected:
    void paintEvent(QPaintEvent* event) override;

private:
    QImage tile_image_;
    bool has_tile_ = false;
};

/// Main window with canvas and minimal chrome.
class MainWindow : public QMainWindow {
    Q_OBJECT

public:
    explicit MainWindow(QWidget* parent = nullptr);
    ~MainWindow() override;

    /// Open a document and render its first tile.
    bool openDocument(const QString& path);

private:
    CanvasWidget* canvas_;
    void* shmem_mapping_ = nullptr;  // platform-specific mmap/MapViewOfFile
    uint32_t shmem_size_ = 0;
};

}  // namespace pdf_platform
