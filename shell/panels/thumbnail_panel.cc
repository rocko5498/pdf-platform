// [FR-THUMB-1, FR-NAV-4, NFR-A11Y, DS-A11Y-SR-1]
#include "thumbnail_panel.h"

#include <QIcon>
#include <QLabel>
#include <QPainter>
#include <QPixmap>
#include <QScrollBar>
#include <QTimer>
#include <QVBoxLayout>

#include <algorithm>

namespace pdf_platform {

namespace {

/// What an entry shows until its pixels arrive: a pale page with its number, so
/// the list is usable at once and the proportions do not jump later.
QPixmap placeholder(const QSize& size, int page) {
    QPixmap pixmap(size);
    pixmap.fill(QColor(235, 235, 235));
    QPainter painter(&pixmap);
    painter.setPen(QColor(150, 150, 150));
    painter.drawRect(pixmap.rect().adjusted(0, 0, -1, -1));
    painter.drawText(pixmap.rect(), Qt::AlignCenter, QString::number(page + 1));
    return pixmap;
}

}  // namespace

ThumbnailPanel::ThumbnailPanel(QWidget* parent) : QWidget(parent) {
    auto* layout = new QVBoxLayout(this);
    layout->setContentsMargins(4, 4, 4, 4);

    auto* title = new QLabel(QStringLiteral("Pages"), this);
    title->setObjectName(QStringLiteral("thumbnailTitle"));
    layout->addWidget(title);

    list_ = new QListWidget(this);
    list_->setObjectName(QStringLiteral("thumbnailList"));
    list_->setAccessibleName(QStringLiteral("Page thumbnails"));
    list_->setAccessibleDescription(
        QStringLiteral("One entry per page. Activate an entry to go to that page."));
    list_->setViewMode(QListView::IconMode);
    list_->setFlow(QListView::LeftToRight);
    list_->setWrapping(true);
    list_->setResizeMode(QListView::Adjust);
    list_->setMovement(QListView::Static);
    list_->setUniformItemSizes(true);
    list_->setIconSize(icon_size_);
    list_->setSpacing(6);
    layout->addWidget(list_);

    // A click and the keyboard both navigate. [UX-KEY, NFR-A11Y]
    connect(list_, &QListWidget::itemClicked, this, [this](QListWidgetItem* item) {
        if (item) emit pageActivated(list_->row(item));
    });
    connect(list_, &QListWidget::itemActivated, this, [this](QListWidgetItem* item) {
        if (item) emit pageActivated(list_->row(item));
    });
    connect(list_->verticalScrollBar(), &QScrollBar::valueChanged, this,
            [this]() { scheduleRequest(); });
}

void ThumbnailPanel::setDocument(int page_count, const QSizeF& page_size_points) {
    clear();
    if (page_count <= 0) return;

    // Keep the page's proportions inside a fixed width.
    const double aspect = (page_size_points.width() > 0.0 && page_size_points.height() > 0.0)
                              ? page_size_points.height() / page_size_points.width()
                              : 1.3;
    icon_size_ = QSize(96, std::clamp(int(96.0 * aspect), 48, 256));
    list_->setIconSize(icon_size_);

    list_->setUpdatesEnabled(false);
    for (int page = 0; page < page_count; ++page) {
        auto* item = new QListWidgetItem(QIcon(placeholder(icon_size_, page)),
                                         QStringLiteral("Page %1").arg(page + 1));
        item->setToolTip(QStringLiteral("Go to page %1").arg(page + 1));
        item->setTextAlignment(Qt::AlignHCenter);
        list_->addItem(item);
    }
    list_->setUpdatesEnabled(true);
    scheduleRequest();
}

void ThumbnailPanel::clear() {
    list_->clear();
    have_.clear();
    requested_.clear();
}

void ThumbnailPanel::setCurrentPage(int page) {
    if (page < 0 || page >= list_->count()) return;
    // Reporting where the reader already is must not navigate.
    const QSignalBlocker blocker(list_);
    list_->setCurrentRow(page);
    list_->scrollToItem(list_->item(page));
    scheduleRequest();
}

void ThumbnailPanel::setThumbnail(int page, const QImage& image) {
    if (page < 0 || page >= list_->count() || image.isNull()) return;
    const QImage scaled = image.scaled(icon_size_, Qt::KeepAspectRatio, Qt::SmoothTransformation);
    list_->item(page)->setIcon(QIcon(QPixmap::fromImage(scaled)));
    have_.insert(page);
}

void ThumbnailPanel::showEvent(QShowEvent* event) {
    QWidget::showEvent(event);
    scheduleRequest();
}

void ThumbnailPanel::resizeEvent(QResizeEvent* event) {
    QWidget::resizeEvent(event);
    scheduleRequest();
}

void ThumbnailPanel::scheduleRequest() {
    // Coalesce scroll and resize bursts, and let the event loop breathe between
    // asking and rendering. [RQ-4, NFR-PERF-1]
    if (request_pending_) return;
    request_pending_ = true;
    QTimer::singleShot(0, this, [this]() {
        request_pending_ = false;
        requestVisible();
    });
}

void ThumbnailPanel::requestVisible() {
    if (list_->count() == 0 || !list_->isVisible()) return;
    const QRect view = list_->viewport()->rect();
    // The first and last entries in view, plus one screenful ahead so scrolling
    // does not meet blank entries.
    QModelIndex first = list_->indexAt(view.topLeft() + QPoint(4, 4));
    QModelIndex last = list_->indexAt(view.bottomRight() - QPoint(4, 4));
    int from = first.isValid() ? first.row() : 0;
    int to = last.isValid() ? last.row() : std::min(list_->count() - 1, from + 24);
    const int span = std::max(1, to - from + 1);
    to = std::min(list_->count() - 1, to + span);
    for (int page = from; page <= to; ++page) {
        if (have_.contains(page) || requested_.contains(page)) continue;
        requested_.insert(page);
        emit thumbnailRequested(page);
    }
}

}  // namespace pdf_platform
