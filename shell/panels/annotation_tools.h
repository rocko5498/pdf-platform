// Annotation tool strip (M4 chrome). [FR-ANNOT, DS-PHIL-7]
// Tools emit intent signals; the shell never mutates document truth (GR-2).
#pragma once

#include <QToolBar>
#include <QString>

namespace pdf_platform {

/// Annotation tool identifiers matching FR-ANNOT-1 types.
enum class AnnotationTool {
    None,
    Highlight,
    Underline,
    StickyNote,
    FreeText,
    Ink,
    Rectangle,
    Stamp,
};

/// Toolbar for choosing the active annotation tool. [FR-ANNOT-5 defaults]
class AnnotationToolBar : public QToolBar {
    Q_OBJECT
public:
    explicit AnnotationToolBar(QWidget* parent = nullptr);

    AnnotationTool currentTool() const { return current_; }

signals:
    void toolChanged(pdf_platform::AnnotationTool tool);

private:
    void select(AnnotationTool tool);
    AnnotationTool current_ = AnnotationTool::None;
};

}  // namespace pdf_platform
