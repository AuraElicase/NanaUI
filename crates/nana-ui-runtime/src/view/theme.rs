//! Reactive access to the installed theme for derived product geometry.
use super::{Signal, on_mount, signal};
use nana_ui_core::{CompiledTheme, ThemeAppearance, builtin_theme_arc};
use std::sync::Arc;

/// The installed theme, initialized on mount and updated on each theme install.
/// Use typed layout tokens for ordinary styles; this signal is for derived
/// geometry such as column counts and cover aspect ratios. Disposed scopes
/// automatically stop receiving updates.
pub fn theme_signal() -> Signal<Arc<CompiledTheme>> {
    let value = signal(builtin_theme_arc(ThemeAppearance::Dark));
    on_mount(move |cx| {
        value.set_if_changed(cx.world().installed_theme_arc());
        cx.theme_signals.push(value);
    });
    value
}
