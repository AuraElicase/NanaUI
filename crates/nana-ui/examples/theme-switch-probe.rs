//! Real-window acceptance for program theme changes: when
//! [`ApplicationState::theme`] hands back a new theme, the host installs it in
//! the document of every window already open and redraws that window, with no
//! `set_theme_tokens` call from the application.
//!
//! The probe switches Dark → Light → Dark on presented frames. Each switch is
//! only observed if the host both installed the tokens and asked for the next
//! frame, so a missing install or a missing redraw times out and fails.
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use nana_ui::runtime::view::text;
use nana_ui::{
    ApplicationState, ApplicationWindow, CompiledTheme, RuntimeApplication, RuntimeProgramContext,
    RuntimeProgramUpdate, ThemeAppearance, WindowDescriptor, builtin_theme_arc,
};
use nana_ui_platform::WindowId;
use nana_ui_platform::host::WindowCommand;

static RESULT: Mutex<Option<Result<(), String>>> = Mutex::new(None);

/// The appearances the probe asks for, in order.
const STEPS: [ThemeAppearance; 3] = [
    ThemeAppearance::Dark,
    ThemeAppearance::Light,
    ThemeAppearance::Dark,
];

struct Probe {
    step: usize,
    deadline: Instant,
}

impl Probe {
    fn finish(&self, id: WindowId, result: Result<(), String>) -> RuntimeProgramUpdate {
        *RESULT.lock().unwrap() = Some(result);
        RuntimeProgramUpdate {
            window_commands: vec![WindowCommand::Close(id)],
            ..RuntimeProgramUpdate::default()
        }
    }
}

impl ApplicationState for Probe {
    type Message = ();
    type Error = String;

    fn initialize(_: &RuntimeProgramContext<Self::Message>) -> Result<Self, Self::Error> {
        Ok(Self {
            step: 0,
            deadline: Instant::now() + Duration::from_secs(20),
        })
    }

    fn build(
        &mut self,
        window: &mut ApplicationWindow,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> Result<(), Self::Error> {
        let document = window.document.document();
        window
            .document
            .context_mut()
            .mount_view_root(document, || text("theme switch probe"))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn update(
        &mut self,
        _: (),
        _: &mut HashMap<WindowId, ApplicationWindow>,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        RuntimeProgramUpdate::default()
    }

    fn theme(&self) -> Arc<CompiledTheme> {
        builtin_theme_arc(STEPS[self.step])
    }

    fn presented(
        &mut self,
        window: &mut ApplicationWindow,
        context: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        let id = context.window_id();
        let installed = window
            .document
            .context()
            .world()
            .installed_theme()
            .appearance();
        if installed != STEPS[self.step] {
            // The frame before the host picked up the switch; the next one
            // must carry it.
            return RuntimeProgramUpdate::default();
        }
        if self.step + 1 == STEPS.len() {
            return self.finish(id, Ok(()));
        }
        // Switch, and leave the next frame to the host: nothing here asks
        // for one.
        self.step += 1;
        RuntimeProgramUpdate::default()
    }

    fn next_wakeup(&self) -> Option<Instant> {
        Some(self.deadline)
    }

    fn wake(
        &mut self,
        now: Instant,
        windows: &mut HashMap<WindowId, ApplicationWindow>,
        _: &RuntimeProgramContext<Self::Message>,
    ) -> RuntimeProgramUpdate {
        if now < self.deadline {
            return RuntimeProgramUpdate::default();
        }
        let id = windows.keys().copied().min().unwrap_or(WindowId::PRIMARY);
        self.finish(
            id,
            Err(format!(
                "timed out waiting for {:?} at step {}",
                STEPS[self.step], self.step
            )),
        )
    }
}

fn main() {
    nana_ui::run_runtime::<RuntimeApplication<Probe>>(
        WindowDescriptor::new("Theme switch probe").initial_size(320.0, 200.0),
    )
    .unwrap();
    match RESULT.lock().unwrap().take() {
        Some(Ok(())) => println!("theme switch probe: ok"),
        Some(Err(error)) => panic!("theme switch probe: {error}"),
        None => panic!("theme switch probe: exited without a result"),
    }
}
