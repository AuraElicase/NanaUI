//! Requirement and element boxes: a header band with the `«kind»` and the
//! name over left-aligned field lines; relationships are dashed arrows
//! labelled `«kind»`, `contains` a solid line with a containment circle at
//! the container.

use nana_ui_core::RadiusTier;

use super::{Model, RelKind};
use crate::families::LayoutCx;
use crate::families::graphkit::{self, HEADER_FILL};
use crate::geom::Marker;
use crate::ir::{Align, Dash, SceneBuilder};
use crate::layout::{EdgeSpec, End, Graph, NodeSpec, RouteKind, layered};
use crate::style;
use crate::text::{FontSpec, LaidText, Label, Run};
use crate::{DiagramError, DiagramScene};

struct Laid {
    kind: LaidText,
    name: LaidText,
    fields: Vec<LaidText>,
    header: f32,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let small = cx.small_font();
    let wrap = cx.wrap_width("requirement.wrappingWidth");
    let name_font = cx.font().weight(m.strong_weight);
    let pad_x = m.node_pad_x * 0.75;
    let pad_y = m.node_pad_y * 0.8;
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];

    let plain = |text: String, font: FontSpec, max: Option<f32>| {
        let label = Label {
            lines: vec![vec![Run::plain(text)]],
            markdown: false,
        };
        cx.text.lay(&label, font, max)
    };

    let mut boxes = Vec::with_capacity(model.blocks.len());
    let mut graph = Graph {
        direction: model.direction,
        ..Graph::default()
    };
    for block in &model.blocks {
        let kind = plain(format!("«{}»", block.kind.title()), small, None);
        let face = FontSpec {
            weight: block.style.font_weight.unwrap_or(name_font.weight),
            italic: block.style.italic.unwrap_or(false),
            ..name_font
        };
        let name = plain(block.id.clone(), face, Some(wrap * 1.2));
        let fields: Vec<LaidText> = block
            .fields
            .iter()
            .map(|(k, v)| plain(format!("{k}: {v}"), small, Some(wrap)))
            .collect();
        let widest = fields
            .iter()
            .map(|f| f.width)
            .fold(kind.width.max(name.width), f32::max);
        let header = kind.height + name.height + 2.0 * pad_y;
        let body = if fields.is_empty() {
            0.0
        } else {
            fields.iter().map(|f| f.height).sum::<f32>() + 2.0 * pad_y
        };
        graph.nodes.push(NodeSpec {
            size: [(widest + 2.0 * pad_x).max(m.font_size * 8.0), header + body],
            cluster: None,
            port_span: 0.7,
        });
        boxes.push(Laid {
            kind,
            name,
            fields,
            header,
        });
    }

    let mut edge_labels = Vec::with_capacity(model.relations.len());
    for rel in &model.relations {
        let laid = plain(format!("«{}»", rel.kind.name()), small, None);
        graph.edges.push(EdgeSpec {
            from: End::Node(rel.from),
            to: End::Node(rel.to),
            label: Some([laid.width + 2.0 * chip_pad[0], laid.height + 2.0 * chip_pad[1]]),
            min_len: 1,
            weight: 1,
        });
        edge_labels.push(laid);
    }

    let placed = layered::layout(&graph, &cx.spacing(), cx.limits)?;
    let mut out = SceneBuilder::new();

    let marker = cx.marker_size();
    for (i, rel) in model.relations.iter().enumerate() {
        let route = &placed.edges[i];
        if route.kind == RouteKind::Dropped || route.points.len() < 2 {
            continue;
        }
        let (stroke, start, end) = if rel.kind == RelKind::Contains {
            (style::edge_stroke(), Some(Marker::Containment), None)
        } else {
            (style::edge_stroke().dash(Dash::Dashed), None, Some(Marker::OpenArrow))
        };
        graphkit::edge(&mut out, &route.points, stroke, start, end, marker, m.bend_radius);
    }
    for (i, laid) in edge_labels.iter().enumerate() {
        if let Some(center) = placed.edges[i].label
            && placed.edges[i].kind != RouteKind::Dropped
        {
            out.chip(laid, center, chip_pad);
        }
    }

    for (b, block) in model.blocks.iter().enumerate() {
        let rect = placed.nodes[b];
        let laid = &boxes[b];
        let paint = graphkit::node_paint(&block.style);
        let band = block.style.fill.is_none().then_some(HEADER_FILL);
        let header = if laid.fields.is_empty() { rect.h } else { laid.header };
        graphkit::header_box(&mut out, rect, header, RadiusTier::Sm, &paint, band);
        let mut y = rect.y + pad_y;
        out.text(&laid.kind, [rect.x, y], rect.w, Align::Center, style::MUTED);
        y += laid.kind.height;
        out.text(&laid.name, [rect.x, y], rect.w, Align::Center, paint.text);
        let mut y = rect.y + laid.header + pad_y;
        for field in &laid.fields {
            out.text(field, [rect.x + pad_x, y], field.width, Align::Start, paint.text);
            y += field.height;
        }
    }

    let content = out.bounds().union(&placed.bounds);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}
