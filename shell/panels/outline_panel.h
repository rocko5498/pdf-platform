// Outline (bookmarks) navigation panel. [FR-BOOK, SDS §2.1, M1]
#pragma once

#include <QListWidget>
#include <QString>
#include <QWidget>

namespace pdf_platform {

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
    /// User activated an outline entry (0-based index in the list).
    void entryActivated(int index);

private:
    QListWidget* list_;
};

}  // namespace pdf_platform
