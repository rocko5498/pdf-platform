// Outline (bookmarks) navigation panel. [FR-BOOK, SDS §2.1, M1]
#pragma once

#include <QListWidget>
#include <QString>
#include <QWidget>
#include <vector>

namespace pdf_platform {

/// Parsed outline entry with destination data for navigation.
struct OutlineDest {
    int page;       /// 0-based page index
    float y;        /// Vertical offset in PDF points
    int depth;      /// Nesting depth (0 = top-level)
};

/// Read-only outline / bookmarks list. Populated from coordinator structure events.
class OutlinePanel : public QWidget {
    Q_OBJECT
public:
    explicit OutlinePanel(QWidget* parent = nullptr);

    /// Replace list contents with serialized outline data from the core.
    void setOutlineData(const QString& data, int entry_count, int total_count);

    /// Clear when no document is open.
    void clear();

signals:
    /// User activated an outline entry with its navigation destination.
    void entryActivated(int page, float y);

private:
    QListWidget* list_;
    std::vector<OutlineDest> dests_;
};

}  // namespace pdf_platform
