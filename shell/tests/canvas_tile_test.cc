// M0 exit criterion: a tile rendered through the real bridge, IPC and shared
// memory — on every OS CI builds. [AGENTS §11, ADR-003, ADR-004, ADR-007,
// ADR-022 T-5, ADR-029, SDS §6.4]
//
// The Rust-side `render_tile_real_pdf_via_pdfium` cannot discharge this: it
// returns early when the fixture is absent and treats a RenderError as an
// acceptable outcome, so it passes on a machine with no engine at all. This
// test does the opposite — every degraded path is a failure — and it goes
// through the cxx bridge and the shared-memory mapping the canvas itself uses,
// rather than a Rust-only harness that skips the C++ boundary.

#include "bridge.h"

#include <QByteArray>
#include <QFileInfo>
#include <QImage>
#include <QTest>

#include <cstdint>
#include <string>

namespace {

/// The one-page fixture also used by corpus-diff. White page, no content.
QString fixturePath() {
    return QStringLiteral(PDF_PLATFORM_FIXTURE_DIR "/valid-1page.pdf");
}

constexpr uint32_t kTileEdge = 256;
constexpr uint32_t kTileBytes = kTileEdge * kTileEdge * 4;

}  // namespace

class CanvasTileTest : public QObject {
    Q_OBJECT

private slots:
    void cleanup();
    void tile_arrives_through_bridge_ipc_and_shmem();
};

void CanvasTileTest::cleanup() {
    pdf_platform::close_document();
}

void CanvasTileTest::tile_arrives_through_bridge_ipc_and_shmem() {
    const QString path = fixturePath();
    QVERIFY2(QFileInfo::exists(path), qPrintable(QStringLiteral("fixture missing: %1").arg(path)));

    // 1. Real open: spawns the sandboxed worker, which loads the provisioned
    //    PDFium and maps the shared region. A failure here is a failure of the
    //    criterion, never a skip. [ADR-038]
    const auto opened = pdf_platform::open_document(path.toStdString());
    QCOMPARE(opened.page_count, 1u);
    QVERIFY2(opened.shmem_handle != 0, "worker returned no shared-memory mapping");
    QVERIFY(opened.page_width > 0.0f);
    QVERIFY(opened.page_height > 0.0f);

    const auto* region = reinterpret_cast<const uint8_t*>(opened.shmem_handle);
    QVERIFY(region != nullptr);

    // 2. Real render: the command crosses IPC to the worker, which rasterizes
    //    with PDFium and writes RGBA8 straight into the shared region.
    const auto tile = pdf_platform::render_tile(0, 0, 0, kTileEdge, kTileEdge, 1.0f, 1);
    QCOMPARE(tile.len, kTileBytes);
    QCOMPARE(tile.generation, 1ull);

    // 3. Read the pixels back exactly as the canvas does.
    const uint8_t* pixels = region + tile.offset;
    const QImage image(pixels, int(kTileEdge), int(kTileEdge), int(kTileEdge) * 4,
                       QImage::Format_RGBA8888);
    QCOMPARE(image.width(), int(kTileEdge));
    QCOMPARE(image.height(), int(kTileEdge));

    // A zeroed region is what an unwritten mapping looks like, and a stub
    // rasterizer is what a missing engine looks like. Neither may pass: this
    // fixture is a white page, so PDFium must have written opaque white.
    qsizetype opaqueWhite = 0;
    for (int y = 0; y < image.height(); ++y) {
        for (int x = 0; x < image.width(); ++x) {
            const QRgb pixel = image.pixel(x, y);
            if (qAlpha(pixel) == 255 && qRed(pixel) == 255 && qGreen(pixel) == 255 &&
                qBlue(pixel) == 255) {
                ++opaqueWhite;
            }
        }
    }
    const qsizetype total = qsizetype(kTileEdge) * kTileEdge;
    QVERIFY2(opaqueWhite == total,
             qPrintable(QStringLiteral("expected %1 opaque white pixels from the rasterized page, got %2 "
                                       "— an unwritten shared region or a stub engine")
                            .arg(total)
                            .arg(opaqueWhite)));

    // 4. A second tile must land too: the session survives one render, which is
    //    what the viewer does on every scroll step.
    const auto second = pdf_platform::render_tile(0, 0, 0, kTileEdge, kTileEdge, 1.0f, 2);
    QCOMPARE(second.len, kTileBytes);
    QCOMPARE(second.generation, 2ull);
}

QTEST_MAIN(CanvasTileTest)
#include "canvas_tile_test.moc"
