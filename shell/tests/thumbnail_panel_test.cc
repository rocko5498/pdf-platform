// The thumbnail list asks only for what is in view. [FR-THUMB-1, NFR-PERF-1, T-10]

#include "thumbnail_panel.h"

#include <QListWidget>
#include <QSignalSpy>
#include <QTest>

using pdf_platform::ThumbnailPanel;

class ThumbnailPanelTest : public QObject {
    Q_OBJECT

private slots:
    void one_entry_per_page();
    void a_long_document_requests_only_the_pages_in_view();
    void a_supplied_thumbnail_is_not_asked_for_again();
    void clicking_an_entry_reports_the_page();
    void reporting_the_current_page_does_not_navigate();
    void clear_removes_every_entry();
};

namespace {
QImage solid() {
    QImage image(40, 52, QImage::Format_RGBA8888);
    image.fill(Qt::red);
    return image;
}
}  // namespace

void ThumbnailPanelTest::one_entry_per_page() {
    ThumbnailPanel panel;
    panel.setDocument(7, QSizeF(612, 792));
    QCOMPARE(panel.pageCount(), 7);
    QCOMPARE(panel.thumbnailCount(), 0);
}

void ThumbnailPanelTest::a_long_document_requests_only_the_pages_in_view() {
    ThumbnailPanel panel;
    panel.resize(160, 400);
    QSignalSpy spy(&panel, &ThumbnailPanel::thumbnailRequested);
    panel.show();
    panel.setDocument(2000, QSizeF(612, 792));
    QVERIFY(QTest::qWaitFor([&]() { return spy.count() > 0; }, 2000));
    QTest::qWait(50);
    QVERIFY2(spy.count() < 100, qPrintable(QString("asked for %1 of 2000 pages").arg(spy.count())));
    QCOMPARE(spy.first().first().toInt(), 0);
}

void ThumbnailPanelTest::a_supplied_thumbnail_is_not_asked_for_again() {
    ThumbnailPanel panel;
    panel.resize(160, 400);
    QSignalSpy spy(&panel, &ThumbnailPanel::thumbnailRequested);
    panel.show();
    panel.setDocument(30, QSizeF(612, 792));
    QVERIFY(QTest::qWaitFor([&]() { return spy.count() > 0; }, 2000));
    const int first = spy.count();
    panel.setThumbnail(0, solid());
    QCOMPARE(panel.thumbnailCount(), 1);
    panel.resize(180, 420);  // provokes another look at what is visible
    QTest::qWait(50);
    int zero_requests = 0;
    for (const auto& args : spy) zero_requests += args.first().toInt() == 0;
    QCOMPARE(zero_requests, 1);
    QVERIFY(spy.count() >= first);
}

void ThumbnailPanelTest::clicking_an_entry_reports_the_page() {
    ThumbnailPanel panel;
    panel.resize(160, 400);
    panel.show();
    panel.setDocument(5, QSizeF(612, 792));
    QSignalSpy spy(&panel, &ThumbnailPanel::pageActivated);
    auto* list = panel.findChild<QListWidget*>(QStringLiteral("thumbnailList"));
    QVERIFY(list);
    const QRect rect = list->visualItemRect(list->item(2));
    QTest::mouseClick(list->viewport(), Qt::LeftButton, Qt::NoModifier, rect.center());
    QCOMPARE(spy.count(), 1);
    QCOMPARE(spy.first().first().toInt(), 2);
}

void ThumbnailPanelTest::reporting_the_current_page_does_not_navigate() {
    ThumbnailPanel panel;
    panel.show();
    panel.setDocument(5, QSizeF(612, 792));
    QSignalSpy spy(&panel, &ThumbnailPanel::pageActivated);
    panel.setCurrentPage(3);
    QCOMPARE(spy.count(), 0);
}

void ThumbnailPanelTest::clear_removes_every_entry() {
    ThumbnailPanel panel;
    panel.show();
    panel.setDocument(5, QSizeF(612, 792));
    panel.setThumbnail(1, solid());
    panel.clear();
    QCOMPARE(panel.pageCount(), 0);
    QCOMPARE(panel.thumbnailCount(), 0);
}

QTEST_MAIN(ThumbnailPanelTest)
#include "thumbnail_panel_test.moc"
