//! The macOS menu bar, which a Mac user expects of any app: Morpheus (About,
//! Services, Hide, Quit), Edit (Cut, Copy, Paste, Select All, which act on
//! the text box that has the focus), Window (Minimize, Close), with their
//! shortcuts: Cmd+W closes the window (the app keeps running, as the Dock's
//! icon brings it back), Cmd+Q quits.
//!
//! There is no Undo or Redo: the text boxes keep no history to go back in.
//! The menus are built on every system (so the tests see them) and only
//! installed on a Mac, where `set_menus` shows them.

use gpui::{App, KeyBinding, Menu, MenuItem, OsAction, SystemMenuType, actions};

actions!(
    morpheus,
    [
        About,
        Quit,
        Hide,
        HideOthers,
        ShowAll,
        CloseWindow,
        Minimize,
        Cut,
        Copy,
        Paste,
        SelectAll
    ]
);

/// The key context of a text box: the Edit menu's shortcuts apply inside it.
pub const TEXT_INPUT: &str = "TextInput";

/// The menu bar's menus.
pub fn menus() -> Vec<Menu> {
    vec![
        Menu {
            name: "Morpheus".into(),
            items: vec![
                MenuItem::action("About Morpheus", About),
                MenuItem::separator(),
                MenuItem::os_submenu("Services", SystemMenuType::Services),
                MenuItem::separator(),
                MenuItem::action("Hide Morpheus", Hide),
                MenuItem::action("Hide Others", HideOthers),
                MenuItem::action("Show All", ShowAll),
                MenuItem::separator(),
                MenuItem::action("Quit Morpheus", Quit),
            ],
        },
        Menu {
            name: "Edit".into(),
            items: vec![
                MenuItem::os_action("Cut", Cut, OsAction::Cut),
                MenuItem::os_action("Copy", Copy, OsAction::Copy),
                MenuItem::os_action("Paste", Paste, OsAction::Paste),
                MenuItem::os_action("Select All", SelectAll, OsAction::SelectAll),
            ],
        },
        Menu {
            name: "Window".into(),
            items: vec![
                MenuItem::action("Minimize", Minimize),
                MenuItem::action("Close", CloseWindow),
            ],
        },
    ]
}

/// The shortcuts shown in the menus (and working without them).
fn bindings() -> Vec<KeyBinding> {
    vec![
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-w", CloseWindow, None),
        KeyBinding::new("cmd-m", Minimize, None),
        KeyBinding::new("cmd-h", Hide, None),
        KeyBinding::new("alt-cmd-h", HideOthers, None),
        KeyBinding::new("cmd-x", Cut, Some(TEXT_INPUT)),
        KeyBinding::new("cmd-c", Copy, Some(TEXT_INPUT)),
        KeyBinding::new("cmd-v", Paste, Some(TEXT_INPUT)),
        KeyBinding::new("cmd-a", SelectAll, Some(TEXT_INPUT)),
    ]
}

/// Put the menu bar and its shortcuts in place (macOS only).
pub fn install(cx: &mut App) {
    if !cfg!(target_os = "macos") {
        return;
    }
    cx.on_action(|_: &Quit, _| std::process::exit(0));
    cx.on_action(|_: &About, _| crate::send_command(crate::Command::Tray(crate::tray::TrayAction::Acknowledgements)));
    cx.on_action(|_: &Hide, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());
    cx.on_action(|_: &ShowAll, cx| cx.unhide_other_apps());
    cx.on_action(|_: &CloseWindow, cx| {
        // The key window, else the only one (a shortcut can land before macOS
        // makes the window key).
        if let Some(window) = cx.active_window().or_else(|| cx.windows().first().copied()) {
            window.update(cx, |_, window, _| window.remove_window()).ok();
        }
    });
    cx.on_action(|_: &Minimize, cx| {
        if let Some(window) = cx.active_window() {
            window.update(cx, |_, window, _| window.minimize_window()).ok();
        }
    });
    // The bindings come first: the menus show the shortcuts they find.
    cx.bind_keys(bindings());
    cx.set_menus(menus());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_menu_bar_has_what_a_mac_app_has() {
        let menus = menus();
        let names: Vec<_> = menus.iter().map(|m| m.name.to_string()).collect();
        assert_eq!(names, ["Morpheus", "Edit", "Window"]);
        let items = |menu: &Menu| -> Vec<String> {
            menu.items
                .iter()
                .filter_map(|item| match item {
                    MenuItem::Action { name, .. } => Some(name.to_string()),
                    _ => None,
                })
                .collect()
        };
        assert!(items(&menus[0]).contains(&"Quit Morpheus".to_owned()));
        assert_eq!(items(&menus[1]), ["Cut", "Copy", "Paste", "Select All"]);
        assert_eq!(items(&menus[2]), ["Minimize", "Close"]);
        // Every shortcut parses (KeyBinding::new panics on a bad one).
        assert_eq!(bindings().len(), 9);
    }
}
