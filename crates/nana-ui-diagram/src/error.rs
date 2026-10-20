//! Errors end a diagram; warnings only note what was skipped.

use std::fmt;

use crate::DiagramKind;

/// Why a diagram has no drawing. The caller shows the source instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagramError {
    /// Nothing but whitespace, comments and directives.
    Empty,
    /// The first statement names no diagram type this engine knows.
    UnknownType { header: String },
    /// A statement the grammar cannot read. Lines and columns are 1-based.
    Syntax {
        line: u32,
        column: u32,
        message: String,
    },
    /// The source or the drawing grew past a [`crate::Limits`] bound.
    TooLarge(LimitKind),
    /// A known type whose native parser has not landed.
    Unsupported(DiagramKind),
}

impl DiagramError {
    pub fn syntax(line: u32, column: u32, message: impl Into<String>) -> Self {
        Self::Syntax {
            line,
            column,
            message: message.into(),
        }
    }
}

impl fmt::Display for DiagramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty diagram"),
            Self::UnknownType { header } => write!(f, "unknown diagram type `{header}`"),
            Self::Syntax {
                line,
                column,
                message,
            } => write!(f, "{line}:{column}: {message}"),
            Self::TooLarge(kind) => write!(f, "diagram exceeds the {kind:?} limit"),
            Self::Unsupported(kind) => write!(f, "{} diagrams are not supported yet", kind.name()),
        }
    }
}

impl std::error::Error for DiagramError {}

/// Which bound a diagram crossed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LimitKind {
    SourceBytes,
    Lines,
    LabelChars,
    Nodes,
    Edges,
    ClusterDepth,
    Clusters,
    Items,
    TextItems,
    Canvas,
    /// A family-specific count: participants, tasks, commits, points, links,
    /// leaves or bits.
    Elements,
}

/// A statement the parser skipped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    pub line: u32,
    pub message: String,
}
