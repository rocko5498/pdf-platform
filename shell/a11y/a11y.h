// Accessibility surface: QAccessible mapping for chrome and canvas.
// [NFR-A11Y, DS-A11Y-*, ADR-003, ADR-026, SDS §2.1]
//
// M1: chrome + canvas roles/names/focus; document structure tree is M2+.

#pragma once

#include <QAccessible>
#include <QAccessibleWidget>
#include <QString>

class QWidget;

namespace pdf_platform {

/// Install factory hooks and ensure QAccessible is active. [NFR-A11Y-1]
void installAccessibility();

/// Apply standard accessible names/roles to main chrome widgets. [DS-A11Y-SR-1]
void configureMainWindowAccessibility(QWidget* main_window, QWidget* canvas);

/// Accessible interface for the document canvas widget. [DS-A11Y-CANVAS-1]
///
/// M1 exposes the canvas as a document pane with page status text.
/// Tagged PDF structure navigation is deferred to M2 text model work.
class CanvasAccessible : public QAccessibleWidget {
public:
    explicit CanvasAccessible(QWidget* widget);

    QString text(QAccessible::Text t) const override;
    QAccessible::Role role() const override;
    QAccessible::State state() const override;

    /// Update the status string announced to AT (e.g. "Page 1 of 12").
    void setDocumentStatus(const QString& status);

private:
    QString document_status_;
};

}  // namespace pdf_platform
