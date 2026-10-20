//! Packet diagrams: `packet-beta` / `packet` with `start-end: "name"`,
//! `start: "name"` and `+count: "name"` fields, laid out in rows of
//! `packet.bitsPerRow` bits (32 by default) with bit numbers above.

use unicode_segmentation::UnicodeSegmentation;

use super::kit::{self, keyword};
use super::{LayoutCx, ParseCx};
use crate::ir::{Align, Geom, Rect, SceneBuilder};
use crate::text::{FontSpec, LaidText, Label, Measurer};
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone)]
pub(crate) struct Field {
    pub start: u32,
    /// Inclusive.
    pub end: u32,
    pub label: Label,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub fields: Vec<Field>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        cx.warn(header, "unexpected text after the packet header");
    }
    let mut model = Model::default();
    let mut next = 0u32;
    for (line, stmt) in kit::statements(body, header) {
        if let Some(title) = keyword(stmt, "title") {
            let title = kit::unquote(title).trim();
            if !title.is_empty() {
                cx.meta.title = Some(title.to_string());
            }
            continue;
        }
        let Some((range, label)) = stmt.split_once(':') else {
            cx.warn(line, format!("unreadable packet field `{stmt}`"));
            continue;
        };
        let range = range.trim();
        let bound = |v: u64| -> Result<u32, DiagramError> {
            if v >= cx.limits.elements as u64 {
                return Err(DiagramError::TooLarge(LimitKind::Elements));
            }
            Ok(v as u32)
        };
        let parsed = if let Some(count) = range.strip_prefix('+') {
            count.trim().parse::<u64>().ok().filter(|c| *c > 0).map(|c| (u64::from(next), u64::from(next) + c - 1))
        } else if let Some((a, b)) = range.split_once('-') {
            a.trim().parse::<u64>().ok().zip(b.trim().parse::<u64>().ok())
        } else {
            range.parse::<u64>().ok().map(|a| (a, a))
        };
        let Some((start, end)) = parsed else {
            cx.warn(line, format!("unreadable bit range `{range}`"));
            continue;
        };
        if end < start {
            cx.warn(line, format!("bit range `{range}` ends before it starts"));
            continue;
        }
        let (start, end) = (bound(start)?, bound(end)?);
        if start < next {
            cx.warn(line, format!("field `{range}` overlaps the previous field"));
            continue;
        }
        if start > next {
            cx.warn(line, format!("bits {next}-{} are not described", start - 1));
        }
        let label = cx.label(kit::unquote(label.trim()))?;
        model.fields.push(Field { start, end, label });
        next = end + 1;
    }
    Ok(model)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let per_row = cx
        .config
        .get_f32("packet.bitsPerRow")
        .map_or(32, |v| v.round().clamp(1.0, 256.0) as u32);
    let show_bits = cx.config.get_bool("packet.showBits").unwrap_or(true);
    let descending = cx.config.get("packet.bitOrder") == Some("descending");
    let label_font = cx.small_font();
    let number_font = FontSpec::new(m.small_size * 0.85);
    let bit_w = m.font_size * 1.85;
    let pad = m.font_size * 0.3;
    let number_h = if show_bits { cx.text.line_advance(number_font) } else { 0.0 };
    let min_row = m.font_size * 2.4;
    let row_gap = if show_bits { 0.0 } else { m.font_size * 0.2 };

    // Split fields into per-row segments.
    struct Segment {
        row: u32,
        start: u32,
        end: u32,
        field: usize,
    }
    let mut segments = Vec::new();
    for (i, f) in model.fields.iter().enumerate() {
        let mut s = f.start;
        while s <= f.end {
            let row = s / per_row;
            let e = f.end.min((row + 1) * per_row - 1);
            segments.push(Segment { row, start: s, end: e, field: i });
            s = e + 1;
        }
    }
    let rows = segments.last().map_or(0, |s| s.row + 1);
    cx.limits.check_count(segments.len(), cx.limits.elements, LimitKind::Elements)?;

    let laid: Vec<LaidText> = segments
        .iter()
        .map(|s| {
            let width = (s.end - s.start + 1) as f32 * bit_w - 4.0;
            fit(&cx.text, &model.fields[s.field].label, label_font, width)
        })
        .collect();
    let mut row_h = vec![min_row; rows as usize];
    for (s, l) in segments.iter().zip(&laid) {
        let h = &mut row_h[s.row as usize];
        *h = h.max(l.height + 2.0 * pad);
    }

    let mut out = SceneBuilder::new();
    let mut row_top = vec![0.0f32; rows as usize];
    let mut y = 0.0;
    for r in 0..rows as usize {
        row_top[r] = y + number_h;
        y += number_h + row_h[r] + row_gap;
    }
    let x_of = |bit: u32| -> f32 {
        let col = bit % per_row;
        let col = if descending { per_row - 1 - col } else { col };
        col as f32 * bit_w
    };
    for (s, l) in segments.iter().zip(&laid) {
        let (a, b) = if descending { (x_of(s.end), x_of(s.start)) } else { (x_of(s.start), x_of(s.end)) };
        let rect = Rect::new(a, row_top[s.row as usize], b + bit_w - a, row_h[s.row as usize]);
        out.shape(Geom::rect(rect), Some(style::NODE_FILL), Some(style::node_stroke()));
        out.text_centered(l, rect.center(), style::TEXT);
        if show_bits {
            let ny = rect.y - number_h;
            let (left, right) = if descending { (s.end, s.start) } else { (s.start, s.end) };
            if s.start == s.end {
                let t = cx.text.lay_plain(&left.to_string(), number_font);
                out.text(&t, [rect.center()[0] - t.width / 2.0, ny], t.width, Align::Center, style::MUTED);
            } else {
                let t = cx.text.lay_plain(&left.to_string(), number_font);
                out.text(&t, [rect.x + 2.0, ny], t.width, Align::Start, style::MUTED);
                let t = cx.text.lay_plain(&right.to_string(), number_font);
                out.text(&t, [rect.right() - 2.0 - t.width, ny], t.width, Align::End, style::MUTED);
            }
        }
    }

    let content = Rect::new(0.0, 0.0, per_row as f32 * bit_w, y.max(1.0)).union(&out.bounds());
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// A label inside `width`: wrapped between words, then in a smaller face,
/// then cut short with an ellipsis. Words never break mid-way except in
/// scripts that wrap per character.
fn fit(text: &Measurer<'_>, label: &Label, font: FontSpec, width: f32) -> LaidText {
    let width = width.max(1.0);
    let full = label.text();
    let widest_word = |f: FontSpec| {
        full.split_whitespace()
            .filter(|w| !w.chars().any(crate::text::is_wide))
            .map(|w| text.width(w, f))
            .fold(0.0f32, f32::max)
    };
    let smaller = FontSpec::new(font.size * 0.85);
    for face in [font, smaller] {
        if widest_word(face) <= width + 0.5 {
            let laid = text.lay(label, face, Some(width));
            if laid.width <= width + 0.5 {
                return laid;
            }
        }
    }
    // One measurement of the whole text, then keep the graphemes that fit.
    let ellipsis = text.width("…", smaller);
    let advances = text.advances(&full, smaller);
    let mut used = ellipsis;
    let mut kept = String::new();
    for (g, a) in full.graphemes(true).zip(advances.iter()) {
        if used + a > width {
            break;
        }
        used += a;
        kept.push_str(g);
    }
    if !kept.trim().is_empty() {
        return text.lay_plain(&format!("{}…", kept.trim_end()), smaller);
    }
    text.lay_plain("", smaller)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.fields.len() as u32,
        edges: 0,
        groups: 0,
    }
}
