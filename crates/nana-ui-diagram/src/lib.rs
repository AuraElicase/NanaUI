//! Native Mermaid diagrams for NanaUI.
//!
//! [`parse`] reads Mermaid source into a typed model and [`layout`] places it
//! into a [`DiagramScene`]: a display list of shapes and single-line text runs
//! whose colours, corner radii and stroke widths are still theme roles. The
//! runtime records that list through its custom-paint path, so a theme switch
//! re-records without laying the diagram out again, and the scene draws it as
//! GPU vector meshes and ordinary text.
//!
//! Nothing here touches the retained tree, a font file or the GPU. Text widths
//! come from the caller's [`TextMeasure`]; tests and goldens use
//! [`FixedMeasure`].

pub mod error;
pub mod geom;
pub mod ir;
pub mod kind;
pub mod layout;
pub mod limits;
pub mod source;
pub mod style;
pub mod text;

mod families;

pub use error::{DiagramError, LimitKind, Warning};
pub use families::native;
pub use ir::dump::dump;
pub use ir::{
    Align, Cap, Corner, Dash, DiagramScene, Geom, Ink, Item, Join, Link, Rect, Stroke, StrokeWidth,
    Summary, Tone,
};
pub use kind::DiagramKind;
pub use limits::Limits;
pub use text::{FixedMeasure, FontSpec, TextMeasure};

/// What a diagram says about itself, for accessibility and fallbacks.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meta {
    /// `title` from frontmatter or the diagram's own title statement.
    pub title: Option<String>,
    /// `accTitle:`.
    pub acc_title: Option<String>,
    /// `accDescr:` or `accDescr { ... }`.
    pub acc_descr: Option<String>,
}

/// Sizes and spacing the layout works in. Every value is a logical pixel;
/// the caller derives them from the installed typography and spacing tokens.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiagramMetrics {
    /// Node and body label size.
    pub font_size: f32,
    /// Edge labels, captions, axis ticks.
    pub small_size: f32,
    /// Diagram and cluster titles.
    pub title_size: f32,
    /// Line advance as a multiple of the font size.
    pub line_height: f32,
    /// Medium weight, for titles and headers.
    pub strong_weight: u16,
    /// Horizontal padding between a node's outline and its label.
    pub node_pad_x: f32,
    /// Vertical padding between a node's outline and its label.
    pub node_pad_y: f32,
    /// Gap between neighbouring nodes on one rank.
    pub node_sep: f32,
    /// Gap between ranks.
    pub rank_sep: f32,
    /// Gap between neighbouring edges.
    pub edge_sep: f32,
    /// Inner padding of a subgraph or group box.
    pub cluster_pad: f32,
    /// Labels wrap past this width unless a diagram says otherwise.
    pub wrap_width: f32,
    /// Corner radius of orthogonal edge bends.
    pub bend_radius: f32,
    /// Outer margin around the whole diagram.
    pub margin: f32,
}

impl DiagramMetrics {
    /// The defaults at NanaUI's stock type scale.
    pub const DEFAULT: Self = Self {
        font_size: 14.0,
        small_size: 12.0,
        title_size: 16.0,
        line_height: 1.4,
        strong_weight: 600,
        node_pad_x: 16.0,
        node_pad_y: 10.0,
        node_sep: 40.0,
        rank_sep: 48.0,
        edge_sep: 14.0,
        cluster_pad: 16.0,
        wrap_width: 200.0,
        bend_radius: 8.0,
        margin: 8.0,
    };

    /// Scale every size from a body font size, keeping the stock proportions.
    pub fn from_font_size(font_size: f32) -> Self {
        let k = if font_size.is_finite() && font_size > 0.0 {
            font_size / Self::DEFAULT.font_size
        } else {
            1.0
        };
        let d = Self::DEFAULT;
        Self {
            font_size: d.font_size * k,
            small_size: d.small_size * k,
            title_size: d.title_size * k,
            line_height: d.line_height,
            strong_weight: d.strong_weight,
            node_pad_x: d.node_pad_x * k,
            node_pad_y: d.node_pad_y * k,
            node_sep: d.node_sep * k,
            rank_sep: d.rank_sep * k,
            edge_sep: d.edge_sep * k,
            cluster_pad: d.cluster_pad * k,
            wrap_width: d.wrap_width * k,
            bend_radius: d.bend_radius * k,
            margin: d.margin * k,
        }
    }

    /// A stable identity for caches.
    pub fn key(&self) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        let mut mix = |bits: u64| {
            hash ^= bits;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        };
        for value in [
            self.font_size,
            self.small_size,
            self.title_size,
            self.line_height,
            self.node_pad_x,
            self.node_pad_y,
            self.node_sep,
            self.rank_sep,
            self.edge_sep,
            self.cluster_pad,
            self.wrap_width,
            self.bend_radius,
            self.margin,
        ] {
            mix(u64::from(value.to_bits()));
        }
        mix(u64::from(self.strong_weight));
        hash
    }
}

impl Default for DiagramMetrics {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A parsed diagram, ready for [`layout`].
#[derive(Debug, Clone)]
pub struct Parsed {
    pub kind: DiagramKind,
    pub meta: Meta,
    /// Statements the parser skipped. They never fail the diagram.
    pub warnings: Vec<Warning>,
    pub(crate) config: source::Config,
    pub(crate) model: families::Model,
}

impl Parsed {
    /// Element counts for an accessible description.
    pub fn summary(&self) -> Summary {
        self.model.summary(self.kind)
    }
}

/// Parse Mermaid source.
pub fn parse(source: &str, limits: &Limits) -> Result<Parsed, DiagramError> {
    limits.check_source(source)?;
    let pre = source::preprocess(source)?;
    let kind = kind::detect(&pre.body).ok_or_else(|| DiagramError::UnknownType {
        header: pre
            .body
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("")
            .trim()
            .chars()
            .take(64)
            .collect(),
    })?;
    let mut cx = families::ParseCx {
        limits,
        config: &pre.config,
        meta: pre.meta,
        warnings: Vec::new(),
    };
    let model = families::parse(kind, &pre.body, pre.body_line, &mut cx)?;
    Ok(Parsed {
        kind,
        meta: cx.meta,
        warnings: cx.warnings,
        config: pre.config,
        model,
    })
}

/// Lay a parsed diagram out.
pub fn layout(
    parsed: &Parsed,
    measure: &dyn TextMeasure,
    metrics: &DiagramMetrics,
    limits: &Limits,
) -> Result<DiagramScene, DiagramError> {
    let mut cx = families::LayoutCx::new(measure, metrics, limits, &parsed.config, &parsed.meta);
    let mut scene = families::layout(&parsed.model, &mut cx)?;
    scene.summary = parsed.summary();
    limits.check_scene(&scene)?;
    Ok(scene)
}

/// [`parse`] then [`layout`].
pub fn render(
    source: &str,
    measure: &dyn TextMeasure,
    metrics: &DiagramMetrics,
    limits: &Limits,
) -> Result<DiagramScene, DiagramError> {
    let parsed = parse(source, limits)?;
    layout(&parsed, measure, metrics, limits)
}
