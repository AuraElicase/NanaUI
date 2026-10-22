//! Block diagrams: `block-beta` / `block`. Blocks flow into a measured
//! column grid, composite blocks nest their own grids, and links run as
//! orthogonal connectors between the blocks' facing sides.

mod layout;
mod parse;

use super::flowchart::LineKind;
use super::{LayoutCx, ParseCx};
use crate::geom::{Marker, Shape};
use crate::source::directives::AuthorStyle;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

/// Which way a block arrow points.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Heads {
    pub left: bool,
    pub right: bool,
    pub up: bool,
    pub down: bool,
}

impl Heads {
    pub(crate) fn horizontal(self) -> bool {
        self.left || self.right || !(self.up || self.down)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Kind {
    Node(Shape),
    Arrow(Heads),
    Space,
    Group { columns: Option<usize> },
}

#[derive(Debug, Clone)]
pub(crate) struct Block {
    /// The author's id, for edges and diagnostics.
    #[allow(dead_code)]
    pub id: String,
    pub label: Label,
    pub kind: Kind,
    /// Columns the block covers in its parent's grid.
    pub span: usize,
    pub children: Vec<usize>,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub from: usize,
    pub to: usize,
    pub label: Option<Label>,
    pub line: LineKind,
    pub start: Option<Marker>,
    pub end: Option<Marker>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// Top-level blocks in source order.
    pub root: Vec<usize>,
    /// `columns N` at the top level; `None` puts everything on one row.
    pub columns: Option<usize>,
    pub blocks: Vec<Block>,
    pub edges: Vec<Edge>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    let count = |f: fn(&Kind) -> bool| model.blocks.iter().filter(|b| f(&b.kind)).count() as u32;
    Summary {
        nodes: count(|k| matches!(k, Kind::Node(_) | Kind::Arrow(_))),
        edges: model
            .edges
            .iter()
            .filter(|e| e.line != LineKind::Invisible)
            .count() as u32,
        groups: count(|k| matches!(k, Kind::Group { .. })),
    }
}
