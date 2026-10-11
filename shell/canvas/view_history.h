// Previous View / Next View. [FR-NAV-3, DS-HIST-1..4]
//
// View-only state: a bounded list of (page, zoom) the reader has been at, with
// a cursor. Nothing here touches the document. [RQ-1, GR-7]
#pragma once

#include <optional>
#include <vector>

namespace pdf_platform {

struct ViewState {
    int page = 0;
    float scale = 1.0f;
    bool operator==(const ViewState& o) const { return page == o.page && scale == o.scale; }
};

class ViewHistory {
public:
    /// Where the reader is now. A visit to a *different* page after going back
    /// discards the forward entries, as in a browser. The same page again only
    /// updates the zoom of the current entry, so zooming does not fill the list.
    void visit(const ViewState& state);

    /// Step back / forward. Empty when there is nowhere to go.
    std::optional<ViewState> back();
    std::optional<ViewState> forward();

    bool canGoBack() const { return cursor_ > 0; }
    bool canGoForward() const { return !entries_.empty() && cursor_ + 1 < entries_.size(); }
    void clear();
    size_t size() const { return entries_.size(); }

private:
    // Bound so a long session cannot grow it without limit. [GR-7]
    static constexpr size_t kMaxEntries = 256;
    std::vector<ViewState> entries_;
    size_t cursor_ = 0;
};

}  // namespace pdf_platform
