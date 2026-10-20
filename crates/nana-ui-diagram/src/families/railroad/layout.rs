//! Railroad drawing: every expression is a box with a track entering at
//! its left edge and leaving at its right edge on the same line. A box
//! knows its width and how far it reaches above (`up`) and below (`down`)
//! that line; compound boxes are built from their parts' boxes and the
//! tracks between them turn through quarter arcs.

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use super::{Expr, Model};
use crate::families::LayoutCx;
use crate::geom::PathData;
use crate::ir::{Align, Corner, Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth};
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene, style};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Leaf {
    Terminal,
    NonTerminal,
    Special,
}

enum Kind {
    Leaf(Leaf, LaidText),
    Comment(LaidText),
    Skip,
    Sequence(Vec<Node>),
    /// Branch track offsets below the main line (the first is 0).
    Choice(Vec<Node>, Vec<f32>),
    /// The bypass line's offset (negative: above).
    Optional(Box<Node>, f32),
    Loop {
        item: Box<Node>,
        separator: Option<Box<Node>>,
        note: Option<LaidText>,
        /// The return line's offset below the main line.
        back: f32,
    },
    Group(Box<Node>, Option<LaidText>),
}

struct Node {
    w: f32,
    up: f32,
    down: f32,
    kind: Kind,
}

struct Metrics {
    arc: f32,
    hgap: f32,
    vgap: f32,
    pad_x: f32,
    box_h: f32,
    frame: f32,
    font: FontSpec,
    mono: FontSpec,
    small: FontSpec,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let k = Metrics {
        arc: m.font_size * 0.6,
        hgap: m.font_size * 0.8,
        vgap: m.font_size * 0.6,
        pad_x: m.font_size * 0.7,
        box_h: cx.text.line_advance(font) + m.font_size * 0.6,
        frame: m.font_size * 0.45,
        font,
        mono: FontSpec { mono: true, ..font },
        small: FontSpec {
            italic: true,
            ..cx.small_font()
        },
    };
    let mut out = SceneBuilder::new();
    let name_font = FontSpec::new(m.small_size).weight(m.strong_weight);
    let lead = k.arc * 2.0;
    let mut y = 0.0;
    for rule in &model.rules {
        let name = cx.text.lay_plain(&rule.name, name_font);
        out.text(&name, [0.0, y], name.width, Align::Start, style::MUTED);
        let node = measure(&rule.expr, cx, &k);
        let track = y + name.height + m.font_size * 0.5 + node.up.max(k.arc);
        let end = lead + node.w + lead;
        let rail = track_stroke();
        for x in [0.0, end] {
            let mut bar = PathData::new();
            bar.move_to([x, track - k.arc]).line_to([x, track + k.arc]);
            out.stroke(Geom::path(bar), rail);
        }
        line(&mut out, [0.0, track], lead);
        draw(&node, lead, track, &mut out, &k);
        line(&mut out, [lead + node.w, track], lead);
        y = track + node.down.max(k.arc) + m.font_size * 1.6;
    }
    let content = if model.rules.is_empty() {
        Rect::new(0.0, 0.0, m.font_size, m.font_size)
    } else {
        out.bounds()
    };
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

fn track_stroke() -> Stroke {
    Stroke::new(style::EDGE, StrokeWidth::Thick)
}

fn line(out: &mut SceneBuilder, from: [f32; 2], len: f32) {
    if len <= 0.01 {
        return;
    }
    let mut p = PathData::new();
    p.move_to(from).line_to([from[0] + len, from[1]]);
    out.stroke(Geom::path(p), track_stroke());
}

fn measure(expr: &Expr, cx: &LayoutCx<'_>, k: &Metrics) -> Node {
    let leaf = |kind: Leaf, text: &str, font: FontSpec| {
        let laid = cx.text.lay_plain(text, font);
        Node {
            w: laid.width + 2.0 * k.pad_x,
            up: k.box_h / 2.0,
            down: k.box_h / 2.0,
            kind: Kind::Leaf(kind, laid),
        }
    };
    match expr {
        Expr::Terminal(t) => leaf(Leaf::Terminal, t, k.mono),
        Expr::NonTerminal(t) => leaf(Leaf::NonTerminal, t, k.font),
        Expr::Special(t) => leaf(Leaf::Special, t, k.font),
        Expr::Comment(t) => {
            let laid = cx.text.lay_plain(t, k.small);
            Node {
                w: laid.width,
                up: laid.height + 2.0,
                down: 0.0,
                kind: Kind::Comment(laid),
            }
        }
        Expr::Skip => Node {
            w: 0.0,
            up: 0.0,
            down: 0.0,
            kind: Kind::Skip,
        },
        Expr::Sequence(items) => {
            let nodes: Vec<Node> = items.iter().map(|e| measure(e, cx, k)).collect();
            let w = nodes.iter().map(|n| n.w).sum::<f32>()
                + k.hgap * nodes.len().saturating_sub(1) as f32;
            Node {
                w,
                up: nodes.iter().map(|n| n.up).fold(0.0, f32::max),
                down: nodes.iter().map(|n| n.down).fold(0.0, f32::max),
                kind: Kind::Sequence(nodes),
            }
        }
        Expr::Choice(items) => {
            let nodes: Vec<Node> = items.iter().map(|e| measure(e, cx, k)).collect();
            let mut ys = Vec::with_capacity(nodes.len());
            let mut y: f32 = 0.0;
            for (i, n) in nodes.iter().enumerate() {
                if i > 0 {
                    y = (y + nodes[i - 1].down + k.vgap + n.up).max(y + 2.0 * k.arc);
                }
                ys.push(y);
            }
            let inner = nodes.iter().map(|n| n.w).fold(0.0, f32::max);
            Node {
                w: inner + 4.0 * k.arc,
                up: nodes.first().map_or(0.0, |n| n.up),
                down: nodes.last().map_or(0.0, |n| n.down) + y,
                kind: Kind::Choice(nodes, ys),
            }
        }
        Expr::Optional(inner) => {
            let item = measure(inner, cx, k);
            let bypass = -(item.up + k.vgap).max(2.0 * k.arc);
            Node {
                w: item.w + 4.0 * k.arc,
                up: -bypass,
                down: item.down,
                kind: Kind::Optional(Box::new(item), bypass),
            }
        }
        Expr::OneOrMore {
            item,
            separator,
            note,
        } => {
            let item = measure(item, cx, k);
            let separator = separator.as_deref().map(|s| Box::new(measure(s, cx, k)));
            let note = note.as_deref().map(|n| cx.text.lay_plain(n, k.small));
            let sep_up = separator.as_ref().map_or(0.0, |s| s.up);
            let sep_down = separator.as_ref().map_or(0.0, |s| s.down);
            let back = (item.down + k.vgap + sep_up).max(2.0 * k.arc);
            let inner = item.w.max(separator.as_ref().map_or(0.0, |s| s.w));
            let note_w = note.as_ref().map_or(0.0, |n| n.width);
            let note_h = note.as_ref().map_or(0.0, |n| n.height + 2.0);
            Node {
                w: inner.max(note_w) + 4.0 * k.arc,
                up: item.up,
                down: back + sep_down.max(1.0) + note_h,
                kind: Kind::Loop {
                    item: Box::new(item),
                    separator,
                    note,
                    back,
                },
            }
        }
        Expr::Group(inner, label) => {
            let item = measure(inner, cx, k);
            let label = label.as_deref().map(|l| cx.text.lay_plain(l, k.small));
            let label_w = label.as_ref().map_or(0.0, |l| l.width);
            let label_h = label.as_ref().map_or(0.0, |l| l.height);
            Node {
                w: item.w.max(label_w) + 2.0 * k.frame + k.hgap,
                up: item.up + k.frame + label_h,
                down: item.down + k.frame,
                kind: Kind::Group(Box::new(item), label),
            }
        }
    }
}

/// Draw `node` with its track entering at `(x, y)`.
fn draw(node: &Node, x: f32, y: f32, out: &mut SceneBuilder, k: &Metrics) {
    let r = k.arc;
    match &node.kind {
        Kind::Leaf(kind, laid) => {
            let rect = Rect::new(x, y - node.up, node.w, node.up + node.down);
            let (corner, fill, stroke, ink) = match kind {
                Leaf::Terminal => (
                    RadiusTier::Full,
                    Ink::Mix(SemanticColorMix::alpha(R::Accent, 0.12)),
                    Stroke::hairline(style::ACCENT),
                    style::TEXT,
                ),
                Leaf::NonTerminal => (
                    RadiusTier::Xs,
                    style::NODE_FILL,
                    style::node_stroke(),
                    style::TEXT,
                ),
                Leaf::Special => (
                    RadiusTier::Sm,
                    Ink::Role(R::Subtle),
                    Stroke::hairline(Ink::Role(R::Border)).dash(Dash::Dashed),
                    style::MUTED,
                ),
            };
            out.shape(
                Geom::Rect {
                    rect,
                    corner: Corner::Tier(corner),
                },
                Some(fill),
                Some(stroke),
            );
            out.text_centered(laid, rect.center(), ink);
        }
        Kind::Comment(laid) => {
            line(out, [x, y], node.w);
            out.text(
                laid,
                [x, y - node.up],
                laid.width,
                Align::Start,
                style::MUTED,
            );
        }
        Kind::Skip => line(out, [x, y], node.w),
        Kind::Sequence(items) => {
            let mut cx = x;
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    line(out, [cx, y], k.hgap);
                    cx += k.hgap;
                }
                draw(item, cx, y, out, k);
                cx += item.w;
            }
        }
        Kind::Choice(items, ys) => {
            let inner = node.w - 4.0 * r;
            let right = x + node.w;
            for (item, &dy) in items.iter().zip(ys) {
                let off = (inner - item.w) / 2.0;
                let start = x + 2.0 * r + off;
                let end = start + item.w;
                if dy == 0.0 {
                    line(out, [x, y], start - x);
                    line(out, [end, y], right - end);
                } else {
                    let by = y + dy;
                    let mut p = PathData::new();
                    p.move_to([x, y])
                        .quad_to([x + r, y], [x + r, y + r])
                        .line_to([x + r, by - r])
                        .quad_to([x + r, by], [x + 2.0 * r, by])
                        .line_to([start, by]);
                    out.stroke(Geom::path(p), track_stroke());
                    let mut q = PathData::new();
                    q.move_to([end, by])
                        .line_to([right - 2.0 * r, by])
                        .quad_to([right - r, by], [right - r, by - r])
                        .line_to([right - r, y + r])
                        .quad_to([right - r, y], [right, y]);
                    out.stroke(Geom::path(q), track_stroke());
                }
                draw(item, start, y + dy, out, k);
            }
        }
        Kind::Optional(item, bypass) => {
            let right = x + node.w;
            line(out, [x, y], 2.0 * r);
            draw(item, x + 2.0 * r, y, out, k);
            line(out, [right - 2.0 * r, y], 2.0 * r);
            let by = y + bypass;
            let mut p = PathData::new();
            p.move_to([x, y])
                .quad_to([x + r, y], [x + r, y - r])
                .line_to([x + r, by + r])
                .quad_to([x + r, by], [x + 2.0 * r, by])
                .line_to([right - 2.0 * r, by])
                .quad_to([right - r, by], [right - r, by + r])
                .line_to([right - r, y - r])
                .quad_to([right - r, y], [right, y]);
            out.stroke(Geom::path(p), track_stroke());
        }
        Kind::Loop {
            item,
            separator,
            note,
            back,
        } => {
            let right = x + node.w;
            let inner = node.w - 4.0 * r;
            let start = x + 2.0 * r + (inner - item.w) / 2.0;
            line(out, [x, y], start - x);
            draw(item, start, y, out, k);
            line(out, [start + item.w, y], right - start - item.w);
            let by = y + back;
            let (sep_start, sep_end) = match separator {
                Some(s) => {
                    let s0 = x + 2.0 * r + (inner - s.w) / 2.0;
                    draw(s, s0, by, out, k);
                    (s0, s0 + s.w)
                }
                None => (x + node.w / 2.0, x + node.w / 2.0),
            };
            let mut p = PathData::new();
            p.move_to([right - 2.0 * r, y])
                .quad_to([right - r, y], [right - r, y + r])
                .line_to([right - r, by - r])
                .quad_to([right - r, by], [right - 2.0 * r, by])
                .line_to([sep_end, by]);
            out.stroke(Geom::path(p), track_stroke());
            let mut q = PathData::new();
            q.move_to([sep_start, by])
                .line_to([x + 2.0 * r, by])
                .quad_to([x + r, by], [x + r, by - r])
                .line_to([x + r, y + r])
                .quad_to([x + r, y], [x + 2.0 * r, y]);
            out.stroke(Geom::path(q), track_stroke());
            // The loop runs right to left.
            let head = if separator.is_some() {
                (sep_end + right - 2.0 * r) / 2.0
            } else {
                x + node.w / 2.0
            };
            let s = r * 0.55;
            out.fill(
                Geom::path(PathData::polygon(&[
                    [head - s * 0.6, by],
                    [head + s * 0.6, by - s * 0.7],
                    [head + s * 0.6, by + s * 0.7],
                ])),
                style::EDGE,
            );
            if let Some(note) = note {
                let sep_down = separator.as_ref().map_or(0.0, |s| s.down).max(1.0);
                out.text(
                    note,
                    [x + (node.w - note.width) / 2.0, by + sep_down + 2.0],
                    note.width,
                    Align::Center,
                    style::MUTED,
                );
            }
        }
        Kind::Group(item, label) => {
            let left = x + k.hgap / 2.0;
            let frame_w = node.w - k.hgap;
            let item_x = left + (frame_w - item.w) / 2.0;
            line(out, [x, y], item_x - x);
            draw(item, item_x, y, out, k);
            line(out, [item_x + item.w, y], x + node.w - item_x - item.w);
            let frame = Rect::new(left, y - node.up, frame_w, node.up + node.down);
            out.stroke(
                Geom::Rect {
                    rect: frame,
                    corner: Corner::Tier(RadiusTier::Sm),
                },
                Stroke::hairline(style::MUTED).dash(Dash::Dashed),
            );
            if let Some(label) = label {
                out.text(
                    label,
                    [left + k.frame, frame.y + 1.0],
                    label.width,
                    Align::Start,
                    style::MUTED,
                );
            }
        }
    }
}
