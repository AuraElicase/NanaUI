//! Class diagrams: `classDiagram` / `classDiagram-v2`.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::geom::Marker;
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

/// One attribute or method line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Member {
    /// As displayed: visibility, name, parameters, type, generics as `<T>`.
    pub text: String,
    pub method: bool,
    pub is_static: bool,
    pub is_abstract: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct Class {
    pub id: String,
    /// The name as displayed, with its generic parameters.
    pub name: String,
    pub annotations: Vec<String>,
    pub attributes: Vec<Member>,
    pub methods: Vec<Member>,
    pub namespace: Option<usize>,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
    pub link: Option<(String, Option<String>)>,
}

#[derive(Debug, Clone)]
pub(crate) struct Namespace {
    pub name: String,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct Relation {
    pub from: usize,
    pub to: usize,
    pub start: Option<Marker>,
    pub end: Option<Marker>,
    pub dashed: bool,
    pub label: Option<Label>,
    pub from_card: Option<String>,
    pub to_card: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Note {
    pub text: Label,
    pub target: Option<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub direction: Direction,
    pub classes: Vec<Class>,
    pub namespaces: Vec<Namespace>,
    pub relations: Vec<Relation>,
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
        nodes: model.classes.len() as u32,
        edges: model.relations.len() as u32,
        groups: model.namespaces.len() as u32,
    }
}
