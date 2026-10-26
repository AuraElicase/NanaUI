//! Native ownership of independently movable auxiliary top-level windows.
//!
//! The host retains this binding until before either native window is destroyed.
//! It never activates a window or requests topmost independently of its owner.

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

/// Native association failure, reported by the window host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuxiliaryWindowError(pub String);

impl std::fmt::Display for AuxiliaryWindowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "auxiliary window association failed: {}", self.0)
    }
}

impl std::error::Error for AuxiliaryWindowError {}

/// An auxiliary window's native relationship to its owner.
pub struct AuxiliaryWindow {
    inner: platform::Binding,
}

impl AuxiliaryWindow {
    /// Associate two live windows on the host thread, before the window is
    /// first shown. On Windows this installs the owner HWND.
    pub fn attach<P, W>(parent: &P, window: &W) -> Result<Self, AuxiliaryWindowError>
    where
        P: HasWindowHandle + HasDisplayHandle + ?Sized,
        W: HasWindowHandle + HasDisplayHandle + ?Sized,
    {
        platform::Binding::attach(parent, window).map(|inner| Self { inner })
    }

    /// Associate Wayland toplevels obtained from the window backend.
    ///
    /// # Safety
    /// Both pointers must be live `xdg_toplevel` proxies on the same display,
    /// used only on the host thread, and outlive this binding. It neither owns
    /// nor dispatches nor destroys the backend's proxies.
    #[cfg(target_os = "linux")]
    pub unsafe fn attach_wayland(
        parent: std::ptr::NonNull<std::ffi::c_void>,
        window: std::ptr::NonNull<std::ffi::c_void>,
    ) -> Result<Self, AuxiliaryWindowError> {
        // SAFETY: forwarded unchanged from the caller's contract.
        unsafe { platform::Binding::wayland(parent, window) }.map(|inner| Self { inner })
    }

    /// Reconcile ownership after visibility changes. A hidden startup owner
    /// must not prevent an independently shown preparation window appearing.
    /// Wayland loses its relationship when the owner is unmapped; the host
    /// calls this again after the owner's first presentation on each show.
    /// Topmost arguments are the host's effective live levels. On Windows,
    /// reconcile native bits when ownership/style work changed them behind
    /// the backend's cached flags; do not activate or reorder matching bands.
    pub fn sync(&self, parent_mapped: bool, parent_topmost: bool, topmost: bool) {
        self.inner.sync(parent_mapped, parent_topmost, topmost);
    }

    /// Set the owner's effective policy before native operations. `None`
    /// leaves the band to the fullscreen backend; ownership stays attached.
    pub fn set_parent_topmost(&self, topmost: Option<bool>) {
        self.inner.set_parent_topmost(topmost);
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use raw_window_handle::RawWindowHandle;
    use std::{cell::Cell, rc::Rc};
    use windows_sys::Win32::Foundation::HWND;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GW_OWNER, GWL_EXSTYLE, GWLP_HWNDPARENT, GetWindow, GetWindowLongPtrW, HWND_NOTOPMOST,
        HWND_TOPMOST, STYLESTRUCT, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
        SetWindowLongPtrW, SetWindowPos, WINDOWPOS, WM_NCDESTROY, WM_STYLECHANGING,
        WM_WINDOWPOSCHANGING, WS_EX_APPWINDOW, WS_EX_TOPMOST,
    };

    /// Subclass identity for the owned window's extended-style guard.
    const OWNED_STYLE_SUBCLASS_ID: usize = 0x4E_41_4F_57;

    /// Keeps an owned window off the taskbar. winit writes `WS_EX_APPWINDOW`
    /// back from its own flags on every style change, and that bit gives an
    /// owned window a taskbar button. Applied after the chain, like the frame
    /// guard in `chrome.rs`; removes itself on `WM_NCDESTROY`.
    unsafe extern "system" fn owned_style_proc(
        window: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
        id: usize,
        _data: usize,
    ) -> isize {
        if message == WM_NCDESTROY {
            unsafe {
                RemoveWindowSubclass(window, Some(owned_style_proc), id);
            }
        }
        let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
        if message == WM_STYLECHANGING && wparam == GWL_EXSTYLE as usize && lparam != 0 {
            // SAFETY: `WM_STYLECHANGING` passes a `STYLESTRUCT` the handler
            // edits in place; the sender owns it for the call.
            let style = unsafe { &mut *(lparam as *mut STYLESTRUCT) };
            style.styleNew &= !WS_EX_APPWINDOW;
        }
        result
    }

    /// Make `parent` the owner of the still hidden `window`.
    ///
    /// The owner is not handed to winit at creation: its Win32 backend turns
    /// every owned window into `WS_POPUP`, which drops the system caption,
    /// adds `WS_EX_NOACTIVATE` and reads positions relative to the owner's
    /// client area (`GetParent` answers the owner of a popup). Set after
    /// creation, the window stays an ordinary overlapped top-level window.
    fn install_owner(parent: HWND, window: HWND) -> Result<(), AuxiliaryWindowError> {
        // SAFETY: both HWNDs belong to live windows on the host thread.
        unsafe {
            if SetWindowSubclass(window, Some(owned_style_proc), OWNED_STYLE_SUBCLASS_ID, 0) == 0 {
                return Err(AuxiliaryWindowError(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
            SetWindowLongPtrW(window, GWLP_HWNDPARENT, parent as isize);
            let ex_style = GetWindowLongPtrW(window, GWL_EXSTYLE);
            if ex_style & WS_EX_APPWINDOW as isize != 0 {
                SetWindowLongPtrW(window, GWL_EXSTYLE, ex_style & !(WS_EX_APPWINDOW as isize));
            }
            if GetWindow(window, GW_OWNER) != parent {
                return Err(AuxiliaryWindowError(
                    "native owner was not installed".into(),
                ));
            }
        }
        Ok(())
    }

    pub(super) struct Binding {
        parent: HWND,
        window: HWND,
        guard: Rc<LevelGuard>,
    }

    struct LevelGuard {
        expected: Cell<Option<bool>>,
        attached: Cell<bool>,
    }

    unsafe extern "system" fn level_proc(
        window: HWND,
        message: u32,
        wparam: usize,
        lparam: isize,
        id: usize,
        data: usize,
    ) -> isize {
        let pointer = data as *const LevelGuard;
        // SAFETY: the binding retains this Rc while installed. Hold a local
        // reference before forwarding: another handler can close the child
        // and release its binding synchronously inside DefSubclassProc.
        let guard = unsafe {
            Rc::increment_strong_count(pointer);
            Rc::from_raw(pointer)
        };
        if message == WM_NCDESTROY {
            guard.attached.set(false);
            unsafe {
                RemoveWindowSubclass(window, Some(level_proc), id);
            }
        }
        let result = unsafe { DefSubclassProc(window, message, wparam, lparam) };
        // Win32 resolves HWND_TOPMOST to an actual insert-after window before
        // this message. With hidden IME windows in an owned group, that window
        // can be non-topmost and the call succeeds without changing the band.
        // Preserve the host's explicit policy at the native commit point. Do
        // not alter matching bands, move-only work or fullscreen-owned policy.
        if message == WM_WINDOWPOSCHANGING
            && guard.attached.get()
            && guard.expected.get() == Some(true)
            && unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST == 0
        {
            let pos = unsafe { &mut *(lparam as *mut WINDOWPOS) };
            if pos.flags & SWP_NOZORDER == 0 {
                pos.hwndInsertAfter = HWND_TOPMOST;
            }
        }
        result
    }

    impl Binding {
        pub(super) fn attach<P, W>(parent: &P, window: &W) -> Result<Self, AuxiliaryWindowError>
        where
            P: HasWindowHandle + HasDisplayHandle + ?Sized,
            W: HasWindowHandle + HasDisplayHandle + ?Sized,
        {
            let parent = parent
                .window_handle()
                .map_err(|e| AuxiliaryWindowError(e.to_string()))?;
            let window = window
                .window_handle()
                .map_err(|e| AuxiliaryWindowError(e.to_string()))?;
            let (RawWindowHandle::Win32(parent), RawWindowHandle::Win32(window)) =
                (parent.as_raw(), window.as_raw())
            else {
                return Err(AuxiliaryWindowError("expected HWNDs".into()));
            };
            let parent = parent.hwnd.get() as HWND;
            let window = window.hwnd.get() as HWND;
            install_owner(parent, window)?;
            let actual =
                unsafe { GetWindowLongPtrW(parent, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0;
            let guard = Rc::new(LevelGuard {
                expected: Cell::new(Some(actual)),
                attached: Cell::new(true),
            });
            if unsafe {
                SetWindowSubclass(
                    parent,
                    Some(level_proc),
                    window as usize,
                    Rc::as_ptr(&guard) as usize,
                )
            } == 0
            {
                return Err(AuxiliaryWindowError(
                    std::io::Error::last_os_error().to_string(),
                ));
            }
            Ok(Self {
                parent,
                window,
                guard,
            })
        }

        pub(super) fn sync(&self, _parent_mapped: bool, parent_topmost: bool, topmost: bool) {
            // Win32 ownership and native style replay can change the owner's
            // band without updating winit's cached flags. Reconcile the actual
            // bits after the native operation; never toggle cached flags or
            // activate a window to force an update. Each call is idempotent.
            if self.guard.attached.get() && self.guard.expected.get().is_some() {
                self.guard.expected.set(Some(parent_topmost));
                conform_band(self.parent, parent_topmost);
                conform_band(self.window, topmost);
            }
        }

        pub(super) fn set_parent_topmost(&self, topmost: Option<bool>) {
            self.guard.expected.set(topmost);
        }
    }

    impl Drop for Binding {
        fn drop(&mut self) {
            if self.guard.attached.replace(false) {
                unsafe {
                    RemoveWindowSubclass(self.parent, Some(level_proc), self.window as usize)
                };
            }
        }
    }

    fn conform_band(window: HWND, topmost: bool) {
        // SAFETY: the host releases this binding before either live HWND.
        let actual = unsafe { GetWindowLongPtrW(window, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0;
        if actual != topmost {
            unsafe {
                SetWindowPos(
                    window,
                    if topmost {
                        HWND_TOPMOST
                    } else {
                        HWND_NOTOPMOST
                    },
                    0,
                    0,
                    0,
                    0,
                    SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
                );
            }
        }
    }

    /// On real HWNDs: ownership is Win32 window-manager state with no model
    /// worth asserting against.
    #[cfg(test)]
    mod tests {
        use super::*;
        use windows_sys::Win32::Foundation::RECT;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DefWindowProcW, DestroyWindow, GWL_STYLE, GetWindowRect,
            RegisterClassW, WNDCLASSW, WS_CAPTION, WS_OVERLAPPEDWINDOW, WS_POPUP, WS_SYSMENU,
        };

        use crate::win32::wide;

        fn fixture_window(class: &[u16], x: i32) -> HWND {
            // SAFETY: a hidden top-level window of the fixture's class, with
            // the taskbar bit winit gives every unowned window.
            let hwnd = unsafe {
                CreateWindowExW(
                    WS_EX_APPWINDOW,
                    class.as_ptr(),
                    wide("fixture").as_ptr(),
                    WS_OVERLAPPEDWINDOW,
                    x,
                    120,
                    200,
                    160,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            assert!(!hwnd.is_null(), "fixture window");
            hwnd
        }

        fn ex_style(hwnd: HWND) -> u32 {
            // SAFETY: `hwnd` is a live fixture window.
            unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32 }
        }

        #[test]
        fn an_owned_window_keeps_its_caption_and_screen_position_and_stays_off_the_taskbar() {
            let class_name = wide("NanaOwnedWindowFixture");
            let class = WNDCLASSW {
                lpfnWndProc: Some(DefWindowProcW),
                lpszClassName: class_name.as_ptr(),
                // SAFETY: all-zero is the documented unset state of WNDCLASSW.
                ..unsafe { std::mem::zeroed() }
            };
            // SAFETY: a plain registration with DefWindowProcW.
            unsafe {
                RegisterClassW(&class);
            }
            let parent = fixture_window(&class_name, 100);
            let window = fixture_window(&class_name, 340);
            let mut before = RECT::default();
            // SAFETY: live window, valid output buffer.
            unsafe { GetWindowRect(window, &mut before) };

            install_owner(parent, window).unwrap();

            // SAFETY: live windows.
            unsafe {
                assert_eq!(GetWindow(window, GW_OWNER), parent);
                let style = GetWindowLongPtrW(window, GWL_STYLE) as u32;
                assert_eq!(style & (WS_CAPTION | WS_SYSMENU), WS_CAPTION | WS_SYSMENU);
                assert_eq!(style & WS_POPUP, 0);
                let mut after = RECT::default();
                GetWindowRect(window, &mut after);
                assert_eq!((after.left, after.top), (before.left, before.top));
            }
            assert_eq!(ex_style(window) & WS_EX_APPWINDOW, 0);

            // winit rewrites the extended style from its own flags.
            // SAFETY: live window.
            unsafe {
                SetWindowLongPtrW(
                    window,
                    GWL_EXSTYLE,
                    (ex_style(window) | WS_EX_APPWINDOW) as isize,
                );
            }
            assert_eq!(ex_style(window) & WS_EX_APPWINDOW, 0);
            // The parent is not owned and keeps its taskbar button.
            assert_ne!(ex_style(parent) & WS_EX_APPWINDOW, 0);

            // SAFETY: destroying the fixture windows, owned one first.
            unsafe {
                DestroyWindow(window);
                DestroyWindow(parent);
            }
        }
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use super::*;
    use objc2::{MainThreadMarker, rc::Retained};
    use objc2_app_kit::{NSView, NSWindow, NSWindowOrderingMode};
    use raw_window_handle::RawWindowHandle;
    use std::cell::Cell;

    pub(super) struct Binding {
        parent: Retained<NSWindow>,
        window: Retained<NSWindow>,
        attached: Cell<bool>,
    }

    fn ns_window<W: HasWindowHandle + ?Sized>(
        window: &W,
    ) -> Result<Retained<NSWindow>, AuxiliaryWindowError> {
        let handle = window
            .window_handle()
            .map_err(|e| AuxiliaryWindowError(e.to_string()))?;
        let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
            return Err(AuxiliaryWindowError("expected an AppKit window".into()));
        };
        // SAFETY: the live handle contains an NSView on the main thread.
        let view = unsafe { Retained::retain(handle.ns_view.as_ptr().cast::<NSView>()) }
            .ok_or_else(|| AuxiliaryWindowError("missing NSView".into()))?;
        view.window()
            .ok_or_else(|| AuxiliaryWindowError("missing NSWindow".into()))
    }

    impl Binding {
        pub(super) fn set_parent_topmost(&self, _topmost: Option<bool>) {}
        pub(super) fn attach<P, W>(parent: &P, window: &W) -> Result<Self, AuxiliaryWindowError>
        where
            P: HasWindowHandle + HasDisplayHandle + ?Sized,
            W: HasWindowHandle + HasDisplayHandle + ?Sized,
        {
            MainThreadMarker::new()
                .ok_or_else(|| AuxiliaryWindowError("not on main thread".into()))?;
            Ok(Self {
                parent: ns_window(parent)?,
                window: ns_window(window)?,
                attached: Cell::new(false),
            })
        }

        pub(super) fn sync(&self, parent_mapped: bool, _parent_topmost: bool, _topmost: bool) {
            if parent_mapped && !self.attached.replace(true) {
                // SAFETY: the host supplies an acyclic parent tree and both
                // retained windows remain live on the main thread.
                unsafe {
                    self.parent
                        .addChildWindow_ordered(&self.window, NSWindowOrderingMode::Above)
                };
            } else if !parent_mapped && self.attached.replace(false) {
                self.parent.removeChildWindow(&self.window);
            }
        }
    }

    impl Drop for Binding {
        fn drop(&mut self) {
            if self.attached.get() {
                self.parent.removeChildWindow(&self.window);
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use raw_window_handle::{RawDisplayHandle, RawWindowHandle};
    use std::{cell::Cell, ffi::c_void, ptr::NonNull};

    pub(super) enum Binding {
        X11,
        Wayland {
            parent: NonNull<c_void>,
            window: NonNull<c_void>,
            mapped: Cell<Option<bool>>,
            library: &'static wayland_sys::client::WaylandClient,
        },
    }

    impl Binding {
        pub(super) fn set_parent_topmost(&self, _topmost: Option<bool>) {}
        pub(super) fn attach<P, W>(parent: &P, window: &W) -> Result<Self, AuxiliaryWindowError>
        where
            P: HasWindowHandle + HasDisplayHandle + ?Sized,
            W: HasWindowHandle + HasDisplayHandle + ?Sized,
        {
            let parent = parent
                .window_handle()
                .map_err(|e| AuxiliaryWindowError(e.to_string()))?;
            let handle = window
                .window_handle()
                .map_err(|e| AuxiliaryWindowError(e.to_string()))?;
            let display = window
                .display_handle()
                .map_err(|e| AuxiliaryWindowError(e.to_string()))?;
            let (
                RawWindowHandle::Xlib(parent),
                RawWindowHandle::Xlib(window),
                RawDisplayHandle::Xlib(display),
            ) = (parent.as_raw(), handle.as_raw(), display.as_raw())
            else {
                return Err(AuxiliaryWindowError(
                    "expected Xlib handles; Wayland requires xdg_toplevel proxies".into(),
                ));
            };
            let display = display
                .display
                .ok_or_else(|| AuxiliaryWindowError("missing X display".into()))?;
            let xlib =
                x11_dl::xlib::Xlib::open().map_err(|e| AuxiliaryWindowError(e.to_string()))?;
            // SAFETY: handles belong to live windows on this X display.
            if unsafe {
                (xlib.XSetTransientForHint)(display.as_ptr().cast(), window.window, parent.window)
            } == 0
            {
                return Err(AuxiliaryWindowError("XSetTransientForHint failed".into()));
            }
            // Flush without a round trip or waiting on the window manager.
            unsafe { (xlib.XFlush)(display.as_ptr().cast()) };
            Ok(Self::X11)
        }

        pub(super) unsafe fn wayland(
            parent: NonNull<c_void>,
            window: NonNull<c_void>,
        ) -> Result<Self, AuxiliaryWindowError> {
            let library = wayland_sys::client::wayland_client_option()
                .ok_or_else(|| AuxiliaryWindowError("libwayland-client unavailable".into()))?;
            Ok(Self::Wayland {
                parent,
                window,
                mapped: Cell::new(None),
                library,
            })
        }

        pub(super) fn sync(&self, parent_mapped: bool, _parent_topmost: bool, _topmost: bool) {
            if let Self::Wayland {
                parent,
                window,
                mapped,
                library,
            } = self
            {
                if mapped.replace(Some(parent_mapped)) == Some(parent_mapped) {
                    return;
                }
                let mut argument = wayland_sys::common::wl_argument {
                    o: if parent_mapped {
                        parent.as_ptr()
                    } else {
                        std::ptr::null_mut()
                    },
                };
                // SAFETY: borrowed xdg_toplevel proxies remain live. Opcode 1
                // is xdg_toplevel.set_parent(?object), since xdg-shell v1.
                // Only marshal a request; never replace listeners or dispatch
                // the event queue owned by winit.
                unsafe {
                    (library.wl_proxy_marshal_array)(window.as_ptr().cast(), 1, &mut argument)
                };
            }
        }
    }

    impl Drop for Binding {
        fn drop(&mut self) {
            self.sync(false, false, false);
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub(super) struct Binding;
    impl Binding {
        pub(super) fn set_parent_topmost(&self, _topmost: Option<bool>) {}
        pub(super) fn attach<P, W>(_parent: &P, _window: &W) -> Result<Self, AuxiliaryWindowError>
        where
            P: HasWindowHandle + HasDisplayHandle + ?Sized,
            W: HasWindowHandle + HasDisplayHandle + ?Sized,
        {
            Err(AuxiliaryWindowError("unsupported platform".into()))
        }
        pub(super) fn sync(&self, _parent_mapped: bool, _parent_topmost: bool, _topmost: bool) {}
    }
}
