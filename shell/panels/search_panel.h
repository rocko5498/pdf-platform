// Search results panel. [FR-SRCH-3, FR-SRCH-4, DS-SEARCHP-*, M2]
#pragma once

#include <QListWidget>
#include <QString>
#include <QWidget>
#include <QLineEdit>
#include <QLabel>
#include <vector>

namespace pdf_platform {

/// Parsed find result with navigation data.
struct SearchResult {
    int page;       /// 0-based page index
    int line;       /// Line index within the page
    int char_offset; /// Character offset within the line
    float x, y, w, h; /// Bounding rect in PDF points
    QString text;   /// Matched text snippet
};

/// Search results panel with input, result list, and navigation.
class SearchPanel : public QWidget {
    Q_OBJECT
public:
    explicit SearchPanel(QWidget* parent = nullptr);

    /// Replace results with a new search hit list.
    void setResults(const QString& data, const QString& query);

    /// Update the current match position ("N of M").
    void setCurrentMatch(int index, int total);

    /// Clear all results.
    void clear();

signals:
    /// User activated a result — navigate to page and highlight.
    void resultActivated(int page, float x, float y, float w, float h);

    /// User pressed Enter in the search box — initiate search.
    void searchRequested(const QString& query);

    /// User wants next/previous match.
    void findNext();
    void findPrevious();

private:
    QLineEdit* input_;
    QLabel* count_label_;
    QListWidget* list_;
    std::vector<SearchResult> results_;
    int current_index_ = -1;
};

}  // namespace pdf_platform
