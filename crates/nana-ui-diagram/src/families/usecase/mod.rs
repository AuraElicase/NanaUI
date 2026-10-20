//! Use case diagrams: `usecase-beta`.

mod json;
mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::geom::Marker;
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActorLook {
    /// The stick figure.
    Normal,
    /// The figure drawn as outlines.
    Hollow,
    /// A filled silhouette.
    Awesome,
    /// An icon actor: a framed silhouette stands in for the icon.
    Icon,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum NodeKind {
    Actor(ActorLook),
    /// `id("label")` and undeclared endpoints.
    Ellipse,
    /// `id[label]`.
    Rect,
    Note,
    /// `json id@{ ... }`: rows of path and value.
    Json(Vec<(String, String)>),
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub id: String,
    pub label: Label,
    pub kind: NodeKind,
    pub stereotype: Option<String>,
    pub business: bool,
    pub boundary: Option<usize>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone)]
pub(crate) struct Boundary {
    pub id: String,
    pub title: Label,
    /// `type: package`: a title tab instead of a centred title.
    pub package: bool,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EdgeKind {
    Association,
    Include,
    Extend,
    Generalization,
    /// A note's dotted link.
    Note,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub from: usize,
    pub to: usize,
    pub kind: EdgeKind,
    pub start: Option<Marker>,
    pub end: Option<Marker>,
    pub label: Option<Label>,
    pub min_len: u32,
    pub id: Option<String>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub boundaries: Vec<Boundary>,
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
        nodes: model.nodes.iter().filter(|n| n.kind != NodeKind::Note).count() as u32,
        edges: model.edges.iter().filter(|e| e.kind != EdgeKind::Note).count() as u32,
        groups: model.boundaries.len() as u32,
    }
}
