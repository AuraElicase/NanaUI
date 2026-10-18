//! Native ownership of independently movable auxiliary top-level windows.
//!
//! The host retains this binding until before either native window is destroyed.
//! It never activates a window or promotes it to the desktop's topmost band.

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
    /// Associate two live windows on the host thread. Windows ownership must
    /// already have been specified in the creation attributes.
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
    pub fn sync(&self, parent_mapped: bool) {
        self.inner.sync(parent_mapped);
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use super::*;
    use raw_window_handle::RawWindowHandle;
    use windows_sys::Win32::UI::WindowsAndMessaging::{GW_OWNER, GetWindow};

    pub(super) struct Binding;

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
            // SAFETY: both HWNDs come from live windows on the host thread.
            if unsafe { GetWindow(window.hwnd.get() as _, GW_OWNER) } != parent.hwnd.get() as _ {
                return Err(AuxiliaryWindowError(
                    "native owner was not installed".into(),
                ));
            }
            Ok(Self)
        }

        pub(super) fn sync(&self, _parent_mapped: bool) {}
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

        pub(super) fn sync(&self, parent_mapped: bool) {
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

        pub(super) fn sync(&self, parent_mapped: bool) {
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
            self.sync(false);
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
mod platform {
    use super::*;
    pub(super) struct Binding;
    impl Binding {
        pub(super) fn attach<P, W>(_parent: &P, _window: &W) -> Result<Self, AuxiliaryWindowError>
        where
            P: HasWindowHandle + HasDisplayHandle + ?Sized,
            W: HasWindowHandle + HasDisplayHandle + ?Sized,
        {
            Err(AuxiliaryWindowError("unsupported platform".into()))
        }
        pub(super) fn sync(&self, _parent_mapped: bool) {}
    }
}
