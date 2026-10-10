// Where a printed page lands on the sheet. [FR-PRINT-2, FR-PRINT-3, T-10]
//
// Pure geometry, so it needs no worker and runs everywhere. The round trip that
// proves pixels reach the output is print_roundtrip_test.cc.

#include "print_job.h"

#include <QTest>

using pdf_platform::PrintOptions;
using pdf_platform::PrintScale;
using pdf_platform::printTargetRect;

class PrintLayoutTest : public QObject {
    Q_OBJECT

private slots:
    void fit_keeps_the_proportions_and_stays_on_the_sheet();
    void fit_touches_the_limiting_edge_and_is_centred();
    void actual_size_is_the_pages_own_size_at_the_top_left();
    void custom_scales_the_actual_size();
    void custom_is_bounded();
};

namespace {
const QSizeF kLetterPt(612.0, 792.0);
// A4 at 300 DPI, whole sheet.
const QSizeF kA4At300(2480.0, 3508.0);
}  // namespace

void PrintLayoutTest::fit_keeps_the_proportions_and_stays_on_the_sheet() {
    PrintOptions options;
    options.scale = PrintScale::FitPage;
    const QRectF rect = printTargetRect(kLetterPt, kA4At300, 300, options);

    QVERIFY2(QRectF(QPointF(0, 0), kA4At300).contains(rect), "the fitted page runs off the sheet");
    QCOMPARE_LE(qAbs(rect.width() / rect.height() - kLetterPt.width() / kLetterPt.height()), 1e-6);
}

void PrintLayoutTest::fit_touches_the_limiting_edge_and_is_centred() {
    PrintOptions options;
    options.scale = PrintScale::FitPage;
    // Letter is wider than A4 relative to its height, so width limits.
    const QRectF rect = printTargetRect(kLetterPt, kA4At300, 300, options);
    QCOMPARE_LE(qAbs(rect.width() - kA4At300.width()), 1e-6);
    QCOMPARE_LE(qAbs(rect.center().x() - kA4At300.width() / 2.0), 1e-6);
    QCOMPARE_LE(qAbs(rect.center().y() - kA4At300.height() / 2.0), 1e-6);
}

void PrintLayoutTest::actual_size_is_the_pages_own_size_at_the_top_left() {
    PrintOptions options;
    options.scale = PrintScale::ActualSize;
    const QRectF rect = printTargetRect(kLetterPt, kA4At300, 300, options);
    // 612 pt is 8.5 in; at 300 DPI that is 2550 pixels, wider than A4's 2480:
    // actual size means it is not shrunk to fit.
    QCOMPARE(rect.topLeft(), QPointF(0, 0));
    QCOMPARE_LE(qAbs(rect.width() - 2550.0), 1e-6);
    QCOMPARE_LE(qAbs(rect.height() - 3300.0), 1e-6);
}

void PrintLayoutTest::custom_scales_the_actual_size() {
    PrintOptions options;
    options.scale = PrintScale::Custom;
    options.custom_percent = 50;
    const QRectF rect = printTargetRect(kLetterPt, kA4At300, 300, options);
    QCOMPARE_LE(qAbs(rect.width() - 1275.0), 1e-6);
    QCOMPARE_LE(qAbs(rect.height() - 1650.0), 1e-6);
    QCOMPARE(rect.topLeft(), QPointF(0, 0));
}

void PrintLayoutTest::custom_is_bounded() {
    PrintOptions options;
    options.scale = PrintScale::Custom;
    options.custom_percent = 100000;
    const QRectF huge = printTargetRect(kLetterPt, kA4At300, 300, options);
    QCOMPARE_LE(huge.width(), 2550.0 * 4.0 + 1e-6);
    options.custom_percent = 0;
    const QRectF tiny = printTargetRect(kLetterPt, kA4At300, 300, options);
    QVERIFY2(tiny.width() > 0.0, "a zero percent scale produced an empty page");
}

QTEST_MAIN(PrintLayoutTest)
#include "print_layout_test.moc"
