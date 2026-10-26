//! Experimental Android window content as a NanaUI Runtime document.
//!
//! The document covers the whole surface. Its root paints the theme
//! background and pads itself by the system insets, so its content fills the
//! Primary region: an appearance page (theme and text size) above the control
//! strip. Hosts [`RuntimeDocument`] + one input source + host text shaping — the same
//! routing `run_runtime` uses on desktop. The Android Activity still owns the
//! window and event loop; this type does not call `run_runtime` (winit).
//! Soft keyboard show/hide is driven by the host from [`Self::text_input_focused`];
//! GameTextInput composition maps to [`CompositionInput`] events routed like
//! any other input. Accessibility name/role/value is the
//! same Runtime projection desktop hosts publish; Click/Focus/SetValue/SetSelection
//! activate Button/Switch/TextInput.

#![cfg_attr(not(target_os = "android"), allow(dead_code))]

use std::sync::{Arc, Mutex};
use std::time::Instant;

use nana_ui::runtime::view::{
    entity_ref, segmented, segmented_option, settings_row, widget, with_refs,
};
use nana_ui::runtime::{
    AccessibilityActionRequest, Activate, Button, DocumentId, Entity, FrameworkError,
    LayoutViewport, List, NodeStyle, RuntimeDocument, SettingsCard, Switch, Text, TextChanged,
    TextInput, ToggleChanged,
};
use nana_ui::{AccessibilityNode, NanaTextShaper, RuntimeAnimationClock};
use nana_ui_core::{
    AlignSpec, FlexDirection, JustifySpec, LengthSpec, PhysicalRect, SemanticColorRole,
    ThemeAppearance, builtin_theme_arc,
};
use nana_ui_platform::{
    CanonicalInputEvent, CommittedText, CompositionInput, CursorIcon, DeviceId, EndpointGeneration,
    HostServiceError, HostServices, InputPayload, InputSequencer, InputSourceId,
    SharedClipboardHost, TextInputContext, default_shared_clipboard, read_shared_clipboard,
    write_shared_clipboard,
};

use crate::shell::{SystemInsets, primary_rect};
use crate::slot_ime::{SlotEditorInfo, SlotImeBuffer, editor_info_from_request};
use crate::slot_input::{
    SlotInputGate, SlotKeyDispatch, SlotKeyMods, SlotLogicalKey, SlotTouchKind, logical_point,
    pointer_in_slot, slot_key_to_dispatch, touch_to_pointer_event,
};

const SLOT_BUTTON_LABEL: &str = "Nana";
const SLOT_TEXT_LABEL: &str = "Shell";
const SLOT_ICON_GLYPH: &str = "⚙";
const SLOT_SWITCH_LABEL: &str = "On";
const SLOT_INPUT_PLACEHOLDER: &str = "Type…";
const PAGE_TITLE: &str = "外观";
const THEME_LABEL: &str = "主题";
const TEXT_SIZE_LABEL: &str = "文字大小";

/// Logical height of the control strip.
const STRIP_HEIGHT: f32 = 64.0;
/// Logical inset between the Primary edges and the content.
const CONTENT_INSET: f32 = 12.0;

/// The theme the page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum ThemeChoice {
    Light,
    Dark,
    #[default]
    System,
}

/// The text size the page asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum TextSizeChoice {
    #[default]
    System,
    Small,
    Standard,
    Large,
}

impl TextSizeChoice {
    fn scale(self, system: f32) -> f32 {
        match self {
            Self::System => system,
            Self::Small => 0.85,
            Self::Standard => 1.0,
            Self::Large => 1.2,
        }
    }
}

/// What the host reads from the Android `Configuration`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SystemAppearance {
    /// `uiMode` night bit.
    pub dark: bool,
    /// `fontScale`, the system text-size setting.
    pub font_scale: f32,
}

impl Default for SystemAppearance {
    fn default() -> Self {
        Self {
            dark: false,
            font_scale: 1.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
struct SlotStripState {
    press_count: u32,
    switch_on: bool,
    input_value: String,
    theme: ThemeChoice,
    text_size: TextSizeChoice,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SlotSnapshot {
    pub press_count: u32,
    pub switch_on: bool,
    pub input_len: usize,
}

/// The slot's host services: the Android clipboard. The soft keyboard
/// follows [`SlotRuntime::text_input_focused`] and there is no cursor.
struct SlotServices {
    clipboard: SharedClipboardHost,
}

impl HostServices for SlotServices {
    fn set_cursor(&mut self, _cursor: CursorIcon) {}

    fn set_text_input(&mut self, _state: Option<&TextInputContext>) {}

    fn read_clipboard(&mut self) -> Result<Option<String>, HostServiceError> {
        read_shared_clipboard(&self.clipboard)
    }

    fn write_clipboard(&mut self, text: &str) -> Result<(), HostServiceError> {
        write_shared_clipboard(&self.clipboard, text)
    }
}

/// The slot's one input source.
const SLOT_SOURCE: InputSourceId = InputSourceId(1);

/// Retained NanaUI document for the window: appearance page and control strip.
pub struct SlotRuntime {
    document: RuntimeDocument,
    shaper: NanaTextShaper,
    sequencer: InputSequencer,
    services: SlotServices,
    clock: RuntimeAnimationClock,
    physical_size: (u32, u32),
    scale: f32,
    insets: SystemInsets,
    system: SystemAppearance,
    /// The theme and text scale last installed on the document.
    applied: Option<(ThemeAppearance, f32)>,
    state: Arc<Mutex<SlotStripState>>,
    gate: SlotInputGate,
    last_touch_in_slot: bool,
    ime_enabled: bool,
    #[cfg_attr(not(test), allow(dead_code))]
    button: Entity<Button>,
    field: Entity<TextInput>,
    root: Entity<List>,
}

impl SlotRuntime {
    pub fn new(physical_size: (u32, u32), scale: f32) -> Result<Self, FrameworkError> {
        let document_id = DocumentId::new(1).expect("android slot document id");
        let mut document = RuntimeDocument::new(document_id);
        let state = Arc::new(Mutex::new(SlotStripState::default()));

        let presses = Arc::clone(&state);
        let toggles = Arc::clone(&state);
        let inputs = Arc::clone(&state);
        let theme = |choice: ThemeChoice| {
            let state = Arc::clone(&state);
            move || lock_state(&state).theme = choice
        };
        let text_size = |choice: TextSizeChoice| {
            let state = Arc::clone(&state);
            move || lock_state(&state).text_size = choice
        };
        document
            .context_mut()
            .bind_input_source(SLOT_SOURCE, EndpointGeneration(1), document_id)
            .expect("a fresh slot document binds its source");
        let (_, (button, field, root)) =
            document.context_mut().mount_view_root(document_id, || {
                let (button, field, root) = (entity_ref(), entity_ref(), entity_ref());
                let page = widget(SettingsCard::new(PAGE_TITLE))
                    .key("appearance")
                    .children((
                        settings_row(THEME_LABEL).control(
                            segmented().key("theme").children((
                                segmented_option("浅色").on_select(theme(ThemeChoice::Light)),
                                segmented_option("深色").on_select(theme(ThemeChoice::Dark)),
                                segmented_option("跟随系统")
                                    .selected(true)
                                    .on_select(theme(ThemeChoice::System)),
                            )),
                        ),
                        settings_row(TEXT_SIZE_LABEL).control(
                            segmented().key("text-size").children((
                                segmented_option("跟随系统")
                                    .selected(true)
                                    .on_select(text_size(TextSizeChoice::System)),
                                segmented_option("小").on_select(text_size(TextSizeChoice::Small)),
                                segmented_option("标准")
                                    .on_select(text_size(TextSizeChoice::Standard)),
                                segmented_option("大").on_select(text_size(TextSizeChoice::Large)),
                            )),
                        ),
                    ));
                let strip = widget(row_strip()).key("row").children((
                    widget(Text::new(SLOT_ICON_GLYPH)).key("icon"),
                    widget(Text::new(SLOT_TEXT_LABEL)).key("caption"),
                    widget(slot_text_input())
                        .key("field")
                        .entity_ref(field)
                        .on_cx(move |field: &mut TextInput, event: &TextChanged, _cx| {
                            field.state.replace_value(event.value.clone());
                            lock_state(&inputs).input_value = event.value.to_string();
                        }),
                    widget(Switch::new(SLOT_SWITCH_LABEL, false))
                        .key("switch")
                        .on_cx(move |switch: &mut Switch, event: &ToggleChanged, _cx| {
                            switch.checked = event.checked;
                            lock_state(&toggles).switch_on = event.checked;
                        }),
                    widget(Button::new(SLOT_BUTTON_LABEL))
                        .key("button")
                        .entity_ref(button)
                        .on_cx(move |button: &mut Button, _event: &Activate, _cx| {
                            let mut slot = lock_state(&presses);
                            slot.press_count = slot.press_count.saturating_add(1);
                            button.label = format!("{SLOT_BUTTON_LABEL} · {}", slot.press_count);
                        }),
                ));
                with_refs(
                    widget(column_host())
                        .key("column")
                        .entity_ref(root)
                        .children((page, strip)),
                    (button, field, root),
                )
            })?;

        let mut runtime = Self {
            document,
            shaper: NanaTextShaper::default(),
            sequencer: InputSequencer::new(SLOT_SOURCE, EndpointGeneration(1)),
            services: SlotServices {
                clipboard: default_shared_clipboard(),
            },
            clock: RuntimeAnimationClock::now(),
            physical_size: (physical_size.0.max(1), physical_size.1.max(1)),
            scale: scale.max(0.25),
            insets: SystemInsets::default(),
            system: SystemAppearance::default(),
            applied: None,
            state,
            gate: SlotInputGate::default(),
            last_touch_in_slot: false,
            ime_enabled: false,
            button,
            field,
            root,
        };
        runtime.pad_root()?;
        runtime.flush()?;
        Ok(runtime)
    }

    /// The surface size and scale, and the system bands over it.
    pub fn resize(
        &mut self,
        physical_size: (u32, u32),
        scale: f32,
        insets: SystemInsets,
    ) -> Result<(), FrameworkError> {
        let physical_size = (physical_size.0.max(1), physical_size.1.max(1));
        let scale = scale.max(0.25);
        if (physical_size, scale, insets) == (self.physical_size, self.scale, self.insets) {
            return Ok(());
        }
        self.physical_size = physical_size;
        self.scale = scale;
        self.insets = insets;
        self.pad_root()
    }

    /// Dark mode and text size from the Android `Configuration`; the page's
    /// “跟随系统” choices follow them.
    pub fn set_system_appearance(&mut self, system: SystemAppearance) {
        self.system = system;
    }

    pub fn scale(&self) -> f32 {
        self.scale
    }

    /// The surface below the status bar and above the navigation bar or soft
    /// keyboard, in physical px. Touches outside it stay with the system.
    pub fn primary(&self) -> PhysicalRect {
        primary_rect(self.physical_size, self.insets)
    }

    /// The theme installed on the document.
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn theme_appearance(&self) -> ThemeAppearance {
        self.document
            .context()
            .world()
            .installed_theme()
            .appearance()
    }

    pub fn press_count(&self) -> u32 {
        lock_state(&self.state).press_count
    }

    pub fn switch_on(&self) -> bool {
        lock_state(&self.state).switch_on
    }

    pub fn input_value(&self) -> String {
        lock_state(&self.state).input_value.clone()
    }

    pub fn last_touch_in_slot(&self) -> bool {
        self.last_touch_in_slot
    }

    /// Whether the Runtime keyboard focus sits on the slot's text input.
    ///
    /// The Android host mirrors this into `show_soft_input` / `hide_soft_input`
    /// so tapping the field raises the soft keyboard and moving focus away
    /// lowers it.
    /// Whether keyboard samples reach Runtime at all — the slot only owns the
    /// keyboard while the last Down landed inside it.
    pub fn accepts_key(&self) -> bool {
        self.gate.accept_key()
    }

    pub fn text_input_focused(&self) -> bool {
        self.document
            .context()
            .world()
            .focused(self.document.document())
            == Some(self.field.stable_id())
    }

    pub(crate) fn snapshot(&self) -> SlotSnapshot {
        SlotSnapshot {
            press_count: self.press_count(),
            switch_on: self.switch_on(),
            input_len: self.input_value().len(),
        }
    }

    pub fn document(&self) -> &RuntimeDocument {
        &self.document
    }

    pub fn flush(&mut self) -> Result<(), FrameworkError> {
        self.apply_appearance()?;
        let (logical_w, logical_h) = self.logical_size();
        self.document
            .flush(LayoutViewport::new(logical_w, logical_h), &mut self.shaper)?;
        Ok(())
    }

    pub(crate) fn logical_size(&self) -> (f32, f32) {
        (
            self.physical_size.0 as f32 / self.scale,
            self.physical_size.1 as f32 / self.scale,
        )
    }

    /// Install the theme and text scale the page's choices resolve to, when
    /// they changed.
    fn apply_appearance(&mut self) -> Result<(), FrameworkError> {
        let (theme, text_size) = {
            let state = lock_state(&self.state);
            (state.theme, state.text_size)
        };
        let appearance = match theme {
            ThemeChoice::Light => ThemeAppearance::Light,
            ThemeChoice::Dark => ThemeAppearance::Dark,
            ThemeChoice::System if self.system.dark => ThemeAppearance::Dark,
            ThemeChoice::System => ThemeAppearance::Light,
        };
        let text_scale = text_size.scale(self.system.font_scale);
        if self.applied == Some((appearance, text_scale)) {
            return Ok(());
        }
        let document = self.document.document();
        let context = self.document.context_mut();
        context.set_theme_tokens(builtin_theme_arc(appearance))?;
        context.set_document_text_scale(document, Some(text_scale));
        self.applied = Some((appearance, text_scale));
        Ok(())
    }

    /// Pad the root by the system insets so the content fills Primary.
    fn pad_root(&mut self) -> Result<(), FrameworkError> {
        let scale = self.scale;
        let top = self.insets.top as f32 / scale + CONTENT_INSET;
        let bottom = self.insets.bottom as f32 / scale + CONTENT_INSET;
        self.document
            .context_mut()
            .update_component(self.root, move |root, _| {
                let layout = Arc::make_mut(&mut root.style.layout);
                layout.padding_top = Some(LengthSpec::Px(top));
                layout.padding_bottom = Some(LengthSpec::Px(bottom));
            })
    }

    /// Queue a physical pointer sample (Android MotionEvent coords + pointer id).
    ///
    /// Returns `false` when the sample is outside Primary (and not part of
    /// a drag that started inside) so the host can leave it `Unhandled` for
    /// the system.
    pub fn push_touch(
        &mut self,
        kind: SlotTouchKind,
        physical_x: f32,
        physical_y: f32,
        pointer_id: i32,
    ) -> Result<bool, FrameworkError> {
        let primary = self.primary();
        self.last_touch_in_slot = pointer_in_slot(primary, physical_x, physical_y);
        if !self
            .gate
            .accept_pointer(Some(primary), kind, physical_x, physical_y, pointer_id)
        {
            return Ok(false);
        }
        if kind == SlotTouchKind::Down && !self.pointer_hits_text_input(physical_x, physical_y) {
            // Disable IME while the field still has Runtime focus so leftover
            // preedit commits through the desktop Disabled path.
            self.commit_ime_on_blur()?;
        }
        let logical = logical_point(physical_x, physical_y, self.scale);
        let event = touch_to_pointer_event(
            kind,
            logical,
            pointer_id,
            nana_ui_platform::InputModifiers::default(),
        );
        self.dispatch(InputPayload::Pointer(event))?;
        self.sync_ime_lifecycle()?;
        Ok(true)
    }

    /// Queue a keyboard sample (Android KeyEvent → Runtime).
    ///
    /// Returns `false` when the slot does not hold keyboard focus so the host
    /// does not swallow whole-window keys. Printable soft-keyboard commits
    /// become [`CompositionInput::Commit`] while the text input is focused; editing
    /// keys stay on the keyboard path. When `key` is `None`, only modifier
    /// state is recorded.
    pub fn push_key(
        &mut self,
        down: bool,
        key: Option<SlotLogicalKey>,
        mods: SlotKeyMods,
        repeat: bool,
    ) -> Result<bool, FrameworkError> {
        if !self.gate.accept_key() {
            return Ok(false);
        }
        let Some(key) = key else {
            return Ok(true);
        };
        let focused = self.text_input_focused();
        if down && focused && key == SlotLogicalKey::Tab {
            // Tab hands focus to the next control, and the world drops a
            // blurred node's IME state: an in-flight composition has to be
            // committed while the field still holds focus or it is simply
            // lost. Pointer Downs flush the same way in `push_touch`.
            self.commit_ime_on_blur()?;
        }
        match slot_key_to_dispatch(down, key, mods, repeat, focused) {
            SlotKeyDispatch::Key { key, text } => {
                let sequence = self.route(InputPayload::Key(key))?;
                if let Some(text) = text {
                    self.route(InputPayload::Text(CommittedText {
                        text,
                        key: Some(sequence),
                    }))?;
                }
                self.flush()?;
            }
            SlotKeyDispatch::Ime(event) => self.dispatch_ime_event(event)?,
        }
        self.sync_ime_lifecycle()?;
        Ok(true)
    }

    /// Route a composition event into the focused Runtime editor.
    ///
    /// GameActivity TextEvents (and tests) call this so composition goes
    /// through the one input route instead of a second buffer.
    pub fn push_ime(&mut self, event: &CompositionInput) -> Result<bool, FrameworkError> {
        if !self.gate.accept_key() {
            return Ok(false);
        }
        self.dispatch_ime_event(event.clone())?;
        self.sync_ime_lifecycle()?;
        Ok(true)
    }

    /// Runtime accessibility nodes for the slot tree (name/role/value/focus).
    pub fn accessibility_nodes(&self) -> Vec<AccessibilityNode> {
        let document = self.document();
        document
            .context()
            .world()
            .project_accessibility(document.document())
    }

    /// Apply a typed accessibility action and republish IME focus.
    pub fn apply_accessibility_action(
        &mut self,
        request: AccessibilityActionRequest,
    ) -> Result<bool, FrameworkError> {
        if request.target != self.field.stable_id() {
            self.commit_ime_on_blur()?;
        }
        let document = self.document.document();
        let applied = self
            .document
            .context_mut()
            .apply_accessibility_action(document, request)?;
        self.flush()?;
        self.sync_ime_lifecycle()?;
        Ok(applied)
    }

    /// Stamp and route one event on the slot's source; the sequence it got.
    fn route(
        &mut self,
        payload: InputPayload,
    ) -> Result<nana_ui_platform::InputSequence, FrameworkError> {
        let now = self.clock.runtime_time(Instant::now());
        let event = CanonicalInputEvent {
            metadata: self.sequencer.stamp(DeviceId(0), now),
            payload,
        };
        let sequence = event.metadata.sequence;
        match self.document.context_mut().route_input(
            &event,
            &mut self.services,
            Some(&mut self.shaper),
        ) {
            Ok(_) => Ok(sequence),
            Err(nana_ui::InputRouteError::Dispatch(error)) => Err(error),
            // The slot binds its one source once and stamps in order; a
            // rejection is a host bug, reported as invalid input.
            Err(_) => Err(FrameworkError::InvalidInput),
        }
    }

    fn dispatch(&mut self, payload: InputPayload) -> Result<(), FrameworkError> {
        self.route(payload)?;
        self.flush()
    }

    fn dispatch_ime_event(&mut self, event: CompositionInput) -> Result<(), FrameworkError> {
        self.dispatch(InputPayload::Composition(event))
    }

    fn sync_ime_lifecycle(&mut self) -> Result<(), FrameworkError> {
        let focused = self.text_input_focused();
        if focused && !self.ime_enabled {
            self.ime_enabled = true;
            self.dispatch_ime_event(CompositionInput::Enabled)?;
        } else if !focused && self.ime_enabled {
            self.commit_ime_on_blur()?;
        }
        Ok(())
    }

    fn commit_ime_on_blur(&mut self) -> Result<(), FrameworkError> {
        let enabled = std::mem::replace(&mut self.ime_enabled, false);
        if !enabled || !self.text_input_focused() {
            return Ok(());
        }
        self.dispatch_ime_event(CompositionInput::Disabled)
    }

    /// GameTextInput mirror of the focused editor (committed text + preedit),
    /// in byte offsets; [`SlotImeBuffer::utf16_spans`] is what goes to Java.
    pub fn ime_buffer(&self) -> Option<SlotImeBuffer> {
        let document = self.document.document();
        let (target, state) = self.document.context().focused_text_input(document)?;
        if !self
            .document
            .context()
            .world()
            .accessibility(target)
            .is_some_and(|node| node.editable)
        {
            return None;
        }
        // What the editor draws: the preedit in place of the committed range
        // it stands in for (the session's display text), not inserted beside
        // a selection that is still on screen.
        let session = state.session();
        let Some(composition) = session.composition() else {
            return Some(SlotImeBuffer {
                text: state.value.to_owned(),
                selection_start: state.selection.anchor,
                selection_end: state.selection.focus,
                compose: None,
            });
        };
        let text = session.display_text().into_owned();
        let preedit = composition.display_range();
        let (selection_start, selection_end) = match &composition.selection {
            Some(selection) => (
                preedit.start + selection.start,
                preedit.start + selection.end,
            ),
            None => (preedit.end, preedit.end),
        };
        let compose = Some((preedit.start, preedit.end));
        Some(SlotImeBuffer {
            text,
            selection_start,
            selection_end,
            compose,
        })
    }

    /// `EditorInfo` password / multiline flags for the focused editor.
    pub fn editor_info(&self) -> Option<SlotEditorInfo> {
        if !self.text_input_focused() {
            return None;
        }
        let document = self.document.document();
        let (target, _) = self.document.context().focused_text_input(document)?;
        let world = self.document.context().world();
        let password = world.standard_visual(target).is_some_and(|visual| {
            matches!(
                visual,
                nana_ui::runtime::StandardVisual::TextInput { secure: true, .. }
            )
        });
        let multiline = world
            .accessibility(target)
            .is_some_and(|node| node.multiline);
        Some(editor_info_from_request(password, multiline))
    }

    fn pointer_hits_text_input(&self, physical_x: f32, physical_y: f32) -> bool {
        let Some(layout) = self
            .document
            .context()
            .world()
            .layout_box(self.field.stable_id())
        else {
            return false;
        };
        let [x, y] = logical_point(physical_x, physical_y, self.scale);
        x >= layout.x
            && x < layout.x + layout.width
            && y >= layout.y
            && y < layout.y + layout.height
    }
}

fn lock_state(state: &Arc<Mutex<SlotStripState>>) -> std::sync::MutexGuard<'_, SlotStripState> {
    state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn column_host() -> List {
    let mut list = List::new();
    list.style.background = Some(SemanticColorRole::Background);
    let layout = std::sync::Arc::make_mut(&mut list.style.layout);
    layout.direction = Some(FlexDirection::Column);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Fill);
    layout.justify_content = JustifySpec::SpaceBetween;
    layout.padding_left = Some(LengthSpec::Px(CONTENT_INSET));
    layout.padding_right = Some(LengthSpec::Px(CONTENT_INSET));
    layout.padding_top = Some(LengthSpec::Px(CONTENT_INSET));
    layout.padding_bottom = Some(LengthSpec::Px(CONTENT_INSET));
    list
}

fn row_strip() -> List {
    let mut list = List::new();
    list.style = NodeStyle::visible();
    let layout = std::sync::Arc::make_mut(&mut list.style.layout);
    layout.direction = Some(FlexDirection::Row);
    layout.width = Some(LengthSpec::Fill);
    layout.height = Some(LengthSpec::Px(STRIP_HEIGHT));
    layout.align_items = AlignSpec::Center;
    layout.gap = Some(LengthSpec::Px(10.0));
    layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
    list
}

fn slot_text_input() -> TextInput {
    let mut field = TextInput::new("").placeholder(SLOT_INPUT_PLACEHOLDER);
    let layout = std::sync::Arc::make_mut(&mut field.style.layout);
    layout.width = Some(LengthSpec::Px(120.0));
    layout.min_width = Some(LengthSpec::Px(64.0));
    layout.flex_shrink = Some(1.0);
    field
}

#[cfg(test)]
mod tests {
    use super::*;
    use nana_ui::runtime::{AccessibilityAction, AccessibilityRole};

    /// Status bar and navigation bar of a 1080×1920 phone at density 2.
    const INSETS: SystemInsets = SystemInsets {
        top: 63,
        bottom: 126,
    };

    fn runtime() -> SlotRuntime {
        let mut slot = SlotRuntime::new((1080, 1920), 2.0).expect("slot runtime");
        slot.resize((1080, 1920), 2.0, INSETS).expect("insets");
        slot.flush().expect("flush");
        slot
    }

    fn field_id(slot: &SlotRuntime) -> nana_ui::runtime::StableNodeId {
        slot.field.stable_id()
    }

    fn button_id(slot: &SlotRuntime) -> nana_ui::runtime::StableNodeId {
        slot.button.stable_id()
    }

    fn tap_entity(slot: &mut SlotRuntime, id: nana_ui::runtime::StableNodeId) {
        let layout = slot
            .document()
            .context()
            .world()
            .layout_box(id)
            .expect("layout");
        let scale = slot.scale();
        let x = (layout.x + layout.width * 0.5) * scale;
        let y = (layout.y + layout.height * 0.5) * scale;
        assert!(slot.push_touch(SlotTouchKind::Down, x, y, 0).expect("down"));
        assert!(slot.push_touch(SlotTouchKind::Up, x, y, 0).expect("up"));
    }

    fn node_with_role(nodes: &[AccessibilityNode], role: AccessibilityRole) -> &AccessibilityNode {
        nodes
            .iter()
            .find(|node| node.role == role)
            .unwrap_or_else(|| panic!("missing {role:?}"))
    }

    #[test]
    fn flush_extracts_scene_primitives() {
        let slot = runtime();
        assert!(slot.document().scene().primitives().count() > 0);
        assert_eq!(slot.press_count(), 0);
        assert!(!slot.switch_on());
        assert!(slot.input_value().is_empty());
    }

    #[test]
    fn pointer_activate_increments_button() {
        let mut slot = runtime();
        let layout = slot
            .document()
            .context()
            .world()
            .layout_box(slot.button.stable_id())
            .expect("button layout");
        assert!(layout.width > 0.0 && layout.height > 0.0);
        let scale = slot.scale();
        let x = (layout.x + layout.width * 0.5) * scale;
        let y = (layout.y + layout.height * 0.5) * scale;
        let slot_rect = slot.primary();
        assert!(
            crate::slot_input::pointer_in_slot(slot_rect, x, y),
            "button center ({x}, {y}) layout={layout:?} must sit in slot {slot_rect:?}"
        );
        assert!(slot.push_touch(SlotTouchKind::Down, x, y, 0).expect("down"));
        assert!(slot.push_touch(SlotTouchKind::Up, x, y, 0).expect("up"));
        assert_eq!(slot.press_count(), 1);
        assert!(slot.last_touch_in_slot());
    }

    #[test]
    fn focused_key_commits_text_input() {
        let mut slot = runtime();
        let layout = slot
            .document()
            .context()
            .world()
            .layout_box(slot.field.stable_id())
            .expect("field layout");
        assert!(layout.width > 0.0 && layout.height > 0.0);
        let scale = slot.scale();
        let x = (layout.x + layout.width * 0.5) * scale;
        let y = (layout.y + layout.height * 0.5) * scale;
        assert!(
            slot.push_touch(SlotTouchKind::Down, x, y, 0)
                .expect("focus down")
        );
        assert!(
            slot.push_touch(SlotTouchKind::Up, x, y, 0)
                .expect("focus up")
        );
        assert!(
            slot.push_key(
                true,
                Some(SlotLogicalKey::Character('h')),
                SlotKeyMods::default(),
                false,
            )
            .expect("type")
        );
        assert_eq!(slot.input_value(), "h");
    }

    #[test]
    fn text_input_focus_tracks_taps_for_soft_input_mirror() {
        let mut slot = runtime();
        let field_center = |slot: &SlotRuntime| {
            let layout = slot
                .document()
                .context()
                .world()
                .layout_box(slot.field.stable_id())
                .expect("field layout");
            let scale = slot.scale();
            (
                (layout.x + layout.width * 0.5) * scale,
                (layout.y + layout.height * 0.5) * scale,
            )
        };
        let button_center = |slot: &SlotRuntime| {
            let layout = slot
                .document()
                .context()
                .world()
                .layout_box(slot.button.stable_id())
                .expect("button layout");
            let scale = slot.scale();
            (
                (layout.x + layout.width * 0.5) * scale,
                (layout.y + layout.height * 0.5) * scale,
            )
        };
        assert!(!slot.text_input_focused());

        let (fx, fy) = field_center(&slot);
        assert!(
            slot.push_touch(SlotTouchKind::Down, fx, fy, 0)
                .expect("field down")
        );
        assert!(
            slot.push_touch(SlotTouchKind::Up, fx, fy, 0)
                .expect("field up")
        );
        assert!(
            slot.text_input_focused(),
            "tapping the field must move Runtime focus onto the text input"
        );

        let (bx, by) = button_center(&slot);
        assert!(
            slot.push_touch(SlotTouchKind::Down, bx, by, 0)
                .expect("button down")
        );
        assert!(
            slot.push_touch(SlotTouchKind::Up, bx, by, 0)
                .expect("button up")
        );
        assert!(
            !slot.text_input_focused(),
            "tapping another control must drop text-input focus so the host hides the keyboard"
        );
    }

    #[test]
    fn pointer_outside_slot_is_not_handled() {
        let mut slot = runtime();
        assert!(
            !slot
                .push_touch(SlotTouchKind::Down, 10.0, 10.0, 0)
                .expect("outside")
        );
        assert_eq!(slot.press_count(), 0);
        assert!(!slot.last_touch_in_slot());
    }

    #[test]
    fn ime_commit_writes_cjk_without_keycode_table() {
        let mut slot = runtime();
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        assert!(slot.text_input_focused());
        assert!(
            slot.push_ime(&CompositionInput::Commit("你好".into()))
                .expect("commit")
        );
        assert_eq!(slot.input_value(), "你好");
    }

    #[test]
    fn ime_preedit_then_commit_uses_desktop_composition_path() {
        let mut slot = runtime();
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        assert!(
            slot.push_ime(&CompositionInput::Update {
                text: "你".into(),
                selection: Some((0, "你".len())),
            })
            .expect("preedit")
        );
        assert!(
            slot.input_value().is_empty(),
            "preedit must not commit the editor value"
        );
        assert!(
            slot.push_ime(&CompositionInput::Commit("你好".into()))
                .expect("commit")
        );
        assert_eq!(slot.input_value(), "你好");
    }

    /// Tab moves focus on, and a blurred node loses its IME state, so the
    /// composition has to be committed on the way out. Tapping another control
    /// already does this; the keyboard must not be the path that loses text.
    #[test]
    fn tabbing_out_of_a_composing_field_commits_it() {
        let mut slot = runtime();
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        assert!(
            slot.push_ime(&CompositionInput::Update {
                text: "\u{4e16}".into(),
                selection: Some((0, "\u{4e16}".len())),
            })
            .expect("preedit")
        );
        slot.push_key(
            true,
            Some(SlotLogicalKey::Tab),
            SlotKeyMods::default(),
            false,
        )
        .expect("tab");
        assert!(!slot.text_input_focused(), "Tab must hand focus on");
        assert_eq!(slot.input_value(), "\u{4e16}");
    }

    /// The host swallows printable keys for the InputConnection while the field
    /// is focused. That check has to ask the gate too: Runtime focus survives a
    /// tap outside the slot, the slot's claim on the keyboard does not.
    #[test]
    fn a_tap_outside_the_slot_hands_the_keyboard_back() {
        let mut slot = runtime();
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        assert!(slot.accepts_key());
        // The status bar, above Primary.
        let (x, y) = (540.0, 10.0);
        assert!(
            !slot
                .push_touch(SlotTouchKind::Down, x, y, 1)
                .expect("down outside")
        );
        assert!(
            slot.text_input_focused(),
            "Runtime focus is unchanged by a tap the slot never saw"
        );
        assert!(
            !slot.accepts_key(),
            "but the slot no longer owns the keyboard, so keys must not be swallowed"
        );
    }

    #[test]
    fn leaving_text_input_commits_leftover_preedit() {
        let mut slot = runtime();
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        assert!(
            slot.push_ime(&CompositionInput::Update {
                text: "世".into(),
                selection: Some((0, "世".len())),
            })
            .expect("preedit")
        );
        let button = button_id(&slot);
        tap_entity(&mut slot, button);
        assert!(
            !slot.text_input_focused(),
            "leaving the field must disable IME"
        );
        assert_eq!(slot.input_value(), "世");
    }

    #[test]
    fn accessibility_nodes_publish_slot_name_role_value() {
        let mut slot = runtime();
        let nodes = slot.accessibility_nodes();
        assert!(
            nodes.iter().any(|node| node.parent.is_none()),
            "slot tree must publish a root"
        );
        let button = node_with_role(&nodes, AccessibilityRole::Button);
        assert_eq!(button.label.as_deref(), Some(SLOT_BUTTON_LABEL));
        let switch = node_with_role(&nodes, AccessibilityRole::Switch);
        assert_eq!(switch.label.as_deref(), Some(SLOT_SWITCH_LABEL));
        let field = node_with_role(&nodes, AccessibilityRole::TextInput);
        assert!(!field.focused);
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        let nodes = slot.accessibility_nodes();
        let field = node_with_role(&nodes, AccessibilityRole::TextInput);
        assert!(field.focused);
        assert!(
            slot.push_key(
                true,
                Some(SlotLogicalKey::Character('h')),
                SlotKeyMods::default(),
                false,
            )
            .expect("type")
        );
        let nodes = slot.accessibility_nodes();
        let field = node_with_role(&nodes, AccessibilityRole::TextInput);
        assert_eq!(field.value.as_deref(), Some("h"));
    }

    #[test]
    fn accessibility_click_activates_slot_button() {
        let mut slot = runtime();
        let nodes = slot.accessibility_nodes();
        let target = node_with_role(&nodes, AccessibilityRole::Button).id;
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::Click,
            })
            .expect("click")
        );
        assert_eq!(slot.press_count(), 1);
    }

    #[test]
    fn accessibility_click_toggles_slot_switch() {
        let mut slot = runtime();
        let nodes = slot.accessibility_nodes();
        let target = node_with_role(&nodes, AccessibilityRole::Switch).id;
        assert!(!slot.switch_on());
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::Click,
            })
            .expect("click")
        );
        assert!(slot.switch_on());
    }

    #[test]
    fn accessibility_click_focuses_text_input() {
        let mut slot = runtime();
        let nodes = slot.accessibility_nodes();
        let target = node_with_role(&nodes, AccessibilityRole::TextInput).id;
        assert!(!slot.text_input_focused());
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::Click,
            })
            .expect("click")
        );
        assert!(slot.text_input_focused());
    }

    #[test]
    fn accessibility_set_value_and_selection_edit_text_input() {
        let mut slot = runtime();
        let nodes = slot.accessibility_nodes();
        let target = node_with_role(&nodes, AccessibilityRole::TextInput).id;
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::Focus,
            })
            .expect("focus")
        );
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::SetValue("你好".into()),
            })
            .expect("set value")
        );
        assert_eq!(slot.input_value(), "你好");
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::SetSelection(nana_ui::runtime::TextSelection::new(
                    0,
                    "你".len()
                )),
            })
            .expect("set selection")
        );
        let nodes = slot.accessibility_nodes();
        let field = node_with_role(&nodes, AccessibilityRole::TextInput);
        assert_eq!(field.value.as_deref(), Some("你好"));
        assert_eq!(
            field
                .selection
                .map(|selection| (selection.anchor, selection.focus)),
            Some((0, "你".len()))
        );
    }

    #[test]
    fn editor_info_mirrors_single_line_text_input() {
        let mut slot = runtime();
        assert!(slot.editor_info().is_none());
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        let info = slot.editor_info().expect("focused editor info");
        assert_eq!(
            info,
            editor_info_from_request(false, false),
            "slot field is a single-line non-password input"
        );
    }

    #[test]
    fn ime_buffer_mirrors_preedit_without_committing() {
        let mut slot = runtime();
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        assert!(
            slot.push_ime(&CompositionInput::Update {
                text: "你".into(),
                selection: Some((0, "你".len())),
            })
            .expect("preedit")
        );
        let buffer = slot.ime_buffer().expect("focused buffer");
        assert_eq!(buffer.text, "你");
        assert_eq!(buffer.compose, Some((0, "你".len())));
        assert!(slot.input_value().is_empty());
    }

    #[test]
    fn ime_buffer_puts_the_preedit_in_place_of_the_selection_it_replaces() {
        let mut slot = runtime();
        let field = field_id(&slot);
        tap_entity(&mut slot, field);
        let nodes = slot.accessibility_nodes();
        let target = node_with_role(&nodes, AccessibilityRole::TextInput).id;
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::SetValue("abcd".into()),
            })
            .expect("set value")
        );
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::SetSelection(nana_ui::runtime::TextSelection::new(
                    1, 3
                )),
            })
            .expect("set selection")
        );
        assert!(
            slot.push_ime(&CompositionInput::Update {
                text: "你".into(),
                selection: None,
            })
            .expect("preedit")
        );
        let buffer = slot.ime_buffer().expect("focused buffer");
        assert_eq!(buffer.text, "a你d", "the preedit replaces bc, as drawn");
        assert_eq!(buffer.compose, Some((1, 1 + "你".len())));
        assert_eq!(
            (buffer.selection_start, buffer.selection_end),
            (1 + "你".len(), 1 + "你".len())
        );
        assert_eq!(
            buffer.utf16_spans(),
            ((2, 2), Some((1, 2))),
            "and in the Java side's UTF-16 units"
        );
        assert_eq!(slot.input_value(), "abcd", "nothing committed");
    }

    fn node_labelled(slot: &SlotRuntime, label: &str) -> nana_ui::runtime::StableNodeId {
        slot.accessibility_nodes()
            .into_iter()
            .find(|node| node.label.as_deref() == Some(label))
            .unwrap_or_else(|| panic!("missing {label:?}"))
            .id
    }

    /// The `index`th option labelled `label` (both segmented controls offer
    /// “跟随系统”: 0 is the theme's, 1 the text size's).
    fn option_labelled(
        slot: &SlotRuntime,
        label: &str,
        index: usize,
    ) -> nana_ui::runtime::StableNodeId {
        slot.accessibility_nodes()
            .into_iter()
            .filter(|node| node.label.as_deref() == Some(label))
            .nth(index)
            .unwrap_or_else(|| panic!("missing {label:?} #{index}"))
            .id
    }

    #[test]
    fn content_fills_primary_and_the_background_covers_the_window() {
        let slot = runtime();
        let world = slot.document().context().world();
        let scale = slot.scale();
        let root = world.layout_box(slot.root.stable_id()).expect("root");
        assert_eq!((root.x, root.y), (0.0, 0.0));
        assert!((root.width - 540.0).abs() < 0.01 && (root.height - 960.0).abs() < 0.01);
        assert_eq!(
            world
                .node_style(slot.root.stable_id())
                .and_then(|style| style.background),
            Some(SemanticColorRole::Background)
        );

        let primary = slot.primary();
        let top = primary.y as f32 / scale;
        let bottom = (primary.y + primary.height) as f32 / scale;
        let page = world
            .layout_box(node_labelled(&slot, PAGE_TITLE))
            .expect("page");
        assert!(
            page.y >= top,
            "page {page:?} must start below the status bar"
        );
        let button = world.layout_box(button_id(&slot)).expect("button");
        assert!(
            button.y + button.height <= bottom,
            "strip {button:?} must end above the navigation bar ({bottom})"
        );
        assert!(
            page.y + page.height <= button.y,
            "page sits above the strip"
        );
    }

    #[test]
    fn insets_move_the_content_without_resizing_the_document() {
        let mut slot = runtime();
        let before = slot
            .document()
            .context()
            .world()
            .layout_box(button_id(&slot))
            .expect("button");
        // The soft keyboard takes over the bottom band.
        slot.resize(
            (1080, 1920),
            2.0,
            SystemInsets {
                top: INSETS.top,
                bottom: 800,
            },
        )
        .expect("ime");
        slot.flush().expect("flush");
        let after = slot
            .document()
            .context()
            .world()
            .layout_box(button_id(&slot))
            .expect("button");
        assert!(
            (before.y - after.y - (800 - INSETS.bottom) as f32 / 2.0).abs() < 0.01,
            "strip must ride the keyboard: {before:?} → {after:?}"
        );
        assert_eq!(slot.primary().y + slot.primary().height, 1920 - 800);
    }

    #[test]
    fn theme_follows_the_system_until_a_choice_is_made() {
        let mut slot = runtime();
        assert_eq!(slot.theme_appearance(), ThemeAppearance::Light);
        slot.set_system_appearance(SystemAppearance {
            dark: true,
            font_scale: 1.0,
        });
        slot.flush().expect("flush");
        assert_eq!(slot.theme_appearance(), ThemeAppearance::Dark);

        let light = option_labelled(&slot, "浅色", 0);
        tap_entity(&mut slot, light);
        assert_eq!(slot.theme_appearance(), ThemeAppearance::Light);
        slot.set_system_appearance(SystemAppearance {
            dark: false,
            font_scale: 1.0,
        });
        slot.set_system_appearance(SystemAppearance {
            dark: true,
            font_scale: 1.0,
        });
        slot.flush().expect("flush");
        assert_eq!(
            slot.theme_appearance(),
            ThemeAppearance::Light,
            "an explicit choice no longer follows the system"
        );

        let system = option_labelled(&slot, "跟随系统", 0);
        tap_entity(&mut slot, system);
        assert_eq!(slot.theme_appearance(), ThemeAppearance::Dark);
    }

    #[test]
    fn tapping_dark_installs_the_dark_theme() {
        let mut slot = runtime();
        let dark = option_labelled(&slot, "深色", 0);
        tap_entity(&mut slot, dark);
        assert_eq!(slot.theme_appearance(), ThemeAppearance::Dark);
        let nodes = slot.accessibility_nodes();
        let dark = nodes.iter().find(|node| node.id == dark).expect("dark");
        assert_eq!(dark.selected, Some(true), "the control shows the choice");
    }

    #[test]
    fn talkback_click_on_an_option_switches_the_theme() {
        let mut slot = runtime();
        let target = option_labelled(&slot, "深色", 0);
        assert!(
            slot.apply_accessibility_action(AccessibilityActionRequest {
                target,
                action: AccessibilityAction::Click,
            })
            .expect("click")
        );
        assert_eq!(slot.theme_appearance(), ThemeAppearance::Dark);
    }

    fn caption_height(slot: &SlotRuntime) -> f32 {
        slot.document()
            .context()
            .world()
            .layout_box(node_labelled(slot, SLOT_TEXT_LABEL))
            .expect("caption")
            .height
    }

    #[test]
    fn text_size_choices_relayout_the_document() {
        let mut slot = runtime();
        let standard = caption_height(&slot);
        let large = option_labelled(&slot, "大", 0);
        tap_entity(&mut slot, large);
        let enlarged = caption_height(&slot);
        assert!(
            enlarged > standard,
            "大 must grow the text: {standard} → {enlarged}"
        );
        let small = option_labelled(&slot, "小", 0);
        tap_entity(&mut slot, small);
        assert!(caption_height(&slot) < standard);
    }

    #[test]
    fn text_size_follows_the_system_font_scale() {
        let mut slot = runtime();
        let standard = caption_height(&slot);
        slot.set_system_appearance(SystemAppearance {
            dark: false,
            font_scale: 1.3,
        });
        slot.flush().expect("flush");
        assert!(caption_height(&slot) > standard);

        let fixed = option_labelled(&slot, "标准", 0);
        tap_entity(&mut slot, fixed);
        assert!((caption_height(&slot) - standard).abs() < 0.01);
    }
}
