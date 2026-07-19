// [FR-FORM, FR-JS, UX-KEY-1, DS-A11Y-SR-1, M5]
#include "forms_panel.h"

#include <QHBoxLayout>
#include <QLabel>
#include <QPushButton>
#include <QVBoxLayout>

namespace pdf_platform {

FormsPanel::FormsPanel(QWidget* parent) : QWidget(parent) {
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(4, 4, 4, 4);

    auto* title = new QLabel(QStringLiteral("Forms"), this);
    title->setObjectName(QStringLiteral("formsTitle"));
    layout->addWidget(title);

    status_ = new QLabel(QStringLiteral("No form session"), this);
    status_->setObjectName(QStringLiteral("formsStatus"));
    status_->setWordWrap(true);
    status_->setAccessibleName(QStringLiteral("Forms status"));
    layout->addWidget(status_);

    list_ = new QListWidget(this);
    list_->setObjectName(QStringLiteral("formsFieldList"));
    list_->setAccessibleName(QStringLiteral("Form fields"));
    list_->setAccessibleDescription(
        QStringLiteral("Session form fields in tab order. Select a field to edit its value."));
    layout->addWidget(list_);

    auto* edit_row = new QHBoxLayout();
    name_edit_ = new QLineEdit(this);
    name_edit_->setObjectName(QStringLiteral("formsFieldName"));
    name_edit_->setPlaceholderText(QStringLiteral("field name"));
    name_edit_->setAccessibleName(QStringLiteral("Field name"));
    value_edit_ = new QLineEdit(this);
    value_edit_->setObjectName(QStringLiteral("formsFieldValue"));
    value_edit_->setPlaceholderText(QStringLiteral("value"));
    value_edit_->setAccessibleName(QStringLiteral("Field value"));
    edit_row->addWidget(name_edit_, 1);
    edit_row->addWidget(value_edit_, 2);
    layout->addLayout(edit_row);

    auto* btn_row = new QHBoxLayout();
    apply_btn_ = new QPushButton(QStringLiteral("Apply"), this);
    apply_btn_->setObjectName(QStringLiteral("formsApply"));
    apply_btn_->setAccessibleName(QStringLiteral("Apply field value"));
    calc_btn_ = new QPushButton(QStringLiteral("Calc"), this);
    calc_btn_->setObjectName(QStringLiteral("formsCalc"));
    calc_btn_->setAccessibleName(QStringLiteral("Run form calculations"));
    seed_btn_ = new QPushButton(QStringLiteral("Seed demo"), this);
    seed_btn_->setObjectName(QStringLiteral("formsSeed"));
    seed_btn_->setAccessibleName(QStringLiteral("Seed demo form fields"));
    js_toggle_ = new QPushButton(QStringLiteral("JS: on"), this);
    js_toggle_->setObjectName(QStringLiteral("formsJsToggle"));
    js_toggle_->setCheckable(true);
    js_toggle_->setChecked(true);
    js_toggle_->setAccessibleName(QStringLiteral("Forms JavaScript kill switch"));
    validate_btn_ = new QPushButton(QStringLiteral("Validate"), this);
    validate_btn_->setObjectName(QStringLiteral("formsValidate"));
    validate_btn_->setAccessibleName(QStringLiteral("Validate form fields"));
    flatten_btn_ = new QPushButton(QStringLiteral("Flatten"), this);
    flatten_btn_->setObjectName(QStringLiteral("formsFlatten"));
    flatten_btn_->setAccessibleName(QStringLiteral("Flatten form fields to page content"));
    btn_row->addWidget(apply_btn_);
    btn_row->addWidget(calc_btn_);
    btn_row->addWidget(seed_btn_);
    btn_row->addWidget(js_toggle_);
    btn_row->addWidget(validate_btn_);
    btn_row->addWidget(flatten_btn_);
    layout->addLayout(btn_row);

    connect(list_, &QListWidget::itemSelectionChanged, this, &FormsPanel::onSelectionChanged);
    connect(apply_btn_, &QPushButton::clicked, this, &FormsPanel::onApply);
    connect(value_edit_, &QLineEdit::returnPressed, this, &FormsPanel::onApply);
    connect(calc_btn_, &QPushButton::clicked, this, [this]() { emit runCalcRequested(); });
    connect(seed_btn_, &QPushButton::clicked, this, [this]() { emit seedDemoRequested(); });
    connect(js_toggle_, &QPushButton::toggled, this, [this](bool on) {
        js_enabled_ = on;
        js_toggle_->setText(on ? QStringLiteral("JS: on") : QStringLiteral("JS: off"));
        emit jsEnabledRequested(on);
    });
    connect(validate_btn_, &QPushButton::clicked, this, [this]() { emit validateRequested(); });
    connect(flatten_btn_, &QPushButton::clicked, this, [this]() { emit flattenRequested(); });
}

void FormsPanel::setFieldsData(const QString& data) {
    list_->clear();
    QStringList lines = data.split(QLatin1Char('\n'), Qt::SkipEmptyParts);
    QStringList meta;
    for (const QString& line : lines) {
        if (line.startsWith(QLatin1String("count=")) || line.startsWith(QLatin1String("has_js="))
            || line.startsWith(QLatin1String("js_enabled="))
            || line.startsWith(QLatin1String("needs_ap="))
            || line.startsWith(QLatin1String("note="))) {
            meta << line;
            if (line.startsWith(QLatin1String("js_enabled="))) {
                const bool on = line.endsWith(QLatin1String("true"));
                js_enabled_ = on;
                js_toggle_->blockSignals(true);
                js_toggle_->setChecked(on);
                js_toggle_->setText(on ? QStringLiteral("JS: on") : QStringLiteral("JS: off"));
                js_toggle_->blockSignals(false);
            }
            continue;
        }
        // name\ttype\tvalue\tro/rw\tcalc/-\tap=yes/no
        const QStringList cols = line.split(QLatin1Char('\t'));
        if (cols.isEmpty()) continue;
        auto* item = new QListWidgetItem(line, list_);
        item->setData(Qt::UserRole, cols.value(0));
        item->setData(Qt::UserRole + 1, cols.value(2));
        item->setToolTip(line);
    }
    if (list_->count() == 0) {
        list_->addItem(QStringLiteral("(no session fields — Seed demo)"));
    }
    status_->setText(meta.isEmpty() ? QStringLiteral("Forms") : meta.join(QLatin1String(" · ")));
}

void FormsPanel::clear() {
    list_->clear();
    name_edit_->clear();
    value_edit_->clear();
    status_->setText(QStringLiteral("No form session"));
}

void FormsPanel::onSelectionChanged() {
    auto* item = list_->currentItem();
    if (!item) return;
    const QString name = item->data(Qt::UserRole).toString();
    if (name.isEmpty()) return;
    name_edit_->setText(name);
    value_edit_->setText(item->data(Qt::UserRole + 1).toString());
    value_edit_->setFocus(Qt::OtherFocusReason);
}

void FormsPanel::onApply() {
    const QString name = name_edit_->text().trimmed();
    if (name.isEmpty()) return;
    emit setFieldRequested(name, value_edit_->text());
}

}  // namespace pdf_platform
