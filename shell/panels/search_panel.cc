// [FR-SRCH-3, FR-SRCH-4, DS-SEARCHP-*, M2 exit: search panel]
#include "search_panel.h"

#include <QVBoxLayout>
#include <QHBoxLayout>

namespace pdf_platform {

SearchPanel::SearchPanel(QWidget* parent) : QWidget(parent) {
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(4, 4, 4, 4);
    layout->setSpacing(4);

    // Search input row
    auto* input_layout = new QHBoxLayout();
    input_ = new QLineEdit(this);
    input_->setPlaceholderText(QStringLiteral("Find in document…"));
    input_->setObjectName(QStringLiteral("searchInput"));
    input_->setAccessibleName(QStringLiteral("Search text"));
    input_layout->addWidget(input_);

    count_label_ = new QLabel(QStringLiteral(""), this);
    count_label_->setObjectName(QStringLiteral("searchCount"));
    count_label_->setAccessibleName(QStringLiteral("Match count"));
    count_label_->setMinimumWidth(60);
    input_layout->addWidget(count_label_);
    layout->addLayout(input_layout);

    // Results list
    list_ = new QListWidget(this);
    list_->setObjectName(QStringLiteral("searchResults"));
    list_->setAccessibleName(QStringLiteral("Search results"));
    list_->setAccessibleDescription(
        QStringLiteral("Search results with page number and text snippet. Activate to navigate."));
    layout->addWidget(list_);

    // Connect Enter in search box to search
    connect(input_, &QLineEdit::returnPressed, this, [this]() {
        emit searchRequested(input_->text());
    });

    // Connect result activation
    connect(list_, &QListWidget::itemActivated, this, [this](QListWidgetItem* item) {
        if (!item) return;
        int row = list_->row(item);
        // row 0 is the count header line, so subtract 1
        int idx = row - 1;
        if (idx >= 0 && idx < int(results_.size())) {
            current_index_ = idx;
            const auto& r = results_[idx];
            emit resultActivated(r.page, r.x, r.y, r.w, r.h);
            // Update count display
            count_label_->setText(
                QStringLiteral("%1 of %2").arg(current_index_ + 1).arg(int(results_.size())));
        }
    });
}

void SearchPanel::setResults(const QString& data, const QString& query) {
    list_->clear();
    results_.clear();
    current_index_ = -1;

    if (query.isEmpty()) {
        count_label_->setText(QStringLiteral(""));
        return;
    }

    // Parse hit lines: "hit page=N line=N offset=N len=N x=F y=F w=F h=F text=..."
    int hit_count = 0;
    for (const QString& line : data.split(QLatin1Char('\n'), Qt::SkipEmptyParts)) {
        if (!line.startsWith(QLatin1String("hit "))) continue;

        SearchResult r{};
        // Parse key=value pairs
        for (const QString& part : line.mid(4).split(QLatin1Char(' '), Qt::SkipEmptyParts)) {
            int eq = part.indexOf(QLatin1Char('='));
            if (eq < 0) continue;
            QString key = part.left(eq);
            QString val = part.mid(eq + 1);
            bool ok = false;
            if (key == QLatin1String("page")) r.page = val.toInt(&ok);
            else if (key == QLatin1String("line")) r.line = val.toInt(&ok);
            else if (key == QLatin1String("offset")) r.char_offset = val.toInt(&ok);
            else if (key == QLatin1String("x")) r.x = val.toFloat(&ok);
            else if (key == QLatin1String("y")) r.y = val.toFloat(&ok);
            else if (key == QLatin1String("w")) r.w = val.toFloat(&ok);
            else if (key == QLatin1String("h")) r.h = val.toFloat(&ok);
            else if (key == QLatin1String("text")) r.text = val;
        }

        if (!r.text.isEmpty()) {
            results_.push_back(r);
            hit_count++;
            // Show "Page N: matched text" in the list
            list_->addItem(QStringLiteral("Page %1: %2").arg(r.page + 1).arg(r.text));
        }
    }

    // Add count header at top
    auto* header = new QListWidgetItem(
        QStringLiteral("%1 match%2 for \"%3\"")
            .arg(hit_count)
            .arg(hit_count == 1 ? "" : "s")
            .arg(query));
    header->setFlags(header->flags() & ~Qt::ItemIsSelectable);
    list_->insertItem(0, header);

    count_label_->setText(
        hit_count > 0
            ? QStringLiteral("0 of %1").arg(hit_count)
            : QStringLiteral("No matches"));
}

void SearchPanel::setCurrentMatch(int index, int total) {
    current_index_ = index;
    count_label_->setText(
        total > 0
            ? QStringLiteral("%1 of %2").arg(index + 1).arg(total)
            : QStringLiteral("No matches"));
    // Highlight the current result in the list
    if (index >= 0 && index + 1 < list_->count()) {
        list_->setCurrentRow(index + 1);  // +1 because row 0 is the header
    }
}

void SearchPanel::clear() {
    list_->clear();
    results_.clear();
    current_index_ = -1;
    count_label_->setText(QStringLiteral(""));
}

}  // namespace pdf_platform
