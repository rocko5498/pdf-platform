// Previous View / Next View. [FR-NAV-3, DS-HIST-1..4, T-10]

#include "view_history.h"

#include <QTest>

using pdf_platform::ViewHistory;
using pdf_platform::ViewState;

class ViewHistoryTest : public QObject {
    Q_OBJECT
private slots:
    void back_returns_the_page_and_zoom_the_reader_left();
    void forward_undoes_back();
    void a_new_visit_after_going_back_drops_the_forward_entries();
    void zooming_on_the_same_page_does_not_add_an_entry();
    void nowhere_to_go_is_empty();
    void it_is_bounded();
};

void ViewHistoryTest::back_returns_the_page_and_zoom_the_reader_left() {
    ViewHistory h;
    h.visit({0, 1.0f});
    h.visit({4, 2.0f});
    h.visit({9, 1.5f});
    const auto prev = h.back();
    QVERIFY(prev.has_value());
    QCOMPARE(prev->page, 4);
    QCOMPARE(prev->scale, 2.0f);
}

void ViewHistoryTest::forward_undoes_back() {
    ViewHistory h;
    h.visit({0, 1.0f});
    h.visit({4, 1.0f});
    h.back();
    const auto next = h.forward();
    QVERIFY(next.has_value());
    QCOMPARE(next->page, 4);
    QVERIFY(!h.canGoForward());
}

void ViewHistoryTest::a_new_visit_after_going_back_drops_the_forward_entries() {
    ViewHistory h;
    h.visit({0, 1.0f});
    h.visit({4, 1.0f});
    h.visit({9, 1.0f});
    h.back();
    h.back();
    h.visit({20, 1.0f});
    QVERIFY(!h.canGoForward());
    QCOMPARE(h.back()->page, 0);
}

void ViewHistoryTest::zooming_on_the_same_page_does_not_add_an_entry() {
    ViewHistory h;
    h.visit({3, 1.0f});
    h.visit({3, 2.0f});
    h.visit({3, 4.0f});
    QCOMPARE(int(h.size()), 1);
    QVERIFY(!h.canGoBack());
}

void ViewHistoryTest::nowhere_to_go_is_empty() {
    ViewHistory h;
    QVERIFY(!h.back().has_value());
    QVERIFY(!h.forward().has_value());
    h.visit({0, 1.0f});
    QVERIFY(!h.back().has_value());
}

void ViewHistoryTest::it_is_bounded() {
    ViewHistory h;
    for (int i = 0; i < 1000; ++i) h.visit({i, 1.0f});
    QVERIFY(h.size() <= 256);
    QCOMPARE(h.back()->page, 998);
}

QTEST_APPLESS_MAIN(ViewHistoryTest)
#include "view_history_test.moc"
