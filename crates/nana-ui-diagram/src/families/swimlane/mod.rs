//! Swimlanes: `swimlane-beta [direction]`. The body is flowchart grammar;
//! every top-level `subgraph` is a lane and its nested subgraphs fold into
//! it. Steps share ranks across lanes and never leave their own lane.

use super::block::{draw_node, link_stroke};
use super::flowchart::{self, EndRef, LineKind};
use super::{LayoutCx, ParseCx, graphkit};
use crate::ir::{Align, Geom, Ink, SceneBuilder, Stroke};
use crate::layout::lanes::{self, LaneEdge, LaneGraph, LaneLayout, LaneNode, LaneSpacing, LaneSpec};
use crate::layout::{attach_end, attach_start};
use crate::style::{self, Paint};
use crate::text::{FontSpec, LaidText, Label};
use crate::{DiagramError, DiagramScene, Summary};
use nana_ui_core::{SemanticColorMix, SemanticColorRole as R};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub chart: flowchart::Model,
    /// Lane titles: top-level subgraphs, then an untitled lane for steps
    /// outside every subgraph when there are any.
    pub lanes: Vec<Label>,
    /// The lane of every node.
    pub lane_of: Vec<usize>,
    /// Edges between two steps, as indices into `chart.edges`.
    pub edges: Vec<usize>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let chart = flowchart::parse(body, cx)?;
    let top = |mut s: usize| {
        while let Some(p) = chart.subgraphs[s].parent {
            s = p;
        }
        s
    };
    let roots: Vec<usize> = (0..chart.subgraphs.len()).filter(|&s| chart.subgraphs[s].parent.is_none()).collect();
    let mut lanes: Vec<Label> = roots.iter().map(|&s| chart.subgraphs[s].title.clone()).collect();
    let loose = chart.nodes.iter().any(|n| n.subgraph.is_none());
    if loose {
        lanes.push(Label::default());
    }
    let lane_of = chart
        .nodes
        .iter()
        .map(|n| match n.subgraph {
            Some(s) => roots.iter().position(|&r| r == top(s)).unwrap_or(0),
            None => lanes.len() - 1,
        })
        .collect();
    let mut edges = Vec::new();
    for (i, e) in chart.edges.iter().enumerate() {
        match (e.from, e.to) {
            (EndRef::Node(_), EndRef::Node(_)) => edges.push(i),
            _ => cx.warn(0, "swimlane links join steps, not lanes"),
        }
    }
    Ok(Model { chart, lanes, lane_of, edges })
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let wrap = cx.wrap_width("swimlane.wrappingWidth");
    let pad = [m.node_pad_x, m.node_pad_y];
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let chart = &model.chart;

    let titles = lane_titles(cx, &model.lanes, wrap);
    let mut paints = Vec::with_capacity(chart.nodes.len());
    let mut labels = Vec::with_capacity(chart.nodes.len());
    let mut graph = LaneGraph {
        direction: chart.direction,
        lanes: titles.iter().map(|t| LaneSpec { title: t.as_ref().map_or([0.0, 0.0], LaidText::size) }).collect(),
        ..LaneGraph::default()
    };
    for (n, node) in chart.nodes.iter().enumerate() {
        let paint = Paint::node().with_author(&node.style);
        let face = FontSpec {
            weight: paint.weight.unwrap_or(font.weight),
            italic: paint.italic,
            ..font
        };
        let laid = cx.text.lay(&node.label, face, Some(wrap));
        graph.nodes.push(LaneNode {
            lane: model.lane_of[n],
            size: node.shape.size(laid.size(), pad),
            rank: None,
        });
        paints.push(paint);
        labels.push(laid);
    }
    let mut edge_labels = Vec::with_capacity(model.edges.len());
    for &e in &model.edges {
        let edge = &chart.edges[e];
        let (EndRef::Node(a), EndRef::Node(b)) = (edge.from, edge.to) else { continue };
        let laid = edge.label.as_ref().map(|l| cx.text.lay(l, small, Some(wrap))).filter(|l| !l.is_empty());
        graph.edges.push(LaneEdge {
            from: a,
            to: b,
            label: laid.as_ref().map(|l| [l.width + 2.0 * chip_pad[0], l.height + 2.0 * chip_pad[1]]),
        });
        edge_labels.push((e, laid));
    }
    let placed = lanes::layout(&graph, &spacing(cx), cx.limits)?;

    let mut out = SceneBuilder::new();
    draw_lanes(&mut out, &placed, &titles);
    let marker = cx.marker_size();
    for (k, (e, _)) in edge_labels.iter().enumerate() {
        let edge = &chart.edges[*e];
        let route = &placed.edges[k];
        if edge.line == LineKind::Invisible || route.points.len() < 2 {
            continue;
        }
        let mut points = route.points.clone();
        let (EndRef::Node(a), EndRef::Node(b)) = (edge.from, edge.to) else { continue };
        if a != b {
            attach_start(&mut points, chart.nodes[a].shape, placed.nodes[a]);
            attach_end(&mut points, chart.nodes[b].shape, placed.nodes[b]);
        }
        graphkit::edge(&mut out, &points, link_stroke(edge.line, &edge.style), edge.start, edge.end, marker, m.bend_radius);
    }
    for (k, (e, laid)) in edge_labels.iter().enumerate() {
        if let (Some(laid), Some(center)) = (laid, placed.edges[k].label)
            && chart.edges[*e].line != LineKind::Invisible
        {
            out.chip(laid, center, chip_pad);
        }
    }
    for (n, node) in chart.nodes.iter().enumerate() {
        let rect = placed.nodes[n];
        draw_node(&mut out, node.shape, rect, &paints[n]);
        let laid = &labels[n];
        if !laid.is_empty() {
            let lr = node.shape.label_rect(rect, laid.size());
            let ink = if node.shape.label_outside() { style::TEXT } else { paints[n].text };
            out.text(laid, [lr.x, lr.y], lr.w, Align::Center, ink);
        }
        if let Some((url, tooltip)) = &node.link {
            out.link(rect, url, tooltip.as_deref());
        }
    }
    let content = out.bounds().union(&placed.bounds);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.chart.nodes.len() as u32,
        edges: model.edges.iter().filter(|&&e| model.chart.edges[e].line != LineKind::Invisible).count() as u32,
        groups: model.lanes.len() as u32,
    }
}

/// Lane spacing from the diagram metrics, shared with event modeling.
pub(crate) fn spacing(cx: &LayoutCx<'_>) -> LaneSpacing {
    let m = cx.metrics;
    LaneSpacing {
        rank_sep: m.rank_sep,
        node_sep: m.node_sep * 0.4,
        lane_pad: m.cluster_pad * 1.25,
        track_sep: m.edge_sep,
        header_pad: m.cluster_pad * 0.75,
    }
}

pub(crate) fn lane_titles(cx: &LayoutCx<'_>, lanes: &[Label], wrap: f32) -> Vec<Option<LaidText>> {
    let face = FontSpec::new(cx.metrics.small_size).weight(cx.metrics.strong_weight);
    lanes.iter().map(|l| Some(cx.text.lay(l, face, Some(wrap * 0.75))).filter(|t| !t.is_empty())).collect()
}

const LANE_STRIPE: Ink = Ink::Mix(SemanticColorMix::alpha(R::Subtle, 0.45));

/// Lane bands clipped to one rounded frame: alternate bands striped, the
/// header strip filled, hairline separators between lanes.
pub(crate) fn draw_lanes(out: &mut SceneBuilder, placed: &LaneLayout, titles: &[Option<LaidText>]) {
    if placed.lanes.is_empty() {
        return;
    }
    let frame = placed.lanes.iter().skip(1).fold(placed.lanes[0], |a, b| a.union(b));
    out.clip(Geom::rounded(frame, style::CLUSTER_CORNER));
    for (l, lane) in placed.lanes.iter().enumerate() {
        if l % 2 == 1 {
            out.fill(Geom::rect(*lane), LANE_STRIPE);
        }
        let head = placed.headers[l];
        if head.w > 0.0 && head.h > 0.0 {
            out.fill(Geom::rect(head), style::CLUSTER_FILL);
        }
    }
    let line = Stroke::hairline(style::CLUSTER_STROKE);
    let vertical_lanes = placed.lanes.len() > 1 && (placed.lanes[1].x - placed.lanes[0].x).abs() > 0.5;
    for lane in placed.lanes.iter().skip(1) {
        let end = if vertical_lanes { [lane.x, lane.bottom()] } else { [lane.right(), lane.y] };
        graphkit::line(out, [lane.x, lane.y], end, line);
    }
    // The header strip's inner edge.
    if let Some(head) = placed.headers.first().filter(|h| h.w > 0.0 && h.h > 0.0) {
        if vertical_lanes {
            graphkit::line(out, [frame.x, head.bottom()], [frame.right(), head.bottom()], line);
        } else {
            graphkit::line(out, [head.right(), frame.y], [head.right(), frame.bottom()], line);
        }
    }
    out.unclip();
    out.stroke(Geom::rounded(frame, style::CLUSTER_CORNER), line);
    for (l, title) in titles.iter().enumerate() {
        if let (Some(title), Some(head)) = (title, placed.headers.get(l)) {
            let c = head.center();
            out.text(title, [c[0] - title.width / 2.0, c[1] - title.height / 2.0], title.width, Align::Center, style::MUTED);
        }
    }
}
