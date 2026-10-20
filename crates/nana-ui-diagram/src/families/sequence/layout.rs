//! Sequence layout: participant columns spaced by what passes between them,
//! then one top-down sweep over the events for rows, activations, fragments
//! and notes. ZenUML lowers onto the same model and draws through here.

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use super::{Event, FragmentKind, LineKind, Message, Model, NotePlace, ParticipantKind};
use crate::families::LayoutCx;
use crate::geom::path::rounded_polyline;
use crate::geom::{PathData, Shape};
use crate::ir::{Align, Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth};
use crate::style;
use crate::text::{FontSpec, Label, LaidText};
use crate::{DiagramError, DiagramScene};

/// Fragment frames: a light wash so nested frames read as steps.
const FRAME_FILL: Ink = Ink::Mix(SemanticColorMix::alpha(R::Subtle, 0.45));
const FRAME_STROKE: Ink = Ink::Role(R::Border);
const TAG_FILL: Ink = Ink::Role(R::Subtle);
/// Lifelines: between the faint text and border greys, so they still read
/// over a box or band wash.
const LIFELINE: Ink = Ink::Mix(SemanticColorMix::new(R::Faint, R::Border, 0.5));
/// Author colours on boxes and `rect` bands are kept as washes, so lines and
/// text in theme colours stay readable over them in light and dark.
const AUTHOR_WASH: f32 = 0.28;
/// Deeper activation stacks overlap instead of walking off the lifeline.
const MAX_BAR_STEPS: usize = 6;

/// Every size the sweep works with, from the metrics.
struct Dims {
    pad: [f32; 2],
    col_gap: f32,
    bar_w: f32,
    loop_w: f32,
    loop_h: f32,
    msg_gap: f32,
    label_gap: f32,
    label_pad: f32,
    frame_pad: f32,
    note_pad: [f32; 2],
    note_gap: f32,
    over_ext: f32,
    num_r: f32,
    tag_pad: [f32; 2],
    marker: f32,
    wrap: f32,
}

/// A participant's header drawing.
struct Head {
    w: f32,
    h: f32,
    label: LaidText,
    /// The body shape for database, collections and queue.
    shape: Option<Shape>,
    /// Height of the figure above the label for actor and the UML icons.
    icon_h: f32,
}

#[derive(Default, Clone)]
enum Lay {
    #[default]
    Skip,
    Msg(Box<MsgLay>),
    Note {
        label: LaidText,
        size: [f32; 2],
        x: f32,
    },
    Open {
        tag: Option<LaidText>,
        cond: Option<Label>,
        comment: Option<LaidText>,
        min_w: f32,
        x: [f32; 2],
    },
    Section {
        label: Option<Label>,
    },
}

#[derive(Clone)]
struct MsgLay {
    label: LaidText,
    comment: Option<LaidText>,
    number: Option<LaidText>,
    from_depth: usize,
    to_depth: usize,
    /// This message creates its receiver.
    creates: bool,
    /// Attachment points, set once columns are known.
    ax: f32,
    bx: f32,
}

struct Frame {
    open: usize,
    kind: FragmentKind,
    rect: Rect,
    color: Option<[f32; 4]>,
    tag: Option<(LaidText, Option<LaidText>, f32)>,
    sections: Vec<(f32, Option<LaidText>)>,
    comment: Option<(LaidText, [f32; 2])>,
}

struct Bar {
    p: usize,
    depth: usize,
    y0: f32,
    y1: f32,
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let strong_small = FontSpec::new(m.small_size).weight(m.strong_weight);
    let num_font = FontSpec::new(m.small_size * 0.85).weight(m.strong_weight);
    let wrap = cx.wrap_width("sequence.width");
    let d = Dims {
        pad: [m.node_pad_x, m.node_pad_y],
        col_gap: m.node_sep,
        bar_w: m.font_size * 0.75,
        loop_w: m.font_size * 2.4,
        loop_h: m.font_size * 1.4,
        msg_gap: m.font_size * 1.1,
        label_gap: m.font_size * 0.3,
        label_pad: m.font_size * 0.8,
        frame_pad: m.cluster_pad * 0.625,
        note_pad: [m.node_pad_x * 0.6, m.node_pad_y * 0.6],
        note_gap: m.font_size * 0.45,
        over_ext: m.font_size * 1.4,
        num_r: m.small_size * 0.75,
        tag_pad: [m.small_size * 0.5, m.small_size * 0.25],
        marker: cx.marker_size(),
        wrap,
    };
    let msg_wrap_always = cx.config.get_bool("sequence.wrap") == Some(true);
    let participants = &model.participants;
    let events = &model.events;
    let n = participants.len();

    // Headers.
    let heads: Vec<Head> = participants
        .iter()
        .map(|p| head(cx, p.kind, &p.label, font, &d))
        .collect();
    let hw: Vec<f32> = heads.iter().map(|h| h.w / 2.0).collect();

    // Which message creates or destroys whom.
    let mut created_by = vec![None; n];
    let mut destroyed_at = vec![None; n];
    {
        let mut pending_create = vec![false; n];
        let mut pending_destroy = vec![false; n];
        for (i, ev) in events.iter().enumerate() {
            match ev {
                Event::Create(p) => pending_create[*p] = created_by[*p].is_none(),
                Event::Destroy(p) => pending_destroy[*p] = destroyed_at[*p].is_none(),
                Event::Message(msg) => {
                    if pending_create[msg.to] && msg.from != msg.to {
                        created_by[msg.to] = Some(i);
                        pending_create[msg.to] = false;
                    }
                    for p in [msg.from, msg.to] {
                        if pending_destroy[p] {
                            destroyed_at[p] = Some(i);
                            pending_destroy[p] = false;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    // Pass 1: measure every event and collect the column constraints.
    let off_r = |depth: usize| bar_offset(depth, d.bar_w, 1.0);
    let off_l = |depth: usize| bar_offset(depth, d.bar_w, -1.0);
    let mut lays: Vec<Lay> = vec![Lay::Skip; events.len()];
    let mut cons: Vec<(usize, usize, f32)> = Vec::new();
    let mut depth = vec![0usize; n];
    let mut numbering: Option<(f64, f64)> = None;
    let mut next_number = 0.0f64;
    for (i, ev) in events.iter().enumerate() {
        match ev {
            Event::Message(msg) => {
                let lay = measure_message(cx, msg, &mut depth, &mut numbering, &mut next_number, font, small, num_font, &d, msg_wrap_always);
                let creates = created_by[msg.to] == Some(i);
                let lw = lay
                    .label
                    .width
                    .max(lay.comment.as_ref().map_or(0.0, |c| c.width));
                let num_w = lay.number.as_ref().map_or(0.0, |t| number_size(t, &d)[0] + 2.0);
                if msg.from == msg.to {
                    let off = off_r(lay.from_depth.max(lay.to_depth));
                    let ext = off + (d.loop_w + d.marker * 0.5).max(lw + 6.0 + num_w);
                    if msg.from + 1 < n {
                        cons.push((msg.from, msg.from + 1, ext + d.bar_w + d.col_gap * 0.25));
                    }
                } else {
                    let right = msg.to > msg.from;
                    let o_f = if right { off_r(lay.from_depth) } else { off_l(lay.from_depth) };
                    let o_t = if creates {
                        if right { -hw[msg.to] } else { hw[msg.to] }
                    } else if right {
                        off_l(lay.to_depth)
                    } else {
                        off_r(lay.to_depth)
                    };
                    let gained = if right { o_t - o_f } else { o_f - o_t };
                    let need = (lw + 2.0 * d.label_pad + 2.0 * num_w).max(d.marker * 3.0);
                    cons.push((msg.from.min(msg.to), msg.from.max(msg.to), need - gained));
                }
                lays[i] = Lay::Msg(Box::new(MsgLay { creates, ..lay }));
            }
            Event::Note { place, label } => {
                let laid = cx.text.lay(label, font, Some(d.wrap));
                let size = [
                    (laid.width + 2.0 * d.note_pad[0]).max(m.font_size * 3.0),
                    laid.height + 2.0 * d.note_pad[1],
                ];
                match *place {
                    NotePlace::Right(p) => {
                        if p + 1 < n {
                            cons.push((p, p + 1, off_r(depth[p]) + size[0] + 2.0 * d.note_gap + d.bar_w * 0.5));
                        }
                    }
                    NotePlace::Left(p) => {
                        if p > 0 {
                            let off = if depth[p] > 0 { d.bar_w * 0.5 } else { 0.0 };
                            cons.push((p - 1, p, off + size[0] + 2.0 * d.note_gap + d.bar_w * 0.5));
                        }
                    }
                    NotePlace::Over(a, b) => {
                        let (a, b) = (a.min(b), a.max(b));
                        if a < b {
                            cons.push((a, b, size[0] - 2.0 * d.over_ext));
                        }
                    }
                }
                lays[i] = Lay::Note {
                    label: laid,
                    size,
                    x: 0.0,
                };
            }
            Event::Activate(p) => depth[*p] += 1,
            Event::Deactivate(p) => depth[*p] = depth[*p].saturating_sub(1),
            Event::Autonumber(v) => {
                numbering = *v;
                if let Some((start, _)) = v {
                    next_number = *start;
                }
            }
            Event::Open {
                kind,
                label,
                comment,
                ..
            } => {
                let tag = (*kind != FragmentKind::Rect).then(|| cx.text.lay_plain(kind.keyword(), strong_small));
                let comment = comment.as_ref().map(|c| cx.text.lay(c, small, Some(d.wrap * 1.5)));
                let mut min_w = 0.0;
                if let Some(t) = &tag {
                    min_w = t.width + 2.0 * d.tag_pad[0];
                    if let Some(label) = label {
                        let cond = cx.text.lay(&bracketed(label), small, Some(d.wrap * 1.2));
                        min_w += cond.width + 2.0 * d.tag_pad[0];
                    }
                    min_w += d.frame_pad;
                }
                lays[i] = Lay::Open {
                    tag,
                    cond: label.as_ref().map(bracketed),
                    comment,
                    min_w,
                    x: [0.0, 0.0],
                };
            }
            Event::Section { label } => {
                lays[i] = Lay::Section {
                    label: label.as_ref().map(bracketed),
                };
            }
            Event::Close | Event::Create(_) | Event::Destroy(_) => {}
        }
    }

    // Box titles need their columns wide enough.
    let mut group_titles: Vec<Option<LaidText>> = Vec::with_capacity(model.groups.len());
    for (g, group) in model.groups.iter().enumerate() {
        let laid = group.title.as_ref().map(|t| cx.text.lay(t, strong_small, Some(d.wrap * 1.5)));
        if let Some(t) = &laid
            && let Some((a, b)) = group_span(model, g)
            && a < b
        {
            cons.push((a, b, t.width + 2.0 * d.frame_pad - hw[a] - hw[b]));
        }
        group_titles.push(laid);
    }

    // Columns: adjacent headers keep a gap, then every span constraint
    // widens the gaps it covers, shortest spans first.
    let mut gaps: Vec<f32> = (0..n.saturating_sub(1))
        .map(|k| hw[k] + hw[k + 1] + d.col_gap)
        .collect();
    cons.sort_by_key(|c| c.1 - c.0);
    for (a, b, need) in cons {
        if a >= b || b > gaps.len() || !need.is_finite() {
            continue;
        }
        let cur: f32 = gaps[a..b].iter().sum();
        if need > cur {
            let add = (need - cur) / (b - a) as f32;
            for g in &mut gaps[a..b] {
                *g += add;
            }
        }
    }
    let mut xs = Vec::with_capacity(n);
    let mut x = 0.0;
    for k in 0..n {
        xs.push(x);
        if k < gaps.len() {
            x += gaps[k];
        }
    }
    let attach = |p: usize, depth: usize, facing_right: bool| {
        xs[p] + if facing_right { off_r(depth) } else { off_l(depth) }
    };

    // Pass 2: horizontal extents of messages, notes and fragments.
    let mut stack: Vec<(usize, Option<[f32; 2]>, f32)> = Vec::new();
    let add = |stack: &mut Vec<(usize, Option<[f32; 2]>, f32)>, lo: f32, hi: f32| {
        if let Some(top) = stack.last_mut() {
            top.1 = Some(top.1.map_or([lo, hi], |e| [e[0].min(lo), e[1].max(hi)]));
        }
    };
    let mut depth = vec![0usize; n];
    for (i, ev) in events.iter().enumerate() {
        match (ev, &mut lays[i]) {
            (Event::Message(msg), Lay::Msg(lay)) => {
                if msg.from == msg.to {
                    lay.ax = attach(msg.from, lay.from_depth, true);
                    lay.bx = attach(msg.to, lay.to_depth, true);
                    let num_w = lay.number.as_ref().map_or(0.0, |t| number_size(t, &d)[0] + 2.0);
                    let lw = lay
                        .label
                        .width
                        .max(lay.comment.as_ref().map_or(0.0, |c| c.width));
                    let reach = (d.loop_w + d.marker * 0.5).max(lw + 6.0 + num_w);
                    add(&mut stack, xs[msg.from] - d.bar_w * 0.5, lay.ax.max(lay.bx) + reach);
                } else {
                    let right = msg.to > msg.from;
                    lay.ax = attach(msg.from, lay.from_depth, right);
                    lay.bx = if lay.creates {
                        xs[msg.to] + if right { -hw[msg.to] } else { hw[msg.to] }
                    } else {
                        attach(msg.to, lay.to_depth, !right)
                    };
                    let r = lay.number.as_ref().map_or(0.0, |t| number_size(t, &d)[0] / 2.0)
                        * if msg.start.is_some() { 2.2 } else { 1.0 };
                    // Whole activation bars at either end stay inside a frame.
                    let lo = (lay.ax.min(lay.bx) - r)
                        .min(xs[msg.from] - d.bar_w * 0.5)
                        .min(xs[msg.to] - d.bar_w * 0.5);
                    let hi = (lay.ax.max(lay.bx) + r)
                        .max(xs[msg.from] + off_r(lay.from_depth).max(d.bar_w * 0.5))
                        .max(xs[msg.to] + off_r(lay.to_depth).max(d.bar_w * 0.5));
                    add(&mut stack, lo, hi);
                }
                if msg.deactivate {
                    depth[msg.from] = depth[msg.from].saturating_sub(1);
                }
                if msg.activate {
                    depth[msg.to] += 1;
                }
            }
            (Event::Note { place, .. }, Lay::Note { label, size, x }) => {
                *x = match *place {
                    NotePlace::Right(p) => attach(p, depth[p], true).max(xs[p]) + d.note_gap,
                    NotePlace::Left(p) => {
                        let off = if depth[p] > 0 { d.bar_w * 0.5 } else { 0.0 };
                        xs[p] - off - d.note_gap - size[0]
                    }
                    NotePlace::Over(a, b) => {
                        let (lo, hi) = (xs[a].min(xs[b]), xs[a].max(xs[b]));
                        if a == b {
                            lo - size[0] / 2.0
                        } else {
                            let w = (hi - lo + 2.0 * d.over_ext).max(size[0]);
                            let inner = w - 2.0 * d.note_pad[0];
                            if inner > d.wrap
                                && let Event::Note { label: source, .. } = ev
                            {
                                *label = cx.text.lay(source, font, Some(inner));
                                size[1] = label.height + 2.0 * d.note_pad[1];
                            }
                            size[0] = w;
                            (lo + hi) / 2.0 - w / 2.0
                        }
                    }
                };
                add(&mut stack, *x, *x + size[0]);
            }
            (Event::Activate(p), _) => depth[*p] += 1,
            (Event::Deactivate(p), _) => depth[*p] = depth[*p].saturating_sub(1),
            (Event::Open { .. }, Lay::Open { min_w, .. }) => stack.push((i, None, *min_w)),
            (Event::Section { .. }, Lay::Section { label }) => {
                if let (Some(label), Some(top)) = (label, stack.last_mut()) {
                    let w = cx.text.lay(label, small, Some(d.wrap)).width;
                    top.2 = top.2.max(w + 2.0 * d.frame_pad);
                }
            }
            (Event::Close, _) => {
                if let Some((open, extent, min_w)) = stack.pop() {
                    let [lo, hi] = extent.unwrap_or_else(|| {
                        let c = xs.first().copied().unwrap_or(0.0);
                        let h = hw.first().copied().unwrap_or(d.pad[0]);
                        [c - h, c + h]
                    });
                    let x0 = lo - d.frame_pad;
                    let x1 = (hi + d.frame_pad).max(x0 + min_w);
                    if let Lay::Open { x, .. } = &mut lays[open] {
                        *x = [x0, x1];
                    }
                    add(&mut stack, x0, x1);
                }
            }
            _ => {}
        }
    }

    // Pass 3: rows.
    let mut layers = Layers::default();
    let any_title = group_titles.iter().any(Option::is_some);
    let title_h = group_titles
        .iter()
        .flatten()
        .map(|t| t.height)
        .fold(0.0, f32::max);
    let head_top = if any_title {
        title_h + d.frame_pad * 1.2
    } else if model.groups.is_empty() {
        0.0
    } else {
        d.frame_pad
    };
    let head_h = heads
        .iter()
        .enumerate()
        .filter(|(p, _)| created_by[*p].is_none())
        .map(|(_, h)| h.h)
        .fold(0.0, f32::max);
    let lifeline_top = head_top + head_h;
    let mut life_start: Vec<f32> = vec![lifeline_top; n];
    let mut life_end: Vec<Option<f32>> = vec![None; n];
    let mut head_rects: Vec<Option<Rect>> = vec![None; n];
    for p in 0..n {
        if created_by[p].is_none() {
            let h = &heads[p];
            head_rects[p] = Some(Rect::new(xs[p] - h.w / 2.0, lifeline_top - h.h, h.w, h.h));
        }
    }

    let mut y = lifeline_top + d.msg_gap;
    let mut bars: Vec<Bar> = Vec::new();
    let mut open_bars: Vec<Vec<(f32, usize)>> = vec![Vec::new(); n];
    let mut frames: Vec<Frame> = Vec::new();
    let mut open_frames: Vec<Frame> = Vec::new();
    let mut last_line: Option<f32> = None;
    let mut crosses: Vec<[f32; 2]> = Vec::new();
    for (i, ev) in events.iter().enumerate() {
        let after_message = last_line.filter(|_| i > 0 && matches!(events[i - 1], Event::Message(_)));
        match (ev, &lays[i]) {
            (Event::Message(msg), Lay::Msg(lay)) => {
                if let Some(c) = &lay.comment {
                    let x0 = lay.ax.min(lay.bx) + 4.0;
                    layers.msgs.text(c, [x0, y], c.width, Align::Start, style::MUTED);
                    y += c.height + 2.0;
                }
                let label_top = y;
                let label_h = if lay.label.is_empty() { 0.0 } else { lay.label.height };
                let mut line_y = label_top + label_h + d.label_gap;
                let stroke = message_stroke(msg);
                if msg.from == msg.to {
                    let top = line_y;
                    let bottom = top + d.loop_h;
                    let right = lay.ax.max(lay.bx) + d.loop_w;
                    let pts = [[lay.ax, top], [right, top], [right, bottom], [lay.bx, bottom]];
                    let radius = m.bend_radius.min(d.loop_h * 0.4);
                    layers.msgs.edge(&pts, move |p| rounded_polyline(p, radius), stroke, msg.start, msg.end, d.marker);
                    let mut lx = lay.ax + 4.0;
                    if let Some(num) = &lay.number {
                        let size = number_size(num, &d);
                        draw_number(&mut layers.msgs, num, [lay.ax, top], size);
                        lx += size[0] / 2.0 + 2.0;
                    }
                    if label_h > 0.0 {
                        layers.msgs.text(&lay.label, [lx, label_top], lay.label.width, Align::Start, style::TEXT);
                    }
                    if msg.deactivate {
                        close_bar(&mut open_bars, &mut bars, msg.from, top, d.font_min(m.font_size));
                    }
                    if msg.activate {
                        open_bars[msg.to].push((bottom, lay.to_depth));
                    }
                    last_line = Some(bottom);
                    y = bottom + d.msg_gap;
                    end_lifelines(msg, i, &destroyed_at, &mut life_end, &mut crosses, &xs, bottom);
                } else {
                    if lay.creates {
                        line_y = line_y.max(y + heads[msg.to].h / 2.0);
                    }
                    let pts = [[lay.ax, line_y], [lay.bx, line_y]];
                    layers.msgs.edge(&pts, PathData::polyline, stroke, msg.start, msg.end, d.marker);
                    if label_h > 0.0 {
                        let mid = (lay.ax + lay.bx) / 2.0;
                        layers.msgs.text(
                            &lay.label,
                            [mid - lay.label.width / 2.0, label_top],
                            lay.label.width,
                            Align::Center,
                            style::TEXT,
                        );
                    }
                    if let Some(num) = &lay.number {
                        let size = number_size(num, &d);
                        // A head at the sender: the number steps out behind it.
                        let back = if msg.start.is_some() {
                            (size[0] / 2.0 + 2.0) * (lay.ax - lay.bx).signum()
                        } else {
                            0.0
                        };
                        draw_number(&mut layers.msgs, num, [lay.ax + back, line_y], size);
                    }
                    if msg.deactivate {
                        close_bar(&mut open_bars, &mut bars, msg.from, line_y, d.font_min(m.font_size));
                    }
                    if lay.creates {
                        let h = &heads[msg.to];
                        let rect = Rect::new(xs[msg.to] - h.w / 2.0, line_y - h.h / 2.0, h.w, h.h);
                        head_rects[msg.to] = Some(rect);
                        life_start[msg.to] = rect.bottom();
                    }
                    if msg.activate {
                        open_bars[msg.to].push((line_y, lay.to_depth));
                    }
                    last_line = Some(line_y);
                    y = if lay.creates {
                        line_y + heads[msg.to].h / 2.0 + d.msg_gap * 0.7
                    } else {
                        line_y + d.msg_gap
                    };
                    end_lifelines(msg, i, &destroyed_at, &mut life_end, &mut crosses, &xs, line_y);
                }
            }
            (Event::Note { .. }, Lay::Note { label, size, x }) => {
                let rect = Rect::new(*x, y, size[0], size[1]);
                layers.notes.fill(Geom::rounded(rect, style::NOTE_CORNER), style::NODE_FILL);
                layers.notes.shape(
                    Geom::rounded(rect, style::NOTE_CORNER),
                    Some(style::NOTE_FILL),
                    Some(Stroke::hairline(style::NOTE_STROKE)),
                );
                let laid = label;
                layers.notes.text(
                    laid,
                    [rect.x + (rect.w - laid.width) / 2.0, rect.y + d.note_pad[1]],
                    laid.width,
                    Align::Center,
                    style::TEXT,
                );
                y = rect.bottom() + d.msg_gap;
            }
            (Event::Activate(p), _) => {
                let start = after_message.unwrap_or(y - d.msg_gap * 0.5);
                let depth = open_bars[*p].len() + 1;
                open_bars[*p].push((start, depth));
            }
            (Event::Deactivate(p), _) => {
                let end = after_message.unwrap_or(y - d.msg_gap * 0.5);
                let min = d.font_min(m.font_size);
                if let Some(&(start, _)) = open_bars[*p].last()
                    && end < start + min
                {
                    y = y.max(start + min + d.msg_gap * 0.5);
                }
                close_bar(&mut open_bars, &mut bars, *p, end, min);
            }
            (
                Event::Open { kind, color, .. },
                Lay::Open {
                    tag,
                    cond,
                    comment,
                    x,
                    ..
                },
            ) => {
                let mut comment_at = None;
                if let Some(c) = comment {
                    comment_at = Some((c.clone(), [x[0], y]));
                    y += c.height + 2.0;
                }
                let top = y;
                let width = x[1] - x[0];
                let tag = tag.as_ref().map(|t| {
                    let tag_w = t.width + 2.0 * d.tag_pad[0];
                    let cond = cond.as_ref().map(|c| {
                        let avail = (width - tag_w - 2.0 * d.tag_pad[0] - d.frame_pad).max(m.font_size * 3.0);
                        cx.text.lay(c, small, Some(avail))
                    });
                    let h = t.height.max(cond.as_ref().map_or(0.0, |c| c.height)) + 2.0 * d.tag_pad[1];
                    (t.clone(), cond, h)
                });
                y = top + tag.as_ref().map_or(d.frame_pad, |t| t.2 + d.frame_pad * 0.8);
                open_frames.push(Frame {
                    open: i,
                    kind: *kind,
                    rect: Rect::new(x[0], top, width, 0.0),
                    color: *color,
                    tag,
                    sections: Vec::new(),
                    comment: comment_at,
                });
            }
            (Event::Section { .. }, Lay::Section { label }) => {
                let sep = y - d.msg_gap * 0.35;
                let laid = open_frames.last().and_then(|f| {
                    label
                        .as_ref()
                        .map(|l| cx.text.lay(l, small, Some((f.rect.w - 2.0 * d.frame_pad).max(m.font_size * 3.0))))
                });
                if let Some(f) = open_frames.last_mut() {
                    y = sep + d.tag_pad[1] * 2.0 + laid.as_ref().map_or(0.0, |l| l.height) + d.frame_pad * 0.6;
                    f.sections.push((sep, laid));
                }
            }
            (Event::Close, _) => {
                if let Some(mut f) = open_frames.pop() {
                    let bottom = y - d.msg_gap * 0.35 + d.frame_pad * 0.4;
                    f.rect.h = (bottom - f.rect.y).max(d.frame_pad);
                    y = f.rect.bottom() + d.msg_gap * 0.6;
                    frames.push(f);
                }
            }
            _ => {}
        }
    }
    let y_end = (y + d.msg_gap * 0.3).max(lifeline_top + d.msg_gap);
    for (p, open) in open_bars.iter_mut().enumerate() {
        while let Some((start, depth)) = open.pop() {
            bars.push(Bar {
                p,
                depth,
                y0: start,
                y1: y_end.max(start + d.font_min(m.font_size)),
            });
        }
    }

    // Groups behind everything.
    let mirror_h = if model.mirror {
        (0..n)
            .filter(|&p| destroyed_at[p].is_none() && participants[p].kind != ParticipantKind::Starter)
            .map(|p| heads[p].h)
            .fold(0.0, f32::max)
    } else {
        0.0
    };
    let bottom_edge = y_end + mirror_h;
    for (g, group) in model.groups.iter().enumerate() {
        let Some((a, b)) = group_span(model, g) else { continue };
        let lo = (a..=b).map(|p| xs[p] - hw[p]).fold(f32::INFINITY, f32::min);
        let hi = (a..=b).map(|p| xs[p] + hw[p]).fold(f32::NEG_INFINITY, f32::max);
        let title = &group_titles[g];
        let mut x0 = lo - d.frame_pad;
        let mut x1 = hi + d.frame_pad;
        if let Some(t) = title {
            let need = t.width + 2.0 * d.frame_pad;
            if x1 - x0 < need {
                let grow = (need - (x1 - x0)) / 2.0;
                x0 -= grow;
                x1 += grow;
            }
        }
        let rect = Rect::new(x0, 0.0, x1 - x0, bottom_edge + d.frame_pad);
        let fill = match group.color {
            Some(c) if c[3] <= 0.0 => None,
            Some(c) => Some(Ink::Rgba([c[0], c[1], c[2], c[3].min(AUTHOR_WASH)])),
            None => Some(style::CLUSTER_FILL),
        };
        layers.groups.shape(
            Geom::rounded(rect, style::CLUSTER_CORNER),
            fill,
            Some(style::cluster_stroke()),
        );
        if let Some(t) = title {
            layers.groups.text(
                t,
                [rect.x + (rect.w - t.width) / 2.0, d.frame_pad * 0.6],
                t.width,
                Align::Center,
                style::MUTED,
            );
        }
    }

    // Fragments, outermost first.
    frames.sort_by_key(|f| f.open);
    for f in &frames {
        draw_frame(&mut layers, f, &d);
    }

    // Lifelines.
    let life = Stroke::hairline(LIFELINE).dash(Dash::Dashed);
    for p in 0..n {
        let end = life_end[p].unwrap_or(y_end);
        let start = life_start[p];
        if end > start + 0.5 {
            layers.lifelines.stroke(Geom::path(PathData::polyline(&[[xs[p], start], [xs[p], end]])), life);
        }
    }

    // Activation bars, shallow under deep.
    bars.sort_by(|a, b| a.depth.cmp(&b.depth).then(a.y0.total_cmp(&b.y0)));
    for bar in &bars {
        let x0 = xs[bar.p] - d.bar_w / 2.0 + (bar.depth.clamp(1, MAX_BAR_STEPS) - 1) as f32 * d.bar_w / 2.0;
        layers.bars.shape(
            Geom::rect(Rect::new(x0, bar.y0, d.bar_w, (bar.y1 - bar.y0).max(1.0))),
            Some(style::NODE_FILL),
            Some(style::node_stroke()),
        );
    }

    // Headers on top, mirrored under the lifelines.
    for p in 0..n {
        let Some(rect) = head_rects[p] else { continue };
        draw_head(&mut layers.heads, participants[p].kind, &heads[p], rect);
        if let Some((url, label)) = &participants[p].link {
            layers.heads.link(rect, url, Some(label));
        }
        if model.mirror && destroyed_at[p].is_none() && participants[p].kind != ParticipantKind::Starter {
            let h = &heads[p];
            let bottom = Rect::new(xs[p] - h.w / 2.0, y_end, h.w, h.h);
            draw_head(&mut layers.heads, participants[p].kind, h, bottom);
        }
    }
    let cross = Stroke::new(style::EDGE, StrokeWidth::Thick);
    let k = m.font_size * 0.45;
    for [cx0, cy0] in crosses {
        let mut path = PathData::new();
        path.move_to([cx0 - k, cy0 - k])
            .line_to([cx0 + k, cy0 + k])
            .move_to([cx0 - k, cy0 + k])
            .line_to([cx0 + k, cy0 - k]);
        layers.heads.stroke(Geom::path(path), cross);
    }

    let mut out = layers.merge();
    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

impl Dims {
    /// The shortest activation bar.
    fn font_min(&self, font_size: f32) -> f32 {
        font_size * 0.8
    }
}

/// Paint order: each layer is drawn whole over the one before.
#[derive(Default)]
struct Layers {
    groups: SceneBuilder,
    frames: SceneBuilder,
    lifelines: SceneBuilder,
    bars: SceneBuilder,
    frame_marks: SceneBuilder,
    msgs: SceneBuilder,
    notes: SceneBuilder,
    heads: SceneBuilder,
}

impl Layers {
    fn merge(self) -> SceneBuilder {
        let mut out = SceneBuilder::new();
        for layer in [
            self.groups,
            self.frames,
            self.lifelines,
            self.bars,
            self.frame_marks,
            self.msgs,
            self.notes,
            self.heads,
        ] {
            // A zero-margin finish at the origin leaves coordinates as they are.
            let scene = layer.finish(Rect::default(), 0.0);
            for item in scene.items {
                out.push(item);
            }
            for link in scene.links {
                out.link(link.rect, &link.href, link.tooltip.as_deref());
            }
        }
        out
    }
}

fn bar_offset(depth: usize, bar_w: f32, side: f32) -> f32 {
    if depth == 0 {
        return 0.0;
    }
    let step = (depth.min(MAX_BAR_STEPS) - 1) as f32 * bar_w / 2.0;
    step + side * bar_w / 2.0
}

fn bracketed(label: &Label) -> Label {
    let mut out = label.clone();
    if let Some(first) = out.lines.first_mut() {
        match first.first_mut() {
            Some(run) => run.text.insert(0, '['),
            None => first.push(crate::text::Run::plain("[")),
        }
    }
    if let Some(last) = out.lines.last_mut() {
        match last.last_mut() {
            Some(run) => run.text.push(']'),
            None => last.push(crate::text::Run::plain("]")),
        }
    }
    out
}

#[allow(clippy::too_many_arguments)]
fn measure_message(
    cx: &LayoutCx<'_>,
    msg: &Message,
    depth: &mut [usize],
    numbering: &mut Option<(f64, f64)>,
    next_number: &mut f64,
    font: FontSpec,
    small: FontSpec,
    num_font: FontSpec,
    d: &Dims,
    wrap_always: bool,
) -> MsgLay {
    let wrap = if msg.wrap || wrap_always { d.wrap } else { d.wrap * 2.0 };
    let label = cx.text.lay(&msg.label, font, Some(wrap));
    let comment = msg.comment.as_ref().map(|c| cx.text.lay(c, small, Some(d.wrap * 1.5)));
    let from_depth = depth[msg.from];
    if msg.deactivate {
        depth[msg.from] = depth[msg.from].saturating_sub(1);
    }
    if msg.activate {
        depth[msg.to] += 1;
    }
    let to_depth = depth[msg.to];
    let number = numbering.map(|(_, step)| {
        let text = format_number(*next_number);
        *next_number += step;
        cx.text.lay_plain(&text, num_font)
    });
    MsgLay {
        label,
        comment,
        number,
        from_depth,
        to_depth,
        creates: false,
        ax: 0.0,
        bx: 0.0,
    }
}

fn format_number(v: f64) -> String {
    if (v - v.round()).abs() < 1e-6 && v.abs() < 1e12 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn number_size(text: &LaidText, d: &Dims) -> [f32; 2] {
    let h = (2.0 * d.num_r).max(text.height * 0.95);
    [(text.width + d.num_r * 0.8).max(h), h]
}

fn draw_number(out: &mut SceneBuilder, text: &LaidText, center: [f32; 2], size: [f32; 2]) {
    let rect = Rect::centered(center, size);
    let geom = if size[0] - size[1] < 0.5 {
        Geom::Ellipse(rect)
    } else {
        Geom::rounded(rect, RadiusTier::Full)
    };
    out.fill(geom, style::ACCENT);
    out.text_centered(text, center, style::ACCENT_TEXT);
}

fn message_stroke(msg: &Message) -> Stroke {
    let stroke = style::edge_stroke();
    match msg.line {
        LineKind::Solid => stroke,
        LineKind::Dashed => stroke.dash(Dash::Dashed),
    }
}

fn close_bar(open: &mut [Vec<(f32, usize)>], bars: &mut Vec<Bar>, p: usize, end: f32, min: f32) {
    if let Some((start, depth)) = open[p].pop() {
        bars.push(Bar {
            p,
            depth,
            y0: start,
            y1: end.max(start + min),
        });
    }
}

fn end_lifelines(
    msg: &Message,
    index: usize,
    destroyed_at: &[Option<usize>],
    life_end: &mut [Option<f32>],
    crosses: &mut Vec<[f32; 2]>,
    xs: &[f32],
    y: f32,
) {
    for p in [msg.from, msg.to] {
        if destroyed_at[p] == Some(index) && life_end[p].is_none() {
            life_end[p] = Some(y);
            crosses.push([xs[p], y]);
        }
    }
}

/// The first and last column of a box's members.
fn group_span(model: &Model, g: usize) -> Option<(usize, usize)> {
    let mut members = model
        .participants
        .iter()
        .enumerate()
        .filter(|(_, p)| p.group == Some(g))
        .map(|(i, _)| i);
    let first = members.next()?;
    let last = members.next_back().unwrap_or(first);
    Some((first, last))
}

fn head(cx: &LayoutCx<'_>, kind: ParticipantKind, label: &Label, font: FontSpec, d: &Dims) -> Head {
    let laid = cx.text.lay(label, font, Some(d.wrap));
    let f = cx.metrics.font_size;
    let min_w = f * 4.0;
    let [lw, lh] = laid.size();
    let below = |icon_w: f32, icon_h: f32| Head {
        w: lw.max(icon_w),
        h: icon_h + f * 0.3 + lh,
        label: laid.clone(),
        shape: None,
        icon_h,
    };
    match kind {
        ParticipantKind::Participant => Head {
            w: (lw + 2.0 * d.pad[0]).max(min_w),
            h: lh + 2.0 * d.pad[1],
            label: laid,
            shape: None,
            icon_h: 0.0,
        },
        ParticipantKind::Actor => below(f * 1.7, f * 2.5),
        ParticipantKind::Boundary => below(f * 3.0, f * 2.0),
        ParticipantKind::Control | ParticipantKind::Entity => below(f * 2.0, f * 2.2),
        ParticipantKind::Database | ParticipantKind::Collections | ParticipantKind::Queue => {
            let shape = match kind {
                ParticipantKind::Database => Shape::Cylinder,
                ParticipantKind::Collections => Shape::StackedRect,
                _ => Shape::HorizontalCylinder,
            };
            let [w, h] = shape.size([lw, lh], [d.pad[0], d.pad[1] * 0.8]);
            Head {
                w: w.max(min_w),
                h,
                label: laid,
                shape: Some(shape),
                icon_h: 0.0,
            }
        }
        ParticipantKind::Starter => Head {
            w: f * 0.8,
            h: f * 0.8,
            label: laid,
            shape: None,
            icon_h: f * 0.8,
        },
    }
}

fn draw_head(out: &mut SceneBuilder, kind: ParticipantKind, head: &Head, rect: Rect) {
    let fill = style::NODE_FILL;
    let line = style::node_stroke();
    let label = &head.label;
    let centered_label = |out: &mut SceneBuilder, area: Rect| {
        if !label.is_empty() {
            out.text(
                label,
                [area.x + (area.w - label.width) / 2.0, area.y + (area.h - label.height) / 2.0],
                label.width,
                Align::Center,
                style::TEXT,
            );
        }
    };
    let cx0 = rect.x + rect.w / 2.0;
    let top = rect.y;
    let icon_h = head.icon_h;
    let label_below = |out: &mut SceneBuilder| {
        if !label.is_empty() {
            out.text(
                label,
                [cx0 - label.width / 2.0, rect.bottom() - label.height],
                label.width,
                Align::Center,
                style::TEXT,
            );
        }
    };
    match kind {
        ParticipantKind::Participant => {
            out.shape(Geom::rounded(rect, RadiusTier::Xs), Some(fill), Some(line));
            centered_label(out, rect);
        }
        ParticipantKind::Database | ParticipantKind::Collections | ParticipantKind::Queue => {
            let shape = head.shape.unwrap_or(Shape::Rect);
            let (behind, over) = shape.decorations(rect);
            for g in behind {
                out.shape(g, Some(fill), Some(line));
            }
            out.shape(shape.outline(rect), Some(fill), Some(line));
            for path in over {
                out.stroke(Geom::path(path), line);
            }
            let lr = shape.label_rect(rect, label.size());
            centered_label(out, lr);
        }
        ParticipantKind::Actor => {
            let r = icon_h * 0.16;
            let half = icon_h * 0.34;
            let neck = top + 2.0 * r + 1.0;
            let hip = top + icon_h * 0.66;
            let mut body = PathData::new();
            body.move_to([cx0, neck])
                .line_to([cx0, hip])
                .move_to([cx0 - half, top + icon_h * 0.42])
                .line_to([cx0 + half, top + icon_h * 0.42])
                .move_to([cx0 - half * 0.8, top + icon_h])
                .line_to([cx0, hip])
                .line_to([cx0 + half * 0.8, top + icon_h]);
            out.stroke(Geom::path(body), line);
            out.shape(
                Geom::Ellipse(Rect::new(cx0 - r, top + 0.5, 2.0 * r, 2.0 * r)),
                Some(fill),
                Some(line),
            );
            label_below(out);
        }
        ParticipantKind::Boundary => {
            let rr = icon_h * 0.5;
            let c = [cx0 + rr * 0.5, top + rr];
            let bar_x = cx0 - rr * 1.5;
            let mut path = PathData::new();
            path.move_to([bar_x, top]).line_to([bar_x, top + 2.0 * rr]).move_to([bar_x, c[1]]).line_to([c[0] - rr, c[1]]);
            out.stroke(Geom::path(path), line);
            out.shape(Geom::Ellipse(Rect::centered(c, [2.0 * rr, 2.0 * rr])), Some(fill), Some(line));
            label_below(out);
        }
        ParticipantKind::Control => {
            let rr = icon_h * 0.42;
            let c = [cx0, top + icon_h - rr - 1.0];
            out.shape(Geom::Ellipse(Rect::centered(c, [2.0 * rr, 2.0 * rr])), Some(fill), Some(line));
            let tip = [c[0] - rr * 0.35, c[1] - rr];
            let k = rr * 0.4;
            let mut head_path = PathData::new();
            head_path
                .move_to([tip[0] + k, tip[1] - k * 0.8])
                .line_to(tip)
                .line_to([tip[0] + k, tip[1] + k * 0.8]);
            out.stroke(Geom::path(head_path), line);
            label_below(out);
        }
        ParticipantKind::Entity => {
            let rr = icon_h * 0.4;
            let c = [cx0, top + rr + 1.0];
            out.shape(Geom::Ellipse(Rect::centered(c, [2.0 * rr, 2.0 * rr])), Some(fill), Some(line));
            let base = c[1] + rr + icon_h * 0.12;
            out.stroke(Geom::path(PathData::polyline(&[[cx0 - rr, base], [cx0 + rr, base]])), line);
            label_below(out);
        }
        ParticipantKind::Starter => {
            out.fill(Geom::Ellipse(rect), style::TEXT);
        }
    }
}

fn draw_frame(layers: &mut Layers, f: &Frame, d: &Dims) {
    let rect = f.rect;
    if f.kind == FragmentKind::Rect {
        let fill = match f.color {
            Some(c) => Ink::Rgba([c[0], c[1], c[2], c[3].min(AUTHOR_WASH)]),
            None => style::CLUSTER_FILL,
        };
        layers.frames.fill(Geom::rounded(rect, RadiusTier::Xs), fill);
        return;
    }
    layers.frames.shape(
        Geom::rounded(rect, RadiusTier::Xs),
        Some(FRAME_FILL),
        Some(Stroke::hairline(FRAME_STROKE)),
    );
    if let Some((text, at)) = &f.comment {
        layers.frame_marks.text(text, *at, text.width, Align::Start, style::MUTED);
    }
    if let Some((tag, cond, h)) = &f.tag {
        let w = tag.width + 2.0 * d.tag_pad[0];
        let cut = (h * 0.35).min(w * 0.3);
        let (x0, y0) = (rect.x, rect.y);
        let pentagon = PathData::polygon(&[
            [x0, y0],
            [x0 + w, y0],
            [x0 + w, y0 + h - cut],
            [x0 + w - cut, y0 + h],
            [x0, y0 + h],
        ]);
        layers
            .frame_marks
            .shape(Geom::path(pentagon), Some(TAG_FILL), Some(Stroke::hairline(FRAME_STROKE)));
        layers.frame_marks.text(
            tag,
            [x0 + d.tag_pad[0], y0 + (h - tag.height) / 2.0],
            tag.width,
            Align::Start,
            style::TEXT,
        );
        if let Some(cond) = cond {
            layers.frame_marks.text(
                cond,
                [x0 + w + d.tag_pad[0], y0 + d.tag_pad[1]],
                cond.width,
                Align::Start,
                style::MUTED,
            );
        }
    }
    let sep = Stroke::hairline(FRAME_STROKE).dash(Dash::Dashed);
    for (y, label) in &f.sections {
        layers.frame_marks.stroke(
            Geom::path(PathData::polyline(&[[rect.x, *y], [rect.right(), *y]])),
            sep,
        );
        if let Some(label) = label {
            layers.frame_marks.text(
                label,
                [rect.x + (rect.w - label.width) / 2.0, y + d.tag_pad[1] * 2.0],
                label.width,
                Align::Center,
                style::MUTED,
            );
        }
    }
}
