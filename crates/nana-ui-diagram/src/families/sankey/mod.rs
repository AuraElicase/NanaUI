//! Sankey diagrams: `sankey-beta` / `sankey` followed by CSV rows of
//! `source,target,value` (RFC 4180 quoting, blank lines allowed).
//!
//! Nodes sit in columns by longest path, stacked by flow with gaps and
//! relaxed toward their neighbours over a few sweeps; links are ribbons
//! bounded by two cubic curves.

use std::collections::{HashMap, VecDeque};

use super::kit;
use super::{LayoutCx, ParseCx};
use crate::geom::PathData;
use crate::ir::{Align, Geom, Ink, Rect, SceneBuilder, Tone};
use crate::source::color;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone)]
pub(crate) struct Link {
    pub source: usize,
    pub target: usize,
    pub value: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub nodes: Vec<String>,
    pub links: Vec<Link>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    let mut model = Model::default();
    let mut ids: HashMap<String, usize> = HashMap::new();
    let first = (!rest.is_empty()).then_some((header, rest));
    for (line, row) in first.into_iter().chain(kit::statements(body, header)) {
        let Some(fields) = csv(row) else {
            cx.warn(line, "unterminated quoted field");
            continue;
        };
        if fields.len() != 3 {
            cx.warn(line, format!("expected source,target,value, found {} fields", fields.len()));
            continue;
        }
        let Some(value) = kit::number(&fields[2]).filter(|v| *v >= 0.0) else {
            cx.warn(line, format!("`{}` is not a flow value", fields[2]));
            continue;
        };
        let (s, t) = (fields[0].trim(), fields[1].trim());
        if s.is_empty() || t.is_empty() {
            cx.warn(line, "a flow needs a source and a target");
            continue;
        }
        if s == t {
            cx.warn(line, "a node cannot flow into itself");
            continue;
        }
        cx.limits.check_label(s)?;
        cx.limits.check_label(t)?;
        let mut id = |name: &str, model: &mut Model| -> Result<usize, DiagramError> {
            if let Some(&i) = ids.get(name) {
                return Ok(i);
            }
            cx.limits.check_count(model.nodes.len() + 1, cx.limits.nodes, LimitKind::Nodes)?;
            ids.insert(name.to_string(), model.nodes.len());
            model.nodes.push(name.to_string());
            Ok(model.nodes.len() - 1)
        };
        let source = id(s, &mut model)?;
        let target = id(t, &mut model)?;
        cx.limits.check_count(model.links.len() + 1, cx.limits.edges, LimitKind::Edges)?;
        model.links.push(Link { source, target, value });
    }
    // Flows must run one way: drop the links that close a cycle.
    let back = back_links(&model);
    if !back.is_empty() {
        cx.warn(header, format!("{} flow(s) close a cycle and were dropped", back.len()));
        let mut i = 0;
        model.links.retain(|_| {
            let keep = !back.contains(&i);
            i += 1;
            keep
        });
    }
    Ok(model)
}

/// One CSV record: `"` quotes a field and `""` is a literal quote.
fn csv(row: &str) -> Option<Vec<String>> {
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut chars = row.chars().peekable();
    let mut quoted = false;
    let mut at_start = true;
    while let Some(c) = chars.next() {
        if quoted {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    field.push('"');
                    chars.next();
                } else {
                    quoted = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }
        match c {
            '"' if at_start || field.trim().is_empty() => {
                field.clear();
                quoted = true;
                at_start = false;
            }
            ',' => {
                fields.push(std::mem::take(&mut field));
                at_start = true;
            }
            _ => {
                field.push(c);
                at_start = false;
            }
        }
    }
    if quoted {
        return None;
    }
    fields.push(field);
    Some(fields.into_iter().map(|f| f.trim().to_string()).collect())
}

/// Links whose removal leaves the graph acyclic (DFS back edges).
fn back_links(model: &Model) -> Vec<usize> {
    let n = model.nodes.len();
    let mut out_links: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, l) in model.links.iter().enumerate() {
        out_links[l.source].push(i);
    }
    let mut state = vec![0u8; n];
    let mut back = Vec::new();
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack = vec![(root, 0usize)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if let Some(&li) = out_links[v].get(*next) {
                *next += 1;
                let w = model.links[li].target;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => back.push(li),
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    back
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let small = cx.small_font();
    let n = model.nodes.len();
    let mut out = SceneBuilder::new();
    if n == 0 {
        let content = cx.title_above(&mut out, Rect::new(0.0, 0.0, 1.0, 1.0));
        return Ok(out.finish(content, m.margin));
    }
    let node_w = cx.config.get_f32("sankey.nodeWidth").unwrap_or(m.font_size * 0.75).clamp(2.0, 60.0);
    let pad = cx.config.get_f32("sankey.nodePadding").unwrap_or(m.font_size).clamp(2.0, 120.0);
    let show_values = cx.config.get_bool("sankey.showValues").unwrap_or(true);

    // Columns by longest path.
    let mut ins: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut outs: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, l) in model.links.iter().enumerate() {
        outs[l.source].push(i);
        ins[l.target].push(i);
    }
    let order = topo(model, &ins, &outs);
    let mut depth = vec![0usize; n];
    for &v in &order {
        for &li in &outs[v] {
            let t = model.links[li].target;
            depth[t] = depth[t].max(depth[v] + 1);
        }
    }
    let mut height = vec![0usize; n];
    for &v in order.iter().rev() {
        for &li in &outs[v] {
            height[v] = height[v].max(height[model.links[li].target] + 1);
        }
    }
    let last = depth.iter().copied().max().unwrap_or(0);
    let align = cx.config.get("sankey.nodeAlignment").unwrap_or("justify");
    let column: Vec<usize> = (0..n)
        .map(|v| match align {
            "left" => depth[v],
            "right" => last - height[v],
            "center" => {
                if !ins[v].is_empty() {
                    depth[v]
                } else if outs[v].is_empty() {
                    0
                } else {
                    outs[v].iter().map(|&li| depth[model.links[li].target]).min().unwrap_or(1).saturating_sub(1)
                }
            }
            _ => {
                if outs[v].is_empty() {
                    last
                } else {
                    depth[v]
                }
            }
        })
        .collect();
    let columns = last + 1;

    // Node values and labels.
    let value: Vec<f64> = (0..n)
        .map(|v| {
            let i: f64 = ins[v].iter().map(|&l| model.links[l].value).sum();
            let o: f64 = outs[v].iter().map(|&l| model.links[l].value).sum();
            i.max(o)
        })
        .collect();
    let labels: Vec<_> = (0..n)
        .map(|v| {
            let text = if show_values {
                format!("{} {}", model.nodes[v], kit::fmt_num(value[v]))
            } else {
                model.nodes[v].clone()
            };
            cx.text.lay(&Label::plain(&text), small, Some(cx.wrap_width("sankey.labelWidth")))
        })
        .collect();
    let mut members: Vec<Vec<usize>> = vec![Vec::new(); columns];
    for v in 0..n {
        members[column[v]].push(v);
    }
    for col in &mut members {
        col.sort_by(|&a, &b| value[b].total_cmp(&value[a]).then(a.cmp(&b)));
    }

    // Vertical scale: the busiest column fills the height.
    let line = cx.text.line_advance(small);
    let busiest = members.iter().map(Vec::len).max().unwrap_or(1);
    let total_h = (m.font_size * 26.0).max(busiest as f32 * (line + pad) * 1.4);
    let ky = members
        .iter()
        .filter(|c| !c.is_empty())
        .map(|c| {
            let sum: f64 = c.iter().map(|&v| value[v]).sum();
            let room = total_h - pad * (c.len() - 1) as f32;
            if sum > 0.0 { room.max(1.0) / sum as f32 } else { f32::INFINITY }
        })
        .fold(f32::INFINITY, f32::min);
    let ky = if ky.is_finite() { ky } else { 0.0 };
    let h: Vec<f32> = value.iter().map(|v| (*v as f32 * ky).max(1.0)).collect();
    // A node claims at least its label's height, so labels never collide.
    let slot: Vec<f32> = (0..n).map(|v| h[v].max(labels[v].height)).collect();

    // Column x positions: room for the labels on either side of each gap.
    let gap = m.font_size * 0.4;
    let mut x = vec![0.0f32; columns];
    for c in 1..columns {
        let right_labels = if c == 1 {
            members[0].iter().map(|&v| labels[v].width).fold(0.0, f32::max)
        } else {
            0.0
        };
        let left_labels = members[c].iter().map(|&v| labels[v].width).fold(0.0, f32::max);
        x[c] = x[c - 1] + node_w + right_labels + left_labels + 4.0 * gap + m.font_size * 4.0;
    }

    // Initial stacking, centred in the height.
    let mut y = vec![0.0f32; n];
    for col in &members {
        let used: f32 = col.iter().map(|&v| slot[v]).sum::<f32>() + pad * col.len().saturating_sub(1) as f32;
        let mut top = ((total_h - used) / 2.0).max(0.0);
        for &v in col {
            y[v] = top + (slot[v] - h[v]) / 2.0;
            top += slot[v] + pad;
        }
    }

    // Relaxation: pull each node toward the weighted centre of its
    // neighbours, then push apart.
    let center = |v: usize, y: &[f32]| y[v] + h[v] / 2.0;
    let mut alpha = 1.0f32;
    for _ in 0..8 {
        alpha *= 0.85;
        for forward in [true, false] {
            let cols: Vec<usize> = if forward { (1..columns).collect() } else { (0..columns.saturating_sub(1)).rev().collect() };
            for c in cols {
                for &v in &members[c] {
                    let links = if forward { &ins[v] } else { &outs[v] };
                    let mut wsum = 0.0f32;
                    let mut acc = 0.0f32;
                    for &li in links {
                        let l = &model.links[li];
                        let other = if forward { l.source } else { l.target };
                        let w = l.value as f32;
                        acc += center(other, &y) * w;
                        wsum += w;
                    }
                    if wsum > 0.0 {
                        let target = acc / wsum;
                        y[v] += (target - center(v, &y)) * alpha;
                    }
                }
                resolve(&members[c], &mut y, &h, &slot, pad, total_h);
            }
        }
    }

    // Link attachment: outgoing sorted by target height, incoming by source.
    let mut sy = vec![0.0f32; model.links.len()];
    let mut ty = vec![0.0f32; model.links.len()];
    let link_h: Vec<f32> = model.links.iter().map(|l| (l.value as f32 * ky).max(0.5)).collect();
    let attach = |links: &[usize], other: fn(&Link) -> usize, top: f32, ends: &mut [f32]| {
        let key = |li: usize| center(other(&model.links[li]), &y);
        let mut sorted = links.to_vec();
        sorted.sort_by(|&a, &b| key(a).total_cmp(&key(b)).then(a.cmp(&b)));
        let mut at = top;
        for li in sorted {
            ends[li] = at;
            at += link_h[li];
        }
    };
    for v in 0..n {
        attach(&outs[v], |l| l.target, y[v], &mut sy);
        attach(&ins[v], |l| l.source, y[v], &mut ty);
    }

    let node_ink = |v: usize, tone: Tone| -> Ink {
        match cx.config.get(&format!("sankey.nodeColors.{}", model.nodes[v])).and_then(color::parse) {
            Some(c) if tone == Tone::Solid => Ink::Rgba(c),
            Some(c) => Ink::Rgba([c[0], c[1], c[2], c[3] * 0.4]),
            None => style::series(v, tone),
        }
    };
    let link_color = cx.config.get("sankey.linkColor").unwrap_or("source");
    let fixed_link = color::parse(link_color).filter(|_| link_color.starts_with('#') || link_color.starts_with("rgb"));

    // Ribbons, widest first so narrow ones stay on top.
    let mut by_width: Vec<usize> = (0..model.links.len()).collect();
    by_width.sort_by(|&a, &b| link_h[b].total_cmp(&link_h[a]).then(a.cmp(&b)));
    for li in by_width {
        let l = &model.links[li];
        let x0 = x[column[l.source]] + node_w;
        let x1 = x[column[l.target]];
        let mx = (x0 + x1) / 2.0;
        let (a0, a1) = (sy[li], sy[li] + link_h[li]);
        let (b0, b1) = (ty[li], ty[li] + link_h[li]);
        let mut path = PathData::new();
        path.move_to([x0, a0])
            .cubic_to([mx, a0], [mx, b0], [x1, b0])
            .line_to([x1, b1])
            .cubic_to([mx, b1], [mx, a1], [x0, a1])
            .close();
        let ink = match (fixed_link, link_color) {
            (Some(c), _) => Ink::Rgba([c[0], c[1], c[2], c[3] * 0.45]),
            (None, "target") => node_ink(l.target, Tone::Soft),
            _ => node_ink(l.source, Tone::Soft),
        };
        out.fill(Geom::path(path), ink);
    }

    // Nodes and labels.
    for v in 0..n {
        let c = column[v];
        let rect = Rect::new(x[c], y[v], node_w, h[v]);
        out.fill(Geom::rect(rect), node_ink(v, Tone::Solid));
        let laid = &labels[v];
        let ly = y[v] + h[v] / 2.0 - laid.height / 2.0;
        if c == 0 && columns > 1 {
            out.text(laid, [rect.right() + gap, ly], laid.width, Align::Start, style::TEXT);
        } else {
            out.text(laid, [rect.x - gap - laid.width, ly], laid.width, Align::End, style::TEXT);
        }
    }

    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// Kahn order; every node appears once (the graph is acyclic by now).
fn topo(model: &Model, ins: &[Vec<usize>], outs: &[Vec<usize>]) -> Vec<usize> {
    let n = model.nodes.len();
    let mut indeg: Vec<usize> = ins.iter().map(Vec::len).collect();
    let mut queue: VecDeque<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let mut order = Vec::with_capacity(n);
    while let Some(v) = queue.pop_front() {
        order.push(v);
        for &li in &outs[v] {
            let t = model.links[li].target;
            indeg[t] -= 1;
            if indeg[t] == 0 {
                queue.push_back(t);
            }
        }
    }
    order
}

/// Keep a column in order without overlaps: push down, then back up from
/// the bottom, never above the top.
fn resolve(col: &[usize], y: &mut [f32], h: &[f32], slot: &[f32], pad: f32, total_h: f32) {
    let mut sorted = col.to_vec();
    sorted.sort_by(|&a, &b| (y[a] + h[a] / 2.0).total_cmp(&(y[b] + h[b] / 2.0)).then(a.cmp(&b)));
    // Work in slot space: a slot is centred on its node.
    let top_of = |v: usize, y: &[f32]| y[v] + h[v] / 2.0 - slot[v] / 2.0;
    let set_top = |v: usize, t: f32, y: &mut [f32]| y[v] = t + slot[v] / 2.0 - h[v] / 2.0;
    // Stack downward without overlaps; returns the floor under the last slot.
    let push_down = |y: &mut [f32]| {
        let mut floor = 0.0f32;
        for &v in &sorted {
            let t = top_of(v, y).max(floor);
            set_top(v, t, y);
            floor = t + slot[v] + pad;
        }
        floor
    };
    let mut ceiling = total_h.max(push_down(y) - pad);
    for &v in sorted.iter().rev() {
        let t = top_of(v, y);
        let bottom = t + slot[v];
        if bottom > ceiling {
            set_top(v, ceiling - slot[v], y);
        }
        ceiling = top_of(v, y) - pad;
    }
    push_down(y);
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.nodes.len() as u32,
        edges: model.links.len() as u32,
        groups: 0,
    }
}
