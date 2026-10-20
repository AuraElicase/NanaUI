//! State drawing: rounded states (a title over descriptions when there
//! are several), start and end dots, fork bars, choice diamonds, composite
//! states as groups with dashed lines between concurrent regions, notes.

use nana_ui_core::RadiusTier;

use super::{Kind, Model, Scope};
use crate::families::LayoutCx;
use crate::families::graphkit;
use crate::geom::{Marker, PathData, Shape};
use crate::ir::{Align, Corner, Dash, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::layout::{ClusterSpec, EdgeSpec, End, Graph, NodeSpec, RouteKind, layered};
use crate::style;
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene};

enum Body {
    /// One centred label.
    Plain(LaidText),
    /// A bold title, a divider, then left-aligned descriptions.
    Titled { title: LaidText, lines: Vec<LaidText> },
    Mark,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let wrap = cx.wrap_width("state.wrappingWidth");
    let pad = [m.node_pad_x, m.node_pad_y];
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let note_pad = [m.node_pad_x * 0.6, m.node_pad_y * 0.7];
    let dot = m.font_size;
    let bar = [m.font_size * 4.6, m.font_size * 0.5];

    // Clusters: composites first, then each composite's regions.
    let nc = model.composites.len();
    let mut region_base = vec![usize::MAX; nc];
    let mut clusters: Vec<(Option<usize>, Option<usize>)> = Vec::new(); // (composite, parent cluster)
    for c in 0..nc {
        clusters.push((Some(c), None));
    }
    for (c, comp) in model.composites.iter().enumerate() {
        if comp.regions > 0 {
            region_base[c] = clusters.len();
            for _ in 0..=comp.regions {
                clusters.push((None, Some(c)));
            }
        }
    }
    let cluster_of = |scope: Scope| -> Option<usize> {
        let c = scope.composite?;
        if region_base[c] == usize::MAX {
            Some(c)
        } else {
            Some(region_base[c] + scope.region.min(model.composites[c].regions) as usize)
        }
    };
    for (c, comp) in model.composites.iter().enumerate() {
        clusters[c].1 = cluster_of(comp.scope);
    }
    // The direction each level is likely to run in, for fork bars.
    let mut cluster_dir = vec![model.direction; clusters.len()];
    for (i, slot) in cluster_dir.iter_mut().enumerate() {
        let mut dir = None;
        let mut cur = Some(i);
        let mut guard = 0;
        while let Some(k) = cur {
            if let Some(d) = clusters[k].0.and_then(|c| model.composites[c].direction) {
                dir = Some(d);
                break;
            }
            cur = clusters[k].1;
            guard += 1;
            if guard > clusters.len() {
                break;
            }
        }
        *slot = dir.unwrap_or(model.direction);
    }
    let dir_of = |cluster: Option<usize>| cluster.map_or(model.direction, |c| cluster_dir[c]);

    let mut graph = Graph {
        direction: model.direction,
        ..Graph::default()
    };
    // Node index per state; composites map to clusters instead.
    let mut node_of = vec![usize::MAX; model.states.len()];
    let mut bodies = Vec::new();
    let mut owners = Vec::new();
    for (s, state) in model.states.iter().enumerate() {
        if state.composite.is_some() {
            continue;
        }
        let cluster = cluster_of(state.scope);
        let (body, size) = match state.kind {
            Kind::Start => (Body::Mark, [dot, dot]),
            Kind::End => (Body::Mark, [dot * 1.3, dot * 1.3]),
            Kind::Choice => (Body::Mark, [dot * 1.8, dot * 1.8]),
            Kind::Fork | Kind::Join => {
                let size = if dir_of(cluster).horizontal() { [bar[1], bar[0]] } else { bar };
                (Body::Mark, size)
            }
            Kind::Normal => {
                let face = FontSpec {
                    weight: state.style.font_weight.unwrap_or(font.weight),
                    italic: state.style.italic.unwrap_or(false),
                    ..font
                };
                if state.descriptions.len() == 1 && state.label.is_none() || state.descriptions.is_empty() {
                    let label = state.descriptions.first().cloned().or_else(|| state.label.clone());
                    let label = label.unwrap_or_else(|| crate::text::Label::plain(&state.id));
                    let laid = cx.text.lay(&label, face, Some(wrap));
                    let size = [
                        (laid.width + 2.0 * pad[0]).max(m.font_size * 3.5),
                        laid.height + 2.0 * pad[1],
                    ];
                    (Body::Plain(laid), size)
                } else {
                    let title_label = state
                        .label
                        .clone()
                        .unwrap_or_else(|| crate::text::Label::plain(&state.id));
                    let title = cx.text.lay(&title_label, face.weight(m.strong_weight), Some(wrap));
                    let lines: Vec<LaidText> = state
                        .descriptions
                        .iter()
                        .map(|d| cx.text.lay(d, small, Some(wrap)))
                        .collect();
                    let w = lines.iter().map(|l| l.width).fold(title.width, f32::max);
                    let h = title.height + lines.iter().map(|l| l.height).sum::<f32>() + 4.0 * pad[1] * 0.75;
                    (Body::Titled { title, lines }, [w + 2.0 * pad[0], h])
                }
            }
        };
        node_of[s] = graph.nodes.len();
        graph.nodes.push(NodeSpec {
            size,
            cluster,
            port_span: if matches!(state.kind, Kind::Normal | Kind::Fork | Kind::Join) { 0.7 } else { 0.0 },
        });
        bodies.push(body);
        owners.push(s);
    }
    let first_note = graph.nodes.len();
    let mut notes = Vec::with_capacity(model.notes.len());
    for note in &model.notes {
        let laid = cx.text.lay(&note.text, small, Some(wrap));
        let size = [laid.width + 2.0 * note_pad[0], laid.height + 2.0 * note_pad[1]];
        let cluster = note.target.and_then(|t| target_cluster(model, t, &cluster_of));
        graph.nodes.push(NodeSpec {
            size,
            cluster,
            port_span: 0.7,
        });
        notes.push(laid);
    }

    let title_font = FontSpec::new(m.small_size).weight(m.strong_weight);
    let mut titles = Vec::with_capacity(clusters.len());
    for &(comp, parent) in &clusters {
        let (title, direction) = match comp {
            Some(c) => {
                let state = &model.states[model.composites[c].state];
                let label = state.label.clone().unwrap_or_else(|| crate::text::Label::plain(&state.id));
                let direction = if model.composites[c].regions > 0 { None } else { model.composites[c].direction };
                (Some(cx.text.lay(&label, title_font, Some(wrap * 1.5))), direction)
            }
            // Regions take their composite's direction.
            None => (None, parent.and_then(|p| model.composites.get(p)).and_then(|c| c.direction)),
        };
        graph.clusters.push(ClusterSpec {
            parent,
            direction,
            title: title.as_ref().map_or([0.0, 0.0], |t| t.size()),
        });
        titles.push(title);
    }

    let end_of = |s: usize| -> End {
        match model.states[s].composite {
            Some(c) => End::Cluster(c),
            None => End::Node(node_of[s]),
        }
    };
    let mut edge_labels = Vec::with_capacity(model.transitions.len());
    for t in &model.transitions {
        let laid = t
            .label
            .as_ref()
            .map(|l| cx.text.lay(l, small, Some(wrap)))
            .filter(|l| !l.is_empty());
        graph.edges.push(EdgeSpec {
            from: end_of(t.from),
            to: end_of(t.to),
            label: laid
                .as_ref()
                .map(|l| [l.width + 2.0 * chip_pad[0], l.height + 2.0 * chip_pad[1]]),
            min_len: 1,
            weight: 1,
        });
        edge_labels.push(laid);
    }
    for (i, note) in model.notes.iter().enumerate() {
        let Some(t) = note.target else { continue };
        let (a, b) = (End::Node(first_note + i), end_of(t));
        let (from, to) = if note.left { (a, b) } else { (b, a) };
        graph.edges.push(EdgeSpec {
            from,
            to,
            label: None,
            min_len: 1,
            weight: 1,
        });
    }

    let placed = layered::layout(&graph, &cx.spacing(), cx.limits)?;
    let mut out = SceneBuilder::new();

    // Composites, outermost first, with region separators.
    let parents: Vec<Option<usize>> = clusters.iter().map(|c| c.1).collect();
    for k in graphkit::outermost_first(&parents) {
        let Some(c) = clusters[k].0 else { continue };
        let state = &model.states[model.composites[c].state];
        let title = titles[k].as_ref().filter(|t| !t.is_empty()).map(|t| (t, placed.titles[k]));
        graphkit::cluster(&mut out, placed.clusters[k], title, &state.style);
        if region_base[c] != usize::MAX {
            let n = model.composites[c].regions as usize + 1;
            let rects: Vec<Rect> = (0..n).map(|r| placed.clusters[region_base[c] + r]).collect();
            region_lines(&mut out, &rects, placed.clusters[k]);
        }
    }

    // Transitions.
    let marker = cx.marker_size();
    for (i, route) in placed.edges.iter().enumerate() {
        if route.kind == RouteKind::Dropped || route.points.len() < 2 {
            continue;
        }
        let mut points = route.points.clone();
        let spec = &graph.edges[i];
        for (end, at_end) in [(spec.from, false), (spec.to, true)] {
            if let End::Node(n) = end
                && route.kind == RouteKind::Normal
                && let Some(outline) = mark_outline(model, &owners, n, placed.nodes[n])
            {
                graphkit::attach(&mut points, &outline, placed.nodes[n], at_end);
            }
        }
        if i < model.transitions.len() {
            graphkit::edge(&mut out, &points, style::edge_stroke(), None, Some(Marker::Arrow), marker, m.bend_radius);
        } else {
            let stroke = Stroke::hairline(style::NOTE_STROKE).dash(Dash::Dashed);
            graphkit::edge(&mut out, &points, stroke, None, None, marker, m.bend_radius);
        }
    }
    for (i, laid) in edge_labels.iter().enumerate() {
        if let (Some(laid), Some(center)) = (laid, placed.edges[i].label) {
            out.chip(laid, center, chip_pad);
        }
    }

    // States.
    for (n, body) in bodies.iter().enumerate() {
        let state = &model.states[owners[n]];
        let rect = placed.nodes[n];
        let paint = graphkit::node_paint(&state.style);
        let solid = match paint.fill {
            Ink::Rgba(_) => paint.fill,
            _ => style::TEXT,
        };
        match state.kind {
            Kind::Start => out.fill(Geom::Ellipse(rect), solid),
            Kind::End => {
                out.shape(Geom::Ellipse(rect), Some(paint.fill), Some(Stroke { ..paint.stroke }));
                let inset = rect.w * 0.22;
                out.fill(Geom::Ellipse(rect.inset(inset, inset)), solid);
            }
            Kind::Fork | Kind::Join => out.fill(
                Geom::Rect {
                    rect,
                    corner: Corner::Tier(RadiusTier::Xs),
                },
                solid,
            ),
            Kind::Choice => out.shape(Shape::Diamond.outline(rect), Some(paint.fill), Some(paint.stroke)),
            Kind::Normal => match body {
                Body::Plain(laid) => {
                    out.shape(
                        Geom::Rect {
                            rect,
                            corner: Corner::Tier(RadiusTier::Md),
                        },
                        Some(paint.fill),
                        Some(paint.stroke),
                    );
                    out.text_centered(laid, rect.center(), paint.text);
                }
                Body::Titled { title, lines } => {
                    let header = title.height + 2.0 * pad[1] * 0.75;
                    graphkit::header_box(&mut out, rect, header, RadiusTier::Md, &paint, None);
                    out.text(title, [rect.x, rect.y + pad[1] * 0.75], rect.w, Align::Center, paint.text);
                    let mut y = rect.y + header + pad[1] * 0.75;
                    for line in lines {
                        out.text(line, [rect.x + pad[0], y], line.width, Align::Start, paint.text);
                        y += line.height;
                    }
                }
                Body::Mark => {}
            },
        }
    }
    for (i, laid) in notes.iter().enumerate() {
        let paint = graphkit::note_paint(&Default::default());
        graphkit::note(&mut out, placed.nodes[first_note + i], laid, note_pad, &paint);
    }

    let content = out.bounds().union(&placed.bounds);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// The cluster a note about state `t` joins: the state's own, or for a
/// composite, the one around it.
fn target_cluster(model: &Model, t: usize, cluster_of: &dyn Fn(Scope) -> Option<usize>) -> Option<usize> {
    cluster_of(model.states[t].scope)
}

/// Outlines that edges should meet exactly: dots and diamonds.
fn mark_outline(model: &Model, owners: &[usize], n: usize, rect: Rect) -> Option<PathData> {
    let state = model.states.get(*owners.get(n)?)?;
    match state.kind {
        Kind::Start | Kind::End => Some(PathData::ellipse(rect)),
        Kind::Choice => Some(Shape::Diamond.outline_path(rect)),
        _ => None,
    }
}

/// Dashed lines between concurrent regions, across the composite.
fn region_lines(out: &mut SceneBuilder, rects: &[Rect], outer: Rect) {
    if rects.len() < 2 {
        return;
    }
    let stroke = Stroke::hairline(style::CLUSTER_STROKE).dash(Dash::Dashed);
    let side_by_side = rects
        .windows(2)
        .all(|w| w[0].right() <= w[1].x + 0.5 || w[1].right() <= w[0].x + 0.5);
    let mut sorted = rects.to_vec();
    let top = rects.iter().map(|r| r.y).fold(f32::INFINITY, f32::min);
    let bottom = rects.iter().map(|r| r.bottom()).fold(f32::NEG_INFINITY, f32::max);
    if side_by_side {
        sorted.sort_by(|a, b| a.x.total_cmp(&b.x));
        for w in sorted.windows(2) {
            let x = (w[0].right() + w[1].x) / 2.0;
            graphkit::line(out, [x, top], [x, bottom], stroke);
        }
    } else {
        sorted.sort_by(|a, b| a.y.total_cmp(&b.y));
        for w in sorted.windows(2) {
            let y = (w[0].bottom() + w[1].y) / 2.0;
            graphkit::line(out, [outer.x, y], [outer.right(), y], stroke);
        }
    }
}
