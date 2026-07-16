// [FR-DIAG, PRIN-6, NFR-A11Y]
#include "diagnostics_panel.h"

#include <QVBoxLayout>
#include <QLabel>

namespace pdf_platform {

DiagnosticsPanel::DiagnosticsPanel(QWidget* parent) : QWidget(parent) {
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(4, 4, 4, 4);

    auto* title = new QLabel(QStringLiteral("Diagnostics"), this);
    layout->addWidget(title);

    view_ = new QPlainTextEdit(this);
    view_->setReadOnly(true);
    view_->setObjectName(QStringLiteral("diagnosticsView"));
    view_->setAccessibleName(QStringLiteral("Document diagnostics"));
    view_->setAccessibleDescription(
        QStringLiteral("Repair ledger, unsupported features, and document flags."));
    layout->addWidget(view_);
}

void DiagnosticsPanel::setReport(const QString& summary, const QStringList& leniency_events) {
    QString text = summary;
    if (!leniency_events.isEmpty()) {
        text += QStringLiteral("\n\nLeniency / repairs:\n");
        for (const QString& e : leniency_events) {
            text += QStringLiteral("  • ") + e + QLatin1Char('\n');
        }
    } else {
        text += QStringLiteral("\n\nNo repairs recorded.");
    }
    view_->setPlainText(text);
}

void DiagnosticsPanel::clear() {
    view_->clear();
}

}  // namespace pdf_platform
