// Printing reaches the output: print to a PDF, open that PDF with the product's
// own engine, and look at the pixels. [FR-PRINT-1, FR-PRINT-3, T-10, T-12]
//
// Checking that `printPages` returned true would pass for a job that drew
// nothing. Re-opening the printed file through the same bridge the viewer uses
// is what can tell a page with text from a page without it.

#include "bridge.h"
#include "print_job.h"

#include <QDir>
#include <QFileInfo>
#include <QImage>
#include <QPageSize>
#include <QPrinter>
#include <QRegularExpression>
#include <QTest>

using namespace pdf_platform;

namespace {

QString fixture(const char* name) {
    return QStringLiteral(PDF_PLATFORM_FIXTURE_DIR "/") + QString::fromLatin1(name);
}

PageSource open(const QString& path) {
    const auto opened = pdf_platform::open_document(path.toStdString());
    PageSource source;
    source.shared_region = reinterpret_cast<const void*>(opened.shmem_handle);
    source.page_count = opened.page_count;
    source.page_width_pt = opened.page_width;
    source.page_height_pt = opened.page_height;
    return source;
}

bool hasDarkPixel(const QImage& image) {
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QRgb p = image.pixel(x, y);
            if (qRed(p) < 128 && qGreen(p) < 128 && qBlue(p) < 128) return true;
        }
    }
    return false;
}

bool allWhite(const QImage& image) {
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            if (image.pixel(x, y) != qRgba(255, 255, 255, 255)) return false;
        }
    }
    return true;
}

/// Print `[first, last]` (0-based) of `fixtureName` to a PDF and return its path.
QString printToPdf(const char* fixtureName, uint32_t first, uint32_t last, uint64_t* generation,
                   const QString& tag) {
    const PageSource source = open(fixture(fixtureName));
    const QString out = QDir::temp().filePath(QStringLiteral("pdf-platform-print-%1.pdf").arg(tag));
    QFile::remove(out);

    QPrinter printer(QPrinter::HighResolution);
    printer.setOutputFormat(QPrinter::PdfFormat);
    printer.setOutputFileName(out);
    printer.setPageSize(QPageSize(QPageSize::A4));

    PrintOptions options;
    options.first_page = first;
    options.last_page = last;
    QString error;
    if (!printPages(printer, source, options, generation, &error)) {
        qWarning() << "printPages failed:" << error;
        return {};
    }
    return out;
}

}  // namespace

class PrintRoundTripTest : public QObject {
    Q_OBJECT

private slots:
    void cleanup() { pdf_platform::close_document(); }
    void a_page_renders_at_the_requested_resolution_with_its_own_content();
    void printing_a_range_prints_exactly_that_range();
    void text_reaches_the_printed_page_and_a_blank_page_stays_blank();
    void a_bad_range_is_refused_and_writes_nothing();

private:
    uint64_t generation_ = 1;
};

void PrintRoundTripTest::a_page_renders_at_the_requested_resolution_with_its_own_content() {
    QString error;
    const PageSource text = open(fixture("text-latin.pdf"));
    const QImage at72 = renderPageImage(text, 0, 72, &generation_, &error);
    const QImage at144 = renderPageImage(text, 0, 144, &generation_, &error);
    QVERIFY2(!at72.isNull() && !at144.isNull(), qPrintable(error));

    QVERIFY2(qAbs(at144.width() - at72.width() * 2) <= 1, "doubling the DPI did not double the width");
    QVERIFY2(qAbs(at144.height() - at72.height() * 2) <= 1, "doubling the DPI did not double the height");
    QVERIFY2(hasDarkPixel(at72), "the page with text rendered with no dark pixel");

    pdf_platform::close_document();
    const PageSource blank = open(fixture("valid-1page.pdf"));
    const QImage white = renderPageImage(blank, 0, 72, &generation_, &error);
    QVERIFY2(!white.isNull(), qPrintable(error));
    QVERIFY2(allWhite(white), "the blank page did not render as white");
}

void PrintRoundTripTest::printing_a_range_prints_exactly_that_range() {
    // valid-3page.pdf: print pages 2 and 3 only.
    const QString out = printToPdf("valid-3page.pdf", 1, 2, &generation_, QStringLiteral("range"));
    QVERIFY2(!out.isEmpty(), "printing failed");

    QFile file(out);
    QVERIFY(file.open(QIODevice::ReadOnly));
    const QByteArray bytes = file.readAll();
    file.close();
    // Count page objects, not the /Pages tree node.
    const QRegularExpression page(QStringLiteral("/Type\\s*/Page(?![s])"));
    const int pages = QString::fromLatin1(bytes).count(page);
    QCOMPARE(pages, 2);
    QFile::remove(out);
}

void PrintRoundTripTest::text_reaches_the_printed_page_and_a_blank_page_stays_blank() {
    const QString text_pdf = printToPdf("text-latin.pdf", 0, 0, &generation_, QStringLiteral("text"));
    QVERIFY(!text_pdf.isEmpty());
    pdf_platform::close_document();
    const QString blank_pdf = printToPdf("valid-1page.pdf", 0, 0, &generation_, QStringLiteral("blank"));
    QVERIFY(!blank_pdf.isEmpty());
    pdf_platform::close_document();

    QString error;
    const PageSource printed_text = open(text_pdf);
    QCOMPARE(printed_text.page_count, 1u);
    const QImage text_image = renderPageImage(printed_text, 0, 150, &generation_, &error);
    QVERIFY2(!text_image.isNull(), qPrintable(error));
    QVERIFY2(hasDarkPixel(text_image),
             "the printed file has no dark pixel: the page's text never reached the printer");

    pdf_platform::close_document();
    const PageSource printed_blank = open(blank_pdf);
    const QImage blank_image = renderPageImage(printed_blank, 0, 150, &generation_, &error);
    QVERIFY2(!blank_image.isNull(), qPrintable(error));
    QVERIFY2(allWhite(blank_image), "the printed blank page is not white");

    QFile::remove(text_pdf);
    QFile::remove(blank_pdf);
}

void PrintRoundTripTest::a_bad_range_is_refused_and_writes_nothing() {
    const PageSource source = open(fixture("valid-3page.pdf"));
    const QString out = QDir::temp().filePath(QStringLiteral("pdf-platform-print-bad.pdf"));
    QFile::remove(out);

    QPrinter printer(QPrinter::HighResolution);
    printer.setOutputFormat(QPrinter::PdfFormat);
    printer.setOutputFileName(out);

    PrintOptions options;
    options.first_page = 2;
    options.last_page = 7;  // The document has 3 pages.
    QString error;
    QVERIFY2(!printPages(printer, source, options, &generation_, &error), "an impossible range printed");
    QVERIFY2(error.contains(QStringLiteral("not within")), qPrintable(error));
    QVERIFY2(!QFileInfo::exists(out), "a refused job left a file behind");

    options.first_page = 2;
    options.last_page = 1;  // Reversed.
    QVERIFY(!printPages(printer, source, options, &generation_, &error));
}

QTEST_MAIN(PrintRoundTripTest)
#include "print_roundtrip_test.moc"
