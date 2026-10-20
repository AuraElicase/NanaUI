//! Architecture diagrams: `architecture-beta`. Services, junctions and
//! nested groups sit on a constraint grid derived from the sides their
//! edges leave and enter by; edges run orthogonally between those sides.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::layout::grid::Side;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone)]
pub(crate) struct Group {
    pub title: Label,
    pub icon: Option<String>,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct Service {
    pub title: Label,
    pub icon: Option<String>,
    pub junction: bool,
    pub parent: Option<usize>,
}

#[derive(Debug, Clone)]
pub(crate) struct Edge {
    pub from: usize,
    pub to: usize,
    pub from_side: Side,
    pub to_side: Side,
    /// `{group}`: the edge leaves or enters the service's group.
    pub from_group: bool,
    pub to_group: bool,
    pub start_arrow: bool,
    pub end_arrow: bool,
    pub label: Option<Label>,
}

#[derive(Debug, Clone)]
pub(crate) struct Align {
    pub row: bool,
    pub members: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub groups: Vec<Group>,
    pub services: Vec<Service>,
    pub edges: Vec<Edge>,
    pub aligns: Vec<Align>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.services.len() as u32,
        edges: model.edges.len() as u32,
        groups: model.groups.len() as u32,
    }
}
