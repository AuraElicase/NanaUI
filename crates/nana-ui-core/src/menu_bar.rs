//! Application menu bar model.
//!
//! Pure data: what the menus are, what they are called, which key each item
//! answers to. Installing it is platform work and lives in `nana-window`;
//! this is here so the window-command vocabulary can name a menu without
//! dragging AppKit or Win32 into every crate that speaks it.
//!
//! Controls never touch this. An application declares the bar and routes the
//! ids it gets back, the same shape as an action registry.
//!
//! The ids come back through one queue whichever menu showed the bar: the
//! platform's native menu and the runtime's in-app `AppMenuBar` both report
//! with [`push_menu_activation`], and [`take_menu_activations`] drains them.

use std::sync::Mutex;

static ACTIVATIONS: Mutex<Vec<u32>> = Mutex::new(Vec::new());

/// Records a chosen item's id. Called by whatever showed the menu: the
/// platform's menu callback or an in-app menu bar.
pub fn push_menu_activation(id: u32) {
    if let Ok(mut queue) = ACTIVATIONS.lock() {
        queue.push(id);
    }
}

/// Menu ids the user chose since the last call, in order.
///
/// Drain this once a frame and route the ids the way you route any other
/// command. Never empty-loops: an untouched menu yields an empty vec.
pub fn take_menu_activations() -> Vec<u32> {
    ACTIVATIONS
        .lock()
        .map(|mut queue| std::mem::take(&mut *queue))
        .unwrap_or_default()
}

/// A key combination shown beside a menu item.
///
/// Only what a menu needs: the platform draws it and claims the shortcut.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuShortcut {
    /// Single character key, e.g. `"s"`. Case-insensitive.
    pub key: String,
    /// Cmd on macOS, Ctrl elsewhere.
    pub primary: bool,
    pub shift: bool,
    pub alt: bool,
}

impl MenuShortcut {
    /// Primary modifier plus `key` — Cmd+S on macOS, Ctrl+S elsewhere.
    pub fn primary(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            primary: true,
            shift: false,
            alt: false,
        }
    }

    #[must_use]
    pub fn shift(mut self) -> Self {
        self.shift = true;
        self
    }

    #[must_use]
    pub fn alt(mut self) -> Self {
        self.alt = true;
        self
    }
}

/// One entry in a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuEntry {
    /// A command. `id` is what [`take_menu_activations`] reports.
    Item {
        id: u32,
        label: String,
        shortcut: Option<MenuShortcut>,
        enabled: bool,
        /// Draws a check mark. The application owns the state.
        checked: bool,
    },
    Separator,
    Submenu(Menu),
}

impl MenuEntry {
    pub fn item(id: u32, label: impl Into<String>) -> Self {
        Self::Item {
            id,
            label: label.into(),
            shortcut: None,
            enabled: true,
            checked: false,
        }
    }

    #[must_use]
    pub fn shortcut(mut self, shortcut: MenuShortcut) -> Self {
        if let Self::Item { shortcut: slot, .. } = &mut self {
            *slot = Some(shortcut);
        }
        self
    }

    #[must_use]
    pub fn enabled(mut self, value: bool) -> Self {
        if let Self::Item { enabled, .. } = &mut self {
            *enabled = value;
        }
        self
    }

    #[must_use]
    pub fn checked(mut self, value: bool) -> Self {
        if let Self::Item { checked, .. } = &mut self {
            *checked = value;
        }
        self
    }
}

/// A titled list of entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub title: String,
    pub entries: Vec<MenuEntry>,
}

impl Menu {
    pub fn new(title: impl Into<String>, entries: impl IntoIterator<Item = MenuEntry>) -> Self {
        Self {
            title: title.into(),
            entries: entries.into_iter().collect(),
        }
    }
}

/// The whole bar, in order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MenuBar {
    pub menus: Vec<Menu>,
}

impl MenuBar {
    pub fn new(menus: impl IntoIterator<Item = Menu>) -> Self {
        Self {
            menus: menus.into_iter().collect(),
        }
    }
}
