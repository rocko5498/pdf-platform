// ADR-032: the registry is the single source of truth for shortcuts.
//
// The only way to tell a data-driven binding from a hard-coded one is to change
// the data and see whether behaviour follows. This test points the shell at a
// fixture registry that binds page-stepping to a key the shipped registry does
// not use, then presses it. A `switch` on a key constant cannot pass.
// [ADR-030, ADR-032, DS-CONV-4, PRIN-4, RQA-1, T-8]

#include "registry.h"

#include <QDir>
#include <QKeyEvent>
#include <QTemporaryDir>
#include <QTest>

#include <stdexcept>

using pdf_platform::chrome::ShortcutRegistry;

namespace {

/// Write a registry to `dir` and return its path.
QString writeRegistry(const QTemporaryDir& dir, const QString& body) {
    const QString path = QDir(dir.path()).filePath(QStringLiteral("ui-registry.toml"));
    QFile file(path);
    if (!file.open(QIODevice::WriteOnly | QIODevice::Text)) {
        return {};
    }
    file.write(body.toUtf8());
    file.close();
    return path;
}

/// A key event as the widget would receive it.
QKeyEvent press(int key, Qt::KeyboardModifiers modifiers = Qt::NoModifier) {
    return QKeyEvent(QEvent::KeyPress, key, modifiers);
}

}  // namespace

class RegistryBindingTest : public QObject {
    Q_OBJECT

private slots:
    void a_rebound_action_follows_the_file();
    void alternates_match_as_well_as_the_primary_key();
    void a_newer_schema_is_rejected();
    void a_missing_file_is_fatal();
    void an_unparseable_sequence_is_rejected();
    void an_unknown_key_name_is_rejected();
    void the_shipped_registry_declares_every_action_the_shell_dispatches();
};

void RegistryBindingTest::a_rebound_action_follows_the_file() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    // Bind page-stepping to J/K, which the shipped registry never uses.
    const QString path = writeRegistry(dir, QStringLiteral(R"(
schema_version = 1
profile_version = "9.9.9"
[shortcuts]
next_page = { key = "J", action = "nav.next_page" }
prev_page = { key = "K", action = "nav.prev_page" }
)"));
    QVERIFY(!path.isEmpty());

    const ShortcutRegistry registry = ShortcutRegistry::load(path);
    QCOMPARE(registry.profileVersion(), QStringLiteral("9.9.9"));

    auto rebound = press(Qt::Key_J);
    QVERIFY2(registry.matches(QStringLiteral("nav.next_page"), &rebound),
             "the rebound key must drive the action");

    // The default binding must stop working once the file no longer declares
    // it. If this fails, something still decides bindings in C++.
    auto old_default = press(Qt::Key_PageDown);
    QVERIFY2(!registry.matches(QStringLiteral("nav.next_page"), &old_default),
             "a key the registry no longer declares must not still work");
}

void RegistryBindingTest::alternates_match_as_well_as_the_primary_key() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = writeRegistry(dir, QStringLiteral(R"(
schema_version = 1
profile_version = "9.9.9"
[shortcuts]
next_page = { key = "PageDown", action = "nav.next_page", alternates = ["Down", "Space"] }
)"));
    const ShortcutRegistry registry = ShortcutRegistry::load(path);

    auto primary = press(Qt::Key_PageDown);
    auto first = press(Qt::Key_Down);
    auto second = press(Qt::Key_Space);
    auto unrelated = press(Qt::Key_Up);
    QVERIFY(registry.matches(QStringLiteral("nav.next_page"), &primary));
    QVERIFY(registry.matches(QStringLiteral("nav.next_page"), &first));
    QVERIFY(registry.matches(QStringLiteral("nav.next_page"), &second));
    QVERIFY(!registry.matches(QStringLiteral("nav.next_page"), &unrelated));
}

void RegistryBindingTest::a_newer_schema_is_rejected() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = writeRegistry(dir, QStringLiteral(R"(
schema_version = 99
profile_version = "9.9.9"
[shortcuts]
next_page = { key = "J", action = "nav.next_page" }
)"));
    bool threw = false;
    try {
        ShortcutRegistry::load(path);
    } catch (const std::runtime_error& error) {
        threw = true;
        QVERIFY(QString::fromUtf8(error.what()).contains(QStringLiteral("schema_version")));
    }
    QVERIFY2(threw, "ADR-032 requires rejecting a schema newer than the parser");
}

void RegistryBindingTest::a_missing_file_is_fatal() {
    bool threw = false;
    try {
        ShortcutRegistry::load(QStringLiteral("/definitely/not/here/ui-registry.toml"));
    } catch (const std::runtime_error&) {
        threw = true;
    }
    QVERIFY2(threw, "a missing registry must fail loudly, never fall back to built-in keys");
}

void RegistryBindingTest::an_unparseable_sequence_is_rejected() {
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = writeRegistry(dir, QStringLiteral(R"(
schema_version = 1
profile_version = "9.9.9"
[shortcuts]
broken = { key = "", action = "nav.next_page" }
)"));
    bool threw = false;
    try {
        ShortcutRegistry::load(path);
    } catch (const std::runtime_error&) {
        threw = true;
    }
    QVERIFY2(threw, "an empty key sequence is a broken contract, not a no-op binding");
}

void RegistryBindingTest::an_unknown_key_name_is_rejected() {
    // "PageDwn" is not a Qt key name. Qt answers with its unknown sentinel
    // rather than an empty sequence, so a parser that only checks isEmpty()
    // accepts it and ships a binding that can never fire. [GR-8, PRIN-6]
    QTemporaryDir dir;
    QVERIFY(dir.isValid());
    const QString path = writeRegistry(dir, QStringLiteral(R"(
schema_version = 1
profile_version = "9.9.9"
[shortcuts]
typo = { key = "PageDwn", action = "nav.next_page" }
)"));
    bool threw = false;
    try {
        ShortcutRegistry::load(path);
    } catch (const std::runtime_error& error) {
        threw = true;
        QVERIFY(QString::fromUtf8(error.what()).contains(QStringLiteral("PageDwn")));
    }
    QVERIFY2(threw, "a key name Qt cannot parse must be rejected, not silently dead");
}

void RegistryBindingTest::the_shipped_registry_declares_every_action_the_shell_dispatches() {
    // The shipped file, not a fixture: this is the completeness check the app
    // performs at startup, run as a test so a missing binding fails CI rather
    // than a user's launch.
    const ShortcutRegistry registry =
        ShortcutRegistry::load(QStringLiteral(PDF_PLATFORM_UI_REGISTRY_SOURCE));
    registry.requireActions({
        QStringLiteral("document.open"),
        QStringLiteral("document.find"),
        QStringLiteral("document.find_next"),
        QStringLiteral("document.find_previous"),
        QStringLiteral("document.save"),
        QStringLiteral("edit.copy"),
        QStringLiteral("edit.undo"),
        QStringLiteral("edit.redo"),
        QStringLiteral("annot.export"),
        QStringLiteral("annot.delete"),
        QStringLiteral("forms.calculate"),
        QStringLiteral("focus.canvas"),
        QStringLiteral("ui.activate"),
        QStringLiteral("nav.next_page"),
        QStringLiteral("nav.prev_page"),
        QStringLiteral("nav.first_page"),
        QStringLiteral("nav.last_page"),
        QStringLiteral("view.zoom_in"),
        QStringLiteral("view.zoom_out"),
    });
    QVERIFY(!registry.profileVersion().isEmpty());
}

QTEST_MAIN(RegistryBindingTest)
#include "registry_binding_test.moc"
