// [FR-BOOK, NFR-A11Y, DS-A11Y-SR-1]
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
        if (item) {
            emit entryActivated(list_->row(item));
        }
    });
}

void OutlinePanel::setOutlineData(const QString& data, int entry_count, int total_count) {
    list_->clear();
    if (entry_count <= 0) {
        list_->addItem(QStringLiteral("(no bookmarks)"));
        return;
    }
    // data is currently "entries=N" or richer JSON later — show summary + lines
    list_->addItem(QStringLiteral("%1 entries (%2 total)").arg(entry_count).arg(total_count));
    if (!data.isEmpty()) {
        for (const QString& line : data.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
            list_->addItem(line);
        }
    }
}

void OutlinePanel::clear() {
    list_->clear();
}

}  // namespace pdf_platform
