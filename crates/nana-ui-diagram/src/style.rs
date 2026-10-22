//! The one table of theme roles every family draws with, so diagrams read
//! as one system and follow the installed palette.

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use crate::ir::{Ink, Stroke, StrokeWidth};
use crate::source::color::luminance;
use crate::source::directives::AuthorStyle;

pub const TEXT: Ink = Ink::Role(R::Text);
pub const MUTED: Ink = Ink::Role(R::Muted);
pub const FAINT: Ink = Ink::Role(R::Faint);
pub const ACCENT: Ink = Ink::Role(R::Accent);
pub const ACCENT_TEXT: Ink = Ink::Role(R::AccentText);

/// Node body.
pub const NODE_FILL: Ink = Ink::Role(R::Surface);
pub const NODE_STROKE: Ink = Ink::Role(R::BorderStrong);
/// Lines between nodes.
pub const EDGE: Ink = Ink::Mix(SemanticColorMix::new(R::Text, R::BorderStrong, 0.55));
/// The patch behind an edge label.
pub const CHIP_FILL: Ink = Ink::Role(R::Surface);
pub const CHIP_STROKE: Ink = Ink::Role(R::BorderSoft);
pub const CHIP_CORNER: RadiusTier = RadiusTier::Xs;
/// Groups: subgraphs, namespaces, boundaries.
pub const CLUSTER_FILL: Ink = Ink::Mix(SemanticColorMix::alpha(R::Subtle, 0.7));
pub const CLUSTER_STROKE: Ink = Ink::Role(R::Border);
pub const CLUSTER_CORNER: RadiusTier = RadiusTier::Md;
/// Notes.
pub const NOTE_FILL: Ink = Ink::Mix(SemanticColorMix::alpha(R::Warning, 0.14));
pub const NOTE_STROKE: Ink = Ink::Mix(SemanticColorMix::alpha(R::Warning, 0.55));
pub const NOTE_CORNER: RadiusTier = RadiusTier::Xs;
/// Status colours: critical, done, active.
pub const DANGER: Ink = Ink::Role(R::Danger);
pub const SUCCESS: Ink = Ink::Role(R::Success);
pub const WARNING: Ink = Ink::Role(R::Warning);
pub const HIGHLIGHT: Ink = Ink::Role(R::Highlight);
/// Grid and axis lines.
pub const GRID: Ink = Ink::Role(R::BorderSoft);
pub const AXIS: Ink = Ink::Role(R::Border);

/// Categorical series colour `index` (cycling past the palette's end).
pub fn series(index: usize, tone: crate::ir::Tone) -> Ink {
    Ink::Series {
        index: index.min(usize::from(u16::MAX)) as u16,
        tone,
    }
}

pub fn node_stroke() -> Stroke {
    Stroke::hairline(NODE_STROKE)
}

pub fn edge_stroke() -> Stroke {
    Stroke::hairline(EDGE)
}

pub fn cluster_stroke() -> Stroke {
    Stroke::hairline(CLUSTER_STROKE)
}

/// Near-black or near-white, whichever reads on an author's fill.
pub fn text_on(fill: [f32; 4]) -> Ink {
    if fill[3] < 0.35 {
        return TEXT;
    }
    if luminance(fill) > 0.36 {
        Ink::Rgba([0.11, 0.11, 0.12, 1.0])
    } else {
        Ink::Rgba([0.97, 0.97, 0.98, 1.0])
    }
}

/// Fill, stroke and text inks after an author's `style` / `classDef`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Paint {
    pub fill: Ink,
    pub stroke: Stroke,
    pub text: Ink,
    pub weight: Option<u16>,
    pub italic: bool,
}

impl Paint {
    pub fn node() -> Self {
        Self {
            fill: NODE_FILL,
            stroke: node_stroke(),
            text: TEXT,
            weight: None,
            italic: false,
        }
    }

    pub fn with_author(mut self, author: &AuthorStyle) -> Self {
        if let Some(fill) = author.fill {
            self.fill = Ink::Rgba(fill);
            self.text = text_on(fill);
        }
        if let Some(stroke) = author.stroke {
            self.stroke.ink = Ink::Rgba(stroke);
        }
        if let Some(width) = author.stroke_width {
            self.stroke.width = StrokeWidth::Px(width);
        }
        if let Some(dashed) = author.dashed {
            self.stroke.dash = if dashed {
                crate::ir::Dash::Dashed
            } else {
                crate::ir::Dash::Solid
            };
        }
        if let Some(color) = author.color {
            self.text = Ink::Rgba(color);
        }
        self.weight = author.font_weight.or(self.weight);
        self.italic = author.italic.unwrap_or(self.italic);
        self
    }
}
