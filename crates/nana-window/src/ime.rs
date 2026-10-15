//! Cancellation of the platform's composition buffer, separate from the
//! host adapter's IME enable state.

/// Discard AppKit's pending marked text without committing it.
///
/// Call on the window event thread, before disabling or replacing the IME
/// session. Clearing winit's marked-text buffer alone does not discard the
/// input method's own buffer: Pinyin can otherwise resume invisible text
/// after a window switch. Other backends cancel through their IME disable
/// request and need no additional native operation here.
pub fn discard_native_ime_preedit<W: raw_window_handle::HasWindowHandle + ?Sized>(window: &W) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSView;
        use raw_window_handle::RawWindowHandle;

        let Ok(handle) = window.window_handle() else {
            return;
        };
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return;
        };
        // SAFETY: the live AppKit window owns this view; the host calls us
        // on its event thread, outside key/composition delivery.
        let view = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
        if let Some(context) = view.inputContext() {
            context.discardMarkedText();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}
