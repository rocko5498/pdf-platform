// The menu bar is the declared taxonomy, not a hand-built copy of it.
// [ADR-032, DS-MENU-1, PRIN-4, RQA-2, T-8]
//
// `[[menus]]` has been in `ui-registry.toml` since M1, validated by
// `tools/check-ui-registry`, and read by nothing: the application had no menu
// bar at all, and `view.zoom_fit` — declared, reviewed, listed in the View menu
// — did nothing when pressed. A taxonomy nobody builds from is not a stability
// contract; it is a comment.
//
// These assert the built menus against the registry, so a menu edited in C++
// rather than in the contract file fails here.

#include "canvas.h"
#include "registry.h"

#include <QAction>
#include <QMenu>
#include <QMenuBar>
#include <QTest>

using pdf_platform::MainWindow;
namespace chrome = pdf_platform::chrome;

class MenuBarTest : public QObject {
    Q_OBJECT

private slots:
    void the_menu_bar_is_the_declared_taxonomy();
    void every_declared_item_has_a_handler();
    void an_item_carries_the_registry_shortcut();
    void triggering_zoom_in_changes_the_zoom();
    void fit_page_scales_the_page_to_the_viewport();
};

void MenuBarTest::the_menu_bar_is_the_declared_taxonomy() {
    MainWindow window;
    const auto& declared = chrome::shortcuts().menus();
    QVERIFY2(!declared.isEmpty(), "the registry declares no menus; this test would prove nothing");

    const QList<QAction*> top = window.menuBar()->actions();
    QCOMPARE(top.size(), declared.size());

    for (int index = 0; index < declared.size(); ++index) {
        const chrome::Menu& menu = declared.at(index);
        QMenu* built = top.at(index)->menu();
        QVERIFY2(built != nullptr, qPrintable(QStringLiteral("menu %1 is not a menu").arg(menu.id)));
        QCOMPARE(built->objectName(), menu.id);
        QCOMPARE(top.at(index)->text(), menu.title);

        const QList<QAction*> items = built->actions();
        QCOMPARE(items.size(), menu.items.size());
        for (int item_index = 0; item_index < menu.items.size(); ++item_index) {
            const chrome::MenuItem& declared_item = menu.items.at(item_index);
            QAction* action = items.at(item_index);
            if (declared_item.separator) {
                QVERIFY2(action->isSeparator(),
                         qPrintable(QStringLiteral("menu %1 item %2 should be a separator")
                                        .arg(menu.id)
                                        .arg(item_index)));
                continue;
            }
            QVERIFY2(!action->isSeparator(),
                     qPrintable(QStringLiteral("menu %1 item %2 should not be a separator")
                                    .arg(menu.id)
                                    .arg(item_index)));
            QCOMPARE(action->text(), declared_item.title);
            QCOMPARE(action->objectName(), declared_item.action);
        }
    }
}

void MenuBarTest::every_declared_item_has_a_handler() {
    // A menu item nothing implements is the dead binding ADR-032 forbids —
    // `view.zoom_fit` was exactly that until this change.
    const QList<QString> handled = MainWindow::handledActions();
    for (const chrome::Menu& menu : chrome::shortcuts().menus()) {
        for (const chrome::MenuItem& item : menu.items) {
            if (item.separator) continue;
            QVERIFY2(handled.contains(item.action),
                     qPrintable(QStringLiteral("menu '%1' declares action '%2' and nothing "
                                               "implements it")
                                    .arg(menu.id, item.action)));
        }
    }
}

void MenuBarTest::an_item_carries_the_registry_shortcut() {
    // The shortcut shown in the menu comes from the same table the key handler
    // consults, so the two cannot disagree.
    MainWindow window;
    const auto& registry = chrome::shortcuts();

    for (QAction* top : window.menuBar()->actions()) {
        QMenu* menu = top->menu();
        if (!menu) continue;
        for (QAction* action : menu->actions()) {
            if (action->isSeparator()) continue;
            const QString id = action->data().toString();
            QVERIFY2(!id.isEmpty(), "a menu action carries no action id");
            QCOMPARE(action->shortcut(), registry.key(id));
        }
    }
}

void MenuBarTest::triggering_zoom_in_changes_the_zoom() {
    // The observable is the zoom the canvas reports, not that the action fired.
    MainWindow window;
    window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));

    QWidget* canvas = window.canvasWidget();
    QVERIFY(canvas != nullptr);

    window.triggerAction(QStringLiteral("view.zoom_in"));
    const int zoomed_in = canvas->property("zoomLevel").toInt();
    QVERIFY2(zoomed_in > 100,
             qPrintable(QStringLiteral("zoom in left the zoom at %1%").arg(zoomed_in)));

    window.triggerAction(QStringLiteral("view.zoom_out"));
    const int zoomed_out = canvas->property("zoomLevel").toInt();
    QVERIFY2(zoomed_out < zoomed_in,
             qPrintable(QStringLiteral("zoom out went from %1% to %2%")
                            .arg(zoomed_in)
                            .arg(zoomed_out)));
}

void MenuBarTest::fit_page_scales_the_page_to_the_viewport() {
    // `view.zoom_fit` did nothing at all before this change: it was declared in
    // the registry, listed in the View menu, and implemented nowhere.
    MainWindow window;
    window.show();
    QVERIFY(QTest::qWaitForWindowExposed(&window));

    QWidget* canvas = window.canvasWidget();
    QVERIFY(canvas != nullptr);
    QVERIFY2(canvas->height() > 0, "the canvas has no height; the expectation below is undefined");

    window.triggerAction(QStringLiteral("view.zoom_in"));
    window.triggerAction(QStringLiteral("view.zoom_fit"));

    // The default page height is A4 (842pt) until a document is open.
    const float expected = float(canvas->height()) / 842.f;
    const int expected_percent = int(qBound(0.25f, expected, 8.f) * 100.f);
    const int actual_percent = canvas->property("zoomLevel").toInt();
    QVERIFY2(qAbs(actual_percent - expected_percent) <= 1,
             qPrintable(QStringLiteral("fit made the zoom %1%, expected about %2% for a %3px "
                                       "viewport")
                            .arg(actual_percent)
                            .arg(expected_percent)
                            .arg(canvas->height())));
}

QTEST_MAIN(MenuBarTest)
#include "menu_bar_test.moc"
