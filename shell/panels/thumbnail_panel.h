// Page thumbnails for navigation. [FR-THUMB-1, FR-NAV-4, NFR-A11Y, SDS §2.1]
//
// View-only over the core (ADR-026, RQ-1): the panel owns no document state and
// never calls the bridge. It lists one entry per page and *asks* for pixels,
// only for the pages scrolled into view, so a 2,000-page document does not
// render 2,000 pages. The window answers with `setThumbnail`.
#pragma once

#include <QImage>
#include <QListWidget>
#include <QSet>
#include <QSizeF>
#include <QWidget>

namespace pdf_platform {

class ThumbnailPanel : public QWidget {
    Q_OBJECT
public:
    explicit ThumbnailPanel(QWidget* parent = nullptr);

    /// One placeholder entry per page, sized to the page's proportions.
    void setDocument(int page_count, const QSizeF& page_size_points);

    /// Remove every entry (no document open).
    void clear();

    /// Highlight the page being read. Does not navigate: the caller is the one
    /// reporting where the reader already is.
    void setCurrentPage(int page);

    /// The pixels asked for through `thumbnailRequested`.
    void setThumbnail(int page, const QImage& image);

    /// Pages whose thumbnail has been supplied.
    int thumbnailCount() const { return int(have_.size()); }
    int pageCount() const { return list_->count(); }

signals:
    /// The user chose a page.
    void pageActivated(int page);
    /// A page scrolled into view without a thumbnail yet.
    void thumbnailRequested(int page);

protected:
    void showEvent(QShowEvent* event) override;
    void resizeEvent(QResizeEvent* event) override;

private:
    void requestVisible();
    void scheduleRequest();

    QListWidget* list_ = nullptr;
    QSet<int> have_;       ///< Pages with a real thumbnail.
    QSet<int> requested_;  ///< Pages already asked for, so none is asked twice.
    bool request_pending_ = false;
    QSize icon_size_{96, 124};
};

}  // namespace pdf_platform
