//! Agent flow drawing: flowchart shapes, flows as nested groups, collapsed
//! flows as one summary box, and the three edge semantics: sequence
//! (solid arrow), reference (dotted, no head) and failure (a crossed line
//! in the danger colour).

use nana_ui_core::RadiusTier;

use super::{EdgeKind, EndRef, Model};
use crate::families::LayoutCx;
use crate::families::graphkit::{self, author_font, padded};
use crate::geom::{Marker, Shape};
use crate::ir::{Align, Corner, Dash, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::layout::{self, ClusterSpec, EdgeSpec, End, Graph, NodeSpec, RouteKind, layered};
use crate::source::directives::AuthorStyle;
use crate::style;
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene};

/// What a graph node stands for.
#[derive(Clone, Copy)]
enum Item {
    Node(usize),
    /// A collapsed flow's summary box.
    Collapsed(usize),
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let wrap = cx.wrap_width("agentflow.wrappingWidth");
    let pad = [m.node_pad_x, m.node_pad_y];
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let title_font = FontSpec::new(m.small_size).weight(m.strong_weight);

    // The outermost collapsed flow hiding each flow (itself included),
    // bounded against parent cycles.
    let nf = model.flows.len();
    let folded: Vec<Option<usize>> = (0..nf)
        .map(|f| {
            std::iter::successors(Some(f), |&c| model.flows[c].parent)
                .take(nf + 1)
                .filter(|&c| model.flows[c].collapsed)
                .last()
        })
        .collect();
    // Groups that still draw, renumbered.
    let mut cluster_of_flow = vec![None; nf];
    let mut clusters = Vec::new();
    for f in 0..nf {
        if folded[f].is_none() {
            cluster_of_flow[f] = Some(clusters.len());
            clusters.push(f);
        }
    }
    let container = |flow: Option<usize>| flow.and_then(|f| cluster_of_flow[f]);

    let mut graph = Graph {
        direction: model.direction,
        ..Graph::default()
    };
    let mut items = Vec::new();
    let mut labels: Vec<LaidText> = Vec::new();
    let mut node_item = vec![usize::MAX; model.nodes.len()];
    for (n, node) in model.nodes.iter().enumerate() {
        if node.flow.and_then(|f| folded[f]).is_some() {
            continue;
        }
        let laid = cx.text.lay(&node.label, author_font(font, &node.style), Some(wrap));
        node_item[n] = items.len();
        graph.nodes.push(NodeSpec {
            size: node.shape.size(laid.size(), pad),
            cluster: container(node.flow),
            port_span: match node.shape {
                Shape::Diamond | Shape::Hexagon | Shape::Circle | Shape::DoubleCircle => 0.3,
                _ => 0.7,
            },
        });
        items.push(Item::Node(n));
        labels.push(laid);
    }
    let mut collapsed_item = vec![usize::MAX; nf];
    for f in 0..nf {
        if folded[f] != Some(f) {
            continue;
        }
        let flow = &model.flows[f];
        let laid = cx.text.lay(&flow.title, title_font, Some(wrap));
        let more = cx.text.lay_plain("···", small);
        collapsed_item[f] = items.len();
        graph.nodes.push(NodeSpec {
            cluster: container(flow.parent),
            ..NodeSpec::new(padded([laid.width.max(more.width), laid.height + more.height], pad))
        });
        items.push(Item::Collapsed(f));
        labels.push(laid);
    }
    let mut titles = Vec::with_capacity(clusters.len());
    for &f in &clusters {
        let flow = &model.flows[f];
        let laid = cx.text.lay(&flow.title, title_font, Some(wrap * 1.5));
        graph.clusters.push(ClusterSpec {
            parent: container(flow.parent),
            direction: None,
            title: if laid.is_empty() { [0.0, 0.0] } else { laid.size() },
        });
        titles.push(laid);
    }

    let end = |e: EndRef| -> End {
        match e {
            EndRef::Node(n) => match model.nodes[n].flow.and_then(|f| folded[f]) {
                Some(c) => End::Node(collapsed_item[c]),
                None => End::Node(node_item[n]),
            },
            EndRef::Flow(f) => match folded[f] {
                Some(c) => End::Node(collapsed_item[c]),
                None => End::Cluster(cluster_of_flow[f].unwrap_or(0)),
            },
        }
    };
    let mut kept = Vec::new();
    let mut edge_labels = Vec::new();
    for (i, e) in model.edges.iter().enumerate() {
        let (from, to) = (end(e.from), end(e.to));
        // Edges inside a collapsed flow fold away with it.
        if from == to && matches!(from, End::Node(k) if matches!(items[k], Item::Collapsed(_))) {
            continue;
        }
        let laid = graphkit::edge_label(cx, e.label.as_ref(), wrap);
        graph.edges.push(EdgeSpec {
            from,
            to,
            label: laid.as_ref().map(|l| padded(l.size(), chip_pad)),
            min_len: e.min_len,
            weight: 1,
        });
        kept.push(i);
        edge_labels.push(laid);
    }

    let placed = layered::layout(&graph, &cx.spacing(), cx.limits)?;
    let mut out = SceneBuilder::new();

    let parents: Vec<Option<usize>> = clusters.iter().map(|&f| container(model.flows[f].parent)).collect();
    for k in graphkit::outermost_first(&parents) {
        let flow = &model.flows[clusters[k]];
        let title = (!titles[k].is_empty()).then(|| (&titles[k], placed.titles[k]));
        graphkit::cluster(&mut out, placed.clusters[k], title, &flow.style);
    }

    let marker = cx.marker_size();
    for (j, &i) in kept.iter().enumerate() {
        let e = &model.edges[i];
        let route = &placed.edges[j];
        if route.kind == RouteKind::Dropped || route.points.len() < 2 {
            continue;
        }
        let mut points = route.points.clone();
        if route.kind == RouteKind::Normal {
            if let End::Node(k) = graph.edges[j].from
                && let Item::Node(n) = items[k]
            {
                layout::attach_start(&mut points, model.nodes[n].shape, placed.nodes[k]);
            }
            if let End::Node(k) = graph.edges[j].to
                && let Item::Node(n) = items[k]
            {
                layout::attach_end(&mut points, model.nodes[n].shape, placed.nodes[k]);
            }
        }
        let (stroke, head) = match e.kind {
            EdgeKind::Sequence => (style::edge_stroke(), Some(Marker::Arrow)),
            EdgeKind::Reference => (
                style::edge_stroke().dash(Dash::Dotted),
                e.arrow.then_some(Marker::Arrow),
            ),
            EdgeKind::Failure => (Stroke::hairline(style::DANGER), Some(Marker::Cross)),
            EdgeKind::Link => (style::edge_stroke(), None),
        };
        graphkit::edge(&mut out, &points, stroke, None, head, marker, m.bend_radius);
    }
    for (j, laid) in edge_labels.iter().enumerate() {
        if let (Some(laid), Some(center)) = (laid, placed.edges[j].label) {
            out.chip(laid, center, chip_pad);
        }
    }

    for (k, item) in items.iter().enumerate() {
        let rect = placed.nodes[k];
        let laid = &labels[k];
        match *item {
            Item::Node(n) => {
                let node = &model.nodes[n];
                let paint = graphkit::node_paint(&node.style);
                graphkit::shape_node(&mut out, node.shape, rect, &paint);
                if !laid.is_empty() {
                    let lr = node.shape.label_rect(rect, laid.size());
                    out.text(laid, [lr.x, lr.y], lr.w, Align::Center, paint.text);
                }
            }
            Item::Collapsed(f) => collapsed(&mut out, rect, laid, &model.flows[f].style, cx, pad[1]),
        }
    }

    let content = out.bounds().union(&placed.bounds);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// A folded flow: the group's colours, its title and a dashed rule over
/// an ellipsis.
fn collapsed(
    out: &mut SceneBuilder,
    rect: Rect,
    title: &LaidText,
    author: &AuthorStyle,
    cx: &LayoutCx<'_>,
    pad_y: f32,
) {
    let (fill, stroke) = graphkit::cluster_inks(author);
    out.shape(
        Geom::Rect {
            rect,
            corner: Corner::Tier(RadiusTier::Sm),
        },
        Some(fill),
        Some(stroke),
    );
    let ink = author.color.map_or(style::TEXT, Ink::Rgba);
    out.text(title, [rect.x, rect.y + pad_y], rect.w, Align::Center, ink);
    let y = rect.y + pad_y + title.height + 2.0;
    graphkit::line(
        out,
        [rect.x + 6.0, y],
        [rect.right() - 6.0, y],
        Stroke::hairline(style::CLUSTER_STROKE).dash(Dash::Dashed),
    );
    let more = cx.text.lay_plain("···", cx.small_font());
    out.text(&more, [rect.x, y], rect.w, Align::Center, style::MUTED);
}
