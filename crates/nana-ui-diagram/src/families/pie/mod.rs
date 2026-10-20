//! Pie charts: `pie [showData] [title ...]` and `"label" : value` rows.
//! Slices run clockwise from twelve o'clock in source order; the legend
//! sits on the right.


use std::f32::consts::{FRAC_PI_2, TAU};

use super::kit::{self, LegendRow, fmt_num, keyword};
use super::{LayoutCx, ParseCx};
use crate::geom::PathData;
use crate::ir::{Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone)]
pub(crate) struct Slice {
    pub label: Label,
    pub value: f64,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub show_data: bool,
    pub slices: Vec<Slice>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    let mut model = Model::default();
    let mut head = rest;
    if let Some(after) = keyword(head, "showData") {
        model.show_data = true;
        head = after;
    }
    if let Some(title) = keyword(head, "title") {
        set_title(cx, title);
    } else if !head.is_empty() {
        cx.warn(header, format!("unknown pie option `{head}`"));
    }
    for (line, stmt) in kit::statements(body, header) {
        if let Some(rest) = keyword(stmt, "showData") {
            model.show_data = true;
            if let Some(title) = keyword(rest, "title") {
                set_title(cx, title);
            }
            continue;
        }
        if let Some(title) = keyword(stmt, "title") {
            set_title(cx, title);
            continue;
        }
        match slice(stmt) {
            Some((label, value)) if value >= 0.0 => {
                cx.limits.check_count(model.slices.len() + 1, cx.limits.elements, LimitKind::Elements)?;
                let label = cx.label(label)?;
                model.slices.push(Slice { label, value });
            }
            Some(_) => cx.warn(line, "pie values must not be negative"),
            None => cx.warn(line, format!("unreadable pie row `{stmt}`")),
        }
    }
    Ok(model)
}

fn set_title(cx: &mut ParseCx<'_>, title: &str) {
    let title = kit::unquote(title).trim();
    if !title.is_empty() {
        cx.meta.title = Some(title.to_string());
    }
}

/// `"label" : value`.
fn slice(stmt: &str) -> Option<(&str, f64)> {
    let quote = stmt.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let close = stmt[1..].find(quote)? + 1;
    let label = &stmt[1..close];
    let value = stmt[close + 1..].trim().strip_prefix(':')?;
    Some((label, kit::number(value)?))
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let small = cx.small_font();
    let radius = m.font_size * 9.0;
    let center = [radius, radius];
    let hole = cx.config.get_f32("pie.donutHole").unwrap_or(0.0).clamp(0.0, 0.9) * radius;
    let text_at = cx.config.get_f32("pie.textPosition").map(|t| t.clamp(0.0, 1.0));
    let mut out = SceneBuilder::new();

    let total: f64 = model.slices.iter().map(|s| s.value).sum();
    let separator = Stroke::new(style::NODE_FILL, StrokeWidth::Thick);
    let drawn: Vec<usize> = (0..model.slices.len()).filter(|&i| model.slices[i].value > 0.0).collect();
    if total <= 0.0 || drawn.is_empty() {
        out.stroke(Geom::Ellipse(Rect::new(0.0, 0.0, radius * 2.0, radius * 2.0)), Stroke::hairline(style::GRID));
    } else if drawn.len() == 1 {
        let i = drawn[0];
        out.fill(ring(center, radius, hole), series(i, Tone::Solid));
    } else {
        let mut start = -FRAC_PI_2;
        for &i in &drawn {
            let sweep = (model.slices[i].value / total) as f32 * TAU;
            out.shape(Geom::path(sector(center, radius, hole, start, sweep)), Some(series(i, Tone::Solid)), Some(separator));
            start += sweep;
        }
    }

    // Percentages inside the slices that have room for them.
    if total > 0.0 {
        let r_text = match text_at {
            Some(t) => hole + (radius - hole) * t,
            None if hole > 0.0 => (hole + radius) / 2.0,
            None => radius * 0.68,
        };
        let mut start = -FRAC_PI_2;
        for &i in &drawn {
            let share = model.slices[i].value / total;
            let sweep = share as f32 * TAU;
            let mid = start + sweep / 2.0;
            start += sweep;
            let pct = format!("{}%", (share * 100.0).round());
            let laid = cx.text.lay_plain(&pct, small);
            let fits = if drawn.len() == 1 {
                true
            } else {
                // Chord room at the label radius and radial room to the rim.
                let chord = 2.0 * r_text * (sweep / 2.0).min(FRAC_PI_2).sin();
                let need = (laid.width.powi(2) + laid.height.powi(2)).sqrt() + 4.0;
                chord >= need && radius - r_text >= laid.height * 0.6 && share >= 0.02
            };
            if !fits {
                continue;
            }
            let at = if drawn.len() == 1 && hole <= 0.0 {
                center
            } else {
                [center[0] + r_text * mid.cos(), center[1] + r_text * mid.sin()]
            };
            out.text_centered(&laid, at, series(i, Tone::On));
        }
    }

    // Legend.
    let font = cx.font();
    let rows: Vec<LegendRow> = model
        .slices
        .iter()
        .enumerate()
        .map(|(i, s)| LegendRow {
            index: i as u16,
            name: cx.text.lay(&s.label, font, Some(cx.wrap_width("pie.legendWidth"))),
            value: model.show_data.then(|| cx.text.lay_plain(&format!("[{}]", fmt_num(s.value)), font)),
        })
        .collect();
    let legend = kit::legend(&mut out, cx, &rows, radius * 2.0 + m.font_size * 2.0, center[1]);

    let content = Rect::new(0.0, 0.0, radius * 2.0, radius * 2.0).union(&legend).union(&out.bounds());
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

fn series(i: usize, tone: Tone) -> Ink {
    Ink::Series {
        index: i as u16,
        tone,
    }
}

/// A full disc or annulus.
fn ring(center: [f32; 2], radius: f32, hole: f32) -> Geom {
    let disc = Rect::centered(center, [radius * 2.0, radius * 2.0]);
    if hole <= 0.0 {
        return Geom::Ellipse(disc);
    }
    let mut path = PathData::new();
    path.arc(center, radius, 0.0, TAU, false).close();
    path.arc(center, hole, 0.0, -TAU, false).close();
    Geom::path(path)
}

/// A wedge from `start` sweeping `sweep` radians clockwise, cut by `hole`.
fn sector(center: [f32; 2], radius: f32, hole: f32, start: f32, sweep: f32) -> PathData {
    let mut path = PathData::new();
    if hole > 0.0 {
        path.arc(center, radius, start, sweep, false);
        path.arc(center, hole, start + sweep, -sweep, true);
    } else {
        path.move_to(center);
        path.arc(center, radius, start, sweep, true);
    }
    path.close();
    path
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.slices.len() as u32,
        edges: 0,
        groups: 0,
    }
}
