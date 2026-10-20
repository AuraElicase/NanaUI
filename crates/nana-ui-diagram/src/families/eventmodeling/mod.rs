//! Event modeling: `eventmodeling`. Time frames run left to right, one
//! column each; screens and processors sit in the trigger lane, commands
//! and read models in the middle lane, events at the bottom, and every
//! namespace (`Inventory.Changed`) opens its own lane of that kind. Frames
//! connect to the frame before unless they reset the flow; `->> NN` adds a
//! relation from an earlier frame.

mod parse;

use nana_ui_core::{SemanticColorMix, SemanticColorRole as R};

use super::swimlane::{draw_lanes, lane_titles, spacing};
use super::{LayoutCx, ParseCx, graphkit};
use crate::geom::Marker;
use crate::ir::{Align, Corner, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::layout::lanes::{self, LaneEdge, LaneGraph, LaneNode, LaneSpec};
use crate::layout::Direction;
use crate::style;
use crate::text::{FontSpec, LaidText, Label};
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum EntityKind {
    Ui,
    Processor,
    Command,
    ReadModel,
    Event,
}

impl EntityKind {
    pub(crate) fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "ui" => Self::Ui,
            "pcr" | "processor" => Self::Processor,
            "cmd" | "command" => Self::Command,
            "rmo" | "readmodel" => Self::ReadModel,
            "evt" | "event" => Self::Event,
            _ => return None,
        })
    }

    /// Which band of lanes the kind lives in, top to bottom.
    fn band(self) -> usize {
        match self {
            Self::Ui | Self::Processor => 0,
            Self::Command | Self::ReadModel => 1,
            Self::Event => 2,
        }
    }

    fn tag(self) -> &'static str {
        match self {
            Self::Ui => "UI",
            Self::Processor => "processor",
            Self::Command => "command",
            Self::ReadModel => "read model",
            Self::Event => "event",
        }
    }

    fn colors(self) -> (Ink, Ink) {
        let tint = |role: R, fill: f32| {
            (
                Ink::Mix(SemanticColorMix::new(role, R::Surface, fill)),
                Ink::Mix(SemanticColorMix::new(role, R::BorderStrong, 0.55)),
            )
        };
        match self {
            Self::Ui => (style::NODE_FILL, style::NODE_STROKE),
            Self::Processor => tint(R::Muted, 0.12),
            Self::Command => tint(R::Accent, 0.16),
            Self::ReadModel => tint(R::Success, 0.16),
            Self::Event => tint(R::Warning, 0.22),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Frame {
    /// `rf` / `resetframe`: no inferred relation from the frame before.
    pub reset: bool,
    pub kind: EntityKind,
    pub namespace: Option<String>,
    pub name: String,
    /// Frames (as indices) this one is related from explicitly.
    pub sources: Vec<usize>,
    pub data: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// In time order.
    pub frames: Vec<Frame>,
}

impl Model {
    /// Inferred and explicit relations, deduplicated, in time order.
    pub(crate) fn relations(&self) -> Vec<(usize, usize)> {
        let mut out = Vec::new();
        for (i, f) in self.frames.iter().enumerate() {
            if i > 0 && !f.reset {
                out.push((i - 1, i));
            }
            for &s in &f.sources {
                if s != i && !out.contains(&(s, i)) {
                    out.push((s, i));
                }
            }
        }
        out
    }

    /// Lanes as `(band, namespace)`, in band order then first use.
    fn lanes(&self) -> Vec<(usize, Option<String>)> {
        let mut lanes: Vec<(usize, Option<String>)> = Vec::new();
        for f in &self.frames {
            let key = (f.kind.band(), f.namespace.clone());
            if !lanes.contains(&key) {
                lanes.push(key);
            }
        }
        lanes.sort_by_key(|(band, ns)| (*band, ns.is_some()));
        lanes
    }
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let mono = FontSpec { mono: true, ..small };
    let wrap = cx.wrap_width("eventmodeling.wrappingWidth") * 0.8;
    let gap = m.small_size * 0.3;

    let lanes = model.lanes();
    let band_name = |band: usize| match band {
        0 => "UI / Automation",
        1 => "Command / Read Model",
        _ => "Events",
    };
    let names: Vec<Label> = lanes
        .iter()
        .map(|(band, ns)| match ns {
            Some(ns) => Label::plain(&format!("{ns}\n{}", band_name(*band))),
            None => Label::plain(band_name(*band)),
        })
        .collect();
    let titles = lane_titles(cx, &names, wrap);

    struct Card {
        tag: LaidText,
        name: LaidText,
        data: Option<LaidText>,
        size: [f32; 2],
    }
    let cards: Vec<Card> = model
        .frames
        .iter()
        .map(|f| {
            let tag = cx.text.lay_plain(f.kind.tag(), small);
            let name = cx.text.lay(&Label::plain(&f.name), font.weight(m.strong_weight), Some(wrap));
            let data = (!f.data.is_empty()).then(|| cx.text.lay(&Label { lines: f.data.iter().map(|d| vec![crate::text::Run::plain(d.clone())]).collect(), markdown: false }, mono, Some(wrap)));
            let mut w = tag.width.max(name.width);
            let mut h = tag.height + gap + name.height;
            if let Some(d) = &data {
                w = w.max(d.width);
                h += gap * 2.0 + d.height;
            }
            Card {
                size: [w.max(m.font_size * 5.0) + 2.0 * m.node_pad_x, h + 2.0 * m.node_pad_y],
                tag,
                name,
                data,
            }
        })
        .collect();

    let relations = model.relations();
    let graph = LaneGraph {
        direction: Direction::LR,
        lanes: titles.iter().map(|t| LaneSpec { title: t.as_ref().map_or([0.0, 0.0], LaidText::size) }).collect(),
        nodes: model
            .frames
            .iter()
            .enumerate()
            .map(|(i, f)| LaneNode {
                lane: lanes.iter().position(|(b, ns)| *b == f.kind.band() && *ns == f.namespace).unwrap_or(0),
                size: cards[i].size,
                rank: Some(i),
            })
            .collect(),
        edges: relations.iter().map(|&(a, b)| LaneEdge { from: a, to: b, label: None }).collect(),
    };
    let placed = lanes::layout(&graph, &spacing(cx), cx.limits)?;

    let mut out = SceneBuilder::new();
    draw_lanes(&mut out, &placed, &titles);
    let marker = cx.marker_size();
    for route in &placed.edges {
        if route.points.len() < 2 {
            continue;
        }
        graphkit::edge(&mut out, &route.points, style::edge_stroke(), None, Some(Marker::Arrow), marker, m.bend_radius);
    }
    for (i, f) in model.frames.iter().enumerate() {
        let rect = placed.nodes[i];
        let card = &cards[i];
        let (fill, stroke) = f.kind.colors();
        out.shape(Geom::Rect { rect, corner: Corner::Tier(nana_ui_core::RadiusTier::Sm) }, Some(fill), Some(Stroke::hairline(stroke)));
        let mut y = rect.y + m.node_pad_y;
        out.text(&card.tag, [rect.x, y], rect.w, Align::Center, style::MUTED);
        y += card.tag.height + gap;
        out.text(&card.name, [rect.x, y], rect.w, Align::Center, style::TEXT);
        y += card.name.height + gap * 2.0;
        if let Some(d) = &card.data {
            out.text(d, [rect.x + m.node_pad_x, y], d.width, Align::Start, style::MUTED);
        }
    }
    let content = out.bounds().union(&placed.bounds);
    let content = if content.w <= 0.0 { Rect::new(0.0, 0.0, 1.0, 1.0) } else { content };
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.frames.len() as u32,
        edges: model.relations().len() as u32,
        groups: model.lanes().len() as u32,
    }
}
