//! Cynefin frameworks (`cynefin-beta`): the fixed five-domain picture —
//! Complex top left, Complicated top right, Chaotic bottom left, Clear
//! bottom right, Confusion in the middle — with each domain's items and
//! `a --> b : "label"` transitions between domains.

use super::pie::kit::{self, keyword};
use super::{LayoutCx, ParseCx};
use crate::geom::{Marker, PathData};
use crate::ir::{Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::text::{LaidText, Label};
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

pub(crate) const NATIVE: bool = true;

/// The confusion ellipse shows at most this many items.
const CONFUSION_ITEMS: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Domain {
    Complex,
    Complicated,
    Clear,
    Chaotic,
    Confusion,
}

impl Domain {
    const ALL: [Self; 5] = [
        Self::Complex,
        Self::Complicated,
        Self::Clear,
        Self::Chaotic,
        Self::Confusion,
    ];

    fn parse(word: &str) -> Option<Self> {
        Some(match word.trim().to_ascii_lowercase().as_str() {
            "complex" => Self::Complex,
            "complicated" => Self::Complicated,
            "clear" | "obvious" | "simple" => Self::Clear,
            "chaotic" => Self::Chaotic,
            "confusion" | "disorder" | "aporetic" => Self::Confusion,
            _ => return None,
        })
    }

    fn index(self) -> usize {
        self as usize
    }

    fn name(self) -> &'static str {
        match self {
            Self::Complex => "Complex",
            Self::Complicated => "Complicated",
            Self::Clear => "Clear",
            Self::Chaotic => "Chaotic",
            Self::Confusion => "Confusion",
        }
    }

    fn description(self) -> (&'static str, &'static str) {
        match self {
            Self::Complex => ("Probe → Sense → Respond", "Emergent practice"),
            Self::Complicated => ("Sense → Analyse → Respond", "Good practice"),
            Self::Clear => ("Sense → Categorise → Respond", "Best practice"),
            Self::Chaotic => ("Act → Sense → Respond", "Novel practice"),
            Self::Confusion => ("", ""),
        }
    }

    /// Quadrant position: (right, bottom).
    fn quadrant(self) -> (bool, bool) {
        match self {
            Self::Complex => (false, false),
            Self::Complicated => (true, false),
            Self::Chaotic => (false, true),
            Self::Clear | Self::Confusion => (true, true),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Transition {
    pub from: Domain,
    pub to: Domain,
    pub label: Option<Label>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// Items per domain, in [`Domain::ALL`] order.
    pub items: [Vec<Label>; 5],
    pub transitions: Vec<Transition>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        cx.warn(header, "unexpected text after `cynefin-beta`");
    }
    let mut model = Model::default();
    let mut current: Option<Domain> = None;
    let mut count = 0usize;
    for (line, stmt) in kit::statements(body, header) {
        if let Some(title) = keyword(stmt, "title") {
            let title = kit::unquote(title).trim();
            if !title.is_empty() {
                cx.meta.title = Some(title.to_string());
            }
        } else if let Some(domain) = Domain::parse(stmt) {
            current = Some(domain);
        } else if let Some((from, rest)) = stmt.split_once("-->") {
            let (to, label) = match rest.split_once(':') {
                Some((to, label)) => (to, Some(label)),
                None => (rest, None),
            };
            match (Domain::parse(from), Domain::parse(to)) {
                (Some(from), Some(to)) if from == to => {}
                (Some(from), Some(to)) => {
                    count += 1;
                    kit::check(cx.limits, count, cx.limits.elements, LimitKind::Elements)?;
                    let label = match label.map(|l| kit::unquote(l.trim()).trim()) {
                        Some(l) if !l.is_empty() => Some(cx.label(l)?),
                        _ => None,
                    };
                    model.transitions.push(Transition { from, to, label });
                }
                _ => cx.warn(line, format!("transitions join two domains, not `{stmt}`")),
            }
        } else if stmt.starts_with('"') || stmt.starts_with('\'') {
            let Some(domain) = current else {
                cx.warn(line, "an item needs a domain above it");
                continue;
            };
            count += 1;
            kit::check(cx.limits, count, cx.limits.elements, LimitKind::Elements)?;
            let label = cx.label(kit::unquote(stmt))?;
            if !label.is_empty() {
                model.items[domain.index()].push(label);
            }
        } else {
            cx.warn(line, format!("unreadable cynefin statement `{stmt}`"));
        }
    }
    Ok(model)
}

/// A wavy line from `a` to `b`, flat at both ends.
fn wave(a: [f32; 2], b: [f32; 2], amplitude: f32, waves: f32) -> Vec<[f32; 2]> {
    let d = [b[0] - a[0], b[1] - a[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1e-3);
    let n = [-d[1] / len, d[0] / len];
    (0..=32)
        .map(|i| {
            let t = i as f32 / 32.0;
            let off = amplitude * (t * waves * std::f32::consts::TAU).sin() * (t * std::f32::consts::PI).sin();
            [a[0] + d[0] * t + n[0] * off, a[1] + d[1] * t + n[1] * off]
        })
        .collect()
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let strong = cx.title_font();
    let pad = m.font_size;
    let show_desc = cx.config.get_bool("cynefin.showDomainDescriptions").unwrap_or(true);
    let item_wrap = m.font_size * 13.0;
    let chip_pad = [m.small_size * 0.6, m.small_size * 0.25];
    let item_gap = m.small_size * 0.5;

    // Measure every domain: name, description and item chips.
    struct Block {
        name: LaidText,
        desc: Vec<LaidText>,
        items: Vec<LaidText>,
        more: Option<LaidText>,
    }
    let blocks: Vec<Block> = Domain::ALL
        .iter()
        .map(|&d| {
            let (a, b) = d.description();
            let desc = if show_desc && !a.is_empty() {
                vec![cx.text.lay_plain(a, small), cx.text.lay_plain(b, small.italic(true))]
            } else {
                Vec::new()
            };
            let all = &model.items[d.index()];
            let shown = if d == Domain::Confusion { all.len().min(CONFUSION_ITEMS) } else { all.len() };
            let items = all[..shown].iter().map(|l| cx.text.lay(l, small, Some(item_wrap))).collect();
            let more = (all.len() > shown).then(|| cx.text.lay_plain(&format!("+{} more", all.len() - shown), small));
            Block {
                name: cx.text.lay_plain(d.name(), if d == Domain::Confusion { font.weight(m.strong_weight) } else { strong }),
                desc,
                items,
                more,
            }
        })
        .collect();
    let chip_size = |l: &LaidText| [l.width + 2.0 * chip_pad[0], l.height + 2.0 * chip_pad[1]];
    let stack_h = |b: &Block| -> f32 {
        let chips: Vec<f32> = b.items.iter().chain(b.more.iter()).map(|l| chip_size(l)[1]).collect();
        chips.iter().sum::<f32>() + item_gap * chips.len().saturating_sub(1) as f32
    };
    let stack_w = |b: &Block| b.items.iter().chain(b.more.iter()).map(|l| chip_size(l)[0]).fold(0.0, f32::max);
    let header_h = |b: &Block| b.name.height + b.desc.iter().map(|d| d.height).sum::<f32>();

    // The confusion ellipse holds its name and items.
    let conf = &blocks[Domain::Confusion.index()];
    let conf_w = stack_w(conf).max(conf.name.width) + 2.0 * pad;
    let conf_h = conf.name.height + if stack_h(conf) > 0.0 { stack_h(conf) + item_gap } else { 0.0 };
    let rx = (conf_w * 0.75).max(m.font_size * 6.0);
    let ry = (conf_h * 0.75).max(m.font_size * 2.6);

    // Transition lanes: room around the ellipse for arrows and their chips.
    let chips: Vec<Option<LaidText>> = model
        .transitions
        .iter()
        .map(|t| t.label.as_ref().map(|l| cx.text.lay(l, small, Some(m.font_size * 10.0))))
        .collect();
    let chip_w = chips.iter().flatten().map(|l| chip_size(l)[0]).fold(0.0, f32::max);
    let chip_h = cx.text.line_advance(small) + 2.0 * chip_pad[1];
    let lane = if model.transitions.is_empty() { 0.0 } else { chip_h * 2.0 + pad * 0.5 };

    // Quadrant size: every domain's header and items fit above or below
    // the ellipse and its lanes.
    let quads = [Domain::Complex, Domain::Complicated, Domain::Chaotic, Domain::Clear];
    let mut qw = m.font_size * 20.0;
    let mut qh = m.font_size * 13.0;
    for d in quads {
        let b = &blocks[d.index()];
        qw = qw.max(stack_w(b) + 2.0 * pad).max(b.name.width + 2.0 * pad);
        for l in &b.desc {
            qw = qw.max(l.width + 2.0 * pad);
        }
        let need = pad + header_h(b) + pad + stack_h(b) + pad + ry + lane;
        qh = qh.max(need);
    }
    qw = qw.max(rx + pad + chip_w + 2.0 * pad);
    let (w, h) = (qw * 2.0, qh * 2.0);
    let c = [qw, qh];
    let mut out = SceneBuilder::new();

    // Domains: four washes cut by wavy boundaries through the centre.
    let amp = m.font_size * 0.5;
    let top = wave([c[0], 0.0], c, amp, 1.0);
    let bottom = wave(c, [c[0], h], amp, 1.0);
    let left = wave([0.0, c[1]], c, amp, 1.0);
    let right = wave(c, [w, c[1]], amp, 1.0);
    let rev = |v: &[[f32; 2]]| v.iter().rev().copied().collect::<Vec<_>>();
    let mut regions: [Vec<[f32; 2]>; 4] = Default::default();
    // Complex: top-left.
    regions[0] = [vec![[0.0, 0.0]], top.clone(), rev(&left)].concat();
    // Complicated: top-right.
    regions[1] = [vec![[w, 0.0]], top.clone(), right.clone()].concat();
    // Chaotic: bottom-left.
    regions[2] = [vec![[0.0, h]], rev(&bottom), rev(&left)].concat();
    // Clear: bottom-right.
    regions[3] = [vec![[w, h]], rev(&bottom), right.clone()].concat();
    let frame = Rect::new(0.0, 0.0, w, h);
    out.clip(Geom::rounded(frame, style::CLUSTER_CORNER));
    for (q, d) in quads.iter().enumerate() {
        // Each ring runs corner → one boundary into the centre → the
        // other boundary back out.
        out.fill(
            Geom::path(PathData::polygon(&regions[q])),
            Ink::Series {
                index: d.index() as u16,
                tone: Tone::Soft,
            },
        );
    }
    out.unclip();
    let boundary = Stroke::hairline(style::NODE_STROKE);
    out.stroke(Geom::path(PathData::polyline(&top)), boundary);
    out.stroke(Geom::path(PathData::polyline(&left)), boundary);
    out.stroke(Geom::path(PathData::polyline(&right)), boundary);
    // The cliff between Clear and Chaotic: heavier, with a curl at its foot.
    let mut cliff = bottom.clone();
    if let Some(&end) = cliff.last() {
        cliff.pop();
        cliff.push([end[0], end[1] - pad * 0.8]);
        cliff.push([end[0] - pad * 0.6, end[1] - pad * 0.2]);
    }
    out.stroke(Geom::path(PathData::polyline(&cliff)), Stroke::new(style::TEXT, StrokeWidth::Thick));
    out.stroke(Geom::rounded(frame, style::CLUSTER_CORNER), Stroke::hairline(style::CLUSTER_STROKE));

    // Headers along the outer edge and item stacks toward the middle.
    for d in quads {
        let b = &blocks[d.index()];
        let (right_side, bottom_side) = d.quadrant();
        let x0 = if right_side { c[0] } else { 0.0 };
        let mid = x0 + qw / 2.0;
        let head = header_h(b);
        let mut y = if bottom_side { h - pad - head } else { pad };
        out.text_centered(&b.name, [mid, y + b.name.height / 2.0], style::TEXT);
        y += b.name.height;
        for l in &b.desc {
            out.text_centered(l, [mid, y + l.height / 2.0], style::MUTED);
            y += l.height;
        }
        // Items: below the header in the top row, above it in the bottom.
        let sh = stack_h(b);
        let mut iy = if bottom_side {
            h - pad - head - pad - sh
        } else {
            pad + head + pad
        };
        for l in b.items.iter().chain(b.more.iter()) {
            let size = chip_size(l);
            let rect = Rect::new(mid - size[0] / 2.0, iy, size[0], size[1]);
            out.shape(Geom::rounded(rect, nana_ui_core::RadiusTier::Full), Some(style::NODE_FILL), Some(Stroke::hairline(style::CHIP_STROKE)));
            out.text_centered(l, rect.center(), style::TEXT);
            iy += size[1] + item_gap;
        }
    }

    // Confusion in the middle.
    let ellipse = Rect::centered(c, [rx * 2.0, ry * 2.0]);
    out.shape(Geom::Ellipse(ellipse), Some(style::NODE_FILL), Some(Stroke::hairline(style::NODE_STROKE).dash(Dash::Dashed)));
    let mut y = c[1] - conf_h / 2.0;
    out.text_centered(&conf.name, [c[0], y + conf.name.height / 2.0], style::TEXT);
    y += conf.name.height + item_gap;
    for l in conf.items.iter().chain(conf.more.iter()) {
        let size = chip_size(l);
        out.text_centered(l, [c[0], y + size[1] / 2.0], style::MUTED);
        y += size[1] + item_gap;
    }

    // Transitions.
    let marker = cx.marker_size();
    let stroke = Stroke::new(style::EDGE, StrokeWidth::Thick);
    let anchor = |d: Domain| -> [f32; 2] {
        match d {
            Domain::Confusion => c,
            _ => {
                let (r, b) = d.quadrant();
                [
                    if r { c[0] + qw * 0.3 } else { c[0] - qw * 0.3 },
                    if b { c[1] + ry + lane / 2.0 } else { c[1] - ry - lane / 2.0 },
                ]
            }
        }
    };
    let mut seen: Vec<(Domain, Domain)> = Vec::new();
    for (t, chip) in model.transitions.iter().zip(&chips) {
        let repeat = seen.iter().filter(|&&(a, b)| (a, b) == (t.from, t.to) || (a, b) == (t.to, t.from)).count();
        seen.push((t.from, t.to));
        let (fq, tq) = (t.from.quadrant(), t.to.quadrant());
        let sign = if repeat % 2 == 0 { 1.0 } else { -1.0 };
        let shift = (repeat / 2) as f32 * chip_h;
        let (points, label_at) = if t.from == Domain::Confusion || t.to == Domain::Confusion {
            // Straight from the ellipse's rim out toward the domain.
            let other = if t.from == Domain::Confusion { t.to } else { t.from };
            let target = anchor(other);
            let d = [target[0] - c[0], target[1] - c[1]];
            let len = (d[0] * d[0] + d[1] * d[1]).sqrt().max(1.0);
            let k = 1.0 / ((d[0] / len / rx).powi(2) + (d[1] / len / ry).powi(2)).sqrt();
            let rim = [c[0] + d[0] / len * k, c[1] + d[1] / len * k];
            let pts = if t.from == Domain::Confusion { vec![rim, target] } else { vec![target, rim] };
            let mid = [(rim[0] + target[0]) / 2.0, (rim[1] + target[1]) / 2.0];
            (pts, mid)
        } else if fq.1 == tq.1 {
            // Across the vertical boundary, in the lane beside the ellipse.
            let y = if fq.1 { c[1] + ry + lane / 2.0 } else { c[1] - ry - lane / 2.0 } + sign * shift;
            let dx = qw * 0.3;
            let (x0, x1) = if fq.0 { (c[0] + dx, c[0] - dx) } else { (c[0] - dx, c[0] + dx) };
            let bow = if fq.1 { chip_h * 0.6 } else { -chip_h * 0.6 } * sign;
            (bezier([x0, y], [c[0], y + bow * 2.0], [x1, y]), [c[0], y + bow])
        } else if fq.0 == tq.0 {
            // Across the horizontal boundary, beside the ellipse.
            let x = if fq.0 { c[0] + rx + pad + chip_w / 2.0 } else { c[0] - rx - pad - chip_w / 2.0 } + sign * shift;
            let dy = ry;
            let (y0, y1) = if fq.1 { (c[1] + dy, c[1] - dy) } else { (c[1] - dy, c[1] + dy) };
            let bow = if fq.0 { pad * 0.6 } else { -pad * 0.6 } * sign;
            (bezier([x, y0], [x + bow * 2.0, c[1]], [x, y1]), [x + bow, c[1]])
        } else {
            // Diagonal: out along the lane, down the side of the ellipse
            // facing the target's column, and in again.
            let (a, b) = (anchor(t.from), anchor(t.to));
            let side = if tq.0 { 1.0 } else { -1.0 };
            let x = c[0] + side * (rx + pad + chip_w / 2.0) + sign * shift;
            let pts = vec![a, [x, a[1]], [x, b[1]], b];
            (pts, [x, c[1]])
        };
        let smooth = !(t.from == Domain::Confusion || t.to == Domain::Confusion) && fq.0 != tq.0 && fq.1 != tq.1;
        if smooth {
            out.edge(&points, crate::geom::path::basis_curve, stroke, None, Some(Marker::Arrow), marker);
            if let Some(l) = chip {
                out.chip(l, label_at, chip_pad);
            }
            continue;
        }
        out.edge(&points, PathData::polyline, stroke, None, Some(Marker::Arrow), marker);
        if let Some(l) = chip {
            out.chip(l, label_at, chip_pad);
        }
    }

    let content = frame.union(&out.bounds());
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// A quadratic curve flattened to points (so line-end markers can follow
/// its last segment).
fn bezier(a: [f32; 2], ctrl: [f32; 2], b: [f32; 2]) -> Vec<[f32; 2]> {
    (0..=16)
        .map(|i| {
            let t = i as f32 / 16.0;
            let u = 1.0 - t;
            [
                u * u * a[0] + 2.0 * u * t * ctrl[0] + t * t * b[0],
                u * u * a[1] + 2.0 * u * t * ctrl[1] + t * t * b[1],
            ]
        })
        .collect()
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.items.iter().map(Vec::len).sum::<usize>() as u32,
        edges: model.transitions.len() as u32,
        groups: 5,
    }
}
