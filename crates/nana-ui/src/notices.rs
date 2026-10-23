//! Notices across an application's windows: one record per notice, projected
//! into the [`ToastStack`] of the window that owns it.
//!
//! A notice is posted from a source (a window, or the application as a whole)
//! and owned by one window from then on; it changes window only when that
//! window is no longer shown, and a hidden window never runs a notice's
//! timeout. The application tells the center which windows are shown and
//! which has focus, and syncs each window's [`NoticeHost`] when it repaints.
//!
//! Every notice has an identity. A notice posted under a key already present
//! replaces that notice's content in place, so a finished task reads as its own
//! notice turning into the result. One-off messages get an identity each,
//! except that the same message again within
//! [`NoticeConfig::repeat_window`] in the same window counts once.
//! [`NoticeConfig::max`] is the usual budget, shown or waiting: the oldest
//! one-off message makes room, and a running task or a result is never
//! silently dropped to keep it.
//!
//! The center knows neither the application's keys (`K`) nor what a notice's
//! button does (`A`): a button pressed comes back from [`NoticeCenter::drain`].

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nana_ui_core::{Icon, ToastTone};
use nana_ui_platform::WindowId;
use nana_ui_runtime::{
    AccessibilityLive, AppContext, DocumentId, Entity, FrameworkError, LayoutBox, MutationQueue,
    PanelInsets, PopoverAlignment, StableNodeId, Stack, Toast, ToastAction, ToastDismissed,
    ToastProgress, ToastStack,
};

/// Where a notice comes from: a window, or the application as a whole.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeSource {
    /// Shown in the window the user is looking at.
    Global,
    /// Shown in this window while it is shown, else in the primary window.
    Window(WindowId),
}

impl Default for NoticeSource {
    fn default() -> Self {
        Self::Window(WindowId::PRIMARY)
    }
}

/// A notice's one button and what it gives back when pressed.
#[derive(Clone, Debug)]
pub struct NoticeAction<A> {
    pub label: String,
    pub action: A,
}

/// One notice's content: what its toast shows, and its button's action.
#[derive(Clone, Debug)]
pub struct Notice<A> {
    pub title: String,
    pub description: Option<String>,
    pub tone: ToastTone,
    /// Replaces the tone's glyph; the tone still colours it.
    pub icon: Option<Icon>,
    pub progress: Option<ToastProgress>,
    pub action: Option<NoticeAction<A>>,
    /// `None` stays until the task ends or the notice is closed.
    pub timeout: Option<Duration>,
    pub dismissible: bool,
}

/// How long a message in `tone` stays: info and success 4 s, a warning 6 s,
/// a failure 8 s.
pub fn message_timeout(tone: ToastTone) -> Duration {
    Duration::from_secs(match tone {
        ToastTone::Info | ToastTone::Success => 4,
        ToastTone::Warning => 6,
        ToastTone::Danger => 8,
    })
}

impl<A> Notice<A> {
    /// A message in `tone`: gone after [`message_timeout`], and one that warns
    /// of a failure can be closed sooner.
    pub fn message(title: impl Into<String>, tone: ToastTone) -> Self {
        Self {
            title: title.into(),
            description: None,
            tone,
            icon: None,
            progress: None,
            action: None,
            timeout: Some(message_timeout(tone)),
            dismissible: tone == ToastTone::Danger,
        }
    }

    /// A task still running, with `progress` and the task's own glyph: it
    /// stays until the task ends.
    pub fn running(title: impl Into<String>, progress: ToastProgress, icon: Icon) -> Self {
        Self {
            title: title.into(),
            description: None,
            tone: ToastTone::Info,
            icon: Some(icon),
            progress: Some(progress),
            action: None,
            timeout: None,
            dismissible: false,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn action(mut self, label: impl Into<String>, action: A) -> Self {
        self.action = Some(NoticeAction {
            label: label.into(),
            action,
        });
        self
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn dismissible(mut self, dismissible: bool) -> Self {
        self.dismissible = dismissible;
        self
    }

    /// Whether `other` would show the same: its toast is left alone.
    fn looks_like(&self, other: &Self) -> bool {
        self.title == other.title
            && self.description == other.description
            && self.tone == other.tone
            && self.icon == other.icon
            && self.progress == other.progress
            && self.action.as_ref().map(|action| &action.label)
                == other.action.as_ref().map(|action| &action.label)
            && self.timeout == other.timeout
            && self.dismissible == other.dismissible
    }

    /// Whether `other` says something else, not just how far along it is:
    /// its words, tone or icon differ. Only that is worth a change in place
    /// ([`AppContext::pulse_toast`]); a bar moving is not.
    fn reads_otherwise(&self, other: &Self) -> bool {
        self.title != other.title
            || self.description != other.description
            || self.tone != other.tone
            || self.icon != other.icon
    }

    fn toast(&self) -> Toast {
        let mut toast = Toast::new(self.title.as_str(), self.tone);
        self.dress(&mut toast);
        toast
    }

    fn dress(&self, toast: &mut Toast) {
        toast.title = self.title.as_str().into();
        toast.description = self.description.as_deref().map(Into::into);
        toast.tone = self.tone;
        toast.icon = self.icon;
        toast.progress = self.progress;
        toast.action = self
            .action
            .as_ref()
            .map(|action| action.label.as_str().into());
        toast.timeout = self.timeout;
        toast.dismissible = self.dismissible;
    }

    /// What a screen reader says for it.
    fn announcement(&self) -> Announcement {
        Announcement {
            title: self.title.clone(),
            description: self.description.clone(),
            tone: self.tone,
        }
    }
}

/// A notice's identity: the application's key, or a one-off message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NoticeId<K> {
    Key(K),
    Message(u64),
}

/// Budget and repeat rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoticeConfig {
    /// Notices kept, shown and waiting together.
    pub max: usize,
    /// The same message again within this long counts once.
    pub repeat_window: Duration,
}

impl Default for NoticeConfig {
    fn default() -> Self {
        Self {
            max: 5,
            repeat_window: Duration::from_secs(1),
        }
    }
}

/// Where a window's notices stand: what [`ToastStack::place_in`] takes, but
/// the viewport.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NoticePlacement {
    pub align: PopoverAlignment,
    pub max_width: f32,
    pub insets: PanelInsets,
    pub max_height: f32,
}

#[derive(Clone, PartialEq)]
struct Announcement {
    title: String,
    description: Option<String>,
    tone: ToastTone,
}

#[derive(Clone, Copy, Debug)]
struct NoticeEvent {
    serial: u64,
    window: WindowId,
    action: bool,
}

/// One notice as the center holds it.
pub struct NoticeEntry<K, A> {
    pub id: NoticeId<K>,
    /// Changes whenever the notice's toast must be rebuilt (it moved window,
    /// or started or stopped being shown elsewhere); callbacks from an older
    /// toast no longer reach it.
    pub serial: u64,
    /// The window showing it; `None` while no window is shown.
    pub owner: Option<WindowId>,
    pub source: NoticeSource,
    pub notice: Notice<A>,
    announced: Option<Announcement>,
    posted: Instant,
}

struct Projection<K, A> {
    id: NoticeId<K>,
    serial: u64,
    toast: Entity<Toast>,
    quiet: bool,
    notice: Notice<A>,
}

/// One window's toast stack and what it shows of the center. Presentation
/// only: content and ownership live in the [`NoticeCenter`].
pub struct NoticeHost<K, A> {
    stack: Entity<ToastStack>,
    projected: Vec<Projection<K, A>>,
    placed: Option<(f32, f32, NoticePlacement)>,
}

impl<K, A> NoticeHost<K, A> {
    /// A toast stack under `parent`, drawn at `z_index`.
    pub fn mount(
        cx: &mut AppContext,
        document: DocumentId,
        parent: StableNodeId,
        z_index: i32,
    ) -> Result<Self, FrameworkError> {
        let mut stack = ToastStack::new();
        stack.style.layout_mut().z_index = Some(z_index);
        let stack = cx.create_detached_component(document, stack)?;
        cx.append_child(Entity::<Stack>::from_stable_id(parent), stack)?;
        Ok(Self {
            stack,
            projected: Vec::new(),
            placed: None,
        })
    }

    pub fn stack(&self) -> Entity<ToastStack> {
        self.stack
    }

    /// The toasts it shows, by the serial of their notice.
    pub fn toasts(&self) -> impl Iterator<Item = (u64, Entity<Toast>, &str)> {
        self.projected.iter().map(|projection| {
            (
                projection.serial,
                projection.toast,
                projection.notice.title.as_str(),
            )
        })
    }

    pub fn len(&self) -> usize {
        self.projected.len()
    }

    pub fn is_empty(&self) -> bool {
        self.projected.is_empty()
    }

    fn place(
        &mut self,
        cx: &mut AppContext,
        width: f32,
        height: f32,
        placement: NoticePlacement,
    ) -> bool {
        if self.placed == Some((width, height, placement)) {
            return false;
        }
        self.placed = Some((width, height, placement));
        let _ = cx.update_component(self.stack, |stack, _| {
            stack.place_in(
                LayoutBox {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                },
                placement.align,
                placement.max_width,
                placement.insets,
                placement.max_height,
            );
        });
        true
    }
}

/// The one authority for notice contents, limits, identities and window
/// ownership.
pub struct NoticeCenter<K, A> {
    config: NoticeConfig,
    entries: Vec<NoticeEntry<K, A>>,
    events: Arc<Mutex<Vec<NoticeEvent>>>,
    next_message: u64,
    next_serial: u64,
    /// Keys the application shows somewhere else for now; no window shows
    /// them in its corner.
    elsewhere: Vec<K>,
    /// Shown windows, in the application's order.
    visible: Vec<WindowId>,
    focused: Option<WindowId>,
    /// Shown before other windows when neither focus nor the primary decides.
    priority: Option<WindowId>,
    /// Focused most recently first.
    recent: Vec<WindowId>,
}

impl<K, A> Default for NoticeCenter<K, A> {
    fn default() -> Self {
        Self::new(NoticeConfig::default())
    }
}

impl<K, A> NoticeCenter<K, A> {
    pub fn new(config: NoticeConfig) -> Self {
        Self {
            config,
            entries: Vec::new(),
            events: Arc::default(),
            next_message: 0,
            next_serial: 0,
            elsewhere: Vec::new(),
            visible: Vec::new(),
            focused: None,
            priority: None,
            recent: Vec::new(),
        }
    }

    pub fn config(&self) -> NoticeConfig {
        self.config
    }

    /// The notices held, oldest first.
    pub fn entries(&self) -> &[NoticeEntry<K, A>] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Drop every notice.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// `id`'s window has focus now, or no longer.
    pub fn focus_changed(&mut self, id: WindowId, focused: bool) {
        if focused {
            self.focused = Some(id);
            self.recent.retain(|window| *window != id);
            self.recent.insert(0, id);
        } else if self.focused == Some(id) {
            self.focused = None;
        }
    }

    /// The window a global notice goes to: the focused one, the primary, the
    /// priority window, the one focused most recently, any shown one.
    fn preferred(&self) -> Option<WindowId> {
        self.focused
            .filter(|id| self.visible.contains(id))
            .or_else(|| {
                self.visible
                    .contains(&WindowId::PRIMARY)
                    .then_some(WindowId::PRIMARY)
            })
            .or_else(|| self.priority.filter(|id| self.visible.contains(id)))
            .or_else(|| {
                self.recent
                    .iter()
                    .copied()
                    .find(|id| self.visible.contains(id))
            })
            .or_else(|| self.visible.first().copied())
    }

    /// Where a window's notice goes when that window is gone: the primary
    /// window first.
    fn fallback(&self) -> Option<WindowId> {
        self.visible
            .contains(&WindowId::PRIMARY)
            .then_some(WindowId::PRIMARY)
            .or_else(|| self.preferred())
    }

    fn owner(&self, source: NoticeSource) -> Option<WindowId> {
        match source {
            NoticeSource::Window(id) if self.visible.contains(&id) => Some(id),
            NoticeSource::Window(_) => self.fallback(),
            NoticeSource::Global => self.preferred(),
        }
    }

    /// The shown windows, and the one preferred after the focused and the
    /// primary window. Focus alone never moves a notice; a notice whose window
    /// is no longer shown moves to the primary window, or the next best.
    pub fn set_visible(&mut self, visible: Vec<WindowId>, priority: Option<WindowId>) {
        self.visible = visible;
        self.priority = priority;
        let fallback = self.fallback();
        let preferred = self.preferred();
        for entry in &mut self.entries {
            let owner = match entry.owner {
                Some(id) if self.visible.contains(&id) => Some(id),
                Some(_) => fallback,
                None => match entry.source {
                    NoticeSource::Window(id) if self.visible.contains(&id) => Some(id),
                    NoticeSource::Window(_) => fallback,
                    NoticeSource::Global => preferred,
                },
            };
            if owner != entry.owner {
                // A fresh toast where it lands: an old one can still be
                // fading out, and its callbacks must not reach this notice.
                self.next_serial += 1;
                entry.serial = self.next_serial;
                entry.owner = owner;
            }
        }
    }

    /// The notice whose button was pressed, or that was closed: it is done.
    /// Returns the actions of pressed buttons, with the window they were
    /// pressed in.
    pub fn drain(&mut self) -> Vec<(NoticeSource, A)> {
        let events = self
            .events
            .lock()
            .map(|mut events| std::mem::take(&mut *events))
            .unwrap_or_default();
        let mut actions = Vec::new();
        for event in events {
            let Some(index) = self.entries.iter().position(|entry| {
                entry.serial == event.serial && entry.owner == Some(event.window)
            }) else {
                continue;
            };
            let entry = self.entries.remove(index);
            if let Some(action) = entry.notice.action.filter(|_| event.action) {
                actions.push((NoticeSource::Window(event.window), action.action));
            }
        }
        actions
    }

    fn next_id(&mut self) -> NoticeId<K> {
        self.next_message += 1;
        NoticeId::Message(self.next_message)
    }

    fn make_room(&mut self) {
        while self.entries.len() >= self.config.max {
            let Some(index) = self
                .entries
                .iter()
                .position(|entry| matches!(entry.id, NoticeId::Message(_)))
            else {
                break;
            };
            self.entries.remove(index);
        }
    }
}

impl<K: PartialEq + Clone, A: Clone> NoticeCenter<K, A> {
    /// Show `notice` under `key`, replacing the content of the notice already
    /// there; it keeps its window.
    pub fn show(&mut self, source: NoticeSource, key: K, notice: Notice<A>, now: Instant) {
        self.post(source, NoticeId::Key(key), notice, now);
    }

    fn post(&mut self, source: NoticeSource, id: NoticeId<K>, notice: Notice<A>, now: Instant) {
        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.id == id) {
            entry.notice = notice;
            return;
        }
        self.make_room();
        let owner = self.owner(source);
        self.next_serial += 1;
        self.entries.push(NoticeEntry {
            id,
            serial: self.next_serial,
            owner,
            source,
            notice,
            announced: None,
            posted: now,
        });
    }

    /// A one-off message, unless the same one went to the same window within
    /// [`NoticeConfig::repeat_window`].
    pub fn message(&mut self, source: NoticeSource, notice: Notice<A>, now: Instant) {
        let owner = self.owner(source);
        let repeat = self.config.repeat_window;
        if self.entries.iter().any(|entry| {
            matches!(entry.id, NoticeId::Message(_))
                && entry.owner == owner
                && (owner.is_some() || entry.source == source)
                && entry.notice.looks_like(&notice)
                && now.saturating_duration_since(entry.posted) < repeat
        }) {
            return;
        }
        let id = self.next_id();
        self.post(source, id, notice, now);
    }

    /// The task under `key` ended: its notice turns into `result` where it is
    /// (a one-off message from then on), or goes away without one.
    pub fn settle(
        &mut self,
        source: NoticeSource,
        key: &K,
        result: Option<Notice<A>>,
        now: Instant,
    ) {
        let id = NoticeId::Key(key.clone());
        let index = self.entries.iter().position(|entry| entry.id == id);
        match (index, result) {
            (Some(index), Some(result)) => {
                let message = self.next_id();
                let entry = &mut self.entries[index];
                entry.notice = result;
                entry.id = message;
                entry.posted = now;
            }
            (Some(_), None) => self.finish(key),
            (None, Some(result)) => self.message(source, result, now),
            (None, None) => {}
        }
    }

    /// The notice under `key` goes away.
    pub fn finish(&mut self, key: &K) {
        self.finish_id(&NoticeId::Key(key.clone()));
    }

    pub fn finish_id(&mut self, id: &NoticeId<K>) {
        self.entries.retain(|entry| entry.id != *id);
    }

    pub fn contains(&self, key: &K) -> bool {
        let id = NoticeId::Key(key.clone());
        self.entries.iter().any(|entry| entry.id == id)
    }

    /// Keys the application shows somewhere else for now (a task's progress
    /// drawn inline on the card that started it): no window shows them in its
    /// corner, and each gets a fresh toast when it comes back.
    pub fn set_shown_elsewhere(&mut self, keys: Vec<K>) {
        if self.elsewhere == keys {
            return;
        }
        for entry in &mut self.entries {
            let NoticeId::Key(key) = &entry.id else {
                continue;
            };
            if self.elsewhere.contains(key) != keys.contains(key) {
                self.next_serial += 1;
                entry.serial = self.next_serial;
            }
        }
        self.elsewhere = keys;
    }

    fn shown_elsewhere(&self, id: &NoticeId<K>) -> bool {
        matches!(id, NoticeId::Key(key) if self.elsewhere.contains(key))
    }

    /// Project the notices `window` owns into its `host`, `width` × `height`
    /// at `placement`. A notice that moved in gets a fresh toast (and
    /// timeout); callbacks from the window it left cannot consume it. Returns
    /// whether the window must be repainted.
    pub fn sync(
        &mut self,
        cx: &mut AppContext,
        host: &mut NoticeHost<K, A>,
        window: WindowId,
        width: f32,
        height: f32,
        placement: NoticePlacement,
    ) -> bool
    where
        K: Send + 'static,
        A: Send + 'static,
    {
        // What reached the screen was said: the same words where it lands next
        // stay quiet.
        let shown = cx.shown_toasts(host.stack);
        for projection in &host.projected {
            if shown.contains(&projection.toast)
                && let Some(entry) = self
                    .entries
                    .iter_mut()
                    .find(|entry| entry.serial == projection.serial)
            {
                entry.announced = Some(projection.notice.announcement());
            }
        }
        let mut changed = host.place(cx, width, height, placement);
        let entries = &self.entries;
        let elsewhere = &self.elsewhere;
        let shown_elsewhere =
            |id: &NoticeId<K>| matches!(id, NoticeId::Key(key) if elsewhere.contains(key));
        host.projected.retain(|projection| {
            let keep = entries.iter().any(|entry| {
                entry.serial == projection.serial
                    && entry.owner == Some(window)
                    && !shown_elsewhere(&entry.id)
            });
            if !keep {
                if shown_elsewhere(&projection.id) {
                    // Drawn elsewhere now: its corner must not stay visible
                    // through the toast's exit fade.
                    let _ = cx.update_component(projection.toast, |toast, _| {
                        toast.style.layout_mut().hidden = true;
                    });
                }
                let _ = cx.remove_toast(host.stack, projection.toast);
                changed = true;
            }
            keep
        });
        let mut silence = MutationQueue::new();
        for index in 0..self.entries.len() {
            let entry = &self.entries[index];
            if entry.owner != Some(window) || self.shown_elsewhere(&entry.id) {
                continue;
            }
            if let Some(projection) = host
                .projected
                .iter_mut()
                .find(|projection| projection.serial == entry.serial)
            {
                if !entry.notice.looks_like(&projection.notice) {
                    projection.quiet &=
                        projection.notice.announcement() == entry.notice.announcement();
                    let _ =
                        cx.update_component(projection.toast, |toast, _| entry.notice.dress(toast));
                    if entry.notice.reads_otherwise(&projection.notice) {
                        let _ = cx.pulse_toast(host.stack, projection.toast);
                    }
                    projection.notice = entry.notice.clone();
                    changed = true;
                }
                if projection.quiet
                    && cx.world().live(projection.toast.stable_id()) != Some(AccessibilityLive::Off)
                {
                    silence.set_live(projection.toast.stable_id(), Some(AccessibilityLive::Off));
                }
                continue;
            }
            let Ok(toast) = cx.push_toast(host.stack, entry.notice.toast()) else {
                continue;
            };
            let events = Arc::clone(&self.events);
            let serial = entry.serial;
            let emit = move |action| {
                if let Ok(mut events) = events.lock() {
                    events.push(NoticeEvent {
                        serial,
                        window,
                        action,
                    });
                }
            };
            let activate = emit.clone();
            let _ = cx.on(toast, move |_, _: &ToastAction, _| activate(true));
            let _ = cx.on(toast, move |_, _: &ToastDismissed, _| emit(false));
            let quiet = entry
                .announced
                .as_ref()
                .is_some_and(|spoken| *spoken == entry.notice.announcement());
            if quiet {
                silence.set_live(toast.stable_id(), Some(AccessibilityLive::Off));
            }
            host.projected.push(Projection {
                id: entry.id.clone(),
                serial: entry.serial,
                toast,
                quiet,
                notice: entry.notice.clone(),
            });
            changed = true;
        }
        if !silence.is_empty() {
            let _ = cx.commit_mutations(silence);
        }
        changed
    }
}

#[cfg(test)]
mod tests;
