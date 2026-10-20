//! Block diagrams: `block-beta` / `block`. Blocks flow into a measured
//! column grid, composite blocks nest their own grids, and links run as
//! orthogonal connectors between the blocks' facing sides.

mod layout;
mod parse;

use super::flowchart::LineKind;
use super::{LayoutCx, ParseCx};
use crate::geom::{Marker, Shape};
use crate::ir::{Dash, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::source::directives::AuthorStyle;
use crate::style::{self, Paint};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

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
        edges: model.edges.iter().filter(|e| e.line != LineKind::Invisible).count() as u32,
        groups: count(|k| matches!(k, Kind::Group { .. })),
    }
}

/// A node body in one of the shared shapes, with its decorations, painted
/// with the shared role table after the author's style. Swimlanes draw
/// their steps with it too.
pub(crate) fn draw_node(out: &mut SceneBuilder, shape: Shape, rect: Rect, paint: &Paint) {
    let (behind, over) = shape.decorations(rect);
    for geom in behind {
        out.shape(geom, Some(paint.fill), Some(paint.stroke));
    }
    if shape.solid() {
        let ink = match paint.fill {
            Ink::Rgba(_) => paint.fill,
            _ => style::TEXT,
        };
        out.shape(shape.outline(rect), Some(ink), None);
    } else if shape == Shape::FramedCircle {
        out.shape(shape.outline(rect), Some(paint.fill), Some(paint.stroke));
        out.shape(Geom::Ellipse(rect.inset(4.0, 4.0)), Some(style::TEXT), None);
        return;
    } else if shape.has_body() {
        out.shape(shape.outline(rect), Some(paint.fill), Some(paint.stroke));
    }
    let line = Stroke {
        dash: Dash::Solid,
        ..paint.stroke
    };
    for path in over {
        out.stroke(Geom::path(path), line);
    }
}

/// The stroke of a flowchart-style link after its line kind and the
/// author's `linkStyle`.
pub(crate) fn link_stroke(line: LineKind, author: &AuthorStyle) -> Stroke {
    let mut stroke = style::edge_stroke();
    match line {
        LineKind::Thick => stroke.width = crate::ir::StrokeWidth::Thick,
        LineKind::Dotted => stroke = stroke.dash(Dash::Dotted),
        _ => {}
    }
    if let Some(c) = author.stroke {
        stroke.ink = Ink::Rgba(c);
    }
    if let Some(w) = author.stroke_width {
        stroke.width = crate::ir::StrokeWidth::Px(w);
    }
    if let Some(d) = author.dashed {
        stroke.dash = if d { Dash::Dashed } else { Dash::Solid };
    }
    stroke
}
