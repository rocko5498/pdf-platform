// Minimal Qt application entry point. [ADR-003, SDS §1.3]
// M0: opens a document, renders page 1, displays in canvas.

#include <QApplication>

#include "canvas.h"

int main(int argc, char* argv[]) {
    QApplication app(argc, argv);
    app.setApplicationName("PDF Platform");
    app.setOrganizationName("pdf-platform");

    pdf_platform::MainWindow window;

    // M0: if a path is provided on the command line, open it.
    // Otherwise, open a test document or show the empty canvas.
    if (argc > 1) {
        window.openDocument(QString::fromLocal8Bit(argv[1]));
    } else {
        // M0 demo: open with no document to show the empty state.
        window.setWindowTitle("PDF Platform — M0 (no document)");
    }

    window.show();
    return app.exec();
}
