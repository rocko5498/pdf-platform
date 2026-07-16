// Accessibility surface implementation. [NFR-A11Y, DS-A11Y-SR-1, ADR-026]
#include "a11y.h"

#include <QAccessible>
#include <QWidget>

namespace pdf_platform {

namespace {

QAccessibleInterface* accessibleFactory(const QString& classname, QObject* object) {
    // Only claim our canvas class name when registered via setProperty.
    if (classname == QLatin1String("pdf_platform::CanvasWidget") ||
        classname == QLatin1String("CanvasWidget")) {
        if (auto* w = qobject_cast<QWidget*>(object)) {
            return new CanvasAccessible(w);
        }
    }
    return nullptr;
}

}  // namespace

void installAccessibility() {
    static bool installed = false;
    if (installed) {
        return;
    }
    QAccessible::installFactory(accessibleFactory);
    installed = true;
}

void configureMainWindowAccessibility(QWidget* main_window, QWidget* canvas) {
    if (main_window) {
        main_window->setAccessibleName(QStringLiteral("PDF Platform"));
        main_window->setAccessibleDescription(
            QStringLiteral("Open-source professional PDF platform"));
        main_window->setWindowRole(QStringLiteral("MainWindow"));
    }
    if (canvas) {
        canvas->setAccessibleName(QStringLiteral("Document canvas"));
        canvas->setAccessibleDescription(
            QStringLiteral("Rendered PDF page. Use arrow keys to scroll when focus is here."));
        canvas->setFocusPolicy(Qt::StrongFocus);
        // Tag for accessibleFactory classname matching.
        canvas->setProperty("accessibleClassName", QStringLiteral("CanvasWidget"));
        canvas->setObjectName(QStringLiteral("documentCanvas"));
    }
}

CanvasAccessible::CanvasAccessible(QWidget* widget)
    : QAccessibleWidget(widget, QAccessible::Document),
      document_status_(QStringLiteral("No document open")) {}

QString CanvasAccessible::text(QAccessible::Text t) const {
    switch (t) {
    case QAccessible::Name:
        return QStringLiteral("Document canvas");
    case QAccessible::Description:
    case QAccessible::Value:
        return document_status_;
    default:
        return QAccessibleWidget::text(t);
    }
}

QAccessible::Role CanvasAccessible::role() const {
    return QAccessible::Document;
}

QAccessible::State CanvasAccessible::state() const {
    QAccessible::State s = QAccessibleWidget::state();
    s.focusable = true;
    s.selectable = false;
    s.multiLine = true;
    return s;
}

void CanvasAccessible::setDocumentStatus(const QString& status) {
    document_status_ = status;
    if (QAccessible::isActive()) {
        QAccessibleEvent ev(object(), QAccessible::NameChanged);
        QAccessible::updateAccessibility(&ev);
        QAccessibleEvent val(object(), QAccessible::ValueChanged);
        QAccessible::updateAccessibility(&val);
    }
}

}  // namespace pdf_platform
