//! Class boxes: a header (annotations and name) over attribute and method
//! compartments split by hairlines, laid out as a layered graph with
//! namespaces as groups.

use nana_ui_core::RadiusTier;

use super::{Member, Model};
use crate::families::LayoutCx;
use crate::families::graphkit::{self, HEADER_FILL};
use crate::ir::{Align, Dash, Ink, SceneBuilder, Stroke};
use crate::layout::{ClusterSpec, EdgeSpec, End, Graph, NodeSpec, RouteKind, layered};
use crate::style;
use crate::text::{FontSpec, LaidText, Label, Run};
use crate::{DiagramError, DiagramScene};

struct Laid {
    annotations: Vec<LaidText>,
    name: LaidText,
    attributes: Vec<LaidText>,
    methods: Vec<LaidText>,
    header: f32,
    attr_h: f32,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let small = cx.small_font();
    let wrap = cx.wrap_width("class.wrappingWidth");
    let name_font = cx.font().weight(m.strong_weight);
    let member_font = FontSpec {
        mono: true,
        ..small
    };
    let pad_x = m.node_pad_x * 0.75;
    let pad_y = m.node_pad_y * 0.8;
    let row_pad = m.node_pad_y * 0.6;
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let note_pad = [m.node_pad_x * 0.6, m.node_pad_y * 0.7];

    let lay_member = |mem: &Member| {
        let font = FontSpec {
            italic: mem.is_abstract,
            ..member_font
        };
        // Built from a plain run so `<T>` never reads as a tag.
        let label = Label {
            lines: vec![vec![Run::plain(mem.text.clone())]],
            markdown: false,
        };
        cx.text.lay(&label, font, None)
    };

    let mut boxes = Vec::with_capacity(model.classes.len());
    let mut graph = Graph {
        direction: model.direction,
        ..Graph::default()
    };
    for class in &model.classes {
        let annotations: Vec<LaidText> = class
            .annotations
            .iter()
            .map(|a| cx.text.lay_plain(&format!("«{a}»"), small))
            .collect();
        let is_abstract = class
            .annotations
            .iter()
            .any(|a| a.eq_ignore_ascii_case("abstract"));
        let name_label = Label {
            lines: vec![vec![Run::plain(class.name.clone())]],
            markdown: false,
        };
        let name = cx.text.lay(&name_label, name_font.italic(is_abstract), Some(wrap * 1.5));
        let attributes: Vec<LaidText> = class.attributes.iter().map(&lay_member).collect();
        let methods: Vec<LaidText> = class.methods.iter().map(&lay_member).collect();
        let widest = annotations
            .iter()
            .chain(std::iter::once(&name))
            .chain(&attributes)
            .chain(&methods)
            .map(|l| l.width)
            .fold(0.0f32, f32::max);
        let header = annotations.iter().map(|l| l.height).sum::<f32>() + name.height + 2.0 * pad_y;
        let has_members = !attributes.is_empty() || !methods.is_empty();
        let section = |rows: &[LaidText]| {
            if rows.is_empty() {
                row_pad * 1.5
            } else {
                rows.iter().map(|l| l.height).sum::<f32>() + 2.0 * row_pad
            }
        };
        let (attr_h, method_h) = if has_members {
            (section(&attributes), section(&methods))
        } else {
            (0.0, 0.0)
        };
        let size = [
            (widest + 2.0 * pad_x).max(m.font_size * 5.0),
            header + attr_h + method_h,
        ];
        graph.nodes.push(NodeSpec {
            size,
            cluster: class.namespace,
            port_span: 0.7,
        });
        boxes.push(Laid {
            annotations,
            name,
            attributes,
            methods,
            header,
            attr_h,
        });
    }

    // Notes follow the classes as plain nodes.
    let mut notes = Vec::with_capacity(model.notes.len());
    for note in &model.notes {
        let laid = cx.text.lay(&note.text, small, Some(wrap));
        let size = [laid.width + 2.0 * note_pad[0], laid.height + 2.0 * note_pad[1]];
        let cluster = note.target.and_then(|t| model.classes[t].namespace);
        graph.nodes.push(NodeSpec {
            size,
            cluster,
            port_span: 0.7,
        });
        notes.push(laid);
    }

    let mut titles = Vec::with_capacity(model.namespaces.len());
    let title_font = FontSpec::new(m.small_size).weight(m.strong_weight);
    for ns in &model.namespaces {
        let laid = cx.text.lay_plain(&ns.name, title_font);
        graph.clusters.push(ClusterSpec {
            parent: ns.parent,
            direction: None,
            title: laid.size(),
        });
        titles.push(laid);
    }

    let mut edge_labels = Vec::with_capacity(model.relations.len());
    for rel in &model.relations {
        let laid = rel
            .label
            .as_ref()
            .map(|l| cx.text.lay(l, small, Some(wrap)))
            .filter(|l| !l.is_empty());
        graph.edges.push(EdgeSpec {
            from: End::Node(rel.from),
            to: End::Node(rel.to),
            label: laid
                .as_ref()
                .map(|l| [l.width + 2.0 * chip_pad[0], l.height + 2.0 * chip_pad[1]]),
            min_len: 1,
            weight: 1,
        });
        edge_labels.push(laid);
    }
    let first_note = model.classes.len();
    for (i, note) in model.notes.iter().enumerate() {
        if let Some(t) = note.target {
            graph.edges.push(EdgeSpec {
                from: End::Node(first_note + i),
                to: End::Node(t),
                label: None,
                min_len: 1,
                weight: 1,
            });
        }
    }

    let placed = layered::layout(&graph, &cx.spacing(), cx.limits)?;
    let mut out = SceneBuilder::new();

    let parents: Vec<Option<usize>> = model.namespaces.iter().map(|n| n.parent).collect();
    for c in graphkit::outermost_first(&parents) {
        let title = (!titles[c].is_empty()).then(|| (&titles[c], placed.titles[c]));
        graphkit::cluster(&mut out, placed.clusters[c], title, &Default::default());
    }

    // Relations, then their labels and multiplicities.
    let marker = cx.marker_size();
    let mut routes: Vec<Option<Vec<[f32; 2]>>> = Vec::with_capacity(graph.edges.len());
    for (i, route) in placed.edges.iter().enumerate() {
        if route.kind == RouteKind::Dropped || route.points.len() < 2 {
            routes.push(None);
            continue;
        }
        let points = route.points.clone();
        if let Some(rel) = model.relations.get(i) {
            let stroke = if rel.dashed {
                style::edge_stroke().dash(Dash::Dashed)
            } else {
                style::edge_stroke()
            };
            graphkit::edge(&mut out, &points, stroke, rel.start, rel.end, marker, m.bend_radius);
        } else {
            let stroke = Stroke::hairline(style::NOTE_STROKE).dash(Dash::Dashed);
            graphkit::edge(&mut out, &points, stroke, None, None, marker, m.bend_radius);
        }
        routes.push(Some(points));
    }
    for (i, rel) in model.relations.iter().enumerate() {
        let Some(points) = &routes[i] else { continue };
        let mut avoid = Vec::new();
        if let (Some(laid), Some(center)) = (&edge_labels[i], placed.edges[i].label) {
            avoid.push(out.chip(laid, center, chip_pad));
        }
        for (card, at_start, head) in [(&rel.from_card, true, rel.start), (&rel.to_card, false, rel.end)] {
            let Some(card) = card else { continue };
            let laid = cx.text.lay_plain(card, small);
            let clear = if head.is_some() { marker * 1.2 } else { 2.0 };
            if let Some(origin) = graphkit::end_label_origin(points, laid.size(), at_start, clear, &avoid) {
                out.text(&laid, origin, laid.width, Align::Start, style::MUTED);
            }
        }
    }

    // Class boxes.
    for (c, class) in model.classes.iter().enumerate() {
        let rect = placed.nodes[c];
        let b = &boxes[c];
        let paint = graphkit::node_paint(&class.style);
        let band = class.style.fill.is_none().then_some(HEADER_FILL);
        graphkit::header_box(&mut out, rect, b.header, RadiusTier::Sm, &paint, band);
        let mut y = rect.y + pad_y;
        for a in &b.annotations {
            out.text(a, [rect.x, y], rect.w, Align::Center, style::MUTED);
            y += a.height;
        }
        out.text(&b.name, [rect.x, y], rect.w, Align::Center, paint.text);
        if !b.attributes.is_empty() || !b.methods.is_empty() {
            let split = rect.y + b.header + b.attr_h;
            graphkit::divider(&mut out, rect.x, rect.right(), split, paint.stroke.ink);
            let mut y = rect.y + b.header + row_pad;
            draw_members(&mut out, &class.attributes, &b.attributes, rect.x + pad_x, &mut y, paint.text);
            let mut y = split + row_pad;
            draw_members(&mut out, &class.methods, &b.methods, rect.x + pad_x, &mut y, paint.text);
        }
        if let Some((url, tooltip)) = &class.link {
            out.link(rect, url, tooltip.as_deref());
        }
    }

    // Notes.
    for (i, laid) in notes.iter().enumerate() {
        let rect = placed.nodes[first_note + i];
        let paint = graphkit::note_paint(&Default::default());
        graphkit::note(&mut out, rect, laid, note_pad, &paint);
    }

    let content = out.bounds().union(&placed.bounds);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

fn draw_members(
    out: &mut SceneBuilder,
    members: &[Member],
    laid: &[LaidText],
    x: f32,
    y: &mut f32,
    ink: Ink,
) {
    for (mem, text) in members.iter().zip(laid) {
        out.text(text, [x, *y], text.width, Align::Start, ink);
        if mem.is_static {
            // UML underlines static members.
            let base = *y + text.line_height * 0.5 + text.font.size * 0.55;
            graphkit::line(
                out,
                [x, base],
                [x + text.width, base],
                Stroke::hairline(ink),
            );
        }
        *y += text.height;
    }
}
