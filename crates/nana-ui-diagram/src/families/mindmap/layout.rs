//! Mind map drawing: measure, split the first-level branches between the
//! two sides, place each side as a tidy tree and draw curves under nodes.

use std::f32::consts::{PI, SQRT_2, TAU};

use super::{Model, NodeShape};
use crate::families::LayoutCx;
use crate::geom::{PathData, Shape};
use crate::ir::{Align, Corner, Geom, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::layout::tree::{TidyGaps, TidyNode, preorder, tidy};
use crate::text::{FontSpec, LaidText};
use crate::style::{self, series};
use crate::{DiagramError, DiagramScene};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Right,
    Left,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let mut out = SceneBuilder::new();
    if model.nodes.is_empty() {
        return Ok(out.finish(Rect::new(0.0, 0.0, 1.0, 1.0), m.margin));
    }
    let wrap = cx.wrap_width("mindmap.maxNodeWidth");
    let font = cx.font();
    let root_font = FontSpec::new(m.title_size).weight(m.strong_weight);
    let icon = m.small_size;
    let icon_gap = m.small_size * 0.4;
    let pad = [m.node_pad_x * 0.75, m.node_pad_y * 0.8];
    // The icon and its gap to the label, if the node has one.
    let icon_room = |has_icon: bool, laid: &LaidText| {
        if !has_icon {
            0.0
        } else if laid.is_empty() {
            icon
        } else {
            icon + icon_gap
        }
    };

    // Measure and size.
    let mut labels: Vec<LaidText> = Vec::with_capacity(model.nodes.len());
    let mut sizes: Vec<[f32; 2]> = Vec::with_capacity(model.nodes.len());
    for (i, node) in model.nodes.iter().enumerate() {
        let face = if i == 0 { root_font } else { font };
        let laid = cx.text.lay(&node.label, face, Some(wrap));
        let mut inner = laid.size();
        if node.icon.is_some() {
            inner[0] += icon_room(true, &laid);
            inner[1] = inner[1].max(icon);
        }
        inner[0] = inner[0].max(m.font_size);
        sizes.push(shape_size(shape_of(model, i), inner, pad));
        labels.push(laid);
    }

    // Balance the first-level branches between the two sides by weight
    // (leaf extent), keeping their order: the right side takes them top to
    // bottom, the left side continues bottom to top.
    let children: Vec<Vec<usize>> = model.nodes.iter().map(|n| n.children.clone()).collect();
    let root = &model.nodes[0];
    let weights: Vec<f32> = root
        .children
        .iter()
        .map(|&c| subtree_weight(&children, &sizes, c))
        .collect();
    let total: f32 = weights.iter().sum();
    let mut split = root.children.len();
    if root.children.len() > 1 {
        let mut best = f32::INFINITY;
        let mut acc = 0.0;
        for (k, w) in weights.iter().enumerate() {
            acc += w;
            let diff = (acc - (total - acc)).abs();
            if diff < best - 1e-3 {
                best = diff;
                split = k + 1;
            }
        }
    }
    let right: Vec<usize> = root.children[..split].to_vec();
    let left: Vec<usize> = root.children[split..].iter().rev().copied().collect();

    let tidy_nodes: Vec<TidyNode> = model
        .nodes
        .iter()
        .zip(&sizes)
        .map(|(node, size)| TidyNode {
            depth_size: size[0],
            breadth_size: size[1],
            children: node.children.clone(),
        })
        .collect();
    let gaps = TidyGaps {
        level: m.rank_sep * 0.9,
        sibling: m.node_sep * 0.3,
        subtree: m.node_sep * 0.45,
    };
    let root_rect = Rect::centered([0.0, 0.0], sizes[0]);
    let root_gap = m.rank_sep * 1.1;
    let mut rects = vec![root_rect; model.nodes.len()];
    let mut sides = vec![Side::Right; model.nodes.len()];
    let mut branch = vec![0; model.nodes.len()];
    for (b, &c) in root.children.iter().enumerate() {
        for (node, _) in preorder(&children, &[c]) {
            branch[node] = b;
        }
    }
    for (side, roots) in [(Side::Right, &right), (Side::Left, &left)] {
        if roots.is_empty() {
            continue;
        }
        let places = tidy(&tidy_nodes, roots, gaps);
        for (node, _) in preorder(&children, roots) {
            let p = places[node];
            let [w, h] = sizes[node];
            let x = match side {
                Side::Right => root_rect.right() + root_gap + p.depth,
                Side::Left => root_rect.x - root_gap - p.depth - w,
            };
            rects[node] = Rect::new(x, p.breadth - h / 2.0, w, h);
            sides[node] = side;
        }
    }

    // Links, under the nodes.
    for (parent, node) in model.nodes.iter().enumerate() {
        for &child in &node.children {
            let side = sides[child];
            let from = if parent == 0 {
                rects[0].center()
            } else {
                anchor(rects[parent], shape_of(model, parent), side, true)
            };
            let to = anchor(rects[child], shape_of(model, child), side, false);
            let dx = (to[0] - from[0]) * 0.5;
            let mut path = PathData::new();
            path.move_to(from)
                .cubic_to([from[0] + dx, from[1]], [to[0] - dx, to[1]], to);
            let width = if parent == 0 {
                StrokeWidth::Thick
            } else {
                StrokeWidth::Hairline
            };
            out.stroke(
                Geom::path(path),
                Stroke::new(series(branch[child], Tone::Solid), width),
            );
        }
    }

    // Nodes.
    for (i, node) in model.nodes.iter().enumerate() {
        let rect = rects[i];
        let shape = shape_of(model, i);
        let (fill, stroke, text) = if i == 0 {
            (style::ACCENT, None, style::ACCENT_TEXT)
        } else {
            let line = series(branch[i], Tone::Solid);
            (
                series(branch[i], Tone::Soft),
                Some(Stroke::hairline(line)),
                style::TEXT,
            )
        };
        match shape {
            NodeShape::Default => {
                let y = rect.bottom();
                let mut line = PathData::new();
                line.move_to([rect.x, y]).line_to([rect.right(), y]);
                let width = if root.children.contains(&i) {
                    StrokeWidth::Thick
                } else {
                    StrokeWidth::Hairline
                };
                out.stroke(
                    Geom::path(line),
                    Stroke::new(series(branch[i], Tone::Solid), width),
                );
            }
            _ => out.shape(outline(shape, rect), Some(fill), stroke),
        }
        let laid = &labels[i];
        let content_w = laid.width + icon_room(node.icon.is_some(), laid);
        let c = rect.center();
        let mut x = c[0] - content_w / 2.0;
        if node.icon.is_some() {
            let glyph = Rect::new(x, c[1] - icon / 2.0, icon, icon);
            let ink = if i == 0 {
                style::ACCENT_TEXT
            } else {
                series(branch[i], Tone::Solid)
            };
            out.shape(
                Geom::Rect {
                    rect: glyph.inset(icon * 0.1, icon * 0.1),
                    corner: Corner::Tier(nana_ui_core::RadiusTier::Xs),
                },
                None,
                Some(Stroke::hairline(ink)),
            );
            out.fill(Geom::Ellipse(glyph.inset(icon * 0.32, icon * 0.32)), ink);
            x += icon + icon_gap;
        }
        if !laid.is_empty() {
            out.text(
                laid,
                [x, c[1] - laid.height / 2.0],
                laid.width,
                Align::Center,
                text,
            );
        }
    }

    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// The root has no bare form: undecorated, it is a rounded box.
fn shape_of(model: &Model, i: usize) -> NodeShape {
    match model.nodes[i].shape {
        NodeShape::Default if i == 0 => NodeShape::Rounded,
        shape => shape,
    }
}

/// The breadth a subtree needs, counted over its leaves.
fn subtree_weight(children: &[Vec<usize>], sizes: &[[f32; 2]], root: usize) -> f32 {
    preorder(children, &[root])
        .into_iter()
        .filter(|&(n, _)| children[n].is_empty())
        .map(|(n, _)| sizes[n][1] + 8.0)
        .sum()
}

fn shape_size(shape: NodeShape, inner: [f32; 2], pad: [f32; 2]) -> [f32; 2] {
    let [w, h] = inner;
    let [px, py] = pad;
    match shape {
        NodeShape::Default => [w + px * 0.5, h + py],
        NodeShape::Square => Shape::Rect.size(inner, pad),
        NodeShape::Rounded => Shape::Rounded.size(inner, pad),
        NodeShape::Circle => Shape::Circle.size(inner, pad),
        NodeShape::Hexagon => Shape::Hexagon.size(inner, pad),
        // The label box fits the inner ellipse; spikes and bumps go
        // outside it.
        NodeShape::Bang | NodeShape::Cloud => {
            let a = (w / 2.0 + px * 0.5) * SQRT_2;
            let b = (h / 2.0 + py * 0.5) * SQRT_2;
            if shape == NodeShape::Bang {
                [2.0 * a / BANG_INNER, 2.0 * b / BANG_INNER]
            } else {
                [2.0 * a * CLOUD_PUFF, 2.0 * b * CLOUD_PUFF]
            }
        }
    }
}

const BANG_INNER: f32 = 0.8;
const CLOUD_PUFF: f32 = 1.14;

fn outline(shape: NodeShape, r: Rect) -> Geom {
    let c = r.center();
    match shape {
        NodeShape::Default | NodeShape::Square => Shape::Rect.outline(r),
        NodeShape::Rounded => Shape::Rounded.outline(r),
        NodeShape::Circle => Shape::Circle.outline(r),
        NodeShape::Hexagon => Shape::Hexagon.outline(r),
        NodeShape::Bang => {
            let spikes = 14;
            let (a, b) = (r.w / 2.0, r.h / 2.0);
            let points: Vec<[f32; 2]> = (0..spikes * 2)
                .map(|k| {
                    let t = k as f32 / (spikes * 2) as f32 * TAU - PI / 2.0;
                    let s = if k % 2 == 0 { 1.0 } else { BANG_INNER };
                    [c[0] + a * s * t.cos(), c[1] + b * s * t.sin()]
                })
                .collect();
            Geom::path(PathData::polygon(&points))
        }
        NodeShape::Cloud => {
            let puffs = 10;
            // Ends sit on the base ellipse, controls outside it so each
            // puff peaks at the rectangle's edge.
            let (a, b) = (r.w / 2.0 / CLOUD_PUFF, r.h / 2.0 / CLOUD_PUFF);
            let half = PI / puffs as f32;
            let reach = (2.0 * CLOUD_PUFF - half.cos()).max(1.0);
            let at = |t: f32, s: f32| [c[0] + a * s * t.cos(), c[1] + b * s * t.sin()];
            let mut p = PathData::new();
            p.move_to(at(0.0, 1.0));
            for k in 0..puffs {
                let t0 = k as f32 * 2.0 * half;
                p.quad_to(at(t0 + half, reach), at(t0 + 2.0 * half, 1.0));
            }
            p.close();
            Geom::path(p)
        }
    }
}

/// Where a link meets a node: bare nodes at their underline, others at the
/// middle of the side facing the link.
fn anchor(rect: Rect, shape: NodeShape, side: Side, outgoing: bool) -> [f32; 2] {
    let y = match shape {
        NodeShape::Default => rect.bottom(),
        _ => rect.center()[1],
    };
    let right_edge = (side == Side::Right) == outgoing;
    let x = if right_edge { rect.right() } else { rect.x };
    [x, y]
}
