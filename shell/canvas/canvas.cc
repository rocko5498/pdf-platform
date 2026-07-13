// Canvas widget implementation. [ADR-007, SDS §6.4]
// M0: software paint from shmem RGBA8 buffer.

#include "canvas.h"

#ifdef _WIN32
#include <windows.h>
#endif

#include <QPainter>
#include <rust/cxx.h>

#include "bridge.h"

namespace pdf_platform {

// ── CanvasWidget ────────────────────────────────────────────────────────────

CanvasWidget::CanvasWidget(QWidget* parent)
    : QWidget(parent) {
    setMinimumSize(256, 256);
    setWindowTitle("PDF Platform — M0 Canvas");
}

void CanvasWidget::setTile(const uint8_t* buffer, uint32_t width, uint32_t height) {
    tile_image_ = QImage(buffer, width, height, width * 4,
                         QImage::Format_RGBA8888).copy();
    has_tile_ = true;
    update();
}

void CanvasWidget::clear() {
    tile_image_ = QImage();
    has_tile_ = false;
    update();
}

void CanvasWidget::paintEvent(QPaintEvent* /*event*/) {
    QPainter painter(this);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, false);

    if (has_tile_ && !tile_image_.isNull()) {
        QRect target = tile_image_.rect();
        target.moveCenter(rect().center());
        painter.drawImage(target, tile_image_);
    } else {
        painter.fillRect(rect(), QColor(240, 240, 240));
        painter.setPen(QColor(128, 128, 128));
        painter.drawText(rect(), Qt::AlignCenter, "No document loaded");
    }
}

// ── MainWindow ──────────────────────────────────────────────────────────────

MainWindow::MainWindow(QWidget* parent)
    : QMainWindow(parent) {
    canvas_ = new CanvasWidget(this);
    setCentralWidget(canvas_);
    resize(800, 600);
}

MainWindow::~MainWindow() {
#ifdef _WIN32
    if (shmem_mapping_) {
        UnmapViewOfFile(shmem_mapping_);
        shmem_mapping_ = nullptr;
    }
#endif
    close_document();
}

bool MainWindow::openDocument(const QString& path) {
    close_document();

    // Open via Rust coordinator. cxx converts Result<T,String> to exceptions.
    pdf_platform::OpenResultFFI info;
    try {
        info = pdf_platform::open_document_impl(path.toStdString());
    } catch (const rust::error& e) {
        qWarning("open_document failed: %s", e.what());
        return false;
    }

    // Map the shared memory region.
#ifdef _WIN32
    HANDLE handle = reinterpret_cast<HANDLE>(info.shmem_handle);
    if (handle == nullptr || handle == INVALID_HANDLE_VALUE) {
        return false;
    }

    shmem_size_ = 256 * 256 * 4;
    shmem_mapping_ = MapViewOfFile(handle, FILE_MAP_READ, 0, 0, shmem_size_);
    if (!shmem_mapping_) {
        return false;
    }
#else
    return false;
#endif

    // Render the first tile.
    pdf_platform::TileResultFFI desc;
    try {
        desc = pdf_platform::render_tile_impl(0, 0, 0, 256, 256, 1.0, 1);
    } catch (const rust::error& e) {
        qWarning("render_tile failed: %s", e.what());
        return false;
    }

    const auto* pixels = static_cast<const uint8_t*>(shmem_mapping_) + desc.offset;
    canvas_->setTile(pixels, 256, 256);

    setWindowTitle(QString("PDF Platform — %1").arg(path));
    return true;
}

}  // namespace pdf_platform
