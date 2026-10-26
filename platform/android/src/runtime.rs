//! Android activity loop: window lifecycle → wgpu Surface → NanaUI document.

use std::time::{Duration, Instant};

use android_activity::input::{
    ImeOptions, InputEvent, InputType, KeyAction, Keycode, MetaState, MotionAction,
    TextInputAction, TextInputState, TextSpan,
};
use android_activity::ndk::configuration::UiModeNight;
use android_activity::{AndroidApp, InputStatus, MainEvent, PollEvent};
use nana_ui_platform::SurfacePhase;

use crate::gpu::GpuSurface;
use crate::shell::{AndroidShellStub, SystemInsets, scale_factor_from_density_dpi};
use crate::slot_ax::SlotAccessibility;
use crate::slot_ime::{SlotEditorInfo, SlotImeBuffer, ime_events_from_buffer_delta};
use crate::slot_input::{
    SlotKeyMods, SlotTouchKind, android_keycode_is_modifier, host_swallows_for_input_connection,
    logical_key_from_android_keycode,
};
use crate::slot_paint::SlotPainter;
use crate::slot_runtime::SystemAppearance;

struct HostState {
    gpu: Option<GpuSurface>,
    slot: Option<SlotPainter>,
    ax: Option<SlotAccessibility>,
    shell: AndroidShellStub,
    last_paint: Instant,
    phase: SurfacePhase,
    /// Mirrors the soft keyboard: `true` after we asked InputMethodManager to
    /// show it while the slot text input held focus.
    ime_shown: bool,
    /// Last GameTextInput buffer we acknowledged (for composition diffs).
    ime_buffer: SlotImeBuffer,
    /// Last EditorInfo pushed to GameActivity.
    editor_info: Option<SlotEditorInfo>,
}

impl HostState {
    fn new() -> Self {
        Self {
            gpu: None,
            slot: None,
            ax: None,
            shell: AndroidShellStub::new(),
            last_paint: Instant::now() - Duration::from_secs(1),
            phase: SurfacePhase::Pending,
            ime_shown: false,
            ime_buffer: SlotImeBuffer::default(),
            editor_info: None,
        }
    }

    fn scale_from_app(app: &AndroidApp) -> f32 {
        let density = app.config().density();
        let scale = scale_factor_from_density_dpi(density);
        log::debug!("nana-android-host: density_dpi={density:?} scale_factor={scale}");
        scale
    }

    fn window_physical_size(app: &AndroidApp) -> (u32, u32) {
        app.native_window()
            .map(|win| (win.width().max(1) as u32, win.height().max(1) as u32))
            .unwrap_or((720, 1280))
    }

    fn apply_scale_and_size(&mut self, physical: (u32, u32), scale: f32) {
        let (w, h) = physical;
        if let Some(gpu) = self.gpu.as_mut() {
            gpu.resize(w, h);
        }
        self.shell.resize(w, h, scale);
        if let Some(painter) = self.slot.as_mut() {
            painter.resize((w, h), scale, self.shell.insets());
        }
    }

    /// Re-read the system bands over the edge-to-edge surface.
    fn on_insets_changed(&mut self, app: &AndroidApp) {
        self.shell.set_insets(system_insets(app));
        let (w, h) = self.shell.physical_size();
        let scale = self.shell.scale_factor();
        if let Some(painter) = self.slot.as_mut() {
            painter.resize((w, h), scale, self.shell.insets());
        }
    }

    /// Dark mode and text size, for the page's “跟随系统” choices.
    fn sync_system_appearance(&mut self, app: &AndroidApp) {
        let system = SystemAppearance {
            dark: app.config().ui_mode_night() == UiModeNight::Yes,
            font_scale: font_scale(app),
        };
        if let Some(painter) = self.slot.as_mut() {
            painter.set_system_appearance(system);
        }
    }

    fn on_window_ready(&mut self, app: &AndroidApp) -> Result<(), String> {
        let (w, h) = Self::window_physical_size(app);
        let scale = Self::scale_from_app(app);

        if self.gpu.is_some() {
            self.apply_scale_and_size((w, h), scale);
            self.phase = SurfacePhase::Ready;
            return Ok(());
        }

        let gpu = GpuSurface::new(app, w, h)?;
        self.slot = Some(SlotPainter::new(
            &gpu.gpu,
            nana_ui::GpuTextureFormat::from_wgpu(gpu.format),
            (gpu.config.width, gpu.config.height),
            scale,
        )?);
        self.gpu = Some(gpu);
        self.shell.resize(w, h, scale);
        self.on_insets_changed(app);
        self.sync_system_appearance(app);
        self.phase = SurfacePhase::Ready;
        match SlotAccessibility::new(app, self.slot.as_ref().expect("slot painter").runtime()) {
            Ok(ax) => self.ax = Some(ax),
            Err(err) => log::warn!("nana-android-host: accessibility attach failed: {err}"),
        }
        log::info!("nana-android-host: NanaUI document ready scale={scale}");
        self.paint_frame()?;
        self.publish_accessibility();
        Ok(())
    }

    /// Density / orientation / dark mode / font scale change — refresh scale,
    /// geometry and the system appearance.
    fn on_config_changed(&mut self, app: &AndroidApp) {
        let scale = Self::scale_from_app(app);
        let physical = app
            .native_window()
            .map(|win| (win.width().max(1) as u32, win.height().max(1) as u32))
            .unwrap_or_else(|| self.shell.physical_size());
        self.apply_scale_and_size(physical, scale);
        self.sync_system_appearance(app);
        log::info!("nana-android-host: ConfigChanged scale={scale} physical={physical:?}");
    }

    fn on_window_destroyed(&mut self) {
        self.slot = None;
        self.gpu = None;
        self.ax = None;
        self.phase = SurfacePhase::Destroyed;
        self.ime_shown = false;
        self.ime_buffer = SlotImeBuffer::default();
        self.editor_info = None;
        log::info!("nana-android-host: surface destroyed");
    }

    /// Publish the slot accessibility tree (no-op until TalkBack initializes
    /// it); called whenever the frame actually changed.
    fn publish_accessibility(&mut self) {
        let Some(ax) = self.ax.as_mut() else {
            return;
        };
        let Some(painter) = self.slot.as_mut() else {
            return;
        };
        ax.drain_actions(painter.runtime_mut());
        ax.push(painter.runtime());
    }

    /// TalkBack activation arrives as a queued action, not input, so the loop
    /// drains it every iteration.
    fn drain_accessibility_actions(&mut self) -> bool {
        let (Some(ax), Some(painter)) = (self.ax.as_mut(), self.slot.as_mut()) else {
            return false;
        };
        ax.drain_actions(painter.runtime_mut())
    }

    fn paint_frame(&mut self) -> Result<(), String> {
        let scale = self.shell.scale_factor();
        let (fw, fh) = {
            let Some(gpu) = self.gpu.as_ref() else {
                return Ok(());
            };
            (gpu.config.width, gpu.config.height)
        };
        self.shell.resize(fw, fh, scale);
        if let Some(painter) = self.slot.as_mut() {
            painter.resize((fw, fh), scale, self.shell.insets());
        }

        let HostState {
            gpu,
            slot,
            last_paint,
            ..
        } = self;
        let Some(gpu) = gpu.as_mut() else {
            return Ok(());
        };
        gpu.present_with_overlay(|target, frame| {
            if let Some(painter) = slot.as_mut() {
                painter.paint_slot(frame, target, (fw, fh));
            }
            Ok(())
        })?;
        *last_paint = Instant::now();
        Ok(())
    }

    /// Mirror Runtime text-input focus onto GameTextInput / the soft keyboard.
    ///
    /// Composition then flows through `CompositionInput::{Update,Commit,DeleteSurrounding}`
    /// on the slot Runtime. GameTextInput's buffer is a host mirror, not a
    /// second editor.
    fn sync_soft_input(&mut self, app: &AndroidApp) {
        let Some(painter) = self.slot.as_ref() else {
            self.ime_shown = false;
            self.editor_info = None;
            return;
        };
        let focused = painter.text_input_focused();
        let next_info = painter.editor_info();
        if focused
            && next_info != self.editor_info
            && let Some(info) = next_info
        {
            apply_editor_info(app, info);
            self.editor_info = Some(info);
        }
        if focused {
            if let Some(buffer) = painter.ime_buffer()
                && buffer != self.ime_buffer
            {
                set_text_input_state(app, &buffer);
                self.ime_buffer = buffer;
            }
        } else {
            self.editor_info = None;
        }
        match (focused, self.ime_shown) {
            (true, false) => {
                app.show_soft_input(true);
                self.ime_shown = true;
                log::debug!("nana-android-host: soft input show (text input focused)");
            }
            (false, true) => {
                app.hide_soft_input(false);
                self.ime_shown = false;
                self.ime_buffer = SlotImeBuffer::default();
                log::debug!("nana-android-host: soft input hide (focus left text input)");
            }
            _ => {}
        }
    }

    fn handle_motion(
        &mut self,
        action: MotionAction,
        physical_x: f32,
        physical_y: f32,
        pointer_id: i32,
    ) -> bool {
        let kind = match action {
            MotionAction::Down | MotionAction::PointerDown => SlotTouchKind::Down,
            MotionAction::Move | MotionAction::HoverMove => SlotTouchKind::Move,
            MotionAction::Up | MotionAction::PointerUp => SlotTouchKind::Up,
            MotionAction::Cancel => SlotTouchKind::Cancel,
            _ => return false,
        };
        let Some(painter) = self.slot.as_mut() else {
            return false;
        };
        // Only samples in Primary (or a captured drag) are Handled; the
        // status and navigation bars stay with the system.
        painter.push_touch(kind, physical_x, physical_y, pointer_id)
    }

    /// GameActivity KeyEvent → Runtime keyboard (US-QWERTY subset + editing keys).
    ///
    /// System keys (Back, …) stay `Unhandled`. While the slot text input is
    /// focused, printable commits arrive as GameTextInput `TextEvent`s and are
    /// not synthesized from KeyEvents (that would double-commit CJK); shortcuts
    /// still reach the Runtime. Keys are
    /// Handled only while the slot holds keyboard focus; otherwise they stay
    /// with the system.
    fn handle_key(
        &mut self,
        action: KeyAction,
        keycode: Keycode,
        meta: MetaState,
        repeat_count: i32,
    ) -> bool {
        let down = match action {
            KeyAction::Down => true,
            KeyAction::Up => false,
            // Treat ACTION_MULTIPLE as a single press for the printable subset.
            KeyAction::Multiple => true,
            _ => return false,
        };
        let keycode_u32: u32 = keycode.into();
        let mods = SlotKeyMods {
            shift: meta.shift_on(),
            ctrl: meta.ctrl_on(),
            alt: meta.alt_on(),
            logo: meta.meta_on(),
        };
        let logical =
            logical_key_from_android_keycode(keycode_u32, mods.shift, meta.caps_lock_on());
        let is_mod = android_keycode_is_modifier(keycode_u32);
        if logical.is_none() && !is_mod {
            return false;
        }
        let Some(painter) = self.slot.as_mut() else {
            return false;
        };
        if host_swallows_for_input_connection(
            down,
            painter.accepts_key(),
            painter.text_input_focused(),
            logical,
            mods,
        ) {
            return true;
        }
        let repeat = down && repeat_count > 0;
        painter.push_key(down, logical, mods, repeat)
    }

    /// GameTextInput state change → `dispatch_ime` (Preedit / Commit / DeleteSurrounding).
    fn handle_text_event(&mut self, state: &TextInputState) -> bool {
        let Some(painter) = self.slot.as_mut() else {
            return false;
        };
        if !painter.text_input_focused() {
            return false;
        }
        let next = slot_ime_buffer_from_android(state);
        let events = ime_events_from_buffer_delta(&self.ime_buffer, &next);
        if events.is_empty() {
            self.ime_buffer = next;
            return false;
        }
        let mut handled = false;
        for event in &events {
            if painter.push_ime(event) {
                handled = true;
            }
        }
        if let Some(buffer) = painter.ime_buffer() {
            self.ime_buffer = buffer;
        } else {
            self.ime_buffer = next;
        }
        handled
    }

    fn handle_text_action(&mut self, action: TextInputAction) -> bool {
        if !matches!(
            action,
            TextInputAction::Done
                | TextInputAction::Go
                | TextInputAction::Send
                | TextInputAction::Next
        ) {
            return false;
        }
        let Some(painter) = self.slot.as_mut() else {
            return false;
        };
        if !painter.text_input_focused() {
            return false;
        }
        painter.push_key(
            true,
            Some(crate::slot_input::SlotLogicalKey::Enter),
            SlotKeyMods::default(),
            false,
        )
    }
}

pub fn run(app: AndroidApp) -> Result<(), String> {
    let mut state = HostState::new();
    let mut running = true;

    while running {
        app.poll_events(Some(Duration::from_millis(16)), |event| match event {
            PollEvent::Main(main) => match main {
                MainEvent::InitWindow { .. } => {
                    if let Err(err) = state.on_window_ready(&app) {
                        log::error!("nana-android-host: window ready failed: {err}");
                    }
                }
                MainEvent::TerminateWindow { .. } => {
                    state.on_window_destroyed();
                }
                MainEvent::Destroy => {
                    state.on_window_destroyed();
                    running = false;
                }
                MainEvent::WindowResized { .. } | MainEvent::RedrawNeeded { .. } => {
                    if let Some(win) = app.native_window() {
                        let physical = (win.width().max(1) as u32, win.height().max(1) as u32);
                        // Keep current scale; ConfigChanged owns density updates.
                        state.apply_scale_and_size(physical, state.shell.scale_factor());
                    }
                    if let Err(err) = state.paint_frame() {
                        log::warn!("nana-android-host: paint: {err}");
                    }
                    state.publish_accessibility();
                }
                MainEvent::InsetsChanged { .. } => {
                    state.on_insets_changed(&app);
                    if let Err(err) = state.paint_frame() {
                        log::warn!("nana-android-host: insets paint: {err}");
                    }
                    state.publish_accessibility();
                }
                MainEvent::ConfigChanged { .. } => {
                    state.on_config_changed(&app);
                    if let Err(err) = state.paint_frame() {
                        log::warn!("nana-android-host: config paint: {err}");
                    }
                    state.publish_accessibility();
                }
                MainEvent::Pause | MainEvent::Stop => {
                    // Android dismisses the IME itself on pause/stop; drop the
                    // mirror so the next focus re-shows it.
                    state.ime_shown = false;
                    log::info!("nana-android-host: pause/stop phase={:?}", state.phase);
                }
                MainEvent::Resume { .. } | MainEvent::Start => {
                    log::info!("nana-android-host: resume/start");
                }
                _ => {}
            },
            PollEvent::Timeout
                if state.phase == SurfacePhase::Ready
                    && state.last_paint.elapsed() > Duration::from_millis(500) =>
            {
                if let Err(err) = state.paint_frame() {
                    log::warn!("nana-android-host: heartbeat paint: {err}");
                }
            }
            _ => {}
        });

        let mut need_paint = false;
        if let Ok(mut iter) = app.input_events_iter() {
            loop {
                let read = iter.next(|event| match event {
                    InputEvent::MotionEvent(motion) => {
                        let action = motion.action();
                        let idx = motion.pointer_index();
                        let pointer = motion.pointer_at_index(idx);
                        let x = pointer.x();
                        let y = pointer.y();
                        let pointer_id = pointer.pointer_id();
                        if state.handle_motion(action, x, y, pointer_id) {
                            // Re-ask on every tap on the focused field: the IME
                            // may have been dismissed (Back) since the last
                            // focus change and GameActivity has no
                            // visibility query to distinguish that state.
                            if matches!(action, MotionAction::Up | MotionAction::PointerUp)
                                && state
                                    .slot
                                    .as_ref()
                                    .is_some_and(|painter| painter.text_input_focused())
                            {
                                app.show_soft_input(true);
                                state.ime_shown = true;
                            }
                            need_paint = true;
                            InputStatus::Handled
                        } else {
                            InputStatus::Unhandled
                        }
                    }
                    InputEvent::KeyEvent(key) => {
                        if state.handle_key(
                            key.action(),
                            key.key_code(),
                            key.meta_state(),
                            key.repeat_count(),
                        ) {
                            need_paint = true;
                            InputStatus::Handled
                        } else {
                            InputStatus::Unhandled
                        }
                    }
                    InputEvent::TextEvent(text) => {
                        if state.handle_text_event(text) {
                            if let Some(buffer) =
                                state.slot.as_ref().and_then(|painter| painter.ime_buffer())
                            {
                                set_text_input_state(&app, &buffer);
                            }
                            need_paint = true;
                            InputStatus::Handled
                        } else {
                            InputStatus::Unhandled
                        }
                    }
                    InputEvent::TextAction(action) => {
                        if state.handle_text_action(*action) {
                            need_paint = true;
                            InputStatus::Handled
                        } else {
                            InputStatus::Unhandled
                        }
                    }
                    _ => InputStatus::Unhandled,
                });
                if !read {
                    break;
                }
            }
        }
        if state.drain_accessibility_actions() {
            need_paint = true;
        }
        if need_paint {
            if let Err(err) = state.paint_frame() {
                log::warn!("nana-android-host: input paint: {err}");
            }
            state.publish_accessibility();
        }
        state.sync_soft_input(&app);
    }

    Ok(())
}

/// GameTextInput hands the Java side's spans through unconverted: UTF-16
/// code units, not the byte offsets the buffer keeps.
fn slot_ime_buffer_from_android(state: &TextInputState) -> SlotImeBuffer {
    SlotImeBuffer::from_utf16(
        state.text.clone(),
        (state.selection.start, state.selection.end),
        state.compose_region.map(|span| (span.start, span.end)),
    )
}

fn set_text_input_state(app: &AndroidApp, buffer: &SlotImeBuffer) {
    let ((start, end), compose) = buffer.utf16_spans();
    app.set_text_input_state(TextInputState {
        text: buffer.text.clone(),
        selection: TextSpan { start, end },
        compose_region: compose.map(|(start, end)| TextSpan { start, end }),
    });
}

fn apply_editor_info(app: &AndroidApp, info: SlotEditorInfo) {
    let input_type = InputType::from_bits_truncate(info.input_type);
    let action = if info.action == crate::slot_ime::IME_ACTION_NONE {
        TextInputAction::None
    } else {
        TextInputAction::Done
    };
    let options = ImeOptions::from_bits_truncate(info.ime_options);
    app.set_ime_editor_info(input_type, action, options);
}

/// Run `read` against the Activity on this thread's JNI env; `None` (and a
/// warning) when any call fails.
fn with_activity<T>(
    app: &AndroidApp,
    what: &str,
    read: impl FnOnce(
        &mut accesskit_android::jni::JNIEnv<'_>,
        &accesskit_android::jni::objects::JObject<'_>,
    ) -> accesskit_android::jni::errors::Result<T>,
) -> Option<T> {
    use std::mem::ManuallyDrop;

    use accesskit_android::jni::{self, objects::JObject};

    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }.ok()?;
    let mut env = vm.attach_current_thread().ok()?;
    // android_main never returns to Java, so only a frame frees these locals.
    let result = env.with_local_frame(16, |env| {
        // `activity_as_ptr` is a global ref; never drop it as a local ref.
        let activity = ManuallyDrop::new(unsafe {
            JObject::from_raw(app.activity_as_ptr() as jni::sys::jobject)
        });
        read(env, &activity)
    });
    match result {
        Ok(value) => Some(value),
        Err(error) => {
            let _ = env.exception_clear();
            log::warn!("nana-android-host: {what}: {error}");
            None
        }
    }
}

/// Status bar (with display cutout) and navigation bar or soft keyboard, in
/// physical px, from the decor view's root insets. Before API 30 the system
/// window insets already include the keyboard under `adjustResize`.
fn system_insets(app: &AndroidApp) -> SystemInsets {
    use accesskit_android::jni::objects::JValue;

    with_activity(app, "window insets", |env, activity| {
        let window = env
            .call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?
            .l()?;
        let decor = env
            .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
            .l()?;
        let insets = env
            .call_method(
                &decor,
                "getRootWindowInsets",
                "()Landroid/view/WindowInsets;",
                &[],
            )?
            .l()?;
        if insets.is_null() {
            return Ok(SystemInsets::default());
        }
        let sdk = env
            .get_static_field("android/os/Build$VERSION", "SDK_INT", "I")?
            .i()?;
        let (top, bottom) = if sdk >= 30 {
            let types = "android/view/WindowInsets$Type";
            let bars = env
                .call_static_method(types, "systemBars", "()I", &[])?
                .i()?;
            let cutout = env
                .call_static_method(types, "displayCutout", "()I", &[])?
                .i()?;
            let ime = env.call_static_method(types, "ime", "()I", &[])?.i()?;
            let mut edge = |mask: i32, side: &str| -> accesskit_android::jni::errors::Result<i32> {
                let rect = env
                    .call_method(
                        &insets,
                        "getInsets",
                        "(I)Landroid/graphics/Insets;",
                        &[JValue::Int(mask)],
                    )?
                    .l()?;
                env.get_field(&rect, side, "I")?.i()
            };
            let top = edge(bars | cutout, "top")?;
            let bottom = edge(bars | cutout, "bottom")?.max(edge(ime, "bottom")?);
            (top, bottom)
        } else {
            let top = env
                .call_method(&insets, "getSystemWindowInsetTop", "()I", &[])?
                .i()?;
            let bottom = env
                .call_method(&insets, "getSystemWindowInsetBottom", "()I", &[])?
                .i()?;
            (top, bottom)
        };
        Ok(SystemInsets {
            top: top.max(0) as u32,
            bottom: bottom.max(0) as u32,
        })
    })
    .unwrap_or_default()
}

/// The system text-size setting, `Resources.getConfiguration().fontScale`.
/// `AConfiguration` does not carry it.
fn font_scale(app: &AndroidApp) -> f32 {
    with_activity(app, "font scale", |env, activity| {
        let resources = env
            .call_method(
                activity,
                "getResources",
                "()Landroid/content/res/Resources;",
                &[],
            )?
            .l()?;
        let configuration = env
            .call_method(
                &resources,
                "getConfiguration",
                "()Landroid/content/res/Configuration;",
                &[],
            )?
            .l()?;
        env.get_field(&configuration, "fontScale", "F")?.f()
    })
    .filter(|scale| scale.is_finite() && *scale > 0.0)
    .unwrap_or(1.0)
}
