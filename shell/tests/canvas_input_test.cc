#include "canvas.h"

#include <QApplication>
#include <QSignalSpy>
#include <QTest>
#include <QWheelEvent>

using pdf_platform::CanvasWidget;

class CanvasInputTest : public QObject {
    Q_OBJECT

private slots:
    void page_keys_emit_steps();
    void control_zoom_keys_emit_steps();
    void wheel_routes_scroll_and_zoom();
};

void CanvasInputTest::page_keys_emit_steps() {
    CanvasWidget canvas;
    QSignalSpy spy(&canvas, &CanvasWidget::pageStepRequested);

    QTest::keyClick(&canvas, Qt::Key_PageDown);
    QTest::keyClick(&canvas, Qt::Key_PageUp);

    QCOMPARE(spy.count(), 2);
    QCOMPARE(spy.at(0).at(0).toInt(), +1);
    QCOMPARE(spy.at(1).at(0).toInt(), -1);
}

void CanvasInputTest::control_zoom_keys_emit_steps() {
    CanvasWidget canvas;
    QSignalSpy spy(&canvas, &CanvasWidget::zoomStepRequested);

    QTest::keyClick(&canvas, Qt::Key_Plus, Qt::ControlModifier);
    QTest::keyClick(&canvas, Qt::Key_Minus, Qt::ControlModifier);

    QCOMPARE(spy.count(), 2);
    QCOMPARE(spy.at(0).at(0).toInt(), +1);
    QCOMPARE(spy.at(1).at(0).toInt(), -1);
}

void CanvasInputTest::wheel_routes_scroll_and_zoom() {
    CanvasWidget canvas;
    QSignalSpy scroll(&canvas, &CanvasWidget::scrollDeltaRequested);
    QSignalSpy zoom(&canvas, &CanvasWidget::zoomStepRequested);

    QWheelEvent plain(QPointF(10, 10), QPointF(10, 10), QPoint(), QPoint(0, 120),
                      Qt::NoButton, Qt::NoModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(&canvas, &plain);
    QCOMPARE(scroll.count(), 1);
    QCOMPARE(scroll.at(0).at(0).toInt(), -120);
    QCOMPARE(zoom.count(), 0);

    QWheelEvent controlled(QPointF(10, 10), QPointF(10, 10), QPoint(), QPoint(0, 120),
                           Qt::NoButton, Qt::ControlModifier, Qt::NoScrollPhase, false);
    QApplication::sendEvent(&canvas, &controlled);
    QCOMPARE(scroll.count(), 1);
    QCOMPARE(zoom.count(), 1);
    QCOMPARE(zoom.at(0).at(0).toInt(), +1);
}

QTEST_MAIN(CanvasInputTest)
#include "canvas_input_test.moc"
