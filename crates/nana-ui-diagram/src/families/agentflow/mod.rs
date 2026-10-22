//! Agent flows: `agentflow-beta`.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::geom::Shape;
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// The author's id, for tests and diagnostics.
    #[allow(dead_code)]
    pub id: String,
    pub label: Label,
    pub shape: Shape,
    /// The innermost `flow` holding it.
    pub flow: Option<usize>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone)]
pub(crate) struct Flow {
    pub id: String,
    pub title: Label,
    pub parent: Option<usize>,
    /// `@{ view: collapsed }`: drawn as one summary node.
    pub collapsed: bool,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndRef {
    Node(usize),
    Flow(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeKind {
    /// `-->`: control or data flows on.
    Sequence,
    /// `-.-`: the source consults the target.
    Reference,
    /// `--x`: the failure path.
    Failure,
    /// `---`: a plain link.
    Link,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub from: EndRef,
    pub to: EndRef,
    pub kind: EdgeKind,
    /// `-.->`: a reference with an arrow.
    pub arrow: bool,
    pub label: Option<Label>,
    pub min_len: u32,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub flows: Vec<Flow>,
    pub edges: Vec<Edge>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.nodes.len() as u32,
        edges: model.edges.len() as u32,
        groups: model.flows.len() as u32,
    }
}
