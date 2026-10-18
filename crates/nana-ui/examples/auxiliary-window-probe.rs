//! Native ownership regression probe. Start with a preparation window above
//! an initially hidden owner. Commands on stdin: main, top, normal, hide,
//! show, focus, minimize, restore, close, reopen, close-owner, quit. Check native ordering
//! alongside the Ready/ModeChanged/Closed and focus events printed as JSON.
use nana_ui::runtime::{DocumentId, RuntimeDocument, Stack, Text, view::widget};
use nana_ui::{
    DocumentAccessError, RuntimeProgram, RuntimeProgramContext, RuntimeProgramUpdate,
    WindowDescriptor, WindowHandle, run_runtime,
};
use nana_ui_platform::{WindowEvent, WindowId, WindowRole, host::WindowCommand};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    io::{self, BufRead, Write},
};

const GUIDE: WindowId = WindowId(1);
const CARD: WindowId = WindowId(2);
const STAGE: WindowId = WindowId(3);

#[derive(Clone)]
struct Message(String);

struct Probe {
    documents: BTreeMap<WindowId, RuntimeDocument>,
    windows: BTreeMap<WindowId, WindowHandle>,
    generations: BTreeMap<WindowId, u64>,
    presented: std::collections::BTreeSet<WindowId>,
}

fn report(value: serde_json::Value) {
    println!("{value}");
    io::stdout().flush().unwrap();
}

fn descriptor(id: WindowId) -> WindowDescriptor {
    let mut settings = WindowDescriptor::new(if id == STAGE {
        "NanaUI auxiliary underlay"
    } else if id == GUIDE {
        "NanaUI auxiliary guide"
    } else {
        "NanaUI auxiliary card"
    })
    .initial_size(360.0, 320.0)
    .minimum_size(240.0, 200.0);
    settings.parent = Some(WindowId::PRIMARY);
    if std::env::args().any(|argument| argument == "--without-shadows") {
        settings.shadow = nana_ui::WindowShadow::None;
    }
    settings.role = if id == STAGE {
        WindowRole::Underlay
    } else {
        WindowRole::Tool
    };
    settings.focus_on_show = id == GUIDE;
    settings.initial_position = Some(if id == GUIDE {
        (240.0, 200.0)
    } else {
        (420.0, 240.0)
    });
    settings
}

impl RuntimeProgram for Probe {
    type Message = Message;
    type Error = Infallible;

    fn initialize(
        context: &RuntimeProgramContext<Message>,
    ) -> Result<(Self, Vec<Message>), Infallible> {
        let mut documents = BTreeMap::new();
        for (index, id, label) in [
            (1, WindowId::PRIMARY, "Owner"),
            (2, GUIDE, "Preparation"),
            (3, CARD, "Card"),
            (4, STAGE, "Stage underlay"),
        ] {
            let doc_id = DocumentId::new(index).unwrap();
            let mut doc = RuntimeDocument::new(doc_id);
            doc.context_mut()
                .mount_view_root(doc_id, move || {
                    widget(Stack::fill_column(12.0)).children(widget(Text::new(label)))
                })
                .unwrap();
            documents.insert(id, doc);
        }
        let context = context.clone();
        std::thread::spawn(move || {
            for line in io::stdin().lock().lines().map_while(Result::ok) {
                context.dispatch(Message(line));
            }
        });
        Ok((
            Self {
                documents,
                windows: BTreeMap::new(),
                generations: BTreeMap::new(),
                presented: Default::default(),
            },
            vec![],
        ))
    }

    fn with_document<R>(
        &self,
        id: WindowId,
        f: impl FnOnce(&RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(self.documents.get(&id).map(f))
    }

    fn with_document_mut<R>(
        &mut self,
        id: WindowId,
        f: impl FnOnce(&mut RuntimeDocument) -> R,
    ) -> Result<Option<R>, DocumentAccessError> {
        Ok(self.documents.get_mut(&id).map(f))
    }

    fn update(
        &mut self,
        Message(command): Message,
        _: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        #[cfg(target_os = "windows")]
        if let Some(pid) = command
            .strip_prefix("allow-external ")
            .and_then(|pid| pid.trim().parse::<u32>().ok())
        {
            // Test-only handoff: the foreground probe permits its parent
            // inspector to perform one genuine external application switch.
            let granted =
                unsafe { windows::Win32::UI::WindowsAndMessaging::AllowSetForegroundWindow(pid) }
                    .is_ok();
            report(
                serde_json::json!({"kind":"foreground-permission", "pid":pid, "granted":granted}),
            );
            return RuntimeProgramUpdate::default();
        }
        let owner = &self.windows[&WindowId::PRIMARY];
        let mut commands = vec![];
        match command.trim() {
            "main" => {
                let _request = owner.set_visible(true);
                commands.push(WindowCommand::Open {
                    id: STAGE,
                    settings: descriptor(STAGE),
                });
                commands.push(WindowCommand::Open {
                    id: CARD,
                    settings: descriptor(CARD),
                });
            }
            "top" | "normal" => commands.push(WindowCommand::SetAlwaysOnTop {
                id: WindowId::PRIMARY,
                always_on_top: command.trim() == "top",
            }),
            "hide" | "show" => {
                if let Some(card) = self.windows.get(&CARD) {
                    let _request = card.set_visible(command.trim() == "show");
                }
            }
            "focus" => {
                let _request = owner.focus();
            }
            "focus-guide" => {
                if let Some(guide) = self.windows.get(&GUIDE) {
                    let _request = guide.focus();
                }
            }
            "minimize" | "restore" => {
                let _request = owner.set_minimized(command.trim() == "minimize");
            }
            "close" => commands.push(WindowCommand::Close(CARD)),
            "reopen" => commands.push(WindowCommand::Open {
                id: CARD,
                settings: descriptor(CARD),
            }),
            "close-owner" => commands.push(WindowCommand::Close(WindowId::PRIMARY)),
            "quit" => return RuntimeProgramUpdate::exit(),
            _ => {}
        }
        RuntimeProgramUpdate {
            window_commands: commands,
            ..RuntimeProgramUpdate::default()
        }
    }

    fn window_event(
        &mut self,
        event: WindowEvent,
        context: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        report(serde_json::json!({ "event": format!("{event:?}") }));
        if let WindowEvent::FocusChanged { id, focused } = &event {
            report(serde_json::json!({"kind":"focus", "window":id.0, "focused":focused}));
        }
        if let WindowEvent::Ready { id, .. } = event {
            let generation = self.generations.entry(id).or_default();
            *generation += 1;
            report(serde_json::json!({"kind":"ready", "window":id.0, "generation":generation}));
            self.windows.insert(id, context.window());
            if id == WindowId::PRIMARY {
                return RuntimeProgramUpdate {
                    window_commands: vec![WindowCommand::Open {
                        id: GUIDE,
                        settings: descriptor(GUIDE),
                    }],
                    ..RuntimeProgramUpdate::default()
                };
            }
        } else if let WindowEvent::Closed { id } = event {
            self.windows.remove(&id);
            self.presented.remove(&id);
        } else if let WindowEvent::OpenFailed { error, .. } = event {
            panic!("{error}");
        } else if let WindowEvent::CloseRequested {
            id: WindowId::PRIMARY,
        } = event
        {
            return RuntimeProgramUpdate::exit();
        }
        RuntimeProgramUpdate::default()
    }

    fn window_frame_presented(
        &mut self,
        id: WindowId,
        _: &RuntimeProgramContext<Message>,
    ) -> RuntimeProgramUpdate {
        if self.presented.insert(id) {
            report(serde_json::json!({"kind":"presented", "window":id.0,
                "generation":self.generations.get(&id)}));
        }
        RuntimeProgramUpdate::default()
    }
}

fn main() -> Result<(), nana_ui::HostedRunError> {
    let mut settings = WindowDescriptor::new("NanaUI auxiliary owner").initial_size(800.0, 600.0);
    settings.visible = false;
    if std::env::args().any(|argument| argument == "--without-shadows") {
        settings.shadow = nana_ui::WindowShadow::None;
    }
    settings.initial_position = Some((100.0, 100.0));
    run_runtime::<Probe>(settings)
}
