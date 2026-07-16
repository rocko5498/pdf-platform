// [FR-ANNOT, UX-KEY-1, DS-A11Y-SR-1]
#include "annotation_tools.h"

#include <QAction>
#include <QActionGroup>

namespace pdf_platform {

AnnotationToolBar::AnnotationToolBar(QWidget* parent) : QToolBar(parent) {
    setObjectName(QStringLiteral("annotationToolBar"));
    setAccessibleName(QStringLiteral("Annotation tools"));
    setMovable(false);

    auto* group = new QActionGroup(this);
    group->setExclusive(true);

    auto addTool = [this, group](const QString& name, AnnotationTool tool) {
        auto* act = addAction(name);
        act->setCheckable(true);
        act->setData(static_cast<int>(tool));
        group->addAction(act);
        connect(act, &QAction::triggered, this, [this, tool]() { select(tool); });
        return act;
    };

    addTool(QStringLiteral("Select"), AnnotationTool::None)->setChecked(true);
    addTool(QStringLiteral("Highlight"), AnnotationTool::Highlight);
    addTool(QStringLiteral("Underline"), AnnotationTool::Underline);
    addTool(QStringLiteral("Note"), AnnotationTool::StickyNote);
    addTool(QStringLiteral("Text"), AnnotationTool::FreeText);
    addTool(QStringLiteral("Ink"), AnnotationTool::Ink);
    addTool(QStringLiteral("Rect"), AnnotationTool::Rectangle);
    addTool(QStringLiteral("Stamp"), AnnotationTool::Stamp);
}

void AnnotationToolBar::select(AnnotationTool tool) {
    current_ = tool;
    emit toolChanged(tool);
}

}  // namespace pdf_platform
