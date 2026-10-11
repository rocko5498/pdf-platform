#include "view_history.h"

namespace pdf_platform {

void ViewHistory::visit(const ViewState& state) {
    if (entries_.empty()) {
        entries_.push_back(state);
        cursor_ = 0;
        return;
    }
    if (entries_[cursor_].page == state.page) {
        entries_[cursor_].scale = state.scale;
        return;
    }
    entries_.resize(cursor_ + 1);
    entries_.push_back(state);
    if (entries_.size() > kMaxEntries) entries_.erase(entries_.begin());
    cursor_ = entries_.size() - 1;
}

std::optional<ViewState> ViewHistory::back() {
    if (!canGoBack()) return std::nullopt;
    return entries_[--cursor_];
}

std::optional<ViewState> ViewHistory::forward() {
    if (!canGoForward()) return std::nullopt;
    return entries_[++cursor_];
}

void ViewHistory::clear() {
    entries_.clear();
    cursor_ = 0;
}

}  // namespace pdf_platform
