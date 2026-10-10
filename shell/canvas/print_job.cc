#include "print_job.h"

#include "bridge.h"

#include <QPainter>
#include <QPrinter>

#include <algorithm>
#include <cmath>
#include <cstring>
#include <exception>

namespace pdf_platform {

namespace {

constexpr uint32_t kTileEdge = 256;

/// One page's raster may not exceed this many bytes: a page is held whole
/// while it prints, and nothing else grows with the document. [GR-7]
constexpr uint64_t kMaxPageBytes = 256ull * 1024 * 1024;

constexpr int kMinRenderDpi = 96;
constexpr int kMaxRenderDpi = 300;

}  // namespace

QRectF printTargetRect(const QSizeF& page_pt, const QSizeF& area_px, int dpi,
                       const PrintOptions& options) {
    const double per_point = double(dpi) / 72.0;
    const QSizeF actual(page_pt.width() * per_point, page_pt.height() * per_point);

    double factor = 1.0;
    switch (options.scale) {
        case PrintScale::FitPage:
            factor = std::min(area_px.width() / actual.width(), area_px.height() / actual.height());
            break;
        case PrintScale::ActualSize:
            factor = 1.0;
            break;
        case PrintScale::Custom:
            factor = double(std::clamp(options.custom_percent, 1, 400)) / 100.0;
            break;
    }
    const QSizeF size(actual.width() * factor, actual.height() * factor);
    if (options.scale == PrintScale::FitPage) {
        return QRectF((area_px.width() - size.width()) / 2.0, (area_px.height() - size.height()) / 2.0,
                      size.width(), size.height());
    }
    return QRectF(0.0, 0.0, size.width(), size.height());
}

QImage renderPageImage(const PageSource& source, uint32_t page, int dpi, uint64_t* generation,
                       QString* error) {
    auto fail = [&](const QString& why) {
        if (error) *error = why;
        return QImage();
    };
    if (!source.shared_region || source.page_count == 0 || source.page_width_pt <= 0.f ||
        source.page_height_pt <= 0.f) {
        return fail(QStringLiteral("no document is open"));
    }
    if (page >= source.page_count) {
        return fail(QStringLiteral("page %1 does not exist").arg(page + 1));
    }

    const float scale = float(dpi) / 72.f;
    const uint32_t width = uint32_t(std::ceil(source.page_width_pt * scale));
    const uint32_t height = uint32_t(std::ceil(source.page_height_pt * scale));
    if (width == 0 || height == 0 || uint64_t(width) * height * 4 > kMaxPageBytes) {
        return fail(QStringLiteral("page %1 at %2 DPI is %3x%4 pixels, over the memory limit")
                        .arg(page + 1)
                        .arg(dpi)
                        .arg(width)
                        .arg(height));
    }

    QImage image(int(width), int(height), QImage::Format_RGBA8888);
    image.fill(Qt::white);

    try {
        for (uint32_t ty = 0; ty < height; ty += kTileEdge) {
            for (uint32_t tx = 0; tx < width; tx += kTileEdge) {
                const uint32_t tw = std::min(kTileEdge, width - tx);
                const uint32_t th = std::min(kTileEdge, height - ty);
                const auto tile = render_tile(page, tx, ty, tw, th, scale, (*generation)++);
                if (tile.len != uint64_t(tw) * th * 4) {
                    return fail(QStringLiteral("tile at %1,%2 of page %3 came back with %4 bytes, "
                                               "expected %5")
                                    .arg(tx)
                                    .arg(ty)
                                    .arg(page + 1)
                                    .arg(tile.len)
                                    .arg(uint64_t(tw) * th * 4));
                }
                const auto* pixels = static_cast<const uint8_t*>(source.shared_region) + tile.offset;
                for (uint32_t row = 0; row < th; ++row) {
                    std::memcpy(image.scanLine(int(ty + row)) + size_t(tx) * 4,
                                pixels + size_t(row) * tw * 4, size_t(tw) * 4);
                }
            }
        }
    } catch (const std::exception& e) {
        return fail(QString::fromUtf8(e.what()));
    }
    return image;
}

bool printPages(QPrinter& printer, const PageSource& source, const PrintOptions& options,
                uint64_t* generation, QString* error) {
    auto fail = [&](const QString& why) {
        if (error) *error = why;
        return false;
    };
    if (source.page_count == 0) return fail(QStringLiteral("no document is open"));
    if (options.first_page > options.last_page || options.last_page >= source.page_count) {
        return fail(QStringLiteral("pages %1 to %2 are not within the document's %3 pages")
                        .arg(options.first_page + 1)
                        .arg(options.last_page + 1)
                        .arg(source.page_count));
    }

    // Render below the device's own resolution when it is very high: the
    // painter scales up, and a 1200 DPI raster of a page would not fit in
    // memory. [GR-7]
    const int device_dpi = std::max(printer.resolution(), 1);
    const int render_dpi = std::clamp(device_dpi, kMinRenderDpi, kMaxRenderDpi);

    QPainter painter;
    if (!painter.begin(&printer)) {
        return fail(QStringLiteral("the printer could not be opened"));
    }

    const QSizeF area = printer.pageRect(QPrinter::DevicePixel).size();
    const QSizeF page_pt(source.page_width_pt, source.page_height_pt);
    for (uint32_t page = options.first_page; page <= options.last_page; ++page) {
        if (page != options.first_page && !printer.newPage()) {
            painter.end();
            return fail(QStringLiteral("the printer refused page %1").arg(page + 1));
        }
        QString why;
        const QImage image = renderPageImage(source, page, render_dpi, generation, &why);
        if (image.isNull()) {
            painter.end();
            return fail(why);
        }
        painter.drawImage(printTargetRect(page_pt, area, device_dpi, options), image);
    }
    painter.end();
    return true;
}

}  // namespace pdf_platform
