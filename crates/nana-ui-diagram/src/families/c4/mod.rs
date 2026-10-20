//! C4 diagrams: `C4Context`, `C4Container`, `C4Component`, `C4Dynamic`
//! and `C4Deployment`. Elements fill rows of `$c4ShapeInRow` and
//! boundaries rows of `$c4BoundaryInRow`, in statement order, as Mermaid
//! places them; relationships run as labelled orthogonal connectors.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ElemKind {
    Person,
    System,
    Container,
    Component,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Variant {
    Plain,
    Db,
    Queue,
}

/// Colours an author set with `UpdateElementStyle` / `UpdateBoundaryStyle`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct Colors {
    pub bg: Option<[f32; 4]>,
    pub font: Option<[f32; 4]>,
    pub border: Option<[f32; 4]>,
}

#[derive(Debug, Clone)]
pub(crate) struct Element {
    pub label: Label,
    pub kind: ElemKind,
    pub variant: Variant,
    pub external: bool,
    pub techn: Option<Label>,
    pub descr: Option<Label>,
    pub parent: Option<usize>,
    pub colors: Colors,
    pub link: Option<String>,
}

impl Element {
    /// The stereotype line: `<<external_system_db>>`.
    pub(crate) fn stereotype(&self) -> String {
        let base = match self.kind {
            ElemKind::Person => "person",
            ElemKind::System => "system",
            ElemKind::Container => "container",
            ElemKind::Component => "component",
        };
        let suffix = match self.variant {
            Variant::Plain => "",
            Variant::Db => "_db",
            Variant::Queue => "_queue",
        };
        let prefix = if self.external { "external_" } else { "" };
        format!("<<{prefix}{base}{suffix}>>")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BoundaryKind {
    Generic,
    Enterprise,
    System,
    Container,
    Node,
}

#[derive(Debug, Clone)]
pub(crate) struct Boundary {
    pub label: Label,
    pub kind: BoundaryKind,
    /// `[type]` under the label.
    pub type_text: Option<String>,
    pub descr: Option<Label>,
    pub parent: Option<usize>,
    pub colors: Colors,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum End {
    Element(usize),
    Boundary(usize),
}

#[derive(Debug, Clone)]
pub(crate) struct Rel {
    pub from: End,
    pub to: End,
    /// The two aliases as written, for `UpdateRelStyle`.
    pub written: (String, String),
    pub label: Option<Label>,
    pub techn: Option<Label>,
    pub both: bool,
    pub text_color: Option<[f32; 4]>,
    pub line_color: Option<[f32; 4]>,
}

#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub elements: Vec<Element>,
    pub boundaries: Vec<Boundary>,
    pub rels: Vec<Rel>,
    pub shape_in_row: usize,
    pub boundary_in_row: usize,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            elements: Vec::new(),
            boundaries: Vec::new(),
            rels: Vec::new(),
            shape_in_row: 4,
            boundary_in_row: 2,
        }
    }
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.elements.len() as u32,
        edges: model.rels.len() as u32,
        groups: model.boundaries.len() as u32,
    }
}
