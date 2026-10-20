//! Entity relationship diagrams: `erDiagram`.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::geom::Marker;
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cardinality {
    ExactlyOne,
    ZeroOrOne,
    OneOrMore,
    ZeroOrMore,
}

impl Cardinality {
    pub(crate) fn marker(self) -> Marker {
        match self {
            Self::ExactlyOne => Marker::ErExactlyOne,
            Self::ZeroOrOne => Marker::ErZeroOrOne,
            Self::OneOrMore => Marker::ErOneOrMore,
            Self::ZeroOrMore => Marker::ErZeroOrMore,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Attribute {
    pub ty: String,
    pub name: String,
    /// `PK`, `FK`, `UK`.
    pub keys: Vec<String>,
    pub comment: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Entity {
    pub id: String,
    /// `ID["alias"]`, else the id.
    pub label: String,
    pub attributes: Vec<Attribute>,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone)]
pub(crate) struct Relationship {
    pub from: usize,
    pub to: usize,
    pub from_card: Cardinality,
    pub to_card: Cardinality,
    /// `--`: identifying (solid); `..`: non-identifying (dashed).
    pub identifying: bool,
    pub label: Option<Label>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub direction: Direction,
    pub entities: Vec<Entity>,
    pub relationships: Vec<Relationship>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.entities.len() as u32,
        edges: model.relationships.len() as u32,
        groups: 0,
    }
}
