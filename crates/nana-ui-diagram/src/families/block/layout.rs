//! Block drawing: measure every block, solve each container's column grid
//! (composite blocks nest their own), stretch blocks to their cells and
//! connect linked blocks orthogonally.

use super::{Heads, Kind, Model, draw_node, link_stroke};
use crate::families::flowchart::LineKind;
use crate::families::{LayoutCx, graphkit};
use crate::geom::{PathData, Shape};
use crate::ir::{Align, Geom, Rect, SceneBuilder};
use crate::layout::grid::{self, Connect, GridItem, GridSpec};
use crate::layout::{self as shared};
use crate::style::{self, Paint};
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene};

struct Geometry {
    labels: Vec<Option<LaidText>>,
    natural: Vec<[f32; 2]>,
    rects: Vec<Rect>,
    gap: f32,
    pad: f32,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let wrap = cx.wrap_width("block.wrappingWidth");
    let pad = [m.node_pad_x, m.node_pad_y];
    let n = model.blocks.len();

    let mut geo = Geometry {
        labels: Vec::with_capacity(n),
        natural: vec![[0.0, 0.0]; n],
        rects: vec![Rect::default(); n],
        gap: m.node_sep * 0.5,
        pad: m.cluster_pad * 0.75,
    };
    let mut paints = Vec::with_capacity(n);
    for block in &model.blocks {
        let paint = Paint::node().with_author(&block.style);
        let face = FontSpec {
            weight: paint.weight.unwrap_or(font.weight),
            italic: paint.italic,
            ..font
        };
        let laid = match block.kind {
            Kind::Node(_) | Kind::Arrow(_) => Some(cx.text.lay(&block.label, face, Some(wrap))).filter(|l| !l.is_empty()),
            _ => None,
        };
        geo.labels.push(laid);
        paints.push(paint);
    }
    // Natural sizes, children before their groups.
    let space = [m.font_size * 4.0, 0.0];
    for &root in &model.root {
        measure(model, root, &mut geo, pad, space);
    }
    let root_spec = grid_spec(model, &model.root, model.columns, &geo, [0.0, 0.0]);
    let root_grid = grid::solve(&root_spec);
    place_children(model, &model.root, model.columns, &mut geo, Rect::new(0.0, 0.0, root_grid.size[0], root_grid.size[1]));

    let mut out = SceneBuilder::new();

    // Groups, outermost first.
    let mut stack: Vec<usize> = model.root.iter().rev().copied().collect();
    while let Some(i) = stack.pop() {
        let block = &model.blocks[i];
        if let Kind::Group { .. } = block.kind {
            graphkit::cluster(&mut out, geo.rects[i], None, &block.style);
            stack.extend(block.children.iter().rev().copied());
        }
    }

    // Links under blocks.
    let leaves: Vec<Rect> = model
        .blocks
        .iter()
        .enumerate()
        .filter(|(_, b)| matches!(b.kind, Kind::Node(_) | Kind::Arrow(_)))
        .map(|(i, _)| geo.rects[i])
        .collect();
    let links: Vec<Connect> = model.edges.iter().map(|e| Connect { from: e.from, to: e.to }).collect();
    let routes = grid::connect_all(&geo.rects, &links, &leaves, geo.gap);
    let marker = cx.marker_size();
    let small = cx.small_font();
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let mut chips = Vec::new();
    let mut taken = Vec::new();
    for (e, edge) in model.edges.iter().enumerate() {
        let mut points = routes[e].clone();
        if edge.line == LineKind::Invisible || points.len() < 2 {
            continue;
        }
        if let Kind::Node(shape) = model.blocks[edge.from].kind {
            shared::attach_start(&mut points, shape, geo.rects[edge.from]);
        }
        if let Kind::Node(shape) = model.blocks[edge.to].kind {
            shared::attach_end(&mut points, shape, geo.rects[edge.to]);
        }
        let stroke = link_stroke(edge.line, &Default::default());
        graphkit::edge(&mut out, &points, stroke, edge.start, edge.end, marker, m.bend_radius);
        if let Some(label) = &edge.label {
            let laid = cx.text.lay(label, small, Some(wrap));
            if !laid.is_empty() {
                let size = [laid.width + 2.0 * chip_pad[0], laid.height + 2.0 * chip_pad[1]];
                let at = grid::place_label(&points, size, &leaves, &mut taken);
                chips.push((laid, at));
            }
        }
    }

    // Blocks.
    for (i, block) in model.blocks.iter().enumerate() {
        let rect = geo.rects[i];
        let paint = &paints[i];
        match block.kind {
            Kind::Node(shape) => {
                draw_node(&mut out, shape, rect, paint);
                if let Some(laid) = &geo.labels[i] {
                    let lr = shape.label_rect(rect, laid.size());
                    let ink = if shape.label_outside() { style::TEXT } else { paint.text };
                    out.text(laid, [lr.x, lr.y], lr.w, Align::Center, ink);
                }
            }
            Kind::Arrow(heads) => {
                let (path, body) = arrow_outline(rect, heads);
                out.shape(Geom::path(path), Some(paint.fill), Some(paint.stroke));
                if let Some(laid) = &geo.labels[i] {
                    out.text_centered(laid, body.center(), paint.text);
                }
            }
            Kind::Space | Kind::Group { .. } => {}
        }
    }
    for (laid, center) in &chips {
        out.chip(laid, *center, chip_pad);
    }

    let content = out.bounds().union(&Rect::new(0.0, 0.0, root_grid.size[0].max(1.0), root_grid.size[1].max(1.0)));
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

fn measure(model: &Model, i: usize, geo: &mut Geometry, pad: [f32; 2], space: [f32; 2]) -> [f32; 2] {
    let block = &model.blocks[i];
    let label = geo.labels[i].as_ref().map_or([0.0, 0.0], LaidText::size);
    let size = match &block.kind {
        Kind::Node(shape) => {
            let s = shape.size(label, pad);
            [s[0].max(pad[0] * 2.0), s[1]]
        }
        Kind::Arrow(heads) => arrow_size(label, pad, *heads),
        Kind::Space => space,
        Kind::Group { columns } => {
            for &c in &block.children {
                measure(model, c, geo, pad, space);
            }
            let spec = grid_spec(model, &block.children, *columns, geo, [0.0, 0.0]);
            let g = grid::solve(&spec);
            [g.size[0] + 2.0 * geo.pad, g.size[1] + 2.0 * geo.pad]
        }
    };
    geo.natural[i] = size;
    size
}

fn grid_spec(model: &Model, children: &[usize], columns: Option<usize>, geo: &Geometry, min: [f32; 2]) -> GridSpec {
    let spans: Vec<usize> = children.iter().map(|&c| model.blocks[c].span).collect();
    let columns = columns.unwrap_or_else(|| spans.iter().sum::<usize>().max(1));
    let cells = grid::flow(&spans, columns);
    GridSpec {
        items: children
            .iter()
            .zip(&cells)
            .map(|(&c, &(col, row, span))| GridItem::new(col, row, geo.natural[c]).span(span, 1))
            .collect(),
        col_gap: geo.gap,
        row_gap: geo.gap,
        min_size: min,
    }
}

fn place_children(model: &Model, children: &[usize], columns: Option<usize>, geo: &mut Geometry, area: Rect) {
    let spec = grid_spec(model, children, columns, geo, [area.w, area.h]);
    let g = grid::solve(&spec);
    for (k, &c) in children.iter().enumerate() {
        let cell = g.cells[k].translated(area.x, area.y);
        let natural = geo.natural[c];
        let rect = match &model.blocks[c].kind {
            Kind::Node(Shape::Circle | Shape::DoubleCircle | Shape::CrossedCircle | Shape::SmallCircle | Shape::FilledCircle | Shape::FramedCircle) => {
                Rect::centered(cell.center(), natural)
            }
            Kind::Arrow(heads) if heads.horizontal() => Rect::centered(cell.center(), [cell.w, natural[1]]),
            Kind::Arrow(_) => Rect::centered(cell.center(), [natural[0], cell.h]),
            _ => cell,
        };
        geo.rects[c] = rect;
        if let Kind::Group { columns } = model.blocks[c].kind {
            let inner = rect.inset(geo.pad, geo.pad);
            let kids = model.blocks[c].children.clone();
            place_children(model, &kids, columns, geo, inner);
        }
    }
}

/// Body thickness as a share of a block arrow's full width across.
const BODY: f32 = 0.6;

fn arrow_size(label: [f32; 2], pad: [f32; 2], heads: Heads) -> [f32; 2] {
    let count = |a: bool, b: bool| f32::from(u8::from(a) + u8::from(b));
    if heads.horizontal() {
        let body = label[1] + 2.0 * pad[1];
        let across = body / BODY;
        let head = across * 0.5;
        [label[0] + 2.0 * pad[0] + head * count(heads.left, heads.right), across]
    } else {
        let body = label[0] + 2.0 * pad[0];
        let across = body / BODY;
        let head = (across * 0.5).min(label[1] + 2.0 * pad[1]);
        [across, label[1] + 2.0 * pad[1] + head * count(heads.up, heads.down)]
    }
}

/// The arrow outline and the rectangle its label sits in.
fn arrow_outline(r: Rect, heads: Heads) -> (PathData, Rect) {
    if heads.horizontal() {
        let (left, right) = if heads.left || heads.right { (heads.left, heads.right) } else { (false, true) };
        let cy = r.y + r.h / 2.0;
        let half = r.h * BODY / 2.0;
        let head = (r.h * 0.5).min(r.w * 0.45);
        let x0 = r.x + if left { head } else { 0.0 };
        let x1 = r.right() - if right { head } else { 0.0 };
        let mut pts = vec![[x0, cy - half], [x1, cy - half]];
        if right {
            pts.extend([[x1, r.y], [r.right(), cy], [x1, r.bottom()]]);
        }
        pts.push([x1, cy + half]);
        pts.push([x0, cy + half]);
        if left {
            pts.extend([[x0, r.bottom()], [r.x, cy], [x0, r.y]]);
        }
        (PathData::polygon(&pts), Rect::new(x0, cy - half, x1 - x0, 2.0 * half))
    } else {
        let cx = r.x + r.w / 2.0;
        let half = r.w * BODY / 2.0;
        let head = (r.w * 0.5).min(r.h * 0.45);
        let y0 = r.y + if heads.up { head } else { 0.0 };
        let y1 = r.bottom() - if heads.down { head } else { 0.0 };
        let mut pts = vec![[cx + half, y0], [cx + half, y1]];
        if heads.down {
            pts.extend([[r.right(), y1], [cx, r.bottom()], [r.x, y1]]);
        }
        pts.push([cx - half, y1]);
        pts.push([cx - half, y0]);
        if heads.up {
            pts.extend([[r.x, y0], [cx, r.y], [r.right(), y0]]);
        }
        (PathData::polygon(&pts), Rect::new(cx - half, y0, 2.0 * half, y1 - y0))
    }
}
