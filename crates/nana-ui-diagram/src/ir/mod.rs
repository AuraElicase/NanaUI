//! The display list a layout produces.
//!
//! Colours are theme roles, corners are radius tiers and line widths are
//! named weights until the runtime records the list against the installed
//! theme, so a laid-out diagram is reused across light, dark and any other
//! palette.

mod builder;
pub mod dump;

use std::sync::Arc;

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole};

pub use builder::SceneBuilder;

use crate::geom::PathData;
use crate::text::FontSpec;

/// An axis-aligned rectangle: origin and size.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn centered(center: [f32; 2], size: [f32; 2]) -> Self {
        Self::new(
            center[0] - size[0] / 2.0,
            center[1] - size[1] / 2.0,
            size[0],
            size[1],
        )
    }

    pub fn center(&self) -> [f32; 2] {
        [self.x + self.w / 2.0, self.y + self.h / 2.0]
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn inset(&self, dx: f32, dy: f32) -> Self {
        Self::new(
            self.x + dx,
            self.y + dy,
            (self.w - 2.0 * dx).max(0.0),
            (self.h - 2.0 * dy).max(0.0),
        )
    }

    pub fn union(&self, other: &Self) -> Self {
        let x = self.x.min(other.x);
        let y = self.y.min(other.y);
        Self::new(
            x,
            y,
            self.right().max(other.right()) - x,
            self.bottom().max(other.bottom()) - y,
        )
    }

    pub fn intersects(&self, other: &Self) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[0] <= self.right() && p[1] >= self.y && p[1] <= self.bottom()
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        Self::new(self.x + dx, self.y + dy, self.w, self.h)
    }
}

/// How a palette colour is picked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ink {
    Role(SemanticColorRole),
    Mix(SemanticColorMix),
    /// The categorical chart palette, cycling past its end.
    Series {
        index: u16,
        tone: Tone,
    },
    /// An author colour from `style` / `classDef`, straight-alpha sRGB.
    Rgba([f32; 4]),
}

impl From<SemanticColorRole> for Ink {
    fn from(role: SemanticColorRole) -> Self {
        Self::Role(role)
    }
}

impl From<SemanticColorMix> for Ink {
    fn from(mix: SemanticColorMix) -> Self {
        Self::Mix(mix)
    }
}

/// A series colour's strength.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tone {
    /// The series colour itself.
    Solid,
    /// A wash of it over the surface, for large fills.
    Soft,
    /// A foreground readable on [`Tone::Solid`].
    On,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Corner {
    None,
    Tier(RadiusTier),
    Px(f32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StrokeWidth {
    /// The theme hairline.
    Hairline,
    /// Twice the hairline: emphasised edges, critical paths.
    Thick,
    Px(f32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dash {
    Solid,
    Dashed,
    Dotted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub width: StrokeWidth,
    pub ink: Ink,
    pub dash: Dash,
    pub cap: Cap,
    pub join: Join,
}

impl Stroke {
    pub fn new(ink: impl Into<Ink>, width: StrokeWidth) -> Self {
        Self {
            width,
            ink: ink.into(),
            dash: Dash::Solid,
            cap: Cap::Butt,
            join: Join::Round,
        }
    }

    pub fn hairline(ink: impl Into<Ink>) -> Self {
        Self::new(ink, StrokeWidth::Hairline)
    }

    pub fn dash(mut self, dash: Dash) -> Self {
        self.dash = dash;
        if dash == Dash::Dotted {
            self.cap = Cap::Round;
        }
        self
    }

    pub fn cap(mut self, cap: Cap) -> Self {
        self.cap = cap;
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Geom {
    Rect { rect: Rect, corner: Corner },
    Ellipse(Rect),
    Path(Arc<PathData>),
}

impl Geom {
    pub fn rect(rect: Rect) -> Self {
        Self::Rect {
            rect,
            corner: Corner::None,
        }
    }

    pub fn rounded(rect: Rect, tier: RadiusTier) -> Self {
        Self::Rect {
            rect,
            corner: Corner::Tier(tier),
        }
    }

    pub fn path(path: PathData) -> Self {
        Self::Path(Arc::new(path))
    }

    /// Bounds, ignoring stroke width.
    pub fn bounds(&self) -> Rect {
        match self {
            Self::Rect { rect, .. } | Self::Ellipse(rect) => *rect,
            Self::Path(path) => path.bounds(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Start,
    Center,
    End,
}

/// One drawing instruction, painted in list order.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Shape {
        geom: Geom,
        fill: Option<Ink>,
        stroke: Option<Stroke>,
    },
    /// One measured single-line run. `rect` is the run's line box: its width
    /// is the measured advance, its height the line advance.
    Text {
        rect: Rect,
        text: Arc<str>,
        font: FontSpec,
        ink: Ink,
        align: Align,
    },
    /// Clip what follows to `geom` until the matching [`Item::ClipEnd`].
    ClipBegin(Geom),
    ClipEnd,
}

/// A link an author attached with `click ... href`.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    pub rect: Rect,
    pub href: Arc<str>,
    pub tooltip: Option<Arc<str>>,
}

/// Element counts, for an accessible description.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub nodes: u32,
    pub edges: u32,
    pub groups: u32,
}

/// A laid-out diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DiagramScene {
    /// Natural size in logical pixels; items lie inside `[0, size]`.
    pub size: [f32; 2],
    pub items: Vec<Item>,
    pub links: Vec<Link>,
    pub summary: Summary,
}
