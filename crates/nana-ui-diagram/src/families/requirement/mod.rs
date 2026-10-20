//! Requirement diagrams: `requirementDiagram`.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockKind {
    Requirement,
    Functional,
    Interface,
    Performance,
    Physical,
    DesignConstraint,
    Element,
}

impl BlockKind {
    pub(crate) fn keyword(word: &str) -> Option<Self> {
        Some(match word {
            "requirement" => Self::Requirement,
            "functionalRequirement" => Self::Functional,
            "interfaceRequirement" => Self::Interface,
            "performanceRequirement" => Self::Performance,
            "physicalRequirement" => Self::Physical,
            "designConstraint" => Self::DesignConstraint,
            "element" => Self::Element,
            _ => return None,
        })
    }

    /// The stereotype shown above the name.
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Requirement => "Requirement",
            Self::Functional => "Functional Requirement",
            Self::Interface => "Interface Requirement",
            Self::Performance => "Performance Requirement",
            Self::Physical => "Physical Requirement",
            Self::DesignConstraint => "Design Constraint",
            Self::Element => "Element",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Block {
    pub id: String,
    pub kind: BlockKind,
    /// Display lines below the name: `ID: 1`, `Text: ...`.
    pub fields: Vec<(String, String)>,
    pub classes: Vec<String>,
    pub style: AuthorStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RelKind {
    Contains,
    Copies,
    Derives,
    Satisfies,
    Verifies,
    Refines,
    Traces,
}

impl RelKind {
    pub(crate) fn parse(word: &str) -> Option<Self> {
        Some(match word.trim().to_ascii_lowercase().as_str() {
            "contains" => Self::Contains,
            "copies" => Self::Copies,
            "derives" => Self::Derives,
            "satisfies" => Self::Satisfies,
            "verifies" => Self::Verifies,
            "refines" => Self::Refines,
            "traces" => Self::Traces,
            _ => return None,
        })
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Copies => "copies",
            Self::Derives => "derives",
            Self::Satisfies => "satisfies",
            Self::Verifies => "verifies",
            Self::Refines => "refines",
            Self::Traces => "traces",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Relation {
    pub from: usize,
    pub to: usize,
    pub kind: RelKind,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub direction: Direction,
    pub blocks: Vec<Block>,
    pub relations: Vec<Relation>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.blocks.len() as u32,
        edges: model.relations.len() as u32,
        groups: 0,
    }
}
