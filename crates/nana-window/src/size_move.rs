//! Live window-resize session. Windows posts `WM_ENTERSIZEMOVE` /
//! `WM_EXITSIZEMOVE` around the nested size-move loop, and `WM_SIZING` for
//! every step of a user resize inside it.

use raw_window_handle::HasWindowHandle;
use std::sync::Arc;

#[cfg(any(target_os = "windows", test))]
const ENTER_SIZE_MOVE: u32 = 0x0231;
#[cfg(any(target_os = "windows", test))]
const EXIT_SIZE_MOVE: u32 = 0x0232;

#[cfg(any(target_os = "windows", test))]
pub(crate) fn size_move_active_after(message: u32, was_active: bool) -> bool {
    match message {
        ENTER_SIZE_MOVE => true,
        EXIT_SIZE_MOVE => false,
        _ => was_active,
    }
}

/// Host-owned observer for the platform live-resize session.
pub struct LiveSizeMove {
    #[cfg(target_os = "windows")]
    inner: windows_hook::Hook,
}

impl LiveSizeMove {
    pub fn install<W: HasWindowHandle + ?Sized>(window: &W) -> Result<Self, String> {
        Self::install_inner(window, None)
    }

    /// Keep host scheduling alive during Windows' nested native move/resize
    /// loop. `wake` must only signal the host's event source, never run host
    /// callbacks inline: native messages can arrive inside a borrowed handler.
    /// The wake source is used only during the operation; other platforms ignore it.
    pub fn install_with_wake<W: HasWindowHandle + ?Sized>(
        window: &W,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self, String> {
        Self::install_inner(window, Some(wake))
    }

    fn install_inner<W: HasWindowHandle + ?Sized>(
        window: &W,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self, String> {
        #[cfg(not(target_os = "windows"))]
        let _ = (window, wake);
        Ok(Self {
            #[cfg(target_os = "windows")]
            inner: windows_hook::Hook::install(window, wake)?,
        })
    }

    pub fn is_active(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            self.inner.is_active()
        }
        #[cfg(not(target_os = "windows"))]
        {
            false
        }
    }

    /// Keep the client area at `ratio` (width over height, finite and
    /// positive) while the user drags a frame edge, never below the window's
    /// minimum size on either edge. `None` releases the lock. Covers both the
    /// platform's own size-move loop and [`crate::LiveFrameResize`]; a
    /// maximized window is left alone. Programmatic resizes and a minimum
    /// with this ratio are the host's to provide.
    ///
    /// Windows only; other platforms ignore it.
    pub fn set_content_aspect_ratio(&self, ratio: Option<f64>) {
        debug_assert!(ratio.is_none_or(|ratio| ratio.is_finite() && ratio > 0.0));
        #[cfg(target_os = "windows")]
        self.inner
            .set_aspect(ratio.map(|ratio| crate::aspect::AspectLock { ratio }));
        #[cfg(not(target_os = "windows"))]
        let _ = ratio;
    }
}

#[cfg(target_os = "windows")]
mod windows_hook {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsZoomed, KillTimer, SetTimer, WM_ENTERSIZEMOVE,
        WM_EXITSIZEMOVE, WM_NCDESTROY, WM_SETTINGCHANGE, WM_SIZING, WM_TIMER,
    };

    use crate::aspect::{AspectLock, SizingBasis};
    use crate::win32::WindowProp;

    const SUBCLASS_ID: usize = 0x4E_41_53_4D;
    const WAKE_TIMER: usize = SUBCLASS_ID;
    const WAKE_INTERVAL_MS: u32 = 16;

    /// Holds the installed `HookState` for readers outside the window
    /// procedure.
    const STATE_PROP: WindowProp = WindowProp::new("NanaUI.SizeMove");

    struct HookState {
        active: AtomicBool,
        wake: Option<Arc<dyn Fn() + Send + Sync>>,
        aspect: Cell<Option<AspectLock>>,
        /// Measured when the current size-move loop began, for a window with
        /// an aspect lock.
        basis: Cell<Option<SizingBasis>>,
    }

    pub(super) struct Hook {
        hwnd: HWND,
        state: *mut HookState,
    }

    impl Hook {
        pub(super) fn install<W: HasWindowHandle + ?Sized>(
            window: &W,
            wake: Option<Arc<dyn Fn() + Send + Sync>>,
        ) -> Result<Self, String> {
            let handle = window
                .window_handle()
                .map_err(|error| format!("failed to acquire Win32 window handle: {error}"))?;
            let RawWindowHandle::Win32(handle) = handle.as_raw() else {
                return Err("live size-move requires an HWND".into());
            };
            let hwnd = handle.hwnd.get() as *mut c_void;
            let state = Box::into_raw(Box::new(HookState {
                active: AtomicBool::new(false),
                wake,
                aspect: Cell::new(None),
                basis: Cell::new(None),
            }));
            // SAFETY: hwnd belongs to the live winit window. `state` remains
            // allocated until the subclass is removed by `Drop`.
            let installed = unsafe {
                SetWindowSubclass(hwnd, Some(size_move_proc), SUBCLASS_ID, state as usize)
            };
            if installed == 0 {
                // SAFETY: ownership was not transferred because install failed.
                unsafe { drop(Box::from_raw(state)) };
                return Err("SetWindowSubclass failed for live size-move".into());
            }
            // The property is removed before `state` is freed.
            if !STATE_PROP.set(hwnd, state as usize) {
                // SAFETY: the subclass was just installed with this state.
                if unsafe { RemoveWindowSubclass(hwnd, Some(size_move_proc), SUBCLASS_ID) } != 0 {
                    unsafe { drop(Box::from_raw(state)) };
                }
                return Err("SetPropW failed for live size-move".into());
            }
            Ok(Self { hwnd, state })
        }

        pub(super) fn is_active(&self) -> bool {
            // SAFETY: the hook owns `state` for its full lifetime.
            unsafe { &*self.state }.active.load(Ordering::Acquire)
        }

        pub(super) fn set_aspect(&self, lock: Option<AspectLock>) {
            // SAFETY: the hook owns `state`; the window procedure that also
            // reads it runs on this same thread.
            unsafe { &*self.state }.aspect.set(lock);
        }
    }

    impl Drop for Hook {
        fn drop(&mut self) {
            // SAFETY: a timer belongs to this HWND; removing it does not dispatch callbacks.
            unsafe { KillTimer(self.hwnd, WAKE_TIMER) };
            // Readers of the property stop seeing `state` before it is freed.
            STATE_PROP.remove(self.hwnd);
            // SAFETY: dropped before the winit Window. Successful removal
            // guarantees the callback can no longer observe `state`.
            let removed =
                unsafe { RemoveWindowSubclass(self.hwnd, Some(size_move_proc), SUBCLASS_ID) };
            if removed != 0 {
                unsafe { drop(Box::from_raw(self.state)) };
            }
        }
    }

    /// The aspect lock installed on `hwnd`. `None` for a window without the
    /// hook or without a lock, and for a maximized window, which keeps
    /// whatever the platform gave it.
    pub(crate) fn aspect_lock(hwnd: HWND) -> Option<AspectLock> {
        let data = STATE_PROP.get(hwnd) as *const HookState;
        if data.is_null() {
            return None;
        }
        // SAFETY: the property holds the live HookState until `Hook::drop`
        // removes it, before the state is freed.
        windowed(hwnd, unsafe { &*data }.aspect.get()?)
    }

    /// `lock`, unless `hwnd` is maximized.
    fn windowed(hwnd: HWND, lock: AspectLock) -> Option<AspectLock> {
        // SAFETY: a plain query on a live window.
        (unsafe { IsZoomed(hwnd) } == 0).then_some(lock)
    }

    /// `hwnd`'s frame, client size and minimum client size in physical
    /// pixels, given its minimum track (outer) size.
    pub(crate) fn sizing_basis(hwnd: HWND, min_track: (f64, f64)) -> Option<SizingBasis> {
        let (mut outer, mut client) = (RECT::default(), RECT::default());
        // SAFETY: plain queries on a live window.
        if unsafe { GetWindowRect(hwnd, &mut outer) } == 0
            || unsafe { GetClientRect(hwnd, &mut client) } == 0
        {
            return None;
        }
        let reference = (
            f64::from(client.right - client.left),
            f64::from(client.bottom - client.top),
        );
        let frame = (
            f64::from(outer.right - outer.left) - reference.0,
            f64::from(outer.bottom - outer.top) - reference.1,
        );
        Some(SizingBasis {
            frame,
            reference,
            minimum: (
                (min_track.0 - frame.0).max(0.0),
                (min_track.1 - frame.1).max(0.0),
            ),
        })
    }

    fn measure(hwnd: HWND) -> Option<SizingBasis> {
        sizing_basis(hwnd, crate::chrome::win32_min_track_size(hwnd))
    }

    /// Bring a `WM_SIZING` drag rectangle to the locked ratio. Answers
    /// whether it did.
    fn constrain_sizing(hwnd: HWND, state: &HookState, wparam: WPARAM, lparam: LPARAM) -> bool {
        let Some(lock) = state.aspect.get() else {
            return false;
        };
        let Some(edge) = crate::aspect::sizing_edge(wparam) else {
            return false;
        };
        let Some(lock) = windowed(hwnd, lock) else {
            return false;
        };
        // A lock set during the loop has no basis yet.
        let Some(basis) = state.basis.get().or_else(|| measure(hwnd)) else {
            return false;
        };
        // SAFETY: WM_SIZING's lParam is the drag rectangle, writable.
        let rect = unsafe { &mut *(lparam as *mut RECT) };
        let next = lock.constrain_frame(
            [
                f64::from(rect.left),
                f64::from(rect.top),
                f64::from(rect.right),
                f64::from(rect.bottom),
            ],
            edge,
            basis,
        );
        *rect = RECT {
            left: next[0] as i32,
            top: next[1] as i32,
            right: next[2] as i32,
            bottom: next[3] as i32,
        };
        true
    }

    unsafe extern "system" fn size_move_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _subclass_id: usize,
        ref_data: usize,
    ) -> LRESULT {
        // Every host window carries this subclass, so it also observes the
        // system setting broadcasts NanaUI reports to programs.
        if message == WM_SETTINGCHANGE {
            crate::motion_preference::observe_setting_change(wparam);
            crate::contrast_preference::observe_setting_change(wparam);
        }
        if matches!(message, WM_ENTERSIZEMOVE | WM_EXITSIZEMOVE) {
            // SAFETY: ref_data is the live HookState installed with this subclass.
            let state = unsafe { &*(ref_data as *const HookState) };
            let active =
                super::size_move_active_after(message, state.active.load(Ordering::Acquire));
            state.active.store(active, Ordering::Release);
            let basis = if active && state.aspect.get().is_some() {
                measure(hwnd)
            } else {
                None
            };
            state.basis.set(basis);
            if let Some(wake) = &state.wake {
                // The outer event loop's waitable timer is not serviced by the
                // nested DefWindowProc loop. WM_TIMER is, even with a stationary pointer.
                // SAFETY: this is the live subclassed HWND on its window thread.
                unsafe {
                    if active {
                        SetTimer(hwnd, WAKE_TIMER, WAKE_INTERVAL_MS, None);
                    } else {
                        KillTimer(hwnd, WAKE_TIMER);
                    }
                }
                wake();
            }
            // SAFETY: hwnd is the subclassed live window.
            unsafe { InvalidateRect(hwnd, ptr::null(), 0) };
        }
        if message == WM_TIMER && wparam == WAKE_TIMER {
            // SAFETY: ref_data belongs to this installed subclass.
            let state = unsafe { &*(ref_data as *const HookState) };
            if state.active.load(Ordering::Acquire)
                && let Some(wake) = &state.wake
            {
                wake();
            }
            return 0;
        }
        if message == WM_NCDESTROY {
            // SAFETY: remove the timer before HWND reuse, even if the owner drops later.
            unsafe { KillTimer(hwnd, WAKE_TIMER) };
        }
        if message == WM_SIZING {
            // winit snaps to resize increments first; the ratio has the last
            // word, because Windows applies the rectangle as it comes back.
            // SAFETY: forwards to winit's original window procedure.
            let result = unsafe { DefSubclassProc(hwnd, message, wparam, lparam) };
            // SAFETY: ref_data is the live HookState installed with this subclass.
            let state = unsafe { &*(ref_data as *const HookState) };
            return if constrain_sizing(hwnd, state, wparam, lparam) {
                1
            } else {
                result
            };
        }
        // SAFETY: forwards every message to winit's original window procedure.
        unsafe { DefSubclassProc(hwnd, message, wparam, lparam) }
    }
}

#[cfg(target_os = "windows")]
pub(crate) use windows_hook::{aspect_lock, sizing_basis};

#[cfg(test)]
mod tests {
    use super::{ENTER_SIZE_MOVE, EXIT_SIZE_MOVE, size_move_active_after};

    #[test]
    fn enter_and_exit_messages_toggle_the_live_session() {
        assert!(size_move_active_after(ENTER_SIZE_MOVE, false));
        assert!(!size_move_active_after(EXIT_SIZE_MOVE, true));
        assert!(size_move_active_after(0x0005, true));
        assert!(!size_move_active_after(0x0005, false));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn native_timer_wakes_only_during_the_session_and_stops_when_dropped() {
        use raw_window_handle::{
            HandleError, HasWindowHandle, RawWindowHandle, Win32WindowHandle, WindowHandle,
        };
        use std::num::NonZeroIsize;
        use std::ptr;
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };
        use std::time::{Duration, Instant};
        use windows_sys::Win32::Foundation::HWND;
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DestroyWindow, DispatchMessageW, MSG, PM_REMOVE, PeekMessageW,
            SendMessageW, WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WS_OVERLAPPEDWINDOW,
        };

        struct Window(HWND);
        impl HasWindowHandle for Window {
            fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
                let raw = Win32WindowHandle::new(NonZeroIsize::new(self.0 as isize).unwrap());
                // SAFETY: this fixture owns the HWND until Drop on the same thread.
                Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::Win32(raw)) })
            }
        }
        impl Drop for Window {
            fn drop(&mut self) {
                // SAFETY: the fixture's own hidden window, on its creating thread.
                unsafe { DestroyWindow(self.0) };
            }
        }
        fn pump(hwnd: HWND) {
            let until = Instant::now() + Duration::from_millis(80);
            while Instant::now() < until {
                let mut message = MSG::default();
                // SAFETY: the message and fixture window remain live through dispatch.
                unsafe {
                    while PeekMessageW(&mut message, hwnd, 0, 0, PM_REMOVE) != 0 {
                        DispatchMessageW(&message);
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }

        // SAFETY: STATIC is a system window class; all pointers are valid for this call.
        let window = Window(unsafe {
            CreateWindowExW(
                0,
                crate::win32::wide("STATIC").as_ptr(),
                ptr::null(),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                64,
                64,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            )
        });
        assert!(!window.0.is_null());
        let count = Arc::new(AtomicUsize::new(0));
        let observed = count.clone();
        let hook = super::LiveSizeMove::install_with_wake(
            &window,
            Arc::new(move || {
                observed.fetch_add(1, Ordering::Relaxed);
            }),
        )
        .unwrap();
        // SAFETY: these carry no pointer payload and target the fixture window.
        unsafe { SendMessageW(window.0, WM_ENTERSIZEMOVE, 0, 0) };
        assert!(hook.is_active());
        let entered = count.load(Ordering::Relaxed);
        pump(window.0);
        assert!(
            count.load(Ordering::Relaxed) > entered,
            "stationary native loop must wake"
        );
        // SAFETY: same fixture and pointer-free native message.
        unsafe { SendMessageW(window.0, WM_EXITSIZEMOVE, 0, 0) };
        assert!(!hook.is_active());
        let exited = count.load(Ordering::Relaxed);
        pump(window.0);
        assert_eq!(count.load(Ordering::Relaxed), exited, "no timer after exit");
        // SAFETY: same fixture and pointer-free native message.
        unsafe { SendMessageW(window.0, WM_ENTERSIZEMOVE, 0, 0) };
        pump(window.0);
        let dropped = count.load(Ordering::Relaxed);
        drop(hook);
        pump(window.0);
        assert_eq!(
            count.load(Ordering::Relaxed),
            dropped,
            "no callback after hook removal"
        );
        assert_eq!(Arc::strong_count(&count), 1, "wake callback was released");
    }
}
