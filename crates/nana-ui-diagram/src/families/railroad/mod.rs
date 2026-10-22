//! Railroad diagrams: grammar rules drawn as tracks.
//!
//! `railroad-ebnf-beta`, `railroad-abnf-beta` and `railroad-peg-beta` read
//! their grammar notation; `railroad-beta` reads EBNF plus the constructor
//! calls of the classic railroad-diagram libraries (`sequence(...)`,
//! `choice(...)`, `zeroOrMore(...)`, ...), which every variant also accepts.
//! Each rule becomes one diagram of nested boxes: terminals are pills,
//! non-terminals rectangles, choices branch below the main line, optional
//! parts get a bypass above it and repetitions a loop back below.

mod layout;
mod parse;

use super::{LayoutCx, ParseCx};
use crate::{DiagramError, DiagramScene, Summary};

/// Which notation the header names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Notation {
    #[default]
    Ebnf,
    Abnf,
    Peg,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    /// A literal: a pill.
    Terminal(String),
    /// A rule reference: a rectangle.
    NonTerminal(String),
    /// Character classes, numeric values, prose: a muted box.
    Special(String),
    /// Text on the track.
    Comment(String),
    /// An empty track.
    Skip,
    Sequence(Vec<Expr>),
    /// The first branch runs on the main line.
    Choice(Vec<Expr>),
    /// A bypass above the item.
    Optional(Box<Expr>),
    /// The item once, then a loop back under it through `separator`.
    OneOrMore {
        item: Box<Expr>,
        separator: Option<Box<Expr>>,
        /// A count note under the loop (`2..5`).
        note: Option<String>,
    },
    /// A dashed frame with an optional caption.
    Group(Box<Expr>, Option<String>),
}

#[derive(Debug, Clone)]
pub(crate) struct Rule {
    pub name: String,
    pub expr: Expr,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub rules: Vec<Rule>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    fn count(e: &Expr) -> u32 {
        match e {
            Expr::Terminal(_) | Expr::NonTerminal(_) | Expr::Special(_) => 1,
            Expr::Comment(_) | Expr::Skip => 0,
            Expr::Sequence(items) | Expr::Choice(items) => items.iter().map(count).sum(),
            Expr::Optional(inner) | Expr::Group(inner, _) => count(inner),
            Expr::OneOrMore {
                item, separator, ..
            } => count(item) + separator.as_deref().map_or(0, count),
        }
    }
    Summary {
        nodes: model.rules.iter().map(|r| count(&r.expr)).sum(),
        edges: 0,
        groups: model.rules.len() as u32,
    }
}
