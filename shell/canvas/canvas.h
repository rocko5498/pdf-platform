// Canvas + main window: multi-page, zoom, find, annotations, live panels.
// [ADR-007, SDS Â§6, FR-VIEW, FR-SRCH, FR-ANNOT, FR-BOOK, FR-DIAG, M1â€“M4]

#pragma once

#include <QImage>
#include <QKeyEvent>
#include <QMainWindow>
#include <QString>
#include <QWidget>
#include <optional>

#if defined(PDF_PLATFORM_USE_OPENGL) && PDF_PLATFORM_USE_OPENGL
#  include <QOpenGLFunctions>
#  include <QOpenGLWidget>
#  define PDF_CANVAS_BASE QOpenGLWidget
#else
#  define PDF_CANVAS_BASE QWidget
#endif

namespace pdf_platform {

class OutlinePanel;
class DiagnosticsPanel;
class AnnotationToolBar;
class FormsPanel;
class SearchPanel;

class CanvasWidget : public PDF_CANVAS_BASE
#if defined(PDF_PLATFORM_USE_OPENGL) && PDF_PLATFORM_USE_OPENGL
    ,
                     protected QOpenGLFunctions
#endif
{
    Q_OBJECT
public:
    explicit CanvasWidget(QWidget* parent = nullptr);
    ~CanvasWidget() override;

    void setTile(const uint8_t* buffer, uint32_t width, uint32_t height);
    void clear();
    void setAccessibleStatus(const QString& status);
    void setSelectionOverlay(const QRectF& rect);
    void clearSelectionOverlay();
    /// Set annotation overlays for the current page. [FR-ANNOT, M4]
    void setAnnotationOverlays(const std::vector<std::pair<QRectF, QColor>>& overlays);
    bool usingGpu() const { return using_gpu_; }

signals:
    void pageStepRequested(int delta);
    void zoomStepRequested(int delta);
    void scrollDeltaRequested(int dy);
    /// Canvas clicked at PDF coordinates. [FR-ANNOT, M4]
    void canvasClicked(float pdf_x, float pdf_y);

protected:
    void paintEvent(QPaintEvent* event) override;
    void keyPressEvent(QKeyEvent* event) override;
    void wheelEvent(QWheelEvent* event) override;
    void focusInEvent(QFocusEvent* event) override;
    void mousePressEvent(QMouseEvent* event) override;

#if defined(PDF_PLATFORM_USE_OPENGL) && PDF_PLATFORM_USE_OPENGL
    void initializeGL() override;
    void paintGL() override;
    void resizeGL(int w, int h) override;
#endif

private:
    void paintSoftware(QPainter& p);
    void uploadTextureIfNeeded();

    QImage tile_image_;
    bool has_tile_ = false;
    bool using_gpu_ = false;
    bool gl_ready_ = false;
    bool texture_dirty_ = false;
    QString accessible_status_;
    std::optional<QRectF> selection_;
    std::vector<std::pair<QRectF, QColor>> annot_overlays_;
#if defined(PDF_PLATFORM_USE_OPENGL) && PDF_PLATFORM_USE_OPENGL
    GLuint texture_id_ = 0;
#endif
};

class MainWindow : public QMainWindow {
    Q_OBJECT
public:
    explicit MainWindow(QWidget* parent = nullptr);
    ~MainWindow() override;

    bool openDocument(const QString& path);
    CanvasWidget* canvasWidget() const { return canvas_; }

public slots:
    void goToPage(int page);
    void zoomBy(int steps);
    void findNext();
    void findPrevious();
    void performSearch(const QString& query);
    void copyPageText();
    void exportAnnotationsXfdf();
    void refreshFormsPanel();
    void seedFormDemo();
    void applyFormField(const QString& name, const QString& value);
    void runFormsCalc();
    void setFormsJsEnabled(bool enabled);

protected:
    void keyPressEvent(QKeyEvent* event) override;

private:
    void setupChrome();
    bool openDocumentWithPassword(const QString& path, const QString& password, bool* needs_password = nullptr);
    bool renderCurrentPage();
    bool renderVisibleTiles();
    void refreshPanels();
    void announceDocumentStatus();
    void clearDocumentUi();
    float docHeightPx() const;
    void clampScroll();
    void mapShmem(qintptr handle);
    void onAnnotationTool(int tool);

    CanvasWidget* canvas_ = nullptr;
    OutlinePanel* outline_ = nullptr;
    DiagnosticsPanel* diagnostics_ = nullptr;
    AnnotationToolBar* annot_tools_ = nullptr;
    FormsPanel* forms_ = nullptr;
    SearchPanel* search_ = nullptr;

    void* shmem_mapping_ = nullptr;
    void* shmem_section_ = nullptr;  // CreateFileMapping handle (Windows); not the file handle
    uint32_t page_count_ = 0;
    uint32_t current_page_ = 0;
    float scale_ = 1.0f;
    float page_width_ = 595.f;
    float page_height_ = 842.f;
    float scroll_y_ = 0.f;
    uint64_t generation_ = 1;
    static constexpr int kTile = 256;
    QString path_;
    QString last_find_query_;
    int find_cursor_page_ = 0;
    int find_cursor_line_ = 0;
    int find_cursor_char_ = 0;
    int find_current_index_ = -1;
    int find_total_matches_ = 0;
    int annot_tool_ = 0;  // AnnotationTool as int
};

}  // namespace pdf_platform
