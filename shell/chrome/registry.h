// The shortcut registry: `ui-registry.toml` read into memory. [ADR-032, ADR-030]
//
// ADR-032 is normative — "No shortcut binding may appear in C++ source. The
// registry is the single source of truth". Widgets ask this class whether an
// event matches an action id; they never name a key.
//
// DS-CONV-4 makes the file the concrete expression of PRIN-4, so a missing or
// malformed registry is fatal rather than something to paper over with
// built-in defaults: a fallback would put bindings back in C++ where nobody
// reviews them. [GR-8, RQA-1]

#pragma once

#include <QHash>
#include <QKeySequence>
#include <QList>
#include <QString>

class QKeyEvent;

namespace pdf_platform::chrome {

/// One action's bindings: the contract key plus any declared alternates.
struct Binding {
    QString action;
    QKeySequence key;
    QList<QKeySequence> alternates;
};

/// One entry in a declared menu: an action, or a separator.
struct MenuItem {
    /// Action id this item invokes. Empty for a separator.
    QString action;
    /// Title as the taxonomy declares it, mnemonic included.
    QString title;
    /// True when this item is a rule between groups.
    bool separator = false;
};

/// One declared top-level menu. [DS-MENU-1, ADR-032]
struct Menu {
    QString id;
    QString title;
    QList<MenuItem> items;
};

/// Parsed `ui-registry.toml`.
class ShortcutRegistry {
public:
    /// Parse `path`. Throws `std::runtime_error` naming the file and the fault.
    static ShortcutRegistry load(const QString& path);

    /// Path the process loads by default: `PDF_PLATFORM_UI_REGISTRY`, then the
    /// executable's directory, then the source tree. The environment override
    /// is what lets a test rebind an action without touching C++.
    static QString defaultPath();

    /// True when `event` matches the action's key or one of its alternates.
    /// Unknown action ids are a programming error and return false after a
    /// warning — the completeness check below is what catches them at startup.
    bool matches(const QString& action, const QKeyEvent* event) const;

    /// Contract key for an action, or an empty sequence if undeclared.
    QKeySequence key(const QString& action) const;

    /// ADR-030 profile identity.
    QString profileVersion() const { return profile_version_; }

    int schemaVersion() const { return schema_version_; }

    /// Action ids the registry declares.
    QList<QString> actions() const { return bindings_.keys(); }

    /// The declared menu taxonomy, in file order.
    ///
    /// `[[menus]]` has been in the contract file since M1 and nothing read it:
    /// the taxonomy DS-MENU-* and PRIN-4 call a stability contract existed only
    /// as a table nobody parsed, and the application had no menu bar at all.
    /// [ADR-032, DS-MENU-1, PRIN-4]
    const QList<Menu>& menus() const { return menus_; }

    /// Throws unless every id in `required` is declared. Called at startup so a
    /// binding the shell dispatches but the registry omits is a loud failure,
    /// not a silently dead key.
    void requireActions(const QList<QString>& required) const;

private:
    QHash<QString, Binding> bindings_;
    QList<Menu> menus_;
    QString profile_version_;
    int schema_version_ = 0;
};

/// Process-wide registry, loaded once from `defaultPath()`.
const ShortcutRegistry& shortcuts();

}  // namespace pdf_platform::chrome
