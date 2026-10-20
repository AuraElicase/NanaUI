//! Sequence diagrams: `sequenceDiagram`.
//!
//! The model here is shared with [`super::zenuml`], which lowers its own
//! grammar onto the same participants and event list, so both draw through
//! one layout.

mod layout;
mod parse;

use std::collections::HashMap;

use super::{LayoutCx, ParseCx};
use crate::geom::Marker;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, LimitKind, Summary};


/// How a participant is drawn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParticipantKind {
    Participant,
    Actor,
    Boundary,
    Control,
    Entity,
    Database,
    Collections,
    Queue,
    /// ZenUML's implicit caller of top-level messages: a dot, no box.
    Starter,
}

impl ParticipantKind {
    pub(crate) fn from_type(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "participant" => Self::Participant,
            "actor" => Self::Actor,
            "boundary" => Self::Boundary,
            "control" => Self::Control,
            "entity" => Self::Entity,
            "database" | "db" => Self::Database,
            "collections" | "collection" => Self::Collections,
            "queue" => Self::Queue,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Participant {
    pub label: Label,
    pub kind: ParticipantKind,
    /// The box this participant was declared in.
    pub group: Option<usize>,
    /// The first `link` / `links` URL, made clickable on the header.
    pub link: Option<(String, String)>,
}

/// A `box ... end` around participant columns.
#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub title: Option<Label>,
    /// `None`: the theme's group wash. `Some([.., 0.0])`: transparent.
    pub color: Option<[f32; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    Solid,
    Dashed,
}

#[derive(Debug, Clone)]
pub(crate) struct Message {
    pub from: usize,
    pub to: usize,
    pub line: LineKind,
    pub start: Option<Marker>,
    pub end: Option<Marker>,
    pub label: Label,
    pub wrap: bool,
    /// `+`: activate the receiver as the message arrives.
    pub activate: bool,
    /// `-`: deactivate the sender as the message leaves.
    pub deactivate: bool,
    /// ZenUML `// comment` lines above the message.
    pub comment: Option<Label>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NotePlace {
    Left(usize),
    Right(usize),
    Over(usize, usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FragmentKind {
    Loop,
    Alt,
    Opt,
    Par,
    Critical,
    Break,
    /// ZenUML `try { } catch { } finally { }`.
    Try,
    /// ZenUML `section(...)` and `ref(...)`.
    Section,
    Ref,
    /// A coloured background band without a frame.
    Rect,
}

impl FragmentKind {
    pub(crate) fn keyword(self) -> &'static str {
        match self {
            Self::Loop => "loop",
            Self::Alt => "alt",
            Self::Opt => "opt",
            Self::Par => "par",
            Self::Critical => "critical",
            Self::Break => "break",
            Self::Try => "try",
            Self::Section => "section",
            Self::Ref => "ref",
            Self::Rect => "",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) enum Event {
    Message(Message),
    Note {
        place: NotePlace,
        label: Label,
    },
    Activate(usize),
    Deactivate(usize),
    Open {
        kind: FragmentKind,
        label: Option<Label>,
        /// `rect` colour.
        color: Option<[f32; 4]>,
        comment: Option<Label>,
    },
    /// `else`, `and`, `option`, `catch`, `finally`: a dashed divider.
    Section {
        label: Option<Label>,
    },
    Close,
    /// The next message to this participant creates it.
    Create(usize),
    /// The next message from or to this participant ends its lifeline.
    Destroy(usize),
    /// `autonumber [start [step]]`, or `None` for `autonumber off`.
    Autonumber(Option<(f64, f64)>),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub participants: Vec<Participant>,
    pub groups: Vec<Group>,
    pub events: Vec<Event>,
    /// Draw the participant headers again under the lifelines.
    pub mirror: bool,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    let count = |f: fn(&Event) -> bool| model.events.iter().filter(|e| f(e)).count();
    Summary {
        nodes: model
            .participants
            .iter()
            .filter(|p| p.kind != ParticipantKind::Starter)
            .count() as u32,
        edges: count(|e| matches!(e, Event::Message(_))) as u32,
        groups: (model.groups.len() + count(|e| matches!(e, Event::Open { .. }))) as u32,
    }
}

/// Grows a [`Model`] under the parse limits: both grammars build through
/// it.
#[derive(Debug, Default)]
pub(crate) struct Builder {
    pub model: Model,
    index: HashMap<String, usize>,
    messages: usize,
    fragments: usize,
    depth: usize,
}

impl Builder {
    pub(crate) fn new(mirror: bool) -> Self {
        Self {
            model: Model {
                mirror,
                ..Model::default()
            },
            ..Self::default()
        }
    }

    pub(crate) fn find(&self, id: &str) -> Option<usize> {
        self.index.get(id).copied()
    }

    /// The participant named `id`, created with its id as label on first
    /// mention.
    pub(crate) fn participant(&mut self, cx: &ParseCx<'_>, id: &str) -> Result<usize, DiagramError> {
        if let Some(&i) = self.index.get(id) {
            return Ok(i);
        }
        let limits = cx.limits;
        limits.check_count(
            self.model.participants.len() + 1,
            limits.nodes.min(limits.elements),
            LimitKind::Nodes,
        )?;
        let label = cx.label(id)?;
        let i = self.model.participants.len();
        self.model.participants.push(Participant {
            label,
            kind: ParticipantKind::Participant,
            group: None,
            link: None,
        });
        self.index.insert(id.to_string(), i);
        Ok(i)
    }

    /// An explicit declaration: sets the kind, label and box of a new or
    /// already mentioned participant.
    pub(crate) fn declare(
        &mut self,
        cx: &ParseCx<'_>,
        id: &str,
        label: Option<Label>,
        kind: ParticipantKind,
        group: Option<usize>,
    ) -> Result<usize, DiagramError> {
        let i = self.participant(cx, id)?;
        let p = &mut self.model.participants[i];
        p.kind = kind;
        if let Some(label) = label {
            p.label = label;
        }
        if p.group.is_none() {
            p.group = group;
        }
        Ok(i)
    }

    pub(crate) fn push(&mut self, cx: &ParseCx<'_>, event: Event) -> Result<(), DiagramError> {
        let limits = cx.limits;
        match &event {
            Event::Message(_) => {
                self.messages += 1;
                limits.check_count(self.messages, limits.edges, LimitKind::Edges)?;
            }
            Event::Open { .. } => {
                self.depth += 1;
                self.fragments += 1;
                limits.check_count(self.depth, limits.cluster_depth, LimitKind::ClusterDepth)?;
                limits.check_count(self.fragments, limits.clusters, LimitKind::Clusters)?;
            }
            Event::Close => self.depth = self.depth.saturating_sub(1),
            _ => {}
        }
        limits.check_count(self.model.events.len() + 1, limits.elements, LimitKind::Elements)?;
        self.model.events.push(event);
        Ok(())
    }

    /// Close whatever is still open at the end of the source.
    pub(crate) fn finish(mut self, cx: &mut ParseCx<'_>, line: usize) -> Model {
        if self.depth > 0 {
            cx.warn(line, "unclosed block: closed at the end of the diagram");
        }
        self.model.events.extend(std::iter::repeat_n(Event::Close, self.depth));
        self.model
    }
}

#[cfg(test)]
mod tests {
    use crate::{DiagramError, DiagramMetrics, FixedMeasure, Item, LimitKind, Limits, render};

    fn draw(source: &str) -> Result<crate::DiagramScene, DiagramError> {
        render(source, &FixedMeasure, &DiagramMetrics::DEFAULT, &Limits::DEFAULT)
    }

    #[test]
    fn runaway_sources_end_in_too_large() {
        let mut many = String::from("sequenceDiagram\n");
        for _ in 0..3_100 {
            many.push_str("A->>B: x\n");
        }
        assert_eq!(draw(&many).unwrap_err(), DiagramError::TooLarge(LimitKind::Edges));
        let deep = format!("sequenceDiagram\n{}A->>B: x\n", "loop again\n".repeat(40));
        assert_eq!(draw(&deep).unwrap_err(), DiagramError::TooLarge(LimitKind::ClusterDepth));
        let mut wide = String::from("sequenceDiagram\n");
        for i in 0..1_600 {
            wide.push_str(&format!("participant P{i}\n"));
        }
        assert_eq!(draw(&wide).unwrap_err(), DiagramError::TooLarge(LimitKind::Nodes));
    }

    #[test]
    fn hostile_statements_draw_or_fail_cleanly() {
        let cases = [
            "sequenceDiagram\nactivate A\nactivate A\nactivate A\nactivate A\nactivate A\nactivate A\nactivate A\nactivate A\nA->>A: deep",
            "sequenceDiagram\ndeactivate A\nA-->>-B: never active",
            "sequenceDiagram\nautonumber 1e300 7\nA->>B: big\nautonumber -3.5 0.25\nB->>A: small",
            "sequenceDiagram\nNote over A,B,C: three\nNote left of : nobody\nNote: none",
            "sequenceDiagram\ncreate participant B\ncreate participant B\ndestroy B\ndestroy B",
            "sequenceDiagram\nbox\nbox Red inner\nparticipant A\nend\nend",
            "sequenceDiagram\nrect\nend\nrect hsl(400, 200%, -5%)\nend",
            "sequenceDiagram\nparticipant A@{ \"type\": \"nope\"\nA->>()B: central\nA()->>()B: both",
            "sequenceDiagram\n->>B: no sender\nA->>: no receiver\n-->>",
            "sequenceDiagram\nalt\nelse\nelse\nend\npar\nand\nend\ncritical\noption\nend",
            "zenuml\n}}}\nA.m() {\n return\n return again\n}\nreturn\n@return\nnew\n= x\nif {",
            "zenuml\ntry {\n} finally {\n}\ncatch {\n}\nelse {\n}\ngroup {\n}\n\"Quoted\".call()",
        ];
        for source in cases {
            if let Ok(scene) = draw(source) {
                assert!(scene.size.iter().all(|v| v.is_finite() && *v > 0.0), "{source}");
            }
        }
    }

    #[test]
    fn messages_between_far_columns_widen_the_gaps_they_cross() {
        let short = draw("sequenceDiagram\nA->>B: x\nB->>C: y").unwrap();
        let long = draw(&format!("sequenceDiagram\nA->>B: x\nB->>C: y\nA->>C: {}", "w".repeat(45))).unwrap();
        assert!(long.size[0] > short.size[0] + 150.0);
        let labels = long
            .items
            .iter()
            .filter(|i| matches!(i, Item::Text { text, .. } if text.starts_with("www")))
            .count();
        assert_eq!(labels, 1);
    }
}
