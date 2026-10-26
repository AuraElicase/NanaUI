//! A host with no native window: drives a [`RuntimeProgram`] through the same
//! hooks, in the same order, as the Scene host.
//!
//! For offscreen acceptance and tests of a whole application. Without it a
//! program could only be driven headlessly by calling its internals, because
//! a [`RuntimeProgramContext`] cannot be built outside the framework, and such
//! a harness drifts from what the native host does with every hook added.
//!
//! What differs from a window, on purpose:
//! - A turn presents every visible window. A document with nothing to do
//!   flushes for free, so a caller never has to know which window asked.
//! - Nothing is painted here. Pixels are a separate step on the last flushed
//!   scene ([`HeadlessRuntime::with_document`]); `nana-ui-devtools` paints and
//!   reads them back.
//! - Window requests succeed against virtual windows and are logged:
//!   creation, visibility and size change the window, every other command
//!   only lands in [`HeadlessRuntime::take_window_commands`].
//! - A file dialog is answered at once from [`HeadlessFileDialog`].

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, channel};
use std::time::Instant;

use nana_ui_core::{CompiledTheme, FileDialogError, FileDialogResult, SharedStore, memory_store};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{
    CommittedText, EndpointGeneration, InputPayload, InputSourceId, WindowEvent, WindowGeometry,
    WindowId,
};
use nana_ui_runtime::{
    AccessibilityActionRequest, FrameworkError, HeadlessInput, LayoutViewport, Task, WorkCounters,
};
use nana_ui_scene::{DocumentAccessError, RuntimeDocument};

use crate::runtime_host::HostDocumentAccess;
use crate::window_service::{Control, Request, WindowService};
use crate::{
    GpuContext, HostFailure, NanaTextShaper, ReportHostFailure, ResolvedWindowPresentation,
    RoutedInput, RuntimeAnimationClock, RuntimeProgram, RuntimeProgramContext,
    RuntimeProgramUpdate, RuntimeRedraw, StartupHandle, WindowDescriptor, WindowError,
};

/// Bounds a turn's message drain, as the Scene host bounds its own: an update
/// that keeps dispatching itself cannot hold the turn forever.
const MAX_PROGRAM_DISPATCHES: usize = 64;

/// How the next file dialog a program opens is answered. Each answer is used
/// once; the dialog after it is cancelled unless another one is set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum HeadlessFileDialog {
    #[default]
    Cancel,
    Fail(FileDialogError),
    Select(Vec<PathBuf>),
    /// Paths chosen with the request's filter at this index selected.
    SelectWithFilter(Vec<PathBuf>, usize),
}

/// A virtual window.
#[derive(Debug, Clone)]
pub struct HeadlessWindow {
    pub geometry: WindowGeometry,
    pub tag: Option<Arc<str>>,
    pub visible: bool,
    pub parent: Option<WindowId>,
}

/// Options fixed for the life of the host.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadlessOptions {
    pub reduced_motion: bool,
    pub high_contrast: bool,
}

/// What updates asked for since [`HeadlessRuntime::take_report`].
#[derive(Debug, Clone, Default)]
pub struct HeadlessReport {
    /// Every redraw an update asked for, joined.
    pub redraw: RuntimeRedraw,
    /// How many of the program's answers to messages, wakes, input and
    /// window events asked for a redraw. A frame's own callbacks (animation
    /// frames, `window_frame_presented`, `bind_window`) are the frame cycle
    /// and are left out, as are messages
    /// [`HeadlessRuntime::set_quiet_messages`] names.
    pub redraw_requests: usize,
    pub exit: bool,
}

/// One turn's UI work.
#[derive(Debug, Clone, Copy, Default)]
pub struct HeadlessTurn {
    /// The work of every flush that did something.
    pub work: WorkCounters,
}

struct WindowState {
    window: HeadlessWindow,
    input: Option<HeadlessInput>,
}

type QuietMessages<M> = Box<dyn Fn(&M) -> bool>;

/// A [`RuntimeProgram`] run without a native window.
pub struct HeadlessRuntime<P: RuntimeProgram> {
    program: P,
    gpu: GpuContext,
    options: HeadlessOptions,
    windows: BTreeMap<WindowId, WindowState>,
    service: WindowService,
    requests: Receiver<Request>,
    dispatch: Arc<dyn Fn(P::Message) + Send + Sync>,
    messages: Receiver<P::Message>,
    tasks: SyncSender<Task<P::Message>>,
    startup: StartupHandle,
    store: SharedStore,
    clock: RuntimeAnimationClock,
    installed_theme: Arc<CompiledTheme>,
    text: NanaTextShaper,
    bind_after_present: BTreeSet<WindowId>,
    next_window_id: u64,
    commands: Vec<WindowCommand>,
    file_dialog: HeadlessFileDialog,
    report: HeadlessReport,
    quiet: Option<QuietMessages<P::Message>>,
}

impl<P: RuntimeProgram> HeadlessRuntime<P> {
    /// Initialize the program on `gpu` with a primary window of `geometry`,
    /// then deliver `Ready` and the startup messages, as the Scene host does.
    pub fn new(
        gpu: GpuContext,
        primary: WindowDescriptor,
        geometry: WindowGeometry,
        options: HeadlessOptions,
    ) -> Result<Self, String> {
        let (service, requests) = WindowService::channel(Arc::new(|| {}));
        let (message_tx, messages) = channel::<P::Message>();
        let sender = message_tx.clone();
        let dispatch: Arc<dyn Fn(P::Message) + Send + Sync> = Arc::new(move |message| {
            let _ = sender.send(message);
        });
        let tasks = crate::scene_host::spawn_detached_task_workers(message_tx);
        service.register(WindowId::PRIMARY);
        let mut windows = BTreeMap::new();
        windows.insert(
            WindowId::PRIMARY,
            WindowState {
                window: HeadlessWindow {
                    geometry,
                    tag: primary.tag.clone(),
                    visible: true,
                    parent: None,
                },
                input: None,
            },
        );
        let startup = StartupHandle::detached();
        let store = memory_store();
        let context = context(
            WindowId::PRIMARY,
            &windows[&WindowId::PRIMARY].window,
            &gpu,
            &dispatch,
            &tasks,
            &startup,
            &store,
            &service,
            options,
        );
        let (program, startup_messages) =
            P::initialize(&context).map_err(|error| error.to_string())?;
        let installed_theme = program.theme();
        let mut host = Self {
            program,
            gpu,
            options,
            windows,
            service,
            requests,
            dispatch,
            messages,
            tasks,
            startup,
            store,
            clock: RuntimeAnimationClock::now(),
            installed_theme,
            text: NanaTextShaper::default(),
            // Natively the message that installs a program also binds its
            // window; a program initialized here gets that bind after its
            // first frame.
            bind_after_present: BTreeSet::from([WindowId::PRIMARY]),
            next_window_id: 1 << 63,
            commands: Vec::new(),
            file_dialog: HeadlessFileDialog::Cancel,
            report: HeadlessReport::default(),
            quiet: None,
        };
        host.program.sync_animation_clock(host.clock.epoch());
        host.sync_accessibility_preferences(WindowId::PRIMARY);
        host.window_event(WindowEvent::Ready {
            id: WindowId::PRIMARY,
            geometry,
        });
        for message in startup_messages {
            host.process_message(message);
        }
        Ok(host)
    }

    pub fn program(&self) -> &P {
        &self.program
    }

    /// For application commands a generic host has no words for. An update
    /// the call produces goes through [`Self::apply_update`].
    pub fn program_mut(&mut self) -> &mut P {
        &mut self.program
    }

    pub fn gpu(&self) -> &GpuContext {
        &self.gpu
    }

    /// The clock animations and input are stamped on.
    pub fn clock(&self) -> RuntimeAnimationClock {
        self.clock
    }

    /// The context the program sees for `id`, for calling a hook directly.
    pub fn context_for(&self, id: WindowId) -> RuntimeProgramContext<P::Message> {
        let window = self
            .windows
            .get(&id)
            .or_else(|| self.windows.get(&WindowId::PRIMARY))
            .map(|state| state.window.clone())
            .unwrap_or(HeadlessWindow {
                geometry: WindowGeometry::default(),
                tag: None,
                visible: false,
                parent: None,
            });
        context(
            id,
            &window,
            &self.gpu,
            &self.dispatch,
            &self.tasks,
            &self.startup,
            &self.store,
            &self.service,
            self.options,
        )
    }

    pub fn window(&self, id: WindowId) -> Option<&HeadlessWindow> {
        self.windows.get(&id).map(|state| &state.window)
    }

    pub fn windows(&self) -> impl Iterator<Item = (WindowId, &HeadlessWindow)> {
        self.windows.iter().map(|(id, state)| (*id, &state.window))
    }

    /// The open window whose descriptor carried `tag`.
    pub fn window_by_tag(&self, tag: &str) -> Option<WindowId> {
        self.windows
            .iter()
            .find(|(_, state)| state.window.tag.as_deref() == Some(tag))
            .map(|(id, _)| *id)
    }

    pub fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        self.program.with_document(id, f)
    }

    pub fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        self.program.with_document_mut(id, f)
    }

    /// Answer the next file dialog the program opens with `answer`.
    pub fn set_file_dialog(&mut self, answer: HeadlessFileDialog) {
        self.file_dialog = answer;
    }

    /// Leave updates for these messages out of
    /// [`HeadlessReport::redraw_requests`]: a program whose frame arrivals
    /// redraw the window by design can count everything else.
    pub fn set_quiet_messages(&mut self, quiet: impl Fn(&P::Message) -> bool + 'static) {
        self.quiet = Some(Box::new(quiet));
    }

    /// Window commands the program issued, in order, since the last call.
    pub fn take_window_commands(&mut self) -> Vec<WindowCommand> {
        std::mem::take(&mut self.commands)
    }

    pub fn take_report(&mut self) -> HeadlessReport {
        std::mem::take(&mut self.report)
    }

    /// Deliver `message` as if a task or another thread had dispatched it.
    /// It is applied on the next [`Self::turn`].
    pub fn dispatch(&self, message: P::Message) {
        (self.dispatch)(message);
    }

    /// Apply `message` now, as the host applies a dispatched one.
    pub fn update(&mut self, message: P::Message) {
        self.process_message(message);
    }

    /// Wake the program now, as the host does once its deadline passes: for
    /// work a caller queued where the program only looks when woken.
    pub fn wake(&mut self) {
        let update = self.wake_program(Instant::now());
        let update = update.merge(self.drain_all_program_messages());
        self.apply_update(update);
    }

    /// One turn of the host: window requests, queued messages, due wakeups
    /// and animation frames, then a frame of every visible window.
    pub fn turn(&mut self) -> HeadlessTurn {
        let mut turn = HeadlessTurn::default();
        self.drain_window_requests();
        self.drain_messages();
        let now = Instant::now();
        let mut update = self.drain_all_program_messages();
        if self.program.next_wakeup().is_some_and(|due| due <= now) {
            update = update.merge(self.wake_program(now));
        }
        update = update.merge(self.animation_frames(now));
        update = update.merge(self.drain_all_program_messages());
        self.apply_update(update);
        let visible: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, state)| state.window.visible)
            .map(|(id, _)| *id)
            .collect();
        for id in visible {
            if let Some(work) = self.present(id) {
                turn.work.accumulate(work);
            }
        }
        turn
    }

    /// One frame of `id`, as a native redraw does it, without painting:
    /// queued messages, `prepare_window_frame`, layout, then
    /// `window_frame_presented` and `bind_window` when due. Returns the
    /// flush's work when it did any.
    pub fn present(&mut self, id: WindowId) -> Option<WorkCounters> {
        if !self.windows.contains_key(&id) {
            return None;
        }
        let queued = self.drain_program_messages(id);
        self.apply_update(queued);
        let context = self.context_for(id);
        self.program.prepare_window_frame(id, &context);
        let now = self.clock.runtime_time(Instant::now());
        let (width, height) = context.geometry().logical_size;
        let text = &mut self.text;
        let flushed = self.program.write_document(id, |document| {
            if document.compositor_needs_tick() {
                document.sync_presentation_clock(now);
            }
            document
                .flush(LayoutViewport::new(width, height), text)
                .map(|update| (!update.is_idle()).then(|| document.context().last_work_counters()))
        });
        let work = match flushed {
            Some(Ok(work)) => work,
            Some(Err(error)) => {
                self.program
                    .report_host_failure(HostFailure::FrameDidNotSettle {
                        window: id,
                        error: error.to_string(),
                    });
                return None;
            }
            None => {
                self.program
                    .report_host_failure(HostFailure::MissingDocument { window: id });
                return None;
            }
        };
        let mut update = self.program.window_frame_presented(id, &context);
        if self.bind_after_present.remove(&id) {
            update = update.merge(self.program.bind_window(id, &context));
        }
        self.apply_update(update);
        work
    }

    /// Route `payload` into `id` as the window's input source does, then show
    /// it to the program. A key press is followed by the `text` it types.
    pub fn input(
        &mut self,
        id: WindowId,
        payload: InputPayload,
        text: Option<&str>,
    ) -> Result<(), FrameworkError> {
        let now = self.clock.runtime_time(Instant::now());
        let mut input = self.bound_input(id)?;
        input.set_now(now.max(input.now()));
        let pressed = input.stamp(payload);
        let typed = text
            .filter(|text| !text.is_empty() && matches!(pressed.payload, InputPayload::Key(_)))
            .map(|text| {
                input.stamp(InputPayload::Text(CommittedText {
                    text: text.to_owned(),
                    key: Some(pressed.metadata.sequence),
                }))
            });
        let mut update = RuntimeProgramUpdate::default();
        let mut result = Ok(());
        for event in [Some(pressed), typed].into_iter().flatten() {
            let text = &mut self.text;
            let routed = self.program.write_document(id, |document| {
                document
                    .context_mut()
                    .route_input(&event, input.services_mut(), Some(text))
            });
            let outcome = match routed {
                Some(Ok(outcome)) => outcome,
                Some(Err(error)) => {
                    self.program
                        .report_host_failure(HostFailure::InputDispatch {
                            window: id,
                            error: error.to_string(),
                        });
                    result = Err(FrameworkError::InvalidInput);
                    continue;
                }
                None => {
                    result = Err(FrameworkError::InvalidInput);
                    break;
                }
            };
            match self.program.input_event(
                id,
                RoutedInput {
                    event: &event,
                    pointer_hit: outcome.pointer_hit,
                    disposition: outcome.disposition(),
                },
                &self.context_for(id),
            ) {
                Ok(program) => update = update.merge(self.counted(program)),
                Err(error) => {
                    self.program.report_host_failure(HostFailure::InputHandler {
                        window: id,
                        error: error.to_string(),
                    });
                    result = Err(error);
                }
            }
            if outcome.invalidated_work {
                update = update.merge(RuntimeProgramUpdate::redraw(id));
            }
        }
        if let Some(state) = self.windows.get_mut(&id) {
            state.input = Some(input);
        }
        update = update.merge(self.drain_program_messages(id));
        self.apply_update(update);
        result
    }

    /// Perform an accessibility action, as assistive technology would.
    pub fn accessibility_action(
        &mut self,
        id: WindowId,
        request: AccessibilityActionRequest,
    ) -> Result<(), FrameworkError> {
        let update = self
            .program
            .accessibility_action(id, request, &self.context_for(id))?;
        let update = self.counted(update);
        let update = update.merge(self.drain_program_messages(id));
        self.apply_update(update);
        Ok(())
    }

    /// Deliver a window event to the program.
    pub fn window_event(&mut self, event: WindowEvent) {
        let update = self.deliver(event);
        self.apply_update(update);
    }

    /// Change `id`'s size, as a native resize reports it.
    pub fn resize(&mut self, id: WindowId, logical_size: (f32, f32), scale_factor: f32) {
        let Some(state) = self.windows.get_mut(&id) else {
            return;
        };
        state.window.geometry = geometry(logical_size, scale_factor);
        let geometry = state.window.geometry;
        self.window_event(WindowEvent::Resized { id, geometry });
    }

    /// Hand the program an update its own code produced outside a hook.
    pub fn apply_update(&mut self, update: RuntimeProgramUpdate) {
        self.record(&update);
        let mut pending: std::collections::VecDeque<_> = update.window_commands.into();
        while let Some(command) = pending.pop_front() {
            let update = self.apply_window_command(command);
            self.record(&update);
            pending.extend(update.window_commands);
        }
        self.sync_appearance();
    }

    fn record(&mut self, update: &RuntimeProgramUpdate) {
        self.report.exit |= update.exit;
        let redraw = std::mem::take(&mut self.report.redraw);
        self.report.redraw = redraw.merge(update.redraw.clone());
    }

    /// Count `update` as a program answer that asked for a redraw.
    fn counted(&mut self, update: RuntimeProgramUpdate) -> RuntimeProgramUpdate {
        if !matches!(update.redraw, RuntimeRedraw::None) {
            self.report.redraw_requests += 1;
        }
        update
    }

    fn quiet(&self, message: &P::Message) -> bool {
        self.quiet.as_ref().is_some_and(|quiet| quiet(message))
    }

    /// `message` applied in `id`'s context.
    fn apply_message(&mut self, id: WindowId, message: P::Message) -> RuntimeProgramUpdate {
        let quiet = self.quiet(&message);
        let update = self.program.update(message, &self.context_for(id));
        if quiet { update } else { self.counted(update) }
    }

    /// `event` delivered to the program.
    fn deliver(&mut self, event: WindowEvent) -> RuntimeProgramUpdate {
        let id = event_window(&event);
        let update = self.program.window_event(event, &self.context_for(id));
        self.counted(update)
    }

    fn wake_program(&mut self, now: Instant) -> RuntimeProgramUpdate {
        let update = self.program.wake(now, &self.context_for(WindowId::PRIMARY));
        self.counted(update)
    }

    fn process_message(&mut self, message: P::Message) {
        self.bind_after_present.insert(WindowId::PRIMARY);
        let update = self.apply_message(WindowId::PRIMARY, message);
        self.apply_update(update);
    }

    fn drain_messages(&mut self) {
        let messages: Vec<_> = self.messages.try_iter().collect();
        for message in messages {
            self.process_message(message);
        }
    }

    fn drain_program_messages(&mut self, id: WindowId) -> RuntimeProgramUpdate {
        let mut update = RuntimeProgramUpdate::default();
        for _ in 0..MAX_PROGRAM_DISPATCHES {
            let queued = self
                .program
                .write_document(id, |document| {
                    document.context_mut().take_program_messages()
                })
                .unwrap_or_default();
            if queued.is_empty() {
                break;
            }
            self.bind_after_present.insert(id);
            for boxed in queued {
                let Ok(message) = boxed.downcast::<P::Message>() else {
                    continue;
                };
                update = update.merge(self.apply_message(id, *message));
            }
        }
        update
    }

    fn drain_all_program_messages(&mut self) -> RuntimeProgramUpdate {
        let ids: Vec<_> = self.windows.keys().copied().collect();
        let mut update = RuntimeProgramUpdate::default();
        for id in ids {
            update = update.merge(self.drain_program_messages(id));
        }
        update
    }

    fn animation_frames(&mut self, now: Instant) -> RuntimeProgramUpdate {
        let clock = self.clock;
        let ids: Vec<_> = self.windows.keys().copied().collect();
        let mut update = RuntimeProgramUpdate::default();
        for id in ids {
            let due = self
                .program
                .read_document(id, |document| {
                    clock
                        .next_wakeup(document.context())
                        .is_some_and(|deadline| deadline <= now)
                })
                .unwrap_or(false);
            if !due {
                continue;
            }
            let Some(frame) = self
                .program
                .write_document(id, |document| clock.wake(document.context_mut(), now))
            else {
                continue;
            };
            let had_samples = frame.has_updates();
            match self
                .program
                .animation_frame(id, frame, &self.context_for(id))
            {
                Ok(frame_update) => update = update.merge(frame_update),
                Err(error) => self
                    .program
                    .report_host_failure(HostFailure::AnimationFrame {
                        window: id,
                        error: error.to_string(),
                    }),
            }
            if had_samples {
                update = update.merge(RuntimeProgramUpdate::redraw(id));
            }
        }
        update
    }

    /// A new theme reaches the documents of windows already open.
    fn sync_appearance(&mut self) {
        let theme = self.program.theme();
        if Arc::ptr_eq(&theme, &self.installed_theme) {
            return;
        }
        let ids: Vec<_> = self.windows.keys().copied().collect();
        for id in ids {
            let _ = self.program.with_document_mut(id, |document| {
                document.context_mut().set_theme_tokens(Arc::clone(&theme))
            });
        }
        self.installed_theme = theme;
    }

    fn sync_accessibility_preferences(&mut self, id: WindowId) {
        let options = self.options;
        let _ = self.program.write_document(id, |document| {
            let context = document.context_mut();
            context.set_system_reduced_motion(options.reduced_motion);
            context.set_high_contrast(options.high_contrast);
        });
    }

    fn bound_input(&mut self, id: WindowId) -> Result<HeadlessInput, FrameworkError> {
        let state = self
            .windows
            .get_mut(&id)
            .ok_or(FrameworkError::InvalidInput)?;
        if let Some(input) = state.input.take() {
            return Ok(input);
        }
        let source = InputSourceId(id.0);
        let bound = self.program.write_document(id, |document| {
            let document_id = document.document();
            HeadlessInput::bind_source(
                document.context_mut(),
                source,
                EndpointGeneration(1),
                document_id,
            )
        });
        match bound {
            Some(Ok(input)) => Ok(input),
            _ => Err(FrameworkError::InvalidInput),
        }
    }

    fn drain_window_requests(&mut self) {
        while let Ok(request) = self.requests.try_recv() {
            match request {
                Request::Displays(reply) => reply.finish(Ok(Vec::new())),
                Request::SurfaceInfo(id, generation, reply) => {
                    let error = if self.service.is_current(id, generation) {
                        WindowError::Unsupported("a headless window has no native surface".into())
                    } else {
                        WindowError::WindowClosed
                    };
                    reply.finish(Err(error));
                }
                Request::Material(_, _, _, reply) => reply.finish(Err(WindowError::Unsupported(
                    "a headless window has no material".into(),
                ))),
                Request::Native(_, _, callback) => callback(Err(WindowError::Unsupported(
                    "a headless window has no native handle".into(),
                ))),
                Request::Create(descriptor, reply) => {
                    let id = loop {
                        let id = WindowId(self.next_window_id);
                        self.next_window_id += 1;
                        if !self.windows.contains_key(&id) {
                            break id;
                        }
                    };
                    let result = self
                        .open_window(id, descriptor)
                        .map(|()| self.service.handle(id))
                        .map_err(WindowError::InitializationFailed);
                    reply.finish(result);
                }
                Request::Control(id, generation, control, reply) => {
                    if !self.service.is_current(id, generation) {
                        reply.finish(Err(WindowError::WindowClosed));
                        continue;
                    }
                    let update = self.control_window(id, control);
                    reply.finish(Ok(()));
                    self.apply_update(update);
                }
            }
        }
    }

    fn open_window(&mut self, id: WindowId, descriptor: WindowDescriptor) -> Result<(), String> {
        let scale = self.windows[&WindowId::PRIMARY]
            .window
            .geometry
            .scale_factor;
        let (width, height) = descriptor.initial_size;
        let window = HeadlessWindow {
            geometry: geometry((width as f32, height as f32), scale),
            tag: descriptor.tag.clone(),
            visible: descriptor.visible,
            parent: descriptor.parent,
        };
        self.service.register(id);
        self.windows.insert(
            id,
            WindowState {
                window,
                input: None,
            },
        );
        if let Err(error) = self.program.initialize_window(id, &self.context_for(id)) {
            self.service.unregister(id);
            self.windows.remove(&id);
            self.program.discard_window(id);
            return Err(error);
        }
        self.program.sync_animation_clock(self.clock.epoch());
        self.sync_accessibility_preferences(id);
        let geometry = self.windows[&id].window.geometry;
        self.window_event(WindowEvent::Ready { id, geometry });
        Ok(())
    }

    fn close_window(&mut self, id: WindowId) {
        let Some(state) = self.windows.remove(&id) else {
            return;
        };
        if state.input.is_some() {
            let now = self.clock.runtime_time(Instant::now());
            self.program.write_document(id, |document| {
                let _ = document
                    .context_mut()
                    .unbind_input_source(InputSourceId(id.0), now);
            });
        }
        self.service.unregister(id);
        self.bind_after_present.remove(&id);
        let children: Vec<_> = self
            .windows
            .iter()
            .filter(|(_, child)| child.window.parent == Some(id))
            .map(|(child, _)| *child)
            .collect();
        for child in children {
            self.close_window(child);
        }
        self.window_event(WindowEvent::Closed { id });
    }

    fn set_visible(&mut self, id: WindowId, visible: bool) -> RuntimeProgramUpdate {
        let Some(state) = self.windows.get_mut(&id) else {
            return RuntimeProgramUpdate::default();
        };
        state.window.visible = visible;
        self.deliver(WindowEvent::VisibilityChanged {
            id,
            hidden: !visible,
        })
    }

    fn control_window(&mut self, id: WindowId, control: Control) -> RuntimeProgramUpdate {
        match control {
            Control::Visible(visible) => self.set_visible(id, visible),
            Control::Size((width, height)) => {
                let Some(state) = self.windows.get_mut(&id) else {
                    return RuntimeProgramUpdate::default();
                };
                let scale = state.window.geometry.scale_factor;
                state.window.geometry = geometry((width as f32, height as f32), scale);
                let geometry = state.window.geometry;
                self.deliver(WindowEvent::Resized { id, geometry })
            }
            Control::Command(command) => RuntimeProgramUpdate {
                window_commands: vec![command],
                ..RuntimeProgramUpdate::default()
            },
            _ => RuntimeProgramUpdate::default(),
        }
    }

    /// Log `command` and act on the ones a virtual window has an answer for.
    fn apply_window_command(&mut self, command: WindowCommand) -> RuntimeProgramUpdate {
        let update = match &command {
            WindowCommand::Open { id, settings } if !self.windows.contains_key(id) => {
                if let Err(error) = self.open_window(*id, settings.clone()) {
                    self.window_event(WindowEvent::OpenFailed { id: *id, error });
                }
                RuntimeProgramUpdate::default()
            }
            WindowCommand::Open { id, .. } | WindowCommand::Focus(id) => {
                self.set_visible_if_hidden(*id)
            }
            WindowCommand::Close(id) => {
                self.close_window(*id);
                RuntimeProgramUpdate::default()
            }
            WindowCommand::OpenFileDialog { id, request } => {
                let result = match std::mem::take(&mut self.file_dialog) {
                    HeadlessFileDialog::Cancel => FileDialogResult::cancelled(request.id),
                    HeadlessFileDialog::Fail(error) => FileDialogResult::failed(request.id, error),
                    HeadlessFileDialog::Select(paths) => {
                        FileDialogResult::selected(request.id, paths)
                    }
                    HeadlessFileDialog::SelectWithFilter(paths, filter) => {
                        FileDialogResult::selected(request.id, paths)
                            .with_selected_filter(Some(filter))
                    }
                };
                // Same completion policy as a native dialog.
                let result = request.apply_extension_policy(result);
                self.deliver(WindowEvent::FileDialogCompleted { id: *id, result })
            }
            _ => RuntimeProgramUpdate::default(),
        };
        self.commands.push(command);
        update
    }

    fn set_visible_if_hidden(&mut self, id: WindowId) -> RuntimeProgramUpdate {
        if self
            .windows
            .get(&id)
            .is_some_and(|state| !state.window.visible)
        {
            self.set_visible(id, true)
        } else {
            RuntimeProgramUpdate::default()
        }
    }
}

fn geometry(logical_size: (f32, f32), scale_factor: f32) -> WindowGeometry {
    WindowGeometry {
        logical_size,
        physical_size: (
            (logical_size.0 * scale_factor).round() as u32,
            (logical_size.1 * scale_factor).round() as u32,
        ),
        scale_factor,
        ..WindowGeometry::default()
    }
}

/// A primary window of `logical_size` at `scale_factor`.
pub fn headless_geometry(logical_size: (f32, f32), scale_factor: f32) -> WindowGeometry {
    geometry(logical_size, scale_factor)
}

#[expect(
    clippy::too_many_arguments,
    reason = "Host-owned resources form one callback context"
)]
fn context<M: Send + 'static>(
    id: WindowId,
    window: &HeadlessWindow,
    gpu: &GpuContext,
    dispatch: &Arc<dyn Fn(M) + Send + Sync>,
    tasks: &SyncSender<Task<M>>,
    startup: &StartupHandle,
    store: &SharedStore,
    service: &WindowService,
    options: HeadlessOptions,
) -> RuntimeProgramContext<M> {
    RuntimeProgramContext::new(
        id,
        window.geometry,
        gpu.clone(),
        ResolvedWindowPresentation::closed(),
        crate::CompositionWork::default(),
        Arc::clone(dispatch),
        tasks.clone(),
        None,
        startup.clone(),
    )
    .with_windows(service)
    .with_window_tag(window.tag.clone())
    .with_reduced_motion(options.reduced_motion)
    .with_high_contrast(options.high_contrast)
    .with_store(Arc::clone(store))
}

fn event_window(event: &WindowEvent) -> WindowId {
    match event {
        WindowEvent::FileDialogRejected { id, .. }
        | WindowEvent::FileDialogCompleted { id, .. }
        | WindowEvent::OpenFailed { id, .. }
        | WindowEvent::MousePassthroughChanged { id, .. }
        | WindowEvent::SkipTaskbarChanged { id, .. }
        | WindowEvent::Ready { id, .. }
        | WindowEvent::Resized { id, .. }
        | WindowEvent::Moved { id, .. }
        | WindowEvent::VisibilityChanged { id, .. }
        | WindowEvent::FocusChanged { id, .. }
        | WindowEvent::ReducedMotionChanged { id, .. }
        | WindowEvent::HighContrastChanged { id, .. }
        | WindowEvent::PointerPresenceChanged { id, .. }
        | WindowEvent::CloseRequested { id }
        | WindowEvent::Closed { id }
        | WindowEvent::AppearanceChanged { id, .. }
        | WindowEvent::ModeChanged { id, .. } => *id,
    }
}
