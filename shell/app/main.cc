// Qt application entry. [ADR-003, SDS §1.3]
// M1: accessible chrome, docked panels, GPU canvas, annotation tools.

#include <QApplication>
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

    if (argc > 1) {
        window.openDocument(QString::fromLocal8Bit(argv[1]));
    } else {
        window.setWindowTitle(QStringLiteral("PDF Platform — no document"));
    }

    window.show();
    return app.exec();
}
