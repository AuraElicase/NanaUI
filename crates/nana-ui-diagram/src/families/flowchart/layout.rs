//! Flowchart drawing: measure labels, size shapes, lay the graph out with
//! nested subgraphs and draw it with the shared role table.

use super::{Curve, EndRef, LineKind, Model, link_stroke};
use crate::families::{LayoutCx, graphkit};
use crate::geom::path::{basis_curve, rounded_polyline};
use crate::geom::{PathData, Shape};
use crate::ir::{Align, Corner, Dash, Geom, Ink, SceneBuilder, StrokeWidth};
use crate::layout::{self, ClusterSpec, EdgeSpec, End, Graph, NodeSpec, RouteKind, layered};
use crate::style::{self, Paint};
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene};

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let wrap = cx.wrap_width("flowchart.wrappingWidth");
    let pad = [m.node_pad_x, m.node_pad_y];
    let chip_pad = cx.chip_pad();

    // Nodes.
    let mut paints = Vec::with_capacity(model.nodes.len());
    let mut labels: Vec<LaidText> = Vec::with_capacity(model.nodes.len());
    let mut graph = Graph {
        direction: model.direction,
        ..Graph::default()
    };
    for node in &model.nodes {
        let paint = Paint::node().with_author(&node.style);
        let face = FontSpec {
            weight: paint.weight.unwrap_or(font.weight),
            italic: paint.italic,
            ..font
        };
        let laid = cx.text.lay(&node.label, face, Some(wrap));
        let size = node.shape.size(laid.size(), pad);
        graph.nodes.push(NodeSpec {
            size,
            cluster: node.subgraph,
            port_span: port_span(node.shape),
        });
        paints.push(paint);
        labels.push(laid);
    }

    // Subgraphs.
    let title_font = FontSpec::new(m.small_size).weight(m.strong_weight);
    let mut titles = Vec::with_capacity(model.subgraphs.len());
    for sub in &model.subgraphs {
        let laid = cx.text.lay(&sub.title, title_font, Some(wrap * 1.5));
        graph.clusters.push(ClusterSpec {
            parent: sub.parent,
            direction: sub.direction,
            title: if laid.is_empty() { [0.0, 0.0] } else { laid.size() },
        });
        titles.push(laid);
    }

    // Edges.
    let mut edge_labels: Vec<Option<LaidText>> = Vec::with_capacity(model.edges.len());
    for edge in &model.edges {
        let laid = edge
            .label
            .as_ref()
            .map(|l| cx.text.lay(l, small, Some(wrap)))
            .filter(|l| !l.is_empty());
        let chip = laid.as_ref().map(|l| [l.width + 2.0 * chip_pad[0], l.height + 2.0 * chip_pad[1]]);
        graph.edges.push(EdgeSpec {
            from: end(edge.from),
            to: end(edge.to),
            label: chip,
            min_len: edge.min_len,
            weight: 1,
        });
        edge_labels.push(laid);
    }

    let placed = layered::layout(&graph, &cx.spacing(), cx.limits)?;
    let mut out = SceneBuilder::new();

    // Subgraph boxes, outermost first.
    let depth = |mut c: usize| {
        let mut d = 0;
        while let Some(p) = model.subgraphs[c].parent {
            d += 1;
            c = p;
        }
        d
    };
    let mut order: Vec<usize> = (0..model.subgraphs.len()).collect();
    order.sort_by_key(|&c| (depth(c), c));
    for &c in &order {
        let sub = &model.subgraphs[c];
        let rect = placed.clusters[c];
        let fill = sub.style.fill.map_or(style::CLUSTER_FILL, Ink::Rgba);
        let mut stroke = style::cluster_stroke();
        if let Some(s) = sub.style.stroke {
            stroke.ink = Ink::Rgba(s);
        }
        if let Some(w) = sub.style.stroke_width {
            stroke.width = StrokeWidth::Px(w);
        }
        if sub.style.dashed == Some(true) {
            stroke.dash = Dash::Dashed;
        }
        out.shape(
            Geom::Rect {
                rect,
                corner: Corner::Tier(style::CLUSTER_CORNER),
            },
            Some(fill),
            Some(stroke),
        );
        let title = &titles[c];
        if !title.is_empty() {
            let t = placed.titles[c];
            let ink = sub.style.color.map_or(style::MUTED, Ink::Rgba);
            out.text(title, [t.x, t.y], t.w, Align::Center, ink);
        }
    }

    // Edges under nodes, labels over edges.
    let marker = cx.marker_size();
    for (i, edge) in model.edges.iter().enumerate() {
        let route = &placed.edges[i];
        if edge.line == LineKind::Invisible || route.kind == RouteKind::Dropped || route.points.len() < 2 {
            continue;
        }
        let stroke = link_stroke(edge.line, &edge.style);
        let curve = edge.curve.or(model.default_curve).unwrap_or(Curve::Rounded);
        let mut points = if route.kind == RouteKind::SelfLoop {
            route.points.clone()
        } else {
            match curve {
                Curve::Rounded | Curve::Step => route.points.clone(),
                Curve::Linear | Curve::Smooth => through_points(&route.points),
            }
        };
        if route.kind == RouteKind::Normal {
            if let EndRef::Node(n) = edge.from {
                layout::attach_start(&mut points, model.nodes[n].shape, placed.nodes[n]);
            }
            if let EndRef::Node(n) = edge.to {
                layout::attach_end(&mut points, model.nodes[n].shape, placed.nodes[n]);
            }
        }
        let radius = m.bend_radius;
        let shape = match (route.kind, curve) {
            (RouteKind::SelfLoop, _) | (_, Curve::Rounded) => Curve::Rounded,
            (_, other) => other,
        };
        let build = move |p: &[[f32; 2]]| match shape {
            Curve::Rounded => rounded_polyline(p, radius),
            Curve::Step | Curve::Linear => PathData::polyline(p),
            Curve::Smooth => basis_curve(p),
        };
        out.edge(&points, build, stroke, edge.start, edge.end, marker);
    }
    for (i, laid) in edge_labels.iter().enumerate() {
        let (Some(laid), Some(center)) = (laid, placed.edges[i].label) else {
            continue;
        };
        if model.edges[i].line == LineKind::Invisible {
            continue;
        }
        out.chip(laid, center, chip_pad);
    }

    // Nodes.
    for (n, node) in model.nodes.iter().enumerate() {
        let rect = placed.nodes[n];
        let paint = paints[n];
        graphkit::shape_node(&mut out, node.shape, rect, &paint);
        let laid = &labels[n];
        if !laid.is_empty() {
            let lr = node.shape.label_rect(rect, laid.size());
            let ink = if node.shape.label_outside() { style::TEXT } else { paint.text };
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

fn end(e: EndRef) -> End {
    match e {
        EndRef::Node(n) => End::Node(n),
        EndRef::Subgraph(s) => End::Cluster(s),
    }
}

fn port_span(shape: Shape) -> f32 {
    match shape {
        Shape::Diamond
        | Shape::Circle
        | Shape::DoubleCircle
        | Shape::CrossedCircle
        | Shape::Hexagon
        | Shape::Triangle
        | Shape::FlippedTriangle
        | Shape::Hourglass
        | Shape::Bolt => 0.3,
        Shape::SmallCircle | Shape::FramedCircle | Shape::FilledCircle => 0.0,
        _ => 0.7,
    }
}

/// For straight and smooth curves: the route's ends plus the middle of
/// each bend run, which is where a spline through the layout points would
/// pass.
fn through_points(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    for w in points.windows(2).skip(1).take(points.len().saturating_sub(3)) {
        out.push([(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0]);
    }
    out.push(points[points.len() - 1]);
    out
}

