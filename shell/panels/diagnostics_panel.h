// Leniency / repair diagnostics panel. [FR-DIAG, PRIN-6, ADR-020, M1]
#pragma once

#include <QPlainTextEdit>
#include <QStringList>
#include <QWidget>

namespace pdf_platform {

/// Shows honest repair/leniency ledger and document flags. [PRIN-6]
class DiagnosticsPanel : public QWidget {
    Q_OBJECT
public:
    explicit DiagnosticsPanel(QWidget* parent = nullptr);

    void setReport(const QString& summary, const QStringList& leniency_events);
    void clear();

private:
    QPlainTextEdit* view_;
};

}  // namespace pdf_platform
