// [FR-BOOK, NFR-A11Y, DS-A11Y-SR-1, M1 exit: bookmark navigation]
#include "outline_panel.h"

#include <QVBoxLayout>
#include <QLabel>

namespace pdf_platform {

OutlinePanel::OutlinePanel(QWidget* parent) : QWidget(parent) {
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(4, 4, 4, 4);

    auto* title = new QLabel(QStringLiteral("Bookmarks"), this);
    title->setObjectName(QStringLiteral("outlineTitle"));
    layout->addWidget(title);

    list_ = new QListWidget(this);
    list_->setObjectName(QStringLiteral("outlineList"));
    list_->setAccessibleName(QStringLiteral("Document outline"));
    list_->setAccessibleDescription(
        QStringLiteral("Bookmarks and outline entries. Activate to navigate."));
    layout->addWidget(list_);

    connect(list_, &QListWidget::itemActivated, this, [this](QListWidgetItem* item) {
        if (!item) return;
        int row = list_->row(item);
        if (row >= 0 && row < int(dests_.size())) {
            const auto& d = dests_[row];
            emit entryActivated(d.page, d.y);
        }
    });
}

void OutlinePanel::setOutlineData(const QString& data, int entry_count, int total_count) {
    list_->clear();
    dests_.clear();
    if (entry_count <= 0) {
        list_->addItem(QStringLiteral("(no bookmarks)"));
        return;
    }
    list_->addItem(QStringLiteral("%1 entries (%2 total)").arg(entry_count).arg(total_count));
    dests_.push_back({0, 0.f, 0});  // placeholder for the summary line

    if (!data.isEmpty()) {
        for (const QString& line : data.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
            if (line.startsWith(QLatin1String("entries="))) continue;

            // Format: "[depth_pipes]page|y|title"
            // depth_pipes is a series of '|' characters indicating nesting level
            QString trimmed = line.trimmed();
            if (trimmed.isEmpty()) continue;

            int depth = 0;
            int cursor = 0;
            while (cursor < trimmed.size() && trimmed[cursor] == QLatin1Char('|')) {
                depth++;
                cursor++;
            }

            // Parse "page|y|title"
            QStringList parts = trimmed.mid(cursor).split(QLatin1Char('|'));
            if (parts.size() < 3) continue;

            bool ok = false;
            int page = parts[0].toInt(&ok);
            if (!ok) continue;

            float y = parts[1].toFloat(&ok);
            if (!ok) y = 0.f;

            // Reconstruct title: unescape \p → |, \n → newline
            QString title = parts.mid(2).join(QLatin1Char('|'));
            title.replace(QLatin1String("\\p"), QLatin1String("|"));
            title.replace(QLatin1String("\\n"), QLatin1String("\n"));

            // Indent by depth for visual hierarchy
            QString display = QString(QLatin1String("    ")).repeated(depth) + title;
            list_->addItem(display);
            dests_.push_back({page, y, depth});
        }
    }
}

void OutlinePanel::clear() {
    list_->clear();
    dests_.clear();
}

}  // namespace pdf_platform
