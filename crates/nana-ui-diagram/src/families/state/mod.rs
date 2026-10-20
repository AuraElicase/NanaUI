//! State diagrams: `stateDiagram` / `stateDiagram-v2`.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Normal,
    /// `[*]` as a source: a filled dot.
    Start,
    /// `[*]` as a target: a dot in a ring.
    End,
    Fork,
    Join,
    Choice,
}

/// Where a state lives: a composite state's region, or the top level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub(crate) struct Scope {
    pub composite: Option<usize>,
    pub region: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct State {
    pub id: String,
    /// `state "Label" as id`.
    pub label: Option<Label>,
    /// `id : description` lines.
    pub descriptions: Vec<Label>,
    pub kind: Kind,
    pub scope: Scope,
    /// Set when the state is a composite: its index in `composites`.
    pub composite: Option<usize>,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone)]
pub(crate) struct Composite {
    pub state: usize,
    pub scope: Scope,
    pub direction: Option<Direction>,
    /// Concurrent regions after `--` separators; 0 for none.
    pub regions: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct Transition {
    pub from: usize,
    pub to: usize,
    pub label: Option<Label>,
}

#[derive(Debug, Clone)]
pub(crate) struct Note {
    pub text: Label,
    pub target: Option<usize>,
    /// `left of`: the note comes before its state.
    pub left: bool,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub direction: Direction,
    pub states: Vec<State>,
    pub composites: Vec<Composite>,
    pub transitions: Vec<Transition>,
    pub notes: Vec<Note>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.states.iter().filter(|s| s.composite.is_none()).count() as u32,
        edges: model.transitions.len() as u32,
        groups: model.composites.len() as u32,
    }
}
