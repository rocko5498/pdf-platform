// Forms fill panel (M5). [FR-FORM, FR-JS, ADR-017, SDS §14 M5]
// Shell is view-only over bridge APIs; no document truth. [ADR-026, GR-2]
#pragma once

#include <QLineEdit>
#include <QListWidget>
#include <QString>
#include <QWidget>

class QLabel;
class QPushButton;

namespace pdf_platform {

/// Session form field list + fill/calc controls. [FR-FORM-1, FR-JS-1]
class FormsPanel : public QWidget {
    Q_OBJECT
public:
    explicit FormsPanel(QWidget* parent = nullptr);

    /// Replace list from core `list_form_fields` text.
    void setFieldsData(const QString& data);

    void clear();

signals:
    /// User asked to seed the demo form (a/b/total SUM).
    void seedDemoRequested();
    /// Apply edited value for the selected field name.
    void setFieldRequested(const QString& name, const QString& value);
    /// Run forms JS subset + appearance regeneration.
    void runCalcRequested();
    /// Toggle forms JS kill switch.
    void jsEnabledRequested(bool enabled);

private:
    void onSelectionChanged();
    void onApply();

    QListWidget* list_ = nullptr;
    QLineEdit* name_edit_ = nullptr;
    QLineEdit* value_edit_ = nullptr;
    QLabel* status_ = nullptr;
    QPushButton* apply_btn_ = nullptr;
    QPushButton* calc_btn_ = nullptr;
    QPushButton* seed_btn_ = nullptr;
    QPushButton* js_toggle_ = nullptr;
    bool js_enabled_ = true;
};

}  // namespace pdf_platform
