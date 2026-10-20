//! Flowcharts: `flowchart` / `graph`.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx, graphkit};
use crate::geom::{Marker, Shape};
use crate::ir::{Dash, Stroke};
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::style;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LineKind {
    Solid,
    Thick,
    Dotted,
    Invisible,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Curve {
    /// Orthogonal runs with rounded bends: the NanaUI default.
    Rounded,
    /// Orthogonal runs with sharp bends (`step`, `stepBefore`, `stepAfter`).
    Step,
    /// Straight segments through the route's points (`linear`).
    Linear,
    /// A smooth spline through the route (`basis`, `cardinal`, `natural`,
    /// `monotone*`, `catmullRom`, `bump*`).
    Smooth,
}

impl Curve {
    pub(crate) fn parse(name: &str) -> Option<Self> {
        let name = name.trim().trim_matches(['"', '\'']);
        Some(match name {
            "rounded" => Self::Rounded,
            "step" | "stepBefore" | "stepAfter" => Self::Step,
            "linear" => Self::Linear,
            "basis" | "cardinal" | "natural" | "monotoneX" | "monotoneY" | "catmullRom"
            | "bumpX" | "bumpY" => Self::Smooth,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    /// The author's id, for tests and diagnostics.
    #[allow(dead_code)]
    pub id: String,
    pub label: Label,
    pub shape: Shape,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
    pub link: Option<(String, Option<String>)>,
    pub subgraph: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndRef {
    Node(usize),
    Subgraph(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub from: EndRef,
    pub to: EndRef,
    pub line: LineKind,
    pub start: Option<Marker>,
    pub end: Option<Marker>,
    pub label: Option<Label>,
    pub min_len: u32,
    pub style: AuthorStyle,
    pub curve: Option<Curve>,
}

#[derive(Debug, Clone)]
pub(crate) struct Subgraph {
    pub id: String,
    pub title: Label,
    pub parent: Option<usize>,
    pub direction: Option<Direction>,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
    pub default_curve: Option<Curve>,
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
        edges: model
            .edges
            .iter()
            .filter(|e| e.line != LineKind::Invisible)
            .count() as u32,
        groups: model.subgraphs.len() as u32,
    }
}

/// The stroke of a flowchart-style link after its line kind and the
/// author's `linkStyle`.
pub(crate) fn link_stroke(line: LineKind, author: &AuthorStyle) -> Stroke {
    let mut base = style::edge_stroke();
    match line {
        LineKind::Thick => base.width = crate::ir::StrokeWidth::Thick,
        LineKind::Dotted => base = base.dash(Dash::Dotted),
        _ => {}
    }
    graphkit::line_stroke(base, author)
}
