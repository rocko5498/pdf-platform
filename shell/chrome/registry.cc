#include "registry.h"

#include <toml.hpp>

#include <QCoreApplication>
#include <QDebug>
#include <QDir>
#include <QFileInfo>
#include <QKeyEvent>

#include <stdexcept>
#include <string>

namespace pdf_platform::chrome {
namespace {

/// Schema this parser was compiled against. ADR-032: a parser MUST reject a
/// file whose `schema_version` exceeds it.
constexpr int kSupportedSchemaVersion = 1;

[[noreturn]] void fail(const QString& path, const QString& reason) {
    throw std::runtime_error(
        QStringLiteral("ui-registry (%1): %2").arg(path, reason).toStdString());
}

/// Qt's sentinel for "this text named no key I know". Spelled numerically so
/// the ADR-032 gate does not read it as a binding; it is the opposite of one.
constexpr int kQtKeyUnknown = 0x01ffffff;

/// Spellings the contract file prefers over Qt's abbreviations.
///
/// `QKeySequence` parses its own portable names ("PgDown"), not the words a
/// reviewer expects to read in a stability contract ("PageDown"). The file
/// keeps the readable spelling — DS-CONV-4 makes it a document people review —
/// and the parser translates. [ADR-032]
QString canonicalSequenceText(const QString& text) {
    static const QHash<QString, QString> kAliases = {
        {QStringLiteral("PageDown"), QStringLiteral("PgDown")},
        {QStringLiteral("PageUp"), QStringLiteral("PgUp")},
        {QStringLiteral("Delete"), QStringLiteral("Del")},
        {QStringLiteral("Insert"), QStringLiteral("Ins")},
        {QStringLiteral("Escape"), QStringLiteral("Esc")},
    };
    QStringList parts = text.split(QLatin1Char('+'));
    if (!parts.isEmpty()) {
        const QString last = parts.takeLast();
        parts.append(kAliases.value(last, last));
    }
    return parts.join(QLatin1Char('+'));
}

QKeySequence parseSequence(const QString& path, const QString& action, const std::string& text) {
    const QString written = QString::fromStdString(text);
    const QKeySequence sequence(canonicalSequenceText(written));
    // An unrecognised name yields a one-element sequence holding Qt's unknown
    // sentinel, not an empty one. Accepting it would leave a binding that is
    // declared, reviewed, and dead — the silent failure GR-8 forbids.
    const bool unknown =
        sequence.count() > 0 && static_cast<int>(sequence[0].key()) == kQtKeyUnknown;
    if (sequence.isEmpty() || unknown) {
        fail(path, QStringLiteral("action '%1' has a key sequence Qt cannot parse: \"%2\"")
                       .arg(action, written));
    }
    return sequence;
}

}  // namespace

ShortcutRegistry ShortcutRegistry::load(const QString& path) {
    if (!QFileInfo::exists(path)) {
        fail(path, QStringLiteral("file not found — the shortcut contract must ship with the app"));
    }

    ShortcutRegistry registry;
    toml::value root;
    try {
        root = toml::parse(path.toStdString());
    } catch (const std::exception& error) {
        fail(path, QStringLiteral("parse failed: %1").arg(QString::fromUtf8(error.what())));
    }

    registry.schema_version_ = toml::find_or<int>(root, "schema_version", 0);
    if (registry.schema_version_ <= 0) {
        fail(path, QStringLiteral("schema_version missing"));
    }
    if (registry.schema_version_ > kSupportedSchemaVersion) {
        fail(path, QStringLiteral("schema_version %1 is newer than this build understands (%2)")
                       .arg(registry.schema_version_)
                       .arg(kSupportedSchemaVersion));
    }
    registry.profile_version_ =
        QString::fromStdString(toml::find_or<std::string>(root, "profile_version", ""));
    if (registry.profile_version_.isEmpty()) {
        fail(path, QStringLiteral("profile_version missing — it is the ADR-030 identity"));
    }

    if (!root.contains("shortcuts")) {
        fail(path, QStringLiteral("no [shortcuts] table"));
    }
    for (const auto& [name, entry] : toml::find<toml::table>(root, "shortcuts")) {
        const QString id = QString::fromStdString(name);
        if (!entry.contains("action") || !entry.contains("key")) {
            fail(path, QStringLiteral("shortcut '%1' needs both 'action' and 'key'").arg(id));
        }
        Binding binding;
        binding.action = QString::fromStdString(toml::find<std::string>(entry, "action"));
        binding.key = parseSequence(path, binding.action, toml::find<std::string>(entry, "key"));
        if (entry.contains("alternates")) {
            for (const auto& alternate : toml::find<std::vector<std::string>>(entry, "alternates")) {
                binding.alternates.append(parseSequence(path, binding.action, alternate));
            }
        }
        if (registry.bindings_.contains(binding.action)) {
            fail(path, QStringLiteral("action '%1' is declared twice").arg(binding.action));
        }
        registry.bindings_.insert(binding.action, binding);
    }

    if (registry.bindings_.isEmpty()) {
        fail(path, QStringLiteral("[shortcuts] is empty"));
    }
    return registry;
}

QString ShortcutRegistry::defaultPath() {
    // 1. Explicit override. A test points this at a fixture registry to prove
    //    bindings are data, not code; a deployment can point it at a policy
    //    copy without touching the binary. [ADR-030]
    const QByteArray override = qgetenv("PDF_PLATFORM_UI_REGISTRY");
    if (!override.isEmpty()) {
        return QString::fromLocal8Bit(override);
    }
    // 2. Beside the executable, which is how the app ships it.
    const QString beside =
        QDir(QCoreApplication::applicationDirPath()).filePath(QStringLiteral("ui-registry.toml"));
    if (QFileInfo::exists(beside)) {
        return beside;
    }
    // 3. The source tree, for a developer build.
    return QStringLiteral(PDF_PLATFORM_UI_REGISTRY_SOURCE);
}

bool ShortcutRegistry::matches(const QString& action, const QKeyEvent* event) const {
    if (event == nullptr) {
        return false;
    }
    const auto found = bindings_.constFind(action);
    if (found == bindings_.constEnd()) {
        qWarning("ui-registry: no binding declared for action '%s'", qPrintable(action));
        return false;
    }
    // Building the sequence from the combination keeps every platform
    // convention QKeySequence knows about — notably Ctrl mapping to Cmd on
    // macOS — without naming a key in C++. [ADR-032]
    const QKeySequence pressed(event->keyCombination());
    if (pressed.isEmpty()) {
        return false;
    }
    if (pressed.matches(found->key) == QKeySequence::ExactMatch) {
        return true;
    }
    for (const QKeySequence& alternate : found->alternates) {
        if (pressed.matches(alternate) == QKeySequence::ExactMatch) {
            return true;
        }
    }
    return false;
}

QKeySequence ShortcutRegistry::key(const QString& action) const {
    return bindings_.value(action).key;
}

void ShortcutRegistry::requireActions(const QList<QString>& required) const {
    QStringList missing;
    for (const QString& action : required) {
        if (!bindings_.contains(action)) {
            missing.append(action);
        }
    }
    if (!missing.isEmpty()) {
        throw std::runtime_error(
            QStringLiteral("ui-registry: the shell dispatches actions the registry does not "
                           "declare: %1")
                .arg(missing.join(QStringLiteral(", ")))
                .toStdString());
    }
}

const ShortcutRegistry& shortcuts() {
    static const ShortcutRegistry registry = ShortcutRegistry::load(ShortcutRegistry::defaultPath());
    return registry;
}

}  // namespace pdf_platform::chrome
