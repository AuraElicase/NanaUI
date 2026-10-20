//! Ishikawa (fishbone) diagrams: causes branching from a spine toward an
//! effect.
//!
//! The first statement is the effect, drawn as the head at the right end of
//! a horizontal spine. Every top-level item after it is a category bone,
//! alternating above and below the spine and slanting back from it; its
//! indented items are causes on horizontal ticks that meet the bone, and
//! causes of causes hang from a smaller bone parallel to it.

use std::collections::HashMap;

use nana_ui_core::RadiusTier;

use super::{LayoutCx, ParseCx, graphkit, kit};
use crate::geom::PathData;
use crate::ir::{Align, Corner, Geom, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::layout::tree::{Outline, indent_width, preorder};
use crate::text::{FontSpec, Label, LaidText};
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub label: Label,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// The effect at the head.
    pub effect: Option<Label>,
    pub nodes: Vec<Node>,
    /// Category bones.
    pub categories: Vec<usize>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model::default();
    let mut outline = Outline::new();
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        model.effect = Some(cx.label(rest)?);
    }
    for line in body.lines().skip(header + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if model.effect.is_none() {
            model.effect = Some(cx.label(trimmed)?);
            continue;
        }
        let id = model.nodes.len();
        cx.limits
            .check_count(id + 1, cx.limits.nodes, LimitKind::Nodes)?;
        let label = cx.label(trimmed)?;
        match outline.push(indent_width(line), id) {
            Some(p) => model.nodes[p].children.push(id),
            None => model.categories.push(id),
        }
        model.nodes.push(Node {
            label,
            children: Vec::new(),
        });
    }
    Ok(model)
}

/// Horizontal run of a bone per unit of distance from what it hangs on.
const SLANT: f32 = 0.6;

struct Row {
    node: usize,
    laid: LaidText,
    /// Distance of the tick line from the spine.
    line: f32,
    /// Distance of the text's spine-side edge from the spine.
    near: f32,
}

/// One category's rows, outward from the spine.
struct Bone {
    rows: Vec<Row>,
    /// Bone length to the tip, where the label box starts.
    length: f32,
    label: LaidText,
    box_size: [f32; 2],
}

/// A tick relative to its category's base on the spine.
struct Tick {
    start: f32,
    end: f32,
    text_x: f32,
    /// Where a smaller bone for the row's own causes leaves the tick.
    anchor: f32,
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let mut out = SceneBuilder::new();
    let font = cx.font();
    let small = cx.small_font();
    let strong = FontSpec::new(m.font_size).weight(m.strong_weight);
    let wrap = cx.wrap_width("ishikawa.wrappingWidth") * 0.8;
    let stub = m.font_size * 1.2;
    let sub_offset = m.font_size * 0.8;
    let row_gap = m.font_size * 0.35;
    let pad = [m.node_pad_x * 0.75, m.node_pad_y * 0.7];
    let children: Vec<Vec<usize>> = model.nodes.iter().map(|n| n.children.clone()).collect();
    let mut parents: Vec<Option<usize>> = vec![None; model.nodes.len()];
    for (p, node) in model.nodes.iter().enumerate() {
        for &c in &node.children {
            parents[c] = Some(p);
        }
    }

    // Measure each category's rows outward from the spine. Text always
    // sits on its tick on screen: above the spine the tick runs along the
    // text's spine side, below it along the far side.
    let mut bones = Vec::with_capacity(model.categories.len());
    for (b, &cat) in model.categories.iter().enumerate() {
        let above = b % 2 == 0;
        let mut rows = Vec::new();
        let mut dist = m.font_size * 1.1;
        for (node, depth) in preorder(&children, &model.nodes[cat].children) {
            let face = if depth == 0 { font } else { small };
            let laid = cx.text.lay(&model.nodes[node].label, face, Some(wrap));
            let near = dist;
            dist += laid.height + 2.0;
            rows.push(Row {
                node,
                line: if above { near } else { dist },
                near,
                laid,
            });
            dist += row_gap;
        }
        let label = cx.text.lay(&model.nodes[cat].label, strong, Some(wrap));
        let box_size = [label.width + 2.0 * pad[0], label.height + 2.0 * pad[1]];
        bones.push(Bone {
            rows,
            length: dist + m.font_size * 0.4,
            label,
            box_size,
        });
    }

    // Where every row's tick meets its bone, relative to the category base
    // (x grows right).
    let mut ticks: Vec<Vec<Tick>> = Vec::with_capacity(bones.len());
    let mut last_child: Vec<Vec<Option<usize>>> = Vec::with_capacity(bones.len());
    let mut extents: Vec<(f32, f32)> = Vec::with_capacity(bones.len());
    for bone in &bones {
        let mut row_of: HashMap<usize, usize> = HashMap::new();
        let mut list: Vec<Tick> = Vec::with_capacity(bone.rows.len());
        let mut last = vec![None; bone.rows.len()];
        let mut left: f32 = -SLANT * bone.length - bone.box_size[0] / 2.0;
        let right = (-SLANT * bone.length + bone.box_size[0] / 2.0).max(0.0);
        for (r, row) in bone.rows.iter().enumerate() {
            let parent_row = parents[row.node].and_then(|p| row_of.get(&p).copied());
            let end = match parent_row {
                Some(pr) => {
                    last[pr] = Some(r);
                    list[pr].anchor - SLANT * (row.line - bone.rows[pr].line).abs()
                }
                None => -SLANT * row.line,
            };
            let reach = if model.nodes[row.node].children.is_empty() {
                stub
            } else {
                stub + sub_offset + SLANT * (row.laid.height + 2.0)
            };
            let text_x = end - reach - row.laid.width;
            let start = text_x - 4.0;
            left = left.min(start);
            list.push(Tick {
                start,
                end,
                text_x,
                anchor: end - sub_offset,
            });
            row_of.insert(row.node, r);
        }
        extents.push((left, right));
        ticks.push(list);
        last_child.push(last);
    }

    // Head on the right; categories in pairs leftward, top then bottom.
    let effect = model
        .effect
        .as_ref()
        .map(|e| cx.text.lay(e, strong, Some(wrap)));
    let head_size = effect.as_ref().map_or([0.0, 0.0], |e| {
        [e.width + 2.0 * m.node_pad_x, e.height + 2.0 * m.node_pad_y]
    });
    let head = Rect::new(0.0, -head_size[1] / 2.0, head_size[0], head_size[1]);
    let gap = m.node_sep * 0.6;
    let mut limit = [-gap, -gap];
    let mut bases = vec![0.0f32; bones.len()];
    for pair in (0..bones.len()).step_by(2) {
        let members = pair..(pair + 2).min(bones.len());
        let base = members
            .clone()
            .map(|b| limit[b % 2] - extents[b].1)
            .fold(f32::INFINITY, f32::min);
        for b in members {
            bases[b] = base;
            limit[b % 2] = base + extents[b].0 - gap;
        }
    }
    let tail = limit[0].min(limit[1]) + gap * 0.5;

    // Spine with a tail.
    let line = style::edge_stroke();
    let thick = Stroke::new(style::EDGE, StrokeWidth::Thick);
    graphkit::line(&mut out, [tail, 0.0], [head.x, 0.0], thick);
    let fin = m.font_size * 0.8;
    let mut tail_path = PathData::new();
    tail_path
        .move_to([tail - fin, -fin])
        .line_to([tail, 0.0])
        .line_to([tail - fin, fin]);
    out.stroke(Geom::path(tail_path), thick);

    for (b, bone) in bones.iter().enumerate() {
        let sign = if b % 2 == 0 { -1.0 } else { 1.0 };
        let bx = bases[b];
        let colour = style::series(b, Tone::Solid);
        let tip = [bx - SLANT * bone.length, sign * bone.length];
        graphkit::line(
            &mut out,
            [bx, 0.0],
            tip,
            Stroke::new(colour, StrokeWidth::Thick),
        );

        for (r, row) in bone.rows.iter().enumerate() {
            let t = &ticks[b][r];
            if let Some(last) = last_child[b][r] {
                let far = bone.rows[last].line;
                graphkit::line(
                    &mut out,
                    [bx + t.anchor, sign * row.line],
                    [bx + t.anchor - SLANT * (far - row.line).abs(), sign * far],
                    line,
                );
            }
            let y = sign * row.line;
            graphkit::line(&mut out, [bx + t.start, y], [bx + t.end, y], line);
            let top = if sign < 0.0 {
                -(row.near + row.laid.height + 2.0)
            } else {
                row.near
            };
            let ink = if parents[row.node] == Some(model.categories[b]) {
                style::TEXT
            } else {
                style::MUTED
            };
            out.text(
                &row.laid,
                [bx + t.text_x, top],
                row.laid.width,
                Align::End,
                ink,
            );
        }

        let [w, h] = bone.box_size;
        let rect = Rect::new(
            tip[0] - w / 2.0,
            if sign < 0.0 { tip[1] - h } else { tip[1] },
            w,
            h,
        );
        out.shape(
            Geom::Rect {
                rect,
                corner: Corner::Tier(RadiusTier::Sm),
            },
            Some(style::series(b, Tone::Soft)),
            Some(Stroke::hairline(colour)),
        );
        out.text_centered(&bone.label, rect.center(), style::TEXT);
    }

    if let Some(effect) = &effect {
        out.shape(
            Geom::Rect {
                rect: head,
                corner: Corner::Tier(RadiusTier::Md),
            },
            Some(style::ACCENT),
            None,
        );
        out.text_centered(effect, head.center(), style::ACCENT_TEXT);
    }

    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.nodes.len() as u32 + u32::from(model.effect.is_some()),
        edges: 0,
        groups: model.categories.len() as u32,
    }
}
