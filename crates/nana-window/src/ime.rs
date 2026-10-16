//! Cancellation of the platform's composition buffer, separate from the
//! host adapter's IME enable state.

/// Discard the text client's and AppKit's pending marked text without committing it.
///
/// Call on the window event thread when cancelling preedit, before any
/// required IME disable. Clearing winit's marked-text buffer alone does not discard the
/// input method's own buffer: Pinyin can otherwise resume invisible text
/// after a window switch. Other backends cancel through their IME disable
/// request and need no additional native operation here.
pub fn discard_native_ime_preedit<W: raw_window_handle::HasWindowHandle + ?Sized>(window: &W) {
    #[cfg(target_os = "macos")]
    {
        use objc2::runtime::NSObjectProtocol;
        use objc2::{msg_send, sel};
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
        // Focus notifications are queued: another window may already own
        // the current conversion session when we drain the old one's loss.
        // Never discard through an inactive client. A Disable can clear
        // its local buffer; its input context resets when it regains key.
        if !view.window().is_some_and(|window| window.isKeyWindow()) {
            return;
        }
        if view.respondsToSelector(sel!(unmarkText)) {
            // SAFETY: NSTextInputClient's unmarkText takes no arguments and
            // returns void. Winit's implementation first clears its marked
            // range, then calls discardMarkedText on the input context and
            // returns its IME state to Ground. Discarding only the context
            // leaves the client and input method disagreeing about the range.
            let _: () = unsafe { msg_send![view, unmarkText] };
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window;
}
