//! Entity boxes: a header band with the name over an attribute table whose
//! columns (type, name, keys, comment) line up across rows; relationships
//! carry crow's-foot cardinalities at both ends.

use nana_ui_core::RadiusTier;

use super::Model;
use crate::families::LayoutCx;
use crate::families::graphkit::{self, HEADER_FILL, ROW_RULE, author_font, padded, verbatim};
use crate::ir::{Align, Dash, SceneBuilder};
use crate::layout::{EdgeSpec, End, Graph, NodeSpec, RouteKind, layered};
use crate::style;
use crate::text::{LaidText, Label};
use crate::{DiagramError, DiagramScene};

/// One attribute row, measured: type, name, keys, comment.
type Row = [Option<LaidText>; 4];

struct Table {
    name: LaidText,
    rows: Vec<Row>,
    header: f32,
    heights: Vec<f32>,
    /// Column widths; zero for a column no row uses.
    cols: [f32; 4],
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let small = cx.small_font();
    let wrap = cx.wrap_width("er.wrappingWidth");
    let name_font = cx.font().weight(m.strong_weight);
    let key_font = small.weight(m.strong_weight);
    let pad_x = m.node_pad_x * 0.7;
    let pad_y = m.node_pad_y * 0.8;
    let row_pad = m.node_pad_y * 0.4;
    let col_gap = m.font_size * 0.9;
    let chip_pad = cx.chip_pad();

    let plain = |text: &str, font| cx.text.lay(&verbatim(text), font, None);

    let mut tables = Vec::with_capacity(model.entities.len());
    let mut graph = Graph {
        direction: model.direction,
        ..Graph::default()
    };
    for entity in &model.entities {
        let face = author_font(name_font, &entity.style);
        let name = cx.text.lay(&Label::plain(&entity.label), face, Some(wrap * 1.5));
        let rows: Vec<Row> = entity
            .attributes
            .iter()
            .map(|a| {
                [
                    Some(plain(&a.ty, small)),
                    Some(plain(&a.name, small)),
                    (!a.keys.is_empty()).then(|| plain(&a.keys.join(", "), key_font)),
                    a.comment.as_deref().map(|c| cx.text.lay(&Label::plain(c), small, Some(wrap))),
                ]
            })
            .collect();
        let mut cols = [0.0f32; 4];
        let mut heights = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut h = cx.text.line_advance(small);
            for (k, cell) in row.iter().enumerate() {
                if let Some(cell) = cell {
                    cols[k] = cols[k].max(cell.width);
                    h = h.max(cell.height);
                }
            }
            heights.push(h + 2.0 * row_pad);
        }
        let used = cols.iter().filter(|w| **w > 0.0).count();
        let table_w: f32 = cols.iter().sum::<f32>() + col_gap * used.saturating_sub(1) as f32;
        let header = name.height + 2.0 * pad_y;
        let body: f32 = heights.iter().sum();
        let width = (name.width.max(table_w) + 2.0 * pad_x).max(m.font_size * 6.0);
        graph.nodes.push(NodeSpec::new([width, header + body]));
        tables.push(Table {
            name,
            rows,
            header,
            heights,
            cols,
        });
    }

    let mut edge_labels = Vec::with_capacity(model.relationships.len());
    for rel in &model.relationships {
        let laid = graphkit::edge_label(cx, rel.label.as_ref(), wrap);
        graph.edges.push(EdgeSpec {
            from: End::Node(rel.from),
            to: End::Node(rel.to),
            label: laid.as_ref().map(|l| padded(l.size(), chip_pad)),
            min_len: 1,
            weight: 1,
        });
        edge_labels.push(laid);
    }

    let placed = layered::layout(&graph, &cx.spacing(), cx.limits)?;
    let mut out = SceneBuilder::new();

    let marker = cx.marker_size();
    for (i, rel) in model.relationships.iter().enumerate() {
        let route = &placed.edges[i];
        if route.kind == RouteKind::Dropped || route.points.len() < 2 {
            continue;
        }
        let stroke = if rel.identifying { style::edge_stroke() } else { style::edge_stroke().dash(Dash::Dashed) };
        let (start, end) = (Some(rel.from_card.marker()), Some(rel.to_card.marker()));
        graphkit::edge(&mut out, &route.points, stroke, start, end, marker, m.bend_radius);
    }
    for (i, laid) in edge_labels.iter().enumerate() {
        if let (Some(laid), Some(center)) = (laid, placed.edges[i].label) {
            out.chip(laid, center, chip_pad);
        }
    }

    for (e, entity) in model.entities.iter().enumerate() {
        let rect = placed.nodes[e];
        let table = &tables[e];
        let paint = graphkit::node_paint(&entity.style);
        let band = (entity.style.fill.is_none() && !table.rows.is_empty()).then_some(HEADER_FILL);
        let header = if table.rows.is_empty() { 0.0 } else { table.header };
        graphkit::header_box(&mut out, rect, header, RadiusTier::Sm, &paint, band);
        let name_y = if table.rows.is_empty() {
            rect.y + (rect.h - table.name.height) / 2.0
        } else {
            rect.y + pad_y
        };
        out.text(&table.name, [rect.x, name_y], rect.w, Align::Center, paint.text);
        let mut y = rect.y + table.header;
        for (r, row) in table.rows.iter().enumerate() {
            if r > 0 {
                graphkit::divider(&mut out, rect.x, rect.right(), y, ROW_RULE);
            }
            let mut x = rect.x + pad_x;
            for (k, cell) in row.iter().enumerate() {
                if table.cols[k] <= 0.0 {
                    continue;
                }
                if let Some(cell) = cell {
                    let ink = match k {
                        0 | 3 => style::MUTED,
                        2 => style::ACCENT,
                        _ => paint.text,
                    };
                    out.text(cell, [x, y + row_pad], cell.width, Align::Start, ink);
                }
                x += table.cols[k] + col_gap;
            }
            y += table.heights[r];
        }
    }

    let content = out.bounds().union(&placed.bounds);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}
