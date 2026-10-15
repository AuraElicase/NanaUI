//! macOS native regression: capture independence, synchronous movement and cleanup.
//! Run on the main thread: cargo run -p nana-window --example underlay-native-probe.

#[cfg(target_os = "macos")]
fn main() {
    use std::ptr::NonNull;

    use nana_window::Underlay;
    use objc2::rc::Retained;
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSApplication, NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask};
    use objc2_foundation::{NSPoint, NSRect, NSSize};
    use raw_window_handle::{
        AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
    };

    struct NativeWindow(Retained<NSWindow>);

    impl HasWindowHandle for NativeWindow {
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            let view = self.0.contentView().ok_or(HandleError::Unavailable)?;
            let handle = AppKitWindowHandle::new(NonNull::from(&*view).cast());
            // SAFETY: the NSWindow retains its content view for this borrow.
            Ok(unsafe { WindowHandle::borrow_raw(RawWindowHandle::AppKit(handle)) })
        }
    }

    fn window(mtm: MainThreadMarker, style: NSWindowStyleMask) -> NativeWindow {
        let frame = NSRect::new(NSPoint::new(200.0, 200.0), NSSize::new(640.0, 480.0));
        // SAFETY: plain NSWindow creation on the main thread.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: Retained owns the window, including after it is closed.
        unsafe { window.setReleasedWhenClosed(false) };
        let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
        window.setContentView(Some(&view));
        NativeWindow(window)
    }

    let mtm = MainThreadMarker::new().expect("run as a main-thread native probe");
    let _app = NSApplication::sharedApplication(mtm);
    let parent = window(
        mtm,
        NSWindowStyleMask::Titled
            | NSWindowStyleMask::Resizable
            | NSWindowStyleMask::FullSizeContentView,
    );
    let picture = window(mtm, NSWindowStyleMask::Borderless);
    parent.0.orderFront(None);
    let binding = Underlay::attach(&parent, &picture).expect("native underlay");
    // Native attachment groups leak their UI into includeChildWindows captures.
    assert!(picture.0.parentWindow().is_none());
    assert!(parent.0.parentWindow().is_none());
    let mut shadow = nana_window::shadow::WindowShadowState::default();
    shadow.apply(
        &parent,
        nana_window::shadow::ShadowPlan::Companion {
            style: nana_window::shadow::ShadowStyle {
                color: [0.0, 0.0, 0.0, 0.2],
                offset: [0.0, 3.0],
                blur: 12.0,
                spread: 0.0,
            },
            shape: nana_window::shadow::ShadowShape {
                rect: [0.0, 0.0, 640.0, 480.0],
                radii: [12.0; 4],
            },
        },
    );
    assert!(shadow.set_ordering_parent(&picture));
    assert!(
        parent
            .0
            .childWindows()
            .is_none_or(|children| children.is_empty())
    );
    assert_eq!(picture.0.childWindows().unwrap().len(), 1);
    let initial_parent = parent.0.frame();
    let initial_picture = picture.0.frame();
    for step in 1..=64 {
        let dx = f64::from(step) * 2.0;
        let dy = f64::from(step);
        parent.0.setFrameOrigin(NSPoint::new(
            initial_parent.origin.x + dx,
            initial_parent.origin.y + dy,
        ));
        // No binding.sync(), event-loop pump or application redraw: the
        // synchronous native notification must have already moved the picture.
        let actual = picture.0.frame().origin;
        assert_eq!(actual.x, initial_picture.origin.x + dx);
        assert_eq!(actual.y, initial_picture.origin.y + dy);
    }
    for (width, height) in [(720.0, 520.0), (900.0, 600.0), (640.0, 480.0)] {
        let mut frame = parent.0.frame();
        frame.size = NSSize::new(width, height);
        parent.0.setFrame_display(frame, false);
        let client = parent
            .0
            .convertRectToScreen(parent.0.contentView().unwrap().frame());
        assert_eq!(picture.0.frame(), client);
    }
    assert!(shadow.set_ordering_parent(&parent));
    assert!(
        picture
            .0
            .childWindows()
            .is_none_or(|children| children.is_empty())
    );
    drop(binding);
    parent.0.setFrameOrigin(NSPoint::new(250.0, 250.0));
    assert!(picture.0.parentWindow().is_none());
    assert!(!picture.0.isVisible());
    shadow.release();
    picture.0.close();
    parent.0.close();
    println!(
        "native underlay: 64 synchronous moves, resize, capture independence, shadow ordering and cleanup passed"
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("underlay-native-probe requires macOS");
}
