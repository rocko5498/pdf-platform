// Printing: pages rendered at printer resolution, laid out on the sheet.
// [FR-PRINT-1, FR-PRINT-2, FR-PRINT-3, SDS §2.6, ADR-003, ADR-016]
//
// Spooling is the shell's job through Qt's native print system; the pixels
// come from the same worker tile path the viewer uses, so a printed page is the
// rendering the screen shows rather than a second interpretation of the file.
// The shell holds no document state: the caller says where the shared region
// is and how big the pages are. [RQ-1, RQ-3]
#pragma once

#include <QImage>
#include <QRectF>
#include <QSizeF>
#include <QString>

#include <cstdint>

class QPrinter;

namespace pdf_platform {

enum class PrintScale { FitPage, ActualSize, Custom };

struct PrintOptions {
    uint32_t first_page = 0;  ///< 0-based, inclusive.
    uint32_t last_page = 0;   ///< 0-based, inclusive.
    PrintScale scale = PrintScale::FitPage;
    int custom_percent = 100;  ///< Used when `scale` is Custom; 1 to 400.
};

/// What the print path needs to know about an open document.
struct PageSource {
    const void* shared_region = nullptr;  ///< Mapped by the bridge; borrowed.
    uint32_t page_count = 0;
    float page_width_pt = 0.f;   ///< Every page is taken to be this size, as in the viewer.
    float page_height_pt = 0.f;
};

/// Where a page of `page_pt` points lands inside `area_px` device pixels when
/// the device resolves `dpi` pixels per inch. Fit centres the page and keeps its
/// proportions; actual size and custom anchor it at the top left, as the
/// incumbents do, and may run off the sheet. [FR-PRINT-2]
QRectF printTargetRect(const QSizeF& page_pt, const QSizeF& area_px, int dpi,
                       const PrintOptions& options);

/// Render one page at `dpi` into an RGBA image. Returns a null image and sets
/// `error` when the page cannot be rendered or would exceed the memory bound.
/// `generation` is the caller's running tile counter. [GR-7, FR-PRINT-3]
QImage renderPageImage(const PageSource& source, uint32_t page, int dpi, uint64_t* generation,
                       QString* error);

/// Print pages `first_page..last_page` to `printer`, one page in memory at a
/// time. Returns false with a reason in `error`; nothing is claimed to have
/// printed that did not. [GR-7, GR-8]
bool printPages(QPrinter& printer, const PageSource& source, const PrintOptions& options,
                uint64_t* generation, QString* error);

}  // namespace pdf_platform
