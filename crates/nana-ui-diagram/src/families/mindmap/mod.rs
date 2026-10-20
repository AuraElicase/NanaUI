//! Mind maps: an indented tree of ideas drawn out from a central root.
//!
//! The first-level branches split between the root's right and left so the
//! two sides carry about the same weight; each side is a tidy tree
//! (`layout::tree`) growing away from the root, every branch in one series
//! colour, linked parent to child with smooth curves.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};


/// The node outlines a mind map names with its brackets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NodeShape {
    /// No brackets: the label over an underline.
    Default,
    /// `[...]`
    Square,
    /// `(...)`
    Rounded,
    /// `((...))`
    Circle,
    /// `))...((`
    Bang,
    /// `)...(`
    Cloud,
    /// `{{...}}`
    Hexagon,
}

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub label: Label,
    pub shape: NodeShape,
    /// `::icon(...)`: drawn as a placeholder glyph.
    pub icon: Option<String>,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// Pre-order; node 0 is the root.
    pub nodes: Vec<Node>,
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
        edges: model.nodes.len().saturating_sub(1) as u32,
        groups: 0,
    }
}
