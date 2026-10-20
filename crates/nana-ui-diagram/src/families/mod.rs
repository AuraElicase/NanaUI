//! One module per diagram type. Each exposes the same items — `Model`,
//! `parse`, `layout` and `summary` — so a family's work stays inside its own
//! directory.

use crate::ir::{Align, Rect, SceneBuilder};
use crate::layout::Spacing;
use crate::source::Config;
use crate::text::{FontSpec, Label, LaidText, Measurer, TextMeasure};
use crate::{
    DiagramError, DiagramKind, DiagramMetrics, DiagramScene, Limits, Meta, Summary, Warning, style,
};

pub(crate) mod graphkit;
pub(crate) mod kit;

macro_rules! families {
    ($($kind:ident => $module:ident),* $(,)?) => {
        $(pub(crate) mod $module;)*

        #[derive(Debug, Clone)]
        pub(crate) enum Model {
            $($kind($module::Model),)*
        }

        pub(crate) fn parse(kind: DiagramKind, body: &str, _first_line: u32, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
            match kind {
                $(DiagramKind::$kind => $module::parse(body, cx).map(Model::$kind),)*
            }
        }

        pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
            match model {
                $(Model::$kind(m) => $module::layout(m, cx),)*
            }
        }

        impl Model {
            pub(crate) fn summary(&self, _kind: DiagramKind) -> Summary {
                match self {
                    $(Model::$kind(m) => $module::summary(m),)*
                }
            }
        }

    };
}

families! {
    Agentflow => agentflow,
    Architecture => architecture,
    Block => block,
    C4 => c4,
    Class => class,
    Cynefin => cynefin,
    Er => er,
    EventModeling => eventmodeling,
    Flowchart => flowchart,
    Gantt => gantt,
    GitGraph => gitgraph,
    Info => info,
    Ishikawa => ishikawa,
    Journey => journey,
    Kanban => kanban,
    Mindmap => mindmap,
    Packet => packet,
    Pie => pie,
    Quadrant => quadrant,
    Radar => radar,
    Railroad => railroad,
    Requirement => requirement,
    Sankey => sankey,
    Sequence => sequence,
    State => state,
    Swimlane => swimlane,
    Timeline => timeline,
    Treemap => treemap,
    TreeView => treeview,
    Usecase => usecase,
    Venn => venn,
    Wardley => wardley,
    XyChart => xychart,
    ZenUml => zenuml,
}

/// Parse `source` with `parse` under default limits and config, for unit
/// tests: the result and the warnings it produced.
#[cfg(test)]
pub(crate) fn parse_for_test<T>(
    source: &str,
    parse: impl FnOnce(&str, &mut ParseCx<'_>) -> Result<T, DiagramError>,
) -> (Result<T, DiagramError>, Vec<Warning>, Meta) {
    let limits = Limits::DEFAULT;
    let config = Config::default();
    let mut cx = ParseCx {
        limits: &limits,
        config: &config,
        meta: Meta::default(),
        warnings: Vec::new(),
    };
    let result = parse(source, &mut cx);
    (result, cx.warnings, cx.meta)
}

/// What a parser reads with and reports to.
pub(crate) struct ParseCx<'a> {
    pub limits: &'a Limits,
    pub config: &'a Config,
    pub meta: Meta,
    pub warnings: Vec<Warning>,
}

impl ParseCx<'_> {
    pub fn warn(&mut self, line: usize, message: impl Into<String>) {
        if self.warnings.len() < 64 {
            self.warnings.push(Warning {
                line: line as u32 + 1,
                message: message.into(),
            });
        }
    }

    /// A label as authored, after the length bound.
    pub fn label(&self, raw: &str) -> Result<Label, DiagramError> {
        self.limits.check_label(raw)?;
        Ok(Label::parse(raw))
    }
}

/// What a layout measures and sizes with.
pub(crate) struct LayoutCx<'a> {
    pub text: Measurer<'a>,
    pub metrics: &'a DiagramMetrics,
    pub limits: &'a Limits,
    pub config: &'a Config,
    pub meta: &'a Meta,
}

impl<'a> LayoutCx<'a> {
    pub fn new(
        measure: &'a dyn TextMeasure,
        metrics: &'a DiagramMetrics,
        limits: &'a Limits,
        config: &'a Config,
        meta: &'a Meta,
    ) -> Self {
        Self {
            text: Measurer::new(measure, metrics.line_height),
            metrics,
            limits,
            config,
            meta,
        }
    }

    pub fn font(&self) -> FontSpec {
        FontSpec::new(self.metrics.font_size)
    }

    pub fn small_font(&self) -> FontSpec {
        FontSpec::new(self.metrics.small_size)
    }

    pub fn title_font(&self) -> FontSpec {
        FontSpec::new(self.metrics.title_size).weight(self.metrics.strong_weight)
    }

    pub fn spacing(&self) -> Spacing {
        let m = self.metrics;
        Spacing {
            node_sep: m.node_sep,
            rank_sep: m.rank_sep,
            edge_sep: m.edge_sep,
            cluster_pad: m.cluster_pad,
            track_sep: m.edge_sep * 0.6,
            port_gap: m.edge_sep,
            self_loop: m.node_sep * 0.6,
        }
    }

    /// Padding inside an edge-label chip.
    pub fn chip_pad(&self) -> [f32; 2] {
        [
            self.metrics.small_size * 0.45,
            self.metrics.small_size * 0.15,
        ]
    }

    /// Head length for line-end markers.
    pub fn marker_size(&self) -> f32 {
        self.metrics.font_size * 0.6
    }

    /// The wrap width a family's config names, or the default.
    pub fn wrap_width(&self, key: &str) -> f32 {
        self.config
            .get_f32(key)
            .filter(|w| *w > 20.0)
            .unwrap_or(self.metrics.wrap_width)
    }

    /// Draw the diagram title above `content` and return the grown bounds.
    pub fn title_above(&self, out: &mut SceneBuilder, content: Rect) -> Rect {
        let Some(title) = self.meta.title.as_deref().filter(|t| !t.trim().is_empty()) else {
            return content;
        };
        let laid: LaidText = self.text.lay(&Label::plain(title), self.title_font(), None);
        let gap = self.metrics.font_size * 0.8;
        let top = content.y - gap - laid.height;
        let x = content.x + (content.w - laid.width) / 2.0;
        out.text(&laid, [x, top], laid.width, Align::Center, style::TEXT);
        content.union(&Rect::new(x, top, laid.width, laid.height))
    }
}
