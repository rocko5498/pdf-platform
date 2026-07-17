// Qt application entry. [ADR-003, SDS A1.3]
// M1: accessible chrome, docked panels, GPU canvas, annotation tools.

#include <QApplication>
#include <QFileInfo>
#include <QSurfaceFormat>

#include "a11y.h"
#include "canvas.h"

int main(int argc, char* argv[]) {
    // Prefer a compatibility profile so QPainter-on-GL works for tile blit. [ADR-007]
    QSurfaceFormat fmt;
    fmt.setDepthBufferSize(16);
    fmt.setStencilBufferSize(8);
    fmt.setVersion(2, 1);
    fmt.setProfile(QSurfaceFormat::CompatibilityProfile);
    QSurfaceFormat::setDefaultFormat(fmt);

    QApplication app(argc, argv);
    app.setApplicationName(QStringLiteral("PDF Platform"));
    app.setOrganizationName(QStringLiteral("pdf-platform"));

    pdf_platform::installAccessibility();

    pdf_platform::MainWindow window;
    pdf_platform::configureMainWindowAccessibility(&window, window.canvasWidget());
    window.setFocusPolicy(Qt::StrongFocus);

    // Command-line file open. Windows splits unquoted paths on spaces, so
    // `R:\Rust Project\file.pdf` becomes argv[1]="R:\Rust" argv[2]="Project\...".
    // Rejoin when the first segment alone is not an existing file. [FR-VIEW]
    const QStringList args = app.arguments();
    if (args.size() > 1) {
        QString path = args.at(1);
        if (!QFileInfo::exists(path) && args.size() > 2) {
            path = args.mid(1).join(QLatin1Char(' '));
        }
        window.openDocument(path);
    } else {
        window.setWindowTitle(QStringLiteral("PDF Platform — no document"));
    }

    window.show();
    return app.exec();
}
