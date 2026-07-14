// Canvas widget: software paint from shmem RGBA8 tile. [ADR-007, SDS §6.4]
// M0: MapViewOfFile + QPainter blit. GPU texture compositor deferred to M1.

#include "canvas.h"

#include <QPainter>

#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

#include "bridge.h"

namespace pdf_platform {

// ---------------------------------------------------------------------------
// CanvasWidget
// ---------------------------------------------------------------------------

CanvasWidget::CanvasWidget(QWidget* parent) : QWidget(parent) {
    setMinimumSize(256, 256);
    setAttribute(Qt::WA_OpaquePaintEvent);
}

void CanvasWidget::setTile(const uint8_t* buffer, uint32_t width, uint32_t height) {
    // Format_RGBA8888 matches our RGBA8 pixel layout.
    // .copy() so we own the data independently of the shmem mapping.
    tile_image_ = QImage(buffer, static_cast<int>(width), static_cast<int>(height),
                         QImage::Format_RGBA8888)
                      .copy();
    has_tile_ = true;
    update();
}

void CanvasWidget::clear() {
    tile_image_ = QImage{};
    has_tile_ = false;
    update();
}

void CanvasWidget::paintEvent(QPaintEvent*) {
    QPainter p(this);
    if (has_tile_) {
        p.drawImage(0, 0, tile_image_);
    } else {
        p.fillRect(rect(), Qt::darkGray);
    }
}

// ---------------------------------------------------------------------------
// MainWindow
// ---------------------------------------------------------------------------

MainWindow::MainWindow(QWidget* parent) : QMainWindow(parent) {
    canvas_ = new CanvasWidget(this);
    setCentralWidget(canvas_);
    resize(256, 256);
    setWindowTitle("PDF Platform — M0");
}

MainWindow::~MainWindow() {
    if (shmem_mapping_) {
        UnmapViewOfFile(shmem_mapping_);
        shmem_mapping_ = nullptr;
    }
    close_document();
}

bool MainWindow::openDocument(const QString& path) {
    if (shmem_mapping_) {
        UnmapViewOfFile(shmem_mapping_);
        shmem_mapping_ = nullptr;
    }

    OpenResultFFI result;
    try {
        result = open_document(path.toStdString());
    } catch (const std::exception& e) {
        setWindowTitle(QString("PDF Platform — open failed: %1").arg(e.what()));
        return false;
    }

    // Map the shmem region for reading tile pixels.
    HANDLE h = reinterpret_cast<HANDLE>(static_cast<intptr_t>(result.shmem_handle));
    shmem_mapping_ = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
    shmem_size_ = 256u * 256u * 4u;  // TILE_RGBA8_BYTES

    if (!shmem_mapping_) {
        setWindowTitle("PDF Platform — shmem map failed");
        return false;
    }

    // Render page 0, 256x256 tile at 1.0x scale.
    try {
        auto tile = render_tile(0, 0, 0, 256, 256, 1.0f, 1);
        const auto* pixels = static_cast<const uint8_t*>(shmem_mapping_) + tile.offset;
        canvas_->setTile(pixels, 256, 256);
    } catch (const std::exception& e) {
        canvas_->clear();
        setWindowTitle(QString("PDF Platform — render failed: %1").arg(e.what()));
        return false;
    }

    setWindowTitle(QString("PDF Platform — M0 — %1 pages").arg(result.page_count));
    return true;
}

}  // namespace pdf_platform
