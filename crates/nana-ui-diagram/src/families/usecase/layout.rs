//! Use case drawing: actors as figures with their name below, use cases as
//! ellipses or rectangles, system boundaries as groups (a title tab for
//! packages), notes, JSON tables, and associations / include / extend /
//! generalization lines.

use nana_ui_core::RadiusTier;

use super::{ActorLook, EdgeKind, Model, NodeKind};
use crate::families::LayoutCx;
use crate::families::graphkit::{self, HEADER_FILL, ROW_RULE, author_font, padded, verbatim};
use crate::geom::PathData;
use crate::ir::{Align, Corner, Dash, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::layout::{ClusterSpec, EdgeSpec, End, Graph, NodeSpec, RouteKind, layered};
use crate::source::directives::AuthorStyle;
use crate::style::{self, Paint};
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene};

/// What sits inside or under a node's outline.
struct Body {
    stereotype: Option<LaidText>,
    label: LaidText,
    /// JSON rows: key and value cells, and the key column's width.
    rows: Vec<(LaidText, LaidText)>,
    key_w: f32,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let wrap = cx.wrap_width("usecase.wrappingWidth");
    let pad = [m.node_pad_x, m.node_pad_y];
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let note_pad = graphkit::note_pad(&m);
    let figure = [m.font_size * 2.0, m.font_size * 3.1];
    let fig_gap = m.font_size * 0.4;
    let row_pad = m.node_pad_y * 0.4;
    let col_gap = m.font_size * 0.9;

    let plain = |text: &str, font| cx.text.lay(&verbatim(text), font, Some(wrap));

    let mut graph = Graph {
        direction: model.direction,
        ..Graph::default()
    };
    let mut bodies = Vec::with_capacity(model.nodes.len());
    for node in &model.nodes {
        let face = author_font(font, &node.style);
        let stereotype = node.stereotype.as_ref().map(|s| plain(&format!("«{s}»"), small));
        let st_h = stereotype.as_ref().map_or(0.0, |s| s.height);
        let st_w = stereotype.as_ref().map_or(0.0, |s| s.width);
        let mut rows = Vec::new();
        let mut key_w = 0.0f32;
        let (label, size) = match &node.kind {
            NodeKind::Actor(_) => {
                let label = cx.text.lay(&node.label, face, Some(wrap * 0.7));
                let size = [figure[0].max(label.width).max(st_w), figure[1] + fig_gap + st_h + label.height];
                (label, size)
            }
            NodeKind::Ellipse => {
                let label = cx.text.lay(&node.label, face, Some(wrap));
                let w = label.width.max(st_w);
                let h = label.height + st_h;
                // An ellipse that holds the text box's corners.
                let a = (w / 2.0 + pad[0]) * 1.2;
                let ratio = (w / 2.0 / a).min(0.9);
                let b = (h / 2.0) / (1.0 - ratio * ratio).sqrt() + pad[1] * 0.5;
                let size = [2.0 * a, 2.0 * b];
                (label, size)
            }
            NodeKind::Rect => {
                let label = cx.text.lay(&node.label, face, Some(wrap));
                let size = padded([label.width.max(st_w), label.height + st_h], pad);
                (label, size)
            }
            NodeKind::Note => {
                let label = cx.text.lay(&node.label, small, Some(wrap));
                let size = padded(label.size(), note_pad);
                (label, size)
            }
            NodeKind::Json(cells) => {
                let title = plain(&node.id, small.weight(m.strong_weight));
                let mut val_w = 0.0f32;
                let mut h = 0.0;
                for (k, v) in cells {
                    let key = plain(k, small);
                    let value = plain(v, small);
                    key_w = key_w.max(key.width);
                    val_w = val_w.max(value.width);
                    h += key.height.max(value.height) + 2.0 * row_pad;
                    rows.push((key, value));
                }
                let header = title.height + 2.0 * row_pad * 1.5;
                let w = (key_w + col_gap + val_w).max(title.width) + 2.0 * pad[0] * 0.75;
                (title, [w, header + h])
            }
        };
        graph.nodes.push(NodeSpec {
            size,
            cluster: node.boundary,
            port_span: match node.kind {
                NodeKind::Ellipse => 0.3,
                NodeKind::Actor(_) => 0.2,
                _ => 0.7,
            },
        });
        bodies.push(Body {
            stereotype,
            label,
            rows,
            key_w,
        });
    }

    let title_font = FontSpec::new(m.small_size).weight(m.strong_weight);
    let mut titles = Vec::with_capacity(model.boundaries.len());
    for b in &model.boundaries {
        let laid = cx.text.lay(&b.title, title_font, Some(wrap * 1.5));
        let size = if b.package {
            [laid.width + 2.0 * pad[0] * 0.75, laid.height + 2.0 * row_pad]
        } else {
            laid.size()
        };
        graph.clusters.push(ClusterSpec {
            parent: None,
            direction: None,
            title: size,
        });
        titles.push(laid);
    }

    let mut edge_labels = Vec::with_capacity(model.edges.len());
    for e in &model.edges {
        let laid = graphkit::edge_label(cx, e.label.as_ref(), wrap);
        graph.edges.push(EdgeSpec {
            from: End::Node(e.from),
            to: End::Node(e.to),
            label: laid.as_ref().map(|l| padded(l.size(), chip_pad)),
            min_len: e.min_len,
            weight: 1,
        });
        edge_labels.push(laid);
    }

    let placed = layered::layout(&graph, &cx.spacing(), cx.limits)?;
    let mut out = SceneBuilder::new();

    // Boundaries.
    for (i, b) in model.boundaries.iter().enumerate() {
        let rect = placed.clusters[i];
        if b.package {
            package(&mut out, rect, &titles[i], &b.style, [pad[0] * 0.75, row_pad]);
        } else {
            let title = (!titles[i].is_empty()).then(|| (&titles[i], placed.titles[i]));
            graphkit::cluster(&mut out, rect, title, &b.style);
        }
    }

    // Lines.
    let marker = cx.marker_size();
    for (i, e) in model.edges.iter().enumerate() {
        let route = &placed.edges[i];
        if route.kind == RouteKind::Dropped || route.points.len() < 2 {
            continue;
        }
        let mut points = route.points.clone();
        if route.kind == RouteKind::Normal {
            for (n, at_end) in [(e.from, false), (e.to, true)] {
                if model.nodes[n].kind == NodeKind::Ellipse {
                    let r = placed.nodes[n];
                    graphkit::attach(&mut points, &PathData::ellipse(r), r, at_end);
                }
            }
        }
        let base = match e.kind {
            EdgeKind::Include | EdgeKind::Extend => style::edge_stroke().dash(Dash::Dashed),
            EdgeKind::Note => Stroke::hairline(style::NOTE_STROKE).dash(Dash::Dotted),
            _ => style::edge_stroke(),
        };
        let stroke = graphkit::line_stroke(base, &e.style);
        graphkit::edge(&mut out, &points, stroke, e.start, e.end, marker, m.bend_radius);
    }
    for (i, laid) in edge_labels.iter().enumerate() {
        if let (Some(laid), Some(center)) = (laid, placed.edges[i].label) {
            out.chip(laid, center, chip_pad);
        }
    }

    // Nodes.
    for (n, node) in model.nodes.iter().enumerate() {
        let rect = placed.nodes[n];
        let body = &bodies[n];
        let paint = graphkit::node_paint(&node.style);
        match &node.kind {
            NodeKind::Actor(look) => {
                let fig = Rect::new(rect.center()[0] - figure[0] / 2.0, rect.y, figure[0], figure[1]);
                actor(&mut out, fig, *look, &paint, node.business);
                let mut y = fig.bottom() + fig_gap;
                if let Some(st) = &body.stereotype {
                    out.text(st, [rect.x, y], rect.w, Align::Center, style::MUTED);
                    y += st.height;
                }
                // The name sits on the page, not on a fill.
                let ink = node.style.color.map_or(style::TEXT, Ink::Rgba);
                out.text(&body.label, [rect.x, y], rect.w, Align::Center, ink);
            }
            NodeKind::Ellipse | NodeKind::Rect => {
                let geom = if node.kind == NodeKind::Ellipse {
                    Geom::Ellipse(rect)
                } else {
                    Geom::Rect {
                        rect,
                        corner: Corner::Tier(RadiusTier::Sm),
                    }
                };
                out.shape(geom, Some(paint.fill), Some(paint.stroke));
                if node.business && node.kind == NodeKind::Ellipse {
                    // The business slash on the left of the ellipse.
                    let a = [rect.x + rect.w * 0.12, rect.y + rect.h * 0.82];
                    let b = [rect.x + rect.w * 0.26, rect.y + rect.h * 0.12];
                    graphkit::line(&mut out, a, b, paint.stroke);
                }
                let st_h = body.stereotype.as_ref().map_or(0.0, |s| s.height);
                let top = rect.center()[1] - (st_h + body.label.height) / 2.0;
                if let Some(st) = &body.stereotype {
                    out.text(st, [rect.x, top], rect.w, Align::Center, style::MUTED);
                }
                out.text(&body.label, [rect.x, top + st_h], rect.w, Align::Center, paint.text);
            }
            NodeKind::Note => {
                let paint = graphkit::note_paint(&node.style);
                graphkit::note(&mut out, rect, &body.label, note_pad, &paint);
            }
            NodeKind::Json(_) => {
                let header = body.label.height + 2.0 * row_pad * 1.5;
                let band = node.style.fill.is_none().then_some(HEADER_FILL);
                graphkit::header_box(&mut out, rect, header, RadiusTier::Sm, &paint, band);
                out.text(&body.label, [rect.x, rect.y + row_pad * 1.5], rect.w, Align::Center, paint.text);
                let mut y = rect.y + header;
                let x = rect.x + pad[0] * 0.75;
                for (r, (key, value)) in body.rows.iter().enumerate() {
                    if r > 0 && !key.is_empty() {
                        graphkit::divider(&mut out, rect.x, rect.right(), y, ROW_RULE);
                    }
                    out.text(key, [x, y + row_pad], key.width, Align::Start, style::MUTED);
                    out.text(value, [x + body.key_w + col_gap, y + row_pad], value.width, Align::Start, paint.text);
                    y += key.height.max(value.height) + 2.0 * row_pad;
                }
            }
        }
    }

    let content = out.bounds().union(&placed.bounds);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// A boundary of `type: package`: a title tab on the top-left corner of
/// the box.
fn package(out: &mut SceneBuilder, rect: Rect, title: &LaidText, author: &AuthorStyle, pad: [f32; 2]) {
    let tab = Rect::new(
        rect.x,
        rect.y,
        (title.width + 2.0 * pad[0]).min(rect.w),
        title.height + 2.0 * pad[1],
    );
    let body = Rect::new(rect.x, tab.bottom(), rect.w, (rect.h - tab.h).max(0.0));
    let (fill, stroke) = graphkit::cluster_inks(author);
    let corner = Corner::Tier(RadiusTier::Xs);
    out.shape(Geom::Rect { rect: tab, corner }, Some(fill), Some(stroke));
    out.shape(Geom::Rect { rect: body, corner }, Some(fill), Some(stroke));
    let ink = author.color.map_or(style::MUTED, Ink::Rgba);
    out.text(title, [tab.x + pad[0], tab.y + pad[1]], title.width, Align::Start, ink);
}

/// An actor figure inside `r` (head on top, feet at the bottom).
fn actor(out: &mut SceneBuilder, r: Rect, look: ActorLook, paint: &Paint, business: bool) {
    let cx = r.center()[0];
    let head_d = r.w * 0.5;
    let head = Rect::new(cx - head_d / 2.0, r.y, head_d, head_d);
    let neck = head.bottom();
    let hip = r.y + r.h * 0.62;
    let arms = neck + (hip - neck) * 0.3;
    let ink = graphkit::solid_ink(paint.stroke.ink);
    let line_stroke = Stroke {
        ink,
        dash: Dash::Solid,
        ..paint.stroke
    };
    match look {
        ActorLook::Normal => {
            out.shape(Geom::Ellipse(head), Some(paint.fill), Some(line_stroke));
            let mut p = PathData::new();
            p.move_to([cx, neck]).line_to([cx, hip]);
            p.move_to([r.x, arms]).line_to([r.right(), arms]);
            p.move_to([r.x + r.w * 0.1, r.bottom()]).line_to([cx, hip]).line_to([r.right() - r.w * 0.1, r.bottom()]);
            out.stroke(Geom::path(p), line_stroke);
        }
        ActorLook::Hollow => {
            out.shape(Geom::Ellipse(head), Some(paint.fill), Some(line_stroke));
            let w = r.w * 0.16;
            let body = PathData::polygon(&[
                [cx - w, neck + 2.0],
                [cx + w, neck + 2.0],
                [cx + w, arms - w * 0.6],
                [r.right(), arms - w * 0.6],
                [r.right(), arms + w * 0.6],
                [cx + w, arms + w * 0.6],
                [cx + w, hip],
                [r.right() - r.w * 0.05, r.bottom()],
                [r.right() - r.w * 0.05 - 2.0 * w, r.bottom()],
                [cx, hip + w * 1.5],
                [r.x + r.w * 0.05 + 2.0 * w, r.bottom()],
                [r.x + r.w * 0.05, r.bottom()],
                [cx - w, hip],
                [cx - w, arms + w * 0.6],
                [r.x, arms + w * 0.6],
                [r.x, arms - w * 0.6],
                [cx - w, arms - w * 0.6],
            ]);
            out.shape(Geom::path(body), Some(paint.fill), Some(line_stroke));
        }
        ActorLook::Awesome => {
            out.fill(Geom::Ellipse(head), ink);
            let mut body = PathData::new();
            let top = neck + r.h * 0.06;
            body.move_to([r.x, r.bottom()])
                .line_to([r.x, top + r.w * 0.45])
                .quad_to([r.x, top], [cx, top])
                .quad_to([r.right(), top], [r.right(), top + r.w * 0.45])
                .line_to([r.right(), r.bottom()])
                .close();
            out.fill(Geom::path(body), ink);
        }
        ActorLook::Icon => {
            let frame = Rect::new(r.x, r.y + (r.h - r.w) / 2.0, r.w, r.w);
            out.shape(
                Geom::Rect {
                    rect: frame,
                    corner: Corner::Tier(RadiusTier::Sm),
                },
                Some(paint.fill),
                Some(line_stroke),
            );
            let d = frame.w * 0.34;
            let c = frame.center();
            out.fill(Geom::Ellipse(Rect::new(c[0] - d / 2.0, frame.y + frame.h * 0.18, d, d)), ink);
            let mut sh = PathData::new();
            let base = frame.y + frame.h * 0.82;
            sh.move_to([frame.x + frame.w * 0.22, base])
                .quad_to([c[0], frame.y + frame.h * 0.42], [frame.right() - frame.w * 0.22, base])
                .close();
            out.fill(Geom::path(sh), ink);
        }
    }
    if business && matches!(look, ActorLook::Normal | ActorLook::Hollow) {
        // The business slash across the lower right of the head.
        let a = [head.x + head.w * 0.35, head.bottom() + head.h * 0.1];
        let b = [head.right() + head.w * 0.15, head.y + head.h * 0.45];
        graphkit::line(out, a, b, line_stroke);
    }
}
