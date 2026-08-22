// Full viewer chrome: multi-page, zoom, find, copy, annotations, live panels.
// [FR-VIEW, FR-SRCH, FR-ANNOT, FR-BOOK, FR-DIAG, FR-REV-4, NFR-A11Y]

#include "canvas.h"

#include "annotation_tools.h"
#include "bridge.h"
#include "diagnostics_panel.h"
#include "forms_panel.h"
#include "outline_panel.h"
#include "search_panel.h"

#include <QAccessible>
#include <QApplication>
#include <QClipboard>
#include <QDockWidget>
#include <QFile>
#include <QFileInfo>
#include <QFileDialog>
#include <QFocusEvent>
#include <QInputDialog>
#include <QKeySequence>

#include "registry.h"
#include <QLineEdit>
#include <QMessageBox>
#include <QMouseEvent>
#include <QPainter>
#include <QStatusBar>
#include <QTextStream>
#include <QWheelEvent>

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

void CanvasWidget::setAnnotationOverlays(const std::vector<std::pair<QRectF, QColor>>& overlays) {
    annot_overlays_ = overlays;
    update();
}

void CanvasWidget::paintSoftware(QPainter& p) {
    if (has_tile_) {
        p.drawImage(rect(), tile_image_);
    } else {
        p.fillRect(rect(), Qt::darkGray);
    }
    // Draw annotation overlays. [FR-ANNOT, M4]
    for (const auto& [r, color] : annot_overlays_) {
        p.setPen(QPen(color, 2));
        p.setBrush(QColor(color.red(), color.green(), color.blue(), 40));
        p.drawRect(r);
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
    // Bindings come from ui-registry.toml, never from this file. ADR-032 makes
    // the registry the single source of truth, and DS-CONV-4 makes it the
    // expression of PRIN-4: rebinding a key is a reviewable one-line diff there
    // plus a profile_version bump, with no C++ change. [ADR-032, ADR-030, RQA-1]
    const auto& keys = chrome::shortcuts();
    if (keys.matches(QStringLiteral("nav.next_page"), event)) {
        emit pageStepRequested(+1);
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("nav.prev_page"), event)) {
        emit pageStepRequested(-1);
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("nav.first_page"), event)) {
        emit pageStepRequested(-100000);
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("nav.last_page"), event)) {
        emit pageStepRequested(+100000);
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("view.zoom_in"), event)) {
        emit zoomStepRequested(+1);
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("view.zoom_out"), event)) {
        emit zoomStepRequested(-1);
        event->accept();
        return;
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
    // Emit click at PDF coordinates for annotation placement. [FR-ANNOT, M4]
    if (event->button() == Qt::LeftButton) {
        float pdf_x = float(event->pos().x());
        float pdf_y = float(event->pos().y());
        emit canvasClicked(pdf_x, pdf_y);
    }
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
    // Canvas click → place annotation at click position. [FR-ANNOT, M4]
    connect(canvas_, &CanvasWidget::canvasClicked, this, [this](float pdf_x, float pdf_y) {
        if (annot_tool_ <= 0 || page_count_ == 0) return;
        static const char* types[] = {"", "highlight", "underline", "note",
                                      "freetext", "ink", "rect", "stamp"};
        int idx = annot_tool_;
        if (idx >= 8) return;
        // Convert screen coords to PDF page coords
        float page_h = page_height_ * scale_ + 16.f;
        int page = int(scroll_y_ / page_h);
        if (page < 0) page = 0;
        if (page >= int(page_count_)) page = int(page_count_) - 1;
        float page_top = float(page) * page_h;
        float pdf_y_page = (pdf_y + scroll_y_ - page_top) / scale_;
        float pdf_x_page = pdf_x / scale_;
        try {
            float w = 120.f, h = 20.f;
            uint64_t id = add_annotation(uint32_t(page), types[idx],
                                         pdf_x_page, pdf_y_page, w, h, "Annotation");
            statusBar()->showMessage(QString("Added annotation id=%1").arg(id), 3000);
            renderVisibleTiles();
        } catch (const std::exception& e) {
            QMessageBox::warning(this, QStringLiteral("Annotate"),
                                 QString::fromUtf8(e.what()));
        }
    });

    annot_tools_ = new AnnotationToolBar(this);
    addToolBar(Qt::TopToolBarArea, annot_tools_);
    connect(annot_tools_, &AnnotationToolBar::toolChanged, this, [this](AnnotationTool t) {
        annot_tool_ = static_cast<int>(t);
        static const char* tool_names[] = {"Select", "Highlight", "Underline", "Note",
                                           "FreeText", "Ink", "Rectangle", "Stamp"};
        int idx = annot_tool_;
        const char* name = (idx >= 0 && idx < 8) ? tool_names[idx] : "Unknown";
        statusBar()->showMessage(
        QStringLiteral("Tool %1 - click canvas to place").arg(name), 2500);
        // Live a11y announcement for tool change. [DS-A11Y-SR-2, NFR-A11Y]
        if (canvas_) {
            canvas_->setProperty("activeTool", QString::fromUtf8(name));
            QAccessibleEvent ev(canvas_, QAccessible::ValueChanged);
            QAccessible::updateAccessibility(&ev);
        }
    });

    outline_ = new OutlinePanel(this);
    auto* od = new QDockWidget(QStringLiteral("Bookmarks"), this);
    od->setObjectName(QStringLiteral("bookmarksDock"));
    od->setAccessibleName(QStringLiteral("Bookmarks"));
    od->setWidget(outline_);
    addDockWidget(Qt::LeftDockWidgetArea, od);
    // Bookmark navigation: activate entry → go to page. [FR-BOOK-1, M1 exit]
    connect(outline_, &OutlinePanel::entryActivated, this, [this](int page, float y) {
        goToPage(page);
        // TODO(M1): scroll to y offset within page (requires viewport model)
    });

    diagnostics_ = new DiagnosticsPanel(this);
    auto* dd = new QDockWidget(QStringLiteral("Diagnostics"), this);
    dd->setObjectName(QStringLiteral("diagnosticsDock"));
    dd->setAccessibleName(QStringLiteral("Diagnostics"));
    dd->setWidget(diagnostics_);
    addDockWidget(Qt::RightDockWidgetArea, dd);

    forms_ = new FormsPanel(this);
    auto* fd = new QDockWidget(QStringLiteral("Forms"), this);
    fd->setObjectName(QStringLiteral("formsDock"));
    fd->setAccessibleName(QStringLiteral("Forms"));
    fd->setWidget(forms_);
    addDockWidget(Qt::RightDockWidgetArea, fd);
    connect(forms_, &FormsPanel::seedDemoRequested, this, &MainWindow::seedFormDemo);
    connect(forms_, &FormsPanel::setFieldRequested, this, &MainWindow::applyFormField);
    connect(forms_, &FormsPanel::runCalcRequested, this, &MainWindow::runFormsCalc);
    connect(forms_, &FormsPanel::jsEnabledRequested, this, &MainWindow::setFormsJsEnabled);
    connect(forms_, &FormsPanel::validateRequested, this, [this]() {
        try {
            QString result = QString::fromStdString(validate_form());
            statusBar()->showMessage(result.section('\n', 0, 0), 5000);
            if (diagnostics_) diagnostics_->setReport(result, {});
        } catch (const std::exception& e) {
            QMessageBox::warning(this, QStringLiteral("Validate"), QString::fromUtf8(e.what()));
        }
    });
    connect(forms_, &FormsPanel::flattenRequested, this, [this]() {
        QMessageBox::StandardButton reply = QMessageBox::question(
            this, QStringLiteral("Flatten Form"),
            QStringLiteral("Flatten all form fields? This cannot be undone."),
            QMessageBox::Yes | QMessageBox::No, QMessageBox::No);
        if (reply != QMessageBox::Yes) return;
        try {
            QString result = QString::fromStdString(flatten_form());
            statusBar()->showMessage(result, 5000);
            refreshFormsPanel();
        } catch (const std::exception& e) {
            QMessageBox::warning(this, QStringLiteral("Flatten"), QString::fromUtf8(e.what()));
        }
    });

    // Search results panel. [FR-SRCH-3, M2 exit: search panel]
    search_ = new SearchPanel(this);
    auto* sd = new QDockWidget(QStringLiteral("Find"), this);
    sd->setObjectName(QStringLiteral("searchDock"));
    sd->setAccessibleName(QStringLiteral("Search results"));
    sd->setWidget(search_);
    addDockWidget(Qt::RightDockWidgetArea, sd);
    connect(search_, &SearchPanel::searchRequested, this, &MainWindow::performSearch);
    connect(search_, &SearchPanel::resultActivated, this, [this](int page, float x, float y, float w, float h) {
        goToPage(page);
        // Highlight match at actual geometry. [FR-SRCH-3, M2: match highlighting]
        // Map PDF points to canvas widget coordinates.
        float scale_px = scale_ * 72.f;  // approximate: PDF points → screen pixels
        canvas_->setSelectionOverlay(QRectF(
            x * scale_px, y * scale_px - scroll_y_,
            w * scale_px, h * scale_px));
    });

    statusBar()->showMessage(
        QStringLiteral("Ready - Ctrl+O open, Ctrl+F find, Ctrl+C copy"));
}

MainWindow::~MainWindow() {
    // The mapped pointer is borrowed from Rust SharedRegion. [ADR-011]
    shmem_mapping_ = nullptr;
    close_document();
}


void MainWindow::mapShmem(qintptr handle) {
    // Same-process FFI: Rust SharedRegion owns the mapped view. [ADR-011, SDS §6.3]
    shmem_mapping_ = handle == 0 ? nullptr : reinterpret_cast<void*>(handle);
}


bool MainWindow::openDocument(const QString& path) {
    // Canonical absolute path so spaces / relative CLI args resolve. [FR-VIEW]
    const QString abs = QFileInfo(path).absoluteFilePath();
    path_ = abs;
    bool needs_password = false;
    if (openDocumentWithPassword(abs, QString(), &needs_password)) {
        return true;
    }
    // Only prompt when the core reported encryption â€” not on missing file, etc. [FR-VIEW]
    if (!needs_password) {
        return false;
    }
    // Retry loop for wrong password. [FR-SEC-2, M1: encrypted open]
    for (int attempt = 0; attempt < 5; ++attempt) {
        bool ok = false;
        QString pw = QInputDialog::getText(
            this, QStringLiteral("Password"),
            attempt == 0
                ? QStringLiteral("Document is encrypted. Enter password:")
                : QStringLiteral("Wrong password. Try again:"),
            QLineEdit::Password, {}, &ok);
        if (!ok) return false;  // user cancelled
        if (openDocumentWithPassword(path_, pw, nullptr)) {
            return true;
        }
    }
    QMessageBox::warning(this, QStringLiteral("Password"),
                         QStringLiteral("Too many failed attempts."));
    return false;
}

bool MainWindow::openDocumentWithPassword(const QString& path, const QString& password,
                                           bool* needs_password) {
    OpenResultFFI result;
    try {
        result = open_document(path.toStdString(), password.toStdString());
    } catch (const std::exception& e) {
        QString msg = QString::fromUtf8(e.what());
        if (msg.contains(QStringLiteral("password"), Qt::CaseInsensitive) && password.isEmpty()) {
            // Only encrypt failures request the password dialog. [FR-VIEW, PRIN-6]
            if (needs_password) {
                *needs_password = true;
            }
            return false;
        }
        QMessageBox::warning(this, QStringLiteral("Open failed"), msg);
        if (diagnostics_) diagnostics_->setReport(msg, {});
        // Keep prior doc only if open failed without replacing session; core already closed.
        clearDocumentUi();
        return false;
    }

    mapShmem(static_cast<qintptr>(result.shmem_handle));
    if (!shmem_mapping_) {
        QMessageBox::warning(this, QStringLiteral("Open failed"),
                             QStringLiteral("Could not map shared memory (ptr=%1).").arg(result.shmem_handle));
        return false;
    }

    page_count_ = result.page_count;
    page_width_ = result.page_width;
    page_height_ = result.page_height;
    current_page_ = 0;
    scale_ = 1.0f;
    scroll_y_ = 0.f;
    generation_ = 1;

    if (!renderCurrentPage()) {
        QMessageBox::warning(this, QStringLiteral("Open failed"),
                             QStringLiteral("Document opened but first-page render failed."));
        return false;
    }

    setWindowTitle(QStringLiteral("PDF Platform - %1 (%2 pages)").arg(path_).arg(page_count_));
    refreshPanels();
    announceDocumentStatus();
    canvas_->setFocus(Qt::OtherFocusReason);
    statusBar()->showMessage(
        QStringLiteral("Opened %1 pages | leniency %2 | GPU %3")
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

        // Render annotation overlays on visible pages. [FR-ANNOT, M4]
        std::vector<std::pair<QRectF, QColor>> overlays;
        float scale_px = scale_;
        for (uint32_t page = 0; page < page_count_; ++page) {
            const float page_top = float(page) * (page_h_px + gap);
            const float page_bot = page_top + page_h_px;
            if (page_bot < y0 || page_top > y1) continue;

            try {
                QString data = QString::fromStdString(get_page_annotations(page));
                for (const QString& line : data.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
                    float ax = 0, ay = 0, aw = 0, ah = 0;
                    int cr = 255, cg = 0, cb = 0;
                    for (const QString& part : line.split(QLatin1Char('|'))) {
                        int eq = part.indexOf(QLatin1Char('='));
                        if (eq < 0) continue;
                        QString k = part.left(eq), v = part.mid(eq + 1);
                        if (k == QLatin1String("x")) ax = v.toFloat();
                        else if (k == QLatin1String("y")) ay = v.toFloat();
                        else if (k == QLatin1String("w")) aw = v.toFloat();
                        else if (k == QLatin1String("h")) ah = v.toFloat();
                        else if (k == QLatin1String("color")) {
                            QStringList c = v.split(QLatin1Char(','));
                            if (c.size() >= 3) {
                                cr = int(c[0].toFloat() * 255);
                                cg = int(c[1].toFloat() * 255);
                                cb = int(c[2].toFloat() * 255);
                            }
                        }
                    }
                    if (aw > 0 && ah > 0) {
                        // Map PDF coordinates to screen coordinates
                        float sx = ax * scale_px;
                        float sy = (ay * scale_px) - y0 + (page_top - y0);
                        float sw = aw * scale_px;
                        float sh = ah * scale_px;
                        overlays.push_back({QRectF(sx, sy, sw, sh), QColor(cr, cg, cb)});
                    }
                }
            } catch (...) {}
        }
        canvas_->setAnnotationOverlays(overlays);

        canvas_->setAccessibleStatus(
            QString("Page %1 of %2, scroll %3, zoom %4%, tool %5")
                .arg(current_page_ + 1)
                .arg(page_count_)
                .arg(int(scroll_y_))
                .arg(int(scale_ * 100))
                .arg(annot_tool_ == 0 ? "Select" : "Annotate"));
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
    // Each panel independently — one worker query must not blank diagnostics. [FR-DIAG, PRIN-6]
    QStringList panel_notes;

    if (outline_) {
        try {
            QString data = QString::fromStdString(get_outline());
            int entries = 0, total = 0;
            for (const QString& line : data.split(QLatin1Char('\n'))) {
                if (line.startsWith(QLatin1String("entries="))) entries = line.mid(8).toInt();
                if (line.startsWith(QLatin1String("total="))) total = line.mid(6).toInt();
            }
            outline_->setOutlineData(data, entries, total > 0 ? total : entries);
        } catch (const std::exception& e) {
            outline_->setOutlineData(QStringLiteral("outline_error=%1").arg(e.what()), 0, 0);
            panel_notes << QStringLiteral("outline: %1").arg(e.what());
        }
    }

    if (diagnostics_) {
        QString report;
        QStringList events;
        try {
            report = QString::fromStdString(diagnostics());
            QString len = QString::fromStdString(leniency_events());
            events = len.isEmpty() ? QStringList{} : len.split(QLatin1Char('\n'), Qt::SkipEmptyParts);
        } catch (const std::exception& e) {
            report = QStringLiteral("diagnostics: %1").arg(e.what());
            panel_notes << report;
        }
        try {
            report += QStringLiteral("\n\nLayers:\n") + QString::fromStdString(get_layers());
        } catch (const std::exception& e) {
            report += QStringLiteral("\n\nLayers: %1").arg(e.what());
            panel_notes << QStringLiteral("layers: %1").arg(e.what());
        }
        try {
            report += QStringLiteral("\n\nAttachments:\n") + QString::fromStdString(get_attachments());
        } catch (const std::exception& e) {
            report += QStringLiteral("\n\nAttachments: %1").arg(e.what());
            panel_notes << QStringLiteral("attachments: %1").arg(e.what());
        }
        try {
            report += QStringLiteral("\nAnnotations: ") + QString::number(annotation_count());
        } catch (...) {
        }
        if (!panel_notes.isEmpty()) {
            report += QStringLiteral("\n\nPanel notes:\n- ") + panel_notes.join(QStringLiteral("\n- "));
        }
        diagnostics_->setReport(report, events);
    }

    refreshFormsPanel();
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


void MainWindow::clearDocumentUi() {
    page_count_ = 0;
    current_page_ = 0;
    scale_ = 1.0f;
    scroll_y_ = 0.f;
    path_.clear();
    // Pointer owned by Rust session; drop local view only.
    shmem_mapping_ = nullptr;
    if (outline_) {
        outline_->clear();
    }
    if (diagnostics_) {
        diagnostics_->clear();
    }
    if (forms_) {
        forms_->clear();
    }
    if (search_) {
        search_->clear();
    }
    find_cursor_page_ = 0;
    find_cursor_line_ = 0;
    find_cursor_char_ = 0;
    find_current_index_ = -1;
    find_total_matches_ = 0;
    last_find_query_.clear();
    if (canvas_) {
        canvas_->clearSelectionOverlay();
    }
    announceDocumentStatus();
    setWindowTitle(QStringLiteral("PDF Platform - no document"));
}

void MainWindow::announceDocumentStatus() {
    if (!canvas_) {
        return;
    }
    QString status;
    if (page_count_ == 0) {
        status = QStringLiteral("No document open");
    } else {
        status = QStringLiteral("Page %1 of %2").arg(current_page_ + 1).arg(page_count_);
    }
    canvas_->setProperty("documentStatus", status);
    QAccessibleEvent ev(canvas_, QAccessible::ValueChanged);
    QAccessible::updateAccessibility(&ev);
}

void MainWindow::goToPage(int page) {
    if (page_count_ == 0) return;
    if (page < 0) page = 0;
    if (page >= int(page_count_)) page = int(page_count_) - 1;
    current_page_ = uint32_t(page);
    canvas_->clearSelectionOverlay();
    renderCurrentPage();
    announceDocumentStatus();
}

void MainWindow::zoomBy(int steps) {
    scale_ *= (steps > 0) ? 1.15f : (1.f / 1.15f);
    if (scale_ < 0.25f) scale_ = 0.25f;
    if (scale_ > 8.f) scale_ = 8.f;
    clampScroll();
    renderVisibleTiles();
    // Live a11y announcement for zoom change. [DS-A11Y-SR-2, NFR-A11Y]
    if (canvas_) {
        canvas_->setProperty("zoomLevel", int(scale_ * 100));
        QAccessibleEvent ev(canvas_, QAccessible::ValueChanged);
        QAccessible::updateAccessibility(&ev);
    }
}

void MainWindow::performSearch(const QString& query) {
    if (query.isEmpty()) return;
    last_find_query_ = query;
    find_cursor_page_ = 0;
    find_cursor_line_ = 0;
    find_cursor_char_ = 0;
    find_current_index_ = -1;
    try {
        QString result = QString::fromStdString(find_text(query.toStdString()));
        if (search_) {
            search_->setResults(result, query);
        }
        // Count hits
        find_total_matches_ = 0;
        for (const QString& line : result.split('\n')) {
            if (line.startsWith(QStringLiteral("hit "))) find_total_matches_++;
        }
        // Navigate to first hit
        if (find_total_matches_ > 0) {
            findNext();
        }
    } catch (const std::exception& e) {
        QMessageBox::information(this, QStringLiteral("Find"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::findNext() {
    if (last_find_query_.isEmpty()) {
        // Open search panel and focus input
        if (search_) {
            // Show the dock if hidden
            if (auto* dock = findChild<QDockWidget*>(QStringLiteral("searchDock"))) {
                dock->show();
            }
            if (auto* input = search_->findChild<QLineEdit*>(QStringLiteral("searchInput"))) {
                input->setFocus(Qt::ShortcutFocusReason);
                input->selectAll();
            }
        }
        return;
    }
    // Advance cursor past last match
    if (find_current_index_ >= 0 && find_total_matches_ > 0) {
        find_cursor_line_++;
        find_cursor_char_ = 0;
    }
    try {
        // Search from current position forward
        QString result = QString::fromStdString(find_text(last_find_query_.toStdString()));
        // Find the first hit at or after our cursor
        int best_page = -1;
        int best_line = -1;
        float bx = 0, by = 0, bw = 0, bh = 0;
        QString best_text;
        for (const QString& line : result.split('\n')) {
            if (!line.startsWith(QStringLiteral("hit "))) continue;
            int p = -1, l = -1, co = -1;
            float x = 0, y = 0, w = 0, h = 0;
            QString txt;
            for (const QString& part : line.mid(4).split(' ')) {
                int eq = part.indexOf('=');
                if (eq < 0) continue;
                QString k = part.left(eq), v = part.mid(eq + 1);
                bool ok;
                if (k == "page") p = v.toInt(&ok);
                else if (k == "line") l = v.toInt(&ok);
                else if (k == "x") x = v.toFloat(&ok);
                else if (k == "y") y = v.toFloat(&ok);
                else if (k == "w") w = v.toFloat(&ok);
                else if (k == "h") h = v.toFloat(&ok);
                else if (k == "text") txt = v;
            }
            if (p < 0) continue;
            // First hit at or after cursor
            if (best_page < 0 || (p > find_cursor_page_) ||
                (p == find_cursor_page_ && l >= find_cursor_line_)) {
                if (best_page < 0) {
                    best_page = p; best_line = l;
                    bx = x; by = y; bw = w; bh = h;
                    best_text = txt;
                }
                break;
            }
        }
        // Wrap: if no hit found after cursor, wrap to first hit
        if (best_page < 0) {
            for (const QString& line : result.split('\n')) {
                if (!line.startsWith(QStringLiteral("hit "))) continue;
                int p = -1, l = -1;
                float x = 0, y = 0, w = 0, h = 0;
                QString txt;
                for (const QString& part : line.mid(4).split(' ')) {
                    int eq = part.indexOf('=');
                    if (eq < 0) continue;
                    QString k = part.left(eq), v = part.mid(eq + 1);
                    bool ok;
                    if (k == "page") p = v.toInt(&ok);
                    else if (k == "line") l = v.toInt(&ok);
                    else if (k == "x") x = v.toFloat(&ok);
                    else if (k == "y") y = v.toFloat(&ok);
                    else if (k == "w") w = v.toFloat(&ok);
                    else if (k == "h") h = v.toFloat(&ok);
                    else if (k == "text") txt = v;
                }
                if (p >= 0) {
                    best_page = p; best_line = l;
                    bx = x; by = y; bw = w; bh = h;
                    best_text = txt;
                    break;
                }
            }
        }
        if (best_page >= 0) {
            goToPage(best_page);
            find_cursor_page_ = best_page;
            find_cursor_line_ = best_line;
            find_current_index_ = (find_current_index_ + 1) % qMax(1, find_total_matches_);
            // Highlight match at actual geometry
            float scale_px = scale_ * 72.f;
            canvas_->setSelectionOverlay(QRectF(
                bx * scale_px, by * scale_px - scroll_y_,
                bw * scale_px, bh * scale_px));
            if (search_) {
                search_->setCurrentMatch(find_current_index_, find_total_matches_);
            }
            statusBar()->showMessage(
                QStringLiteral("%1 of %2 — %3")
                    .arg(find_current_index_ + 1)
                    .arg(find_total_matches_)
                    .arg(best_text), 3000);
        } else {
            statusBar()->showMessage(QStringLiteral("No matches found"), 3000);
        }
    } catch (const std::exception& e) {
        QMessageBox::information(this, QStringLiteral("Find"), QString::fromUtf8(e.what()));
    }
}

void MainWindow::findPrevious() {
    if (last_find_query_.isEmpty()) return;
    // Move cursor backward
    find_cursor_line_ = qMax(0, find_cursor_line_ - 1);
    find_cursor_char_ = 0;
    // For backward search, we do a forward search from 0 and pick the last hit before cursor
    try {
        QString result = QString::fromStdString(find_text(last_find_query_.toStdString()));
        int best_page = -1, best_line = -1;
        float bx = 0, by = 0, bw = 0, bh = 0;
        QString best_text;
        for (const QString& line : result.split('\n')) {
            if (!line.startsWith(QStringLiteral("hit "))) continue;
            int p = -1, l = -1;
            float x = 0, y = 0, w = 0, h = 0;
            QString txt;
            for (const QString& part : line.mid(4).split(' ')) {
                int eq = part.indexOf('=');
                if (eq < 0) continue;
                QString k = part.left(eq), v = part.mid(eq + 1);
                bool ok;
                if (k == "page") p = v.toInt(&ok);
                else if (k == "line") l = v.toInt(&ok);
                else if (k == "x") x = v.toFloat(&ok);
                else if (k == "y") y = v.toFloat(&ok);
                else if (k == "w") w = v.toFloat(&ok);
                else if (k == "h") h = v.toFloat(&ok);
                else if (k == "text") txt = v;
            }
            if (p < 0) continue;
            // Take hits before cursor
            if (p < find_cursor_page_ || (p == find_cursor_page_ && l < find_cursor_line_)) {
                best_page = p; best_line = l;
                bx = x; by = y; bw = w; bh = h;
                best_text = txt;
            }
        }
        // Wrap: if nothing before cursor, take the last hit
        if (best_page < 0) {
            for (const QString& line : result.split('\n')) {
                if (!line.startsWith(QStringLiteral("hit "))) continue;
                int p = -1, l = -1;
                float x = 0, y = 0, w = 0, h = 0;
                QString txt;
                for (const QString& part : line.mid(4).split(' ')) {
                    int eq = part.indexOf('=');
                    if (eq < 0) continue;
                    QString k = part.left(eq), v = part.mid(eq + 1);
                    bool ok;
                    if (k == "page") p = v.toInt(&ok);
                    else if (k == "line") l = v.toInt(&ok);
                    else if (k == "x") x = v.toFloat(&ok);
                    else if (k == "y") y = v.toFloat(&ok);
                    else if (k == "w") w = v.toFloat(&ok);
                    else if (k == "h") h = v.toFloat(&ok);
                    else if (k == "text") txt = v;
                }
                if (p >= 0) {
                    best_page = p; best_line = l;
                    bx = x; by = y; bw = w; bh = h;
                    best_text = txt;
                }
            }
        }
        if (best_page >= 0) {
            goToPage(best_page);
            find_cursor_page_ = best_page;
            find_cursor_line_ = best_line;
            find_current_index_ = find_current_index_ > 0
                ? find_current_index_ - 1
                : find_total_matches_ - 1;
            float scale_px = scale_ * 72.f;
            canvas_->setSelectionOverlay(QRectF(
                bx * scale_px, by * scale_px - scroll_y_,
                bw * scale_px, bh * scale_px));
            if (search_) {
                search_->setCurrentMatch(find_current_index_, find_total_matches_);
            }
            statusBar()->showMessage(
                QStringLiteral("%1 of %2 — %3")
                    .arg(find_current_index_ + 1)
                    .arg(find_total_matches_)
                    .arg(best_text), 3000);
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
    // As in CanvasWidget: every binding below is resolved through the registry.
    // [ADR-032, DS-CONV-4, PRIN-4]
    const auto& keys = chrome::shortcuts();
    if (keys.matches(QStringLiteral("document.open"), event)) {
        QString path = QFileDialog::getOpenFileName(this, QStringLiteral("Open PDF"), {},
                                                    QStringLiteral("PDF (*.pdf)"));
        if (!path.isEmpty()) openDocument(path);
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("document.find"), event)) {
        // Focus the search panel input. [FR-SRCH-1, M2]
        if (search_) {
            if (auto* dock = findChild<QDockWidget*>(QStringLiteral("searchDock"))) {
                dock->show();
            }
            if (auto* input = search_->findChild<QLineEdit*>(QStringLiteral("searchInput"))) {
                input->setFocus(Qt::ShortcutFocusReason);
                input->selectAll();
            }
        }
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("document.find_next"), event) ||
        keys.matches(QStringLiteral("document.find_previous"), event)) {
        // find next; the shifted binding walks backwards. [FR-SRCH-3, M2]
        if (keys.matches(QStringLiteral("document.find_previous"), event)) {
            findPrevious();
        } else {
            findNext();
        }
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("edit.copy"), event)) {
        copyPageText();
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("focus.canvas"), event)) {
        // Focus cycling: canvas → left panels → right panels → canvas [DS-FOCUS-5, M1]
        // Determine current focus region
        QWidget* focused = focusWidget();
        enum Region { CanvasRegion, LeftPanel, RightPanel, RegionCount };
        Region current = CanvasRegion;
        if (focused) {
            // Check which dock widget contains the focused widget
            if (auto* dock = qobject_cast<QDockWidget*>(focused->parentWidget())) {
                if (dock == findChild<QDockWidget*>(QStringLiteral("bookmarksDock"))) {
                    current = LeftPanel;
                } else if (dock == findChild<QDockWidget*>(QStringLiteral("diagnosticsDock"))
                           || dock == findChild<QDockWidget*>(QStringLiteral("formsDock"))) {
                    current = RightPanel;
                }
            }
        }
        Region next = static_cast<Region>((current + 1) % RegionCount);
        QWidget* target = nullptr;
        switch (next) {
        case CanvasRegion:
            target = canvas_;
            break;
        case LeftPanel:
            if (outline_) target = outline_->findChild<QWidget*>(QStringLiteral("outlineList"));
            if (!target) target = outline_;
            break;
        case RightPanel:
            // Prefer diagnostics, then forms
            if (diagnostics_) target = diagnostics_->findChild<QWidget*>(QStringLiteral("diagnosticsView"));
            if (!target && diagnostics_) target = diagnostics_;
            if (!target && forms_) target = forms_;
            break;
        default:
            target = canvas_;
            break;
        }
        if (target) target->setFocus(Qt::ShortcutFocusReason);
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("document.save"), event)) {
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
    if (keys.matches(QStringLiteral("edit.undo"), event)) {
        // undo. [FR-ANNOT-4, FR-FORM-6, M4]
        try {
            QString msg = QString::fromStdString(undo());
            statusBar()->showMessage(msg, 3000);
            refreshPanels();
        } catch (const std::exception& e) {
            statusBar()->showMessage(QString::fromUtf8(e.what()), 3000);
        }
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("edit.redo"), event)) {
        // redo. [FR-ANNOT-4, FR-FORM-6, M4]
        try {
            QString msg = QString::fromStdString(redo());
            statusBar()->showMessage(msg, 3000);
            refreshPanels();
        } catch (const std::exception& e) {
            statusBar()->showMessage(QString::fromUtf8(e.what()), 3000);
        }
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("annot.export"), event)) {
        exportAnnotationsXfdf();
        event->accept();
        return;
    }
    if (keys.matches(QStringLiteral("forms.calculate"), event)) {
        runFormsCalc();
        event->accept();
        return;
    }
    // Place annotation at center of page on Enter when a tool is active
    if (keys.matches(QStringLiteral("ui.activate"), event)) {
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
    // Delete key: delete last annotation on current page. [FR-ANNOT-4, M4]
    if (keys.matches(QStringLiteral("annot.delete"), event) && page_count_ > 0) {
        try {
            // Find the last annotation on the current page
            uint32_t count = annotation_count();
            if (count > 0) {
                // Delete the most recently added annotation (simple UX for now)
                // The FFI tracks annotations by ID; we iterate to find the last one
                QString result = QString::fromStdString(
                    delete_annotation(count));  // ID = count (sequential)
                statusBar()->showMessage(result, 3000);
                refreshPanels();
            }
        } catch (const std::exception& e) {
            statusBar()->showMessage(QString::fromUtf8(e.what()), 3000);
        }
        event->accept();
        return;
    }
    QMainWindow::keyPressEvent(event);
}

}  // namespace pdf_platform
