// Full viewer chrome: multi-page, zoom, find, copy, annotations, live panels.
// [FR-VIEW, FR-SRCH, FR-ANNOT, FR-BOOK, FR-DIAG, FR-REV-4, NFR-A11Y]

#include "canvas.h"

#include "annotation_tools.h"
#include "bridge.h"
#include "diagnostics_panel.h"
#include "forms_panel.h"
#include "outline_panel.h"

#include <QApplication>
#include <QClipboard>
#include <QDockWidget>
#include <QFile>
#include <QFileDialog>
#include <QFocusEvent>
#include <QInputDialog>
#include <QKeySequence>
#include <QLineEdit>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QStatusBar>
#include <QTextStream>
#include <QWheelEvent>

#ifndef WIN32_LEAN_AND_MEAN
#  define WIN32_LEAN_AND_MEAN
#endif
#include <windows.h>

namespace pdf_platform {

// ---------------------------------------------------------------------------
// CanvasWidget
// ---------------------------------------------------------------------------

CanvasWidget::CanvasWidget(QWidget* parent) : PDF_CANVAS_BASE(parent) {
    setMinimumSize(256, 256);
    setFocusPolicy(Qt::StrongFocus);
    setAccessibleName(QStringLiteral("Document canvas"));
    setMouseTracking(true);
#if !defined(PDF_PLATFORM_USE_OPENGL) || !PDF_PLATFORM_USE_OPENGL
    setAttribute(Qt::WA_OpaquePaintEvent);
#endif
}

CanvasWidget::~CanvasWidget() {
#if defined(PDF_PLATFORM_USE_OPENGL) && PDF_PLATFORM_USE_OPENGL
    if (gl_ready_ && texture_id_) {
        makeCurrent();
        glDeleteTextures(1, &texture_id_);
        doneCurrent();
    }
#endif
}

void CanvasWidget::setTile(const uint8_t* buffer, uint32_t width, uint32_t height) {
    tile_image_ = QImage(buffer, int(width), int(height), QImage::Format_RGBA8888).copy();
    has_tile_ = true;
    texture_dirty_ = true;
    update();
}

void CanvasWidget::clear() {
    tile_image_ = {};
    has_tile_ = false;
    texture_dirty_ = true;
    selection_.reset();
    update();
}

void CanvasWidget::setAccessibleStatus(const QString& status) {
    accessible_status_ = status;
    setAccessibleDescription(status);
}

void CanvasWidget::setSelectionOverlay(const QRectF& rect) {
    selection_ = rect;
    update();
}

void CanvasWidget::clearSelectionOverlay() {
    selection_.reset();
    update();
}

void CanvasWidget::paintSoftware(QPainter& p) {
    if (has_tile_) {
        p.drawImage(rect(), tile_image_);
    } else {
        p.fillRect(rect(), Qt::darkGray);
    }
    if (selection_) {
        // Map PDF-ish overlay rect already in widget coords from caller.
        p.setPen(QPen(QColor(0, 120, 215), 2));
        p.setBrush(QColor(0, 120, 215, 60));
        p.drawRect(*selection_);
    }
}

void CanvasWidget::paintEvent(QPaintEvent* event) {
#if defined(PDF_PLATFORM_USE_OPENGL) && PDF_PLATFORM_USE_OPENGL
    if (!gl_ready_) {
        QPainter p(this);
        paintSoftware(p);
        return;
    }
    QOpenGLWidget::paintEvent(event);
#else
    Q_UNUSED(event);
    QPainter p(this);
    paintSoftware(p);
#endif
}

void CanvasWidget::keyPressEvent(QKeyEvent* event) {
    switch (event->key()) {
    case Qt::Key_PageDown:
    case Qt::Key_Down:
    case Qt::Key_Space:
        emit pageStepRequested(+1);
        event->accept();
        return;
    case Qt::Key_PageUp:
    case Qt::Key_Up:
        emit pageStepRequested(-1);
        event->accept();
        return;
    case Qt::Key_Home:
        emit pageStepRequested(-100000);
        event->accept();
        return;
    case Qt::Key_End:
        emit pageStepRequested(+100000);
        event->accept();
        return;
    case Qt::Key_Plus:
    case Qt::Key_Equal:
        if (event->modifiers() & Qt::ControlModifier) {
            emit zoomStepRequested(+1);
            event->accept();
            return;
        }
        break;
    case Qt::Key_Minus:
        if (event->modifiers() & Qt::ControlModifier) {
            emit zoomStepRequested(-1);
            event->accept();
            return;
        }
        break;
    default:
        break;
    }
    PDF_CANVAS_BASE::keyPressEvent(event);
}

void CanvasWidget::wheelEvent(QWheelEvent* event) {
    if (event->modifiers() & Qt::ControlModifier) {
        emit zoomStepRequested(event->angleDelta().y() > 0 ? +1 : -1);
    } else {
        // Continuous scroll in document space. [SDS section 6.9, M1]
        emit scrollDeltaRequested(-event->angleDelta().y());
    }
    event->accept();
}

void CanvasWidget::focusInEvent(QFocusEvent* event) {
    PDF_CANVAS_BASE::focusInEvent(event);
    update();
}

void CanvasWidget::mousePressEvent(QMouseEvent* event) {
    // Forward to parent for annotation placement (MainWindow handles tool).
    event->ignore();
    PDF_CANVAS_BASE::mousePressEvent(event);
}

#if defined(PDF_PLATFORM_USE_OPENGL) && PDF_PLATFORM_USE_OPENGL
void CanvasWidget::initializeGL() {
    initializeOpenGLFunctions();
    gl_ready_ = true;
    using_gpu_ = true;
    glGenTextures(1, &texture_id_);
    glClearColor(0.2f, 0.2f, 0.2f, 1.f);
}

void CanvasWidget::uploadTextureIfNeeded() {
    if (!gl_ready_ || !texture_dirty_ || !has_tile_) return;
    glBindTexture(GL_TEXTURE_2D, texture_id_);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_LINEAR);
    glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_LINEAR);
    glTexImage2D(GL_TEXTURE_2D, 0, GL_RGBA, tile_image_.width(), tile_image_.height(), 0, GL_RGBA,
                 GL_UNSIGNED_BYTE, tile_image_.constBits());
    texture_dirty_ = false;
}

void CanvasWidget::paintGL() {
    glClear(GL_COLOR_BUFFER_BIT);
    if (!has_tile_) return;
    uploadTextureIfNeeded();
    QPainter p(this);
    paintSoftware(p);
}

void CanvasWidget::resizeGL(int w, int h) { glViewport(0, 0, w, h); }
#endif

// ---------------------------------------------------------------------------
// MainWindow
// ---------------------------------------------------------------------------

MainWindow::MainWindow(QWidget* parent) : QMainWindow(parent) {
    setupChrome();
    resize(1100, 800);
    setWindowTitle(QStringLiteral("PDF Platform"));
    setAccessibleName(QStringLiteral("PDF Platform"));
}

void MainWindow::setupChrome() {
    canvas_ = new CanvasWidget(this);
    setCentralWidget(canvas_);
    connect(canvas_, &CanvasWidget::pageStepRequested, this, [this](int d) {
        if (d <= -100000)
            goToPage(0);
        else if (d >= 100000)
            goToPage(int(page_count_) - 1);
        else
            goToPage(int(current_page_) + d);
    });
    connect(canvas_, &CanvasWidget::zoomStepRequested, this, &MainWindow::zoomBy);
    connect(canvas_, &CanvasWidget::scrollDeltaRequested, this, [this](int dy) {
        scroll_y_ += float(dy);
        clampScroll();
        const float page_h = page_height_ * scale_ + 16.f;
        if (page_h > 0.f && page_count_ > 0) {
            int p = int(scroll_y_ / page_h);
            if (p < 0) p = 0;
            if (p >= int(page_count_)) p = int(page_count_) - 1;
            current_page_ = uint32_t(p);
        }
        renderVisibleTiles();
    });

    annot_tools_ = new AnnotationToolBar(this);
    addToolBar(Qt::TopToolBarArea, annot_tools_);
    connect(annot_tools_, &AnnotationToolBar::toolChanged, this, [this](AnnotationTool t) {
        annot_tool_ = static_cast<int>(t);
        statusBar()->showMessage(QStringLiteral("Tool %1 — click canvas to place").arg(annot_tool_),
                                 2500);
    });

    outline_ = new OutlinePanel(this);
    auto* od = new QDockWidget(QStringLiteral("Bookmarks"), this);
    od->setWidget(outline_);
    addDockWidget(Qt::LeftDockWidgetArea, od);

    diagnostics_ = new DiagnosticsPanel(this);
    auto* dd = new QDockWidget(QStringLiteral("Diagnostics"), this);
    dd->setWidget(diagnostics_);
    addDockWidget(Qt::RightDockWidgetArea, dd);

    forms_ = new FormsPanel(this);
    auto* fd = new QDockWidget(QStringLiteral("Forms"), this);
    fd->setObjectName(QStringLiteral("formsDock"));
    fd->setWidget(forms_);
    addDockWidget(Qt::RightDockWidgetArea, fd);
    connect(forms_, &FormsPanel::seedDemoRequested, this, &MainWindow::seedFormDemo);
    connect(forms_, &FormsPanel::setFieldRequested, this, &MainWindow::applyFormField);
    connect(forms_, &FormsPanel::runCalcRequested, this, &MainWindow::runFormsCalc);
    connect(forms_, &FormsPanel::jsEnabledRequested, this, &MainWindow::setFormsJsEnabled);

    statusBar()->showMessage(
        QStringLiteral("Ready — Ctrl+O open, Ctrl+F find, Ctrl+G forms calc, Ctrl+C copy text"));
}

MainWindow::~MainWindow() {
    if (shmem_mapping_) {
        UnmapViewOfFile(shmem_mapping_);
        shmem_mapping_ = nullptr;
    }
    close_document();
}

void MainWindow::mapShmem(qintptr handle) {
    if (shmem_mapping_) {
        UnmapViewOfFile(shmem_mapping_);
        shmem_mapping_ = nullptr;
    }
    HANDLE h = reinterpret_cast<HANDLE>(handle);
    shmem_mapping_ = MapViewOfFile(h, FILE_MAP_READ, 0, 0, 0);
}

bool MainWindow::openDocument(const QString& path) {
    path_ = path;
    // Try without password first; if core reports password required, prompt.
    if (openDocumentWithPassword(path, QString())) {
        return true;
    }
    // Second chance: password dialog. [FR-VIEW encrypt]
    bool ok = false;
    QString pw = QInputDialog::getText(this, QStringLiteral("Password"),
                                       QStringLiteral("Document is encrypted. Enter password:"),
                                       QLineEdit::Password, {}, &ok);
    if (!ok) return false;
    return openDocumentWithPassword(path, pw);
}

bool MainWindow::openDocumentWithPassword(const QString& path, const QString& password) {
    OpenResultFFI result;
    try {
        result = open_document(path.toStdString(), password.toStdString());
    } catch (const std::exception& e) {
        QString msg = QString::fromUtf8(e.what());
        if (msg.contains(QStringLiteral("password"), Qt::CaseInsensitive) && password.isEmpty()) {
            // Caller may retry with password.
            return false;
        }
        QMessageBox::warning(this, QStringLiteral("Open failed"), msg);
        if (diagnostics_) diagnostics_->setReport(msg, {});
        return false;
    }

    mapShmem(static_cast<qintptr>(result.shmem_handle));
    if (!shmem_mapping_) {
        QMessageBox::warning(this, QStringLiteral("Open failed"),
                             QStringLiteral("Could not map shared memory."));
        return false;
    }

    page_count_ = result.page_count;
    page_width_ = result.page_width;
    page_height_ = result.page_height;
    current_page_ = 0;
    scale_ = 1.0f;
    scroll_y_ = 0.f;
    generation_ = 1;

    if (!renderCurrentPage()) return false;

    setWindowTitle(QString("PDF Platform — %1 (%2 pages)").arg(path).arg(page_count_));
    refreshPanels();
    canvas_->setFocus(Qt::OtherFocusReason);
    statusBar()->showMessage(
        QString("Opened %1 pages · leniency %2 · GPU %3")
            .arg(page_count_)
            .arg(result.leniency_count)
            .arg(canvas_->usingGpu() ? "yes" : "software"),
        5000);
    return true;
}


float MainWindow::docHeightPx() const {
    return float(page_count_) * (page_height_ * scale_ + 16.f);
}

void MainWindow::clampScroll() {
    const float max_s = qMax(0.f, docHeightPx() - float(canvas_->height()));
    if (scroll_y_ < 0.f) scroll_y_ = 0.f;
    if (scroll_y_ > max_s) scroll_y_ = max_s;
}

bool MainWindow::renderCurrentPage() {
    scroll_y_ = float(current_page_) * (page_height_ * scale_ + 16.f);
    return renderVisibleTiles();
}

bool MainWindow::renderVisibleTiles() {
    // Multi-tile continuous composite. [SDS section 6, ADR-007, M1 exit]
    if (!shmem_mapping_ || page_count_ == 0) return false;
    const int vw = qMax(kTile, canvas_->width());
    const int vh = qMax(kTile, canvas_->height());
    QImage composite(vw, vh, QImage::Format_RGBA8888);
    composite.fill(QColor(40, 40, 40));

    const float page_h_px = page_height_ * scale_;
    const float page_w_px = page_width_ * scale_;
    const float gap = 16.f;
    const float y0 = scroll_y_;
    const float y1 = scroll_y_ + float(vh);

    try {
        for (uint32_t page = 0; page < page_count_; ++page) {
            const float page_top = float(page) * (page_h_px + gap);
            const float page_bot = page_top + page_h_px;
            if (page_bot < y0 || page_top > y1) continue;

            const uint32_t dev_w = uint32_t(qMax(1.f, page_w_px));
            const uint32_t dev_h = uint32_t(qMax(1.f, page_h_px));

            const int vis_y0 = int(qMax(0.f, y0 - page_top));
            const int vis_y1 = int(qMin(float(dev_h), y1 - page_top));
            const int vis_x1 = int(qMin(float(dev_w), float(vw)));

            const int edge = kTile;
            const int start_col = 0;
            const int end_col = qMax(0, (vis_x1 - 1) / edge);
            const int start_row = vis_y0 / edge;
            const int end_row = qMax(start_row, (vis_y1 - 1) / edge);

            for (int row = start_row; row <= end_row; ++row) {
                for (int col = start_col; col <= end_col; ++col) {
                    const uint32_t tx = uint32_t(col * edge);
                    const uint32_t ty = uint32_t(row * edge);
                    const uint32_t tw = qMin(uint32_t(edge), dev_w > tx ? dev_w - tx : 0u);
                    const uint32_t th = qMin(uint32_t(edge), dev_h > ty ? dev_h - ty : 0u);
                    if (tw == 0 || th == 0) continue;

                    auto tr = render_tile(page, tx, ty, tw, th, scale_, generation_++);
                    const auto* pixels =
                        static_cast<const uint8_t*>(shmem_mapping_) + tr.offset;
                    // Deep-copy tile pixels before next render overwrites shmem.
                    QImage tile(pixels, int(tw), int(th), int(tw) * 4, QImage::Format_RGBA8888);
                    tile = tile.copy();
                    const int dx = int(tx);
                    const int dy = int(page_top - y0) + int(ty);
                    QPainter p(&composite);
                    p.drawImage(dx, dy, tile);
                }
            }
        }

        // Upload composite (setTile copies into owned QImage).
        canvas_->setTile(composite.constBits(), uint32_t(composite.width()),
                         uint32_t(composite.height()));
        canvas_->setAccessibleStatus(
            QString("Page %1 of %2, scroll %3, zoom %4%")
                .arg(current_page_ + 1)
                .arg(page_count_)
                .arg(int(scroll_y_))
                .arg(int(scale_ * 100)));
        statusBar()->showMessage(
            QString("Page %1/%2  scroll %3px  zoom %4%")
                .arg(current_page_ + 1)
                .arg(page_count_)
                .arg(int(scroll_y_))
                .arg(int(scale_ * 100)),
            1500);
        return true;
    } catch (const std::exception& e) {
        QMessageBox::warning(this, QStringLiteral("Render failed"), QString::fromUtf8(e.what()));
        return false;
    }
}


void MainWindow::refreshPanels() {
    try {
        if (outline_) {
            QString data = QString::fromStdString(get_outline());
            // Parse entries= from first line if present
            int entries = 0, total = 0;
            for (const QString& line : data.split('\n')) {
                if (line.startsWith("entries=")) entries = line.mid(8).toInt();
                if (line.startsWith("total=")) total = line.mid(6).toInt();
            }
            outline_->setOutlineData(data, entries, total > 0 ? total : entries);
        }
        if (diagnostics_) {
            QString report = QString::fromStdString(diagnostics());
            QString len = QString::fromStdString(leniency_events());
            QStringList events = len.isEmpty() ? QStringList{} : len.split('\n', Qt::SkipEmptyParts);
            // Also surface layers/attachments summary
            try {
                report += QStringLiteral("\n\nLayers:\n") + QString::fromStdString(get_layers());
                report += QStringLiteral("\n\nAttachments:\n") + QString::fromStdString(get_attachments());
                report += QStringLiteral("\nAnnotations: ") + QString::number(annotation_count());
            } catch (...) {
            }
            diagnostics_->setReport(report, events);
        }
        refreshFormsPanel();
    } catch (const std::exception& e) {
        if (diagnostics_) {
            diagnostics_->setReport(QString("Panel refresh: %1").arg(e.what()), {});
        }
    }
}

void MainWindow::refreshFormsPanel() {
    if (!forms_) return;
    try {
        forms_->setFieldsData(QString::fromStdString(list_form_fields()));
    } catch (const std::exception& e) {
        forms_->setFieldsData(QStringLiteral("count=0\nnote=%1").arg(e.what()));
    }
}

void MainWindow::seedFormDemo() {
    try {
        QString msg = QString::fromStdString(seed_form_demo());
        statusBar()->showMessage(msg, 4000);
        refreshFormsPanel();
    } catch (const std::exception& e) {
        QMessageBox::warning(this, QStringLiteral("Forms"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::applyFormField(const QString& name, const QString& value) {
    try {
        QString msg = QString::fromStdString(
            set_form_field(name.toStdString(), value.toStdString()));
        statusBar()->showMessage(msg, 4000);
        refreshFormsPanel();
    } catch (const std::exception& e) {
        QMessageBox::warning(this, QStringLiteral("Forms"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::runFormsCalc() {
    try {
        QString msg = QString::fromStdString(run_forms_calc());
        statusBar()->showMessage(msg.section('\n', 0, 0), 5000);
        if (diagnostics_) {
            diagnostics_->setReport(QStringLiteral("Forms calc\n%1").arg(msg), {});
        }
        refreshFormsPanel();
    } catch (const std::exception& e) {
        QMessageBox::warning(this, QStringLiteral("Forms calc"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::setFormsJsEnabled(bool enabled) {
    try {
        QString msg = QString::fromStdString(set_forms_js_enabled(enabled));
        statusBar()->showMessage(msg, 3000);
        refreshFormsPanel();
    } catch (const std::exception& e) {
        QMessageBox::warning(this, QStringLiteral("Forms JS"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::goToPage(int page) {
    if (page_count_ == 0) return;
    if (page < 0) page = 0;
    if (page >= int(page_count_)) page = int(page_count_) - 1;
    current_page_ = uint32_t(page);
    canvas_->clearSelectionOverlay();
    renderCurrentPage();
}

void MainWindow::zoomBy(int steps) {
    scale_ *= (steps > 0) ? 1.15f : (1.f / 1.15f);
    if (scale_ < 0.25f) scale_ = 0.25f;
    if (scale_ > 8.f) scale_ = 8.f;
    clampScroll();
    renderVisibleTiles();
}

void MainWindow::findNext() {
    bool ok = false;
    QString q = QInputDialog::getText(this, QStringLiteral("Find"),
                                      QStringLiteral("Find text:"), QLineEdit::Normal,
                                      last_find_query_, &ok);
    if (!ok || q.isEmpty()) return;
    last_find_query_ = q;
    try {
        QString result = QString::fromStdString(find_text(q.toStdString()));
        statusBar()->showMessage(result.section('\n', 0, 0), 8000);
        // Jump to first hit page if present
        for (const QString& line : result.split('\n')) {
            if (line.startsWith("hit page=")) {
                int p = line.mid(9).section(' ', 0, 0).toInt();
                goToPage(p);
                // Simple highlight band at top of canvas
                canvas_->setSelectionOverlay(QRectF(10, 10, width() * 0.5, 24));
                break;
            }
        }
        if (diagnostics_) {
            diagnostics_->setReport(QString("Find %1\n%2").arg(q, result), {});
        }
    } catch (const std::exception& e) {
        QMessageBox::information(this, QStringLiteral("Find"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::copyPageText() {
    try {
        QString text = QString::fromStdString(extract_page_text(current_page_));
        // Strip reliable= header line
        int nl = text.indexOf('\n');
        if (nl > 0) text = text.mid(nl + 1);
        QApplication::clipboard()->setText(text);
        statusBar()->showMessage(QStringLiteral("Copied page text to clipboard"), 3000);
    } catch (const std::exception& e) {
        QMessageBox::warning(this, QStringLiteral("Copy"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::exportAnnotationsXfdf() {
    try {
        QString xfdf = QString::fromStdString(export_xfdf());
        QString path = QFileDialog::getSaveFileName(this, QStringLiteral("Export XFDF"),
                                                    QStringLiteral("annotations.xfdf"),
                                                    QStringLiteral("XFDF (*.xfdf)"));
        if (path.isEmpty()) return;
        QFile f(path);
        if (!f.open(QIODevice::WriteOnly | QIODevice::Text)) {
            QMessageBox::warning(this, QStringLiteral("Export"), QStringLiteral("Cannot write file"));
            return;
        }
        QTextStream out(&f);
        out << xfdf;
        statusBar()->showMessage(QString("Exported %1 annotations").arg(annotation_count()), 4000);
    } catch (const std::exception& e) {
        QMessageBox::warning(this, QStringLiteral("Export XFDF"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::keyPressEvent(QKeyEvent* event) {
    if (event->matches(QKeySequence::Open)) {
        QString path = QFileDialog::getOpenFileName(this, QStringLiteral("Open PDF"), {},
                                                    QStringLiteral("PDF (*.pdf)"));
        if (!path.isEmpty()) openDocument(path);
        event->accept();
        return;
    }
    if (event->matches(QKeySequence::Find)) {
        findNext();
        event->accept();
        return;
    }
    if (event->matches(QKeySequence::Copy)) {
        copyPageText();
        event->accept();
        return;
    }
    if (event->key() == Qt::Key_F6) {
        canvas_->setFocus(Qt::ShortcutFocusReason);
        event->accept();
        return;
    }
    if (event->modifiers() & Qt::ControlModifier && event->key() == Qt::Key_S) {
        QString path = QFileDialog::getSaveFileName(
            this, QStringLiteral("Save PDF"), path_,
            QStringLiteral("PDF files (*.pdf)"));
        if (!path.isEmpty()) {
            try {
                QString msg = QString::fromStdString(save_document(path.toStdString()));
                statusBar()->showMessage(msg, 5000);
            } catch (const std::exception& e) {
                QMessageBox::warning(this, QStringLiteral("Save"), QString::fromUtf8(e.what()));
            }
        }
        event->accept();
        return;
    }
    if (event->modifiers() & Qt::ControlModifier && event->key() == Qt::Key_E) {
        exportAnnotationsXfdf();
        event->accept();
        return;
    }
    if (event->modifiers() & Qt::ControlModifier && event->key() == Qt::Key_G) {
        runFormsCalc();
        event->accept();
        return;
    }
    // Place annotation at center of page on Enter when a tool is active
    if (event->key() == Qt::Key_Return || event->key() == Qt::Key_Enter) {
        if (annot_tool_ > 0 && page_count_ > 0) {
            static const char* types[] = {"",     "highlight", "underline", "note",
                                          "freetext", "ink",       "rect",      "stamp"};
            int idx = annot_tool_;
            if (idx < 8) {
                try {
                    float w = 120.f, h = 20.f;
                    float x = 72.f, y = page_height_ - 120.f - float(annotation_count() % 20) * 24.f;
                    uint64_t id = add_annotation(current_page_, types[idx], x, y, w, h,
                                                 "Annotation");
                    statusBar()->showMessage(QString("Added annotation id=%1").arg(id), 3000);
                    refreshPanels();
                    // Visual cue
                    canvas_->setSelectionOverlay(QRectF(20, 40 + (annotation_count() % 5) * 30, 200, 22));
                } catch (const std::exception& e) {
                    QMessageBox::warning(this, QStringLiteral("Annotate"),
                                         QString::fromUtf8(e.what()));
                }
            }
            event->accept();
            return;
        }
    }
    QMainWindow::keyPressEvent(event);
}

}  // namespace pdf_platform
