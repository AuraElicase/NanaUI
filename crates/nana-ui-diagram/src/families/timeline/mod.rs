//! Timelines: `timeline`. Time periods run left to right along an axis;
//! each period's events stack below it as cards. Sections group periods
//! and give them one series colour.

use nana_ui_core::RadiusTier;

use super::kit::{self, first_word};
use super::{LayoutCx, ParseCx};
use crate::geom::{Marker, PathData};
use crate::ir::{Align, Dash, Geom, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::style::{self, series};
use crate::text::{FontSpec, LaidText, Label};
use crate::{DiagramError, DiagramScene, LimitKind, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone)]
pub(crate) struct Period {
    pub label: Label,
    pub section: Option<usize>,
    pub events: Vec<Label>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub sections: Vec<Label>,
    pub periods: Vec<Period>,
    /// `timeline.disableMulticolor`: every period in the first series colour.
    pub one_color: bool,
}

/// Split on `:` followed by whitespace or the end, as Mermaid's lexer does,
/// so `10:30` inside an event stays whole.
fn fields(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b':' && bytes.get(i + 1).is_none_or(|n| n.is_ascii_whitespace()) {
            out.push(line[start..i].trim());
            start = i + 1;
        }
    }
    out.push(line[start..].trim());
    out
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model {
        one_color: cx.config.get_bool("timeline.disableMulticolor").unwrap_or(false),
        ..Model::default()
    };
    let (header, rest) = kit::header(body)?;
    if !matches!(rest, "" | "LR" | "TD") {
        cx.warn(header, format!("unknown direction `{rest}`"));
    }
    let mut count = 0usize;
    for (index, line) in kit::statements(body, header) {
        let (word, rest) = first_word(line);
        match word {
            "title" => {
                cx.meta.title = Some(rest.to_string());
                continue;
            }
            "section" => {
                if model.sections.len() >= cx.limits.clusters {
                    return Err(DiagramError::TooLarge(LimitKind::Clusters));
                }
                model.sections.push(cx.label(rest)?);
                continue;
            }
            _ => {}
        }
        let parts = fields(line);
        let (period, events) = parts.split_first().map_or(("", &[][..]), |(p, e)| (*p, e));
        count += usize::from(!period.is_empty()) + events.len();
        if count > cx.limits.elements {
            return Err(DiagramError::TooLarge(LimitKind::Elements));
        }
        let events = events
            .iter()
            .filter(|e| !e.is_empty())
            .map(|e| cx.label(e))
            .collect::<Result<Vec<_>, _>>()?;
        if period.is_empty() {
            // `: event` continues the period above.
            match model.periods.last_mut() {
                Some(p) => p.events.extend(events),
                None => cx.warn(index, "event before any time period"),
            }
            continue;
        }
        model.periods.push(Period {
            label: cx.label(period)?,
            section: model.sections.len().checked_sub(1),
            events,
        });
    }
    Ok(model)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let k = m.font_size / 14.0;
    let font = cx.font();
    let strong = FontSpec::new(m.font_size).weight(m.strong_weight);
    let wrap = 150.0 * k;
    let pad = [m.node_pad_x * 0.75, m.node_pad_y * 0.8];
    let col_gap = 16.0 * k;
    let gap = 10.0 * k;
    let mut out = SceneBuilder::new();

    let periods: Vec<LaidText> = model.periods.iter().map(|p| cx.text.lay(&p.label, strong, Some(wrap))).collect();
    let events: Vec<Vec<LaidText>> = model
        .periods
        .iter()
        .map(|p| p.events.iter().map(|e| cx.text.lay(e, font, Some(wrap))).collect())
        .collect();
    let sections: Vec<LaidText> = model.sections.iter().map(|s| cx.text.lay(s, strong, Some(wrap * 2.0))).collect();

    // One column width for every period, as wide as the widest content.
    let widest = periods
        .iter()
        .chain(events.iter().flatten())
        .map(|l| l.width)
        .fold(0.0_f32, f32::max);
    let col_w = (widest + 2.0 * pad[0]).max(100.0 * k);
    let col_x = |i: usize| i as f32 * (col_w + col_gap);

    let color = |i: usize| {
        if model.one_color {
            0
        } else if model.sections.is_empty() {
            i
        } else {
            model.periods[i].section.unwrap_or(0)
        }
    };

    // Section bands above their periods.
    let mut y = 0.0;
    if !model.sections.is_empty() {
        let band_h = sections.iter().map(|s| s.height).fold(cx.text.line_advance(strong), f32::max) + 2.0 * pad[1];
        for (s, laid) in sections.iter().enumerate() {
            let in_section = |p: &Period| p.section == Some(s);
            let (Some(first), Some(last)) = (model.periods.iter().position(in_section), model.periods.iter().rposition(in_section)) else {
                continue;
            };
            let rect = Rect::new(col_x(first), y, col_x(last) + col_w - col_x(first), band_h);
            let index = if model.one_color { 0 } else { s };
            out.shape(
                Geom::rounded(rect, RadiusTier::Sm),
                Some(series(index, Tone::Solid)),
                None,
            );
            out.text(laid, [rect.x, rect.y + (rect.h - laid.height) / 2.0], rect.w, Align::Center, series(index, Tone::On));
        }
        y += band_h + gap;
    }

    // Periods.
    let period_h = periods.iter().map(|p| p.height).fold(cx.text.line_advance(strong), f32::max) + 2.0 * pad[1];
    let period_top = y;
    let axis_y = period_top + period_h + 1.5 * gap;
    let events_top = axis_y + 1.5 * gap;
    if model.periods.is_empty() {
        let content = cx.title_above(&mut out, Rect::default());
        return Ok(out.finish(content, m.margin));
    }

    // Connectors under everything: period to its last event.
    let mut card_rects: Vec<Vec<Rect>> = Vec::with_capacity(model.periods.len());
    for (i, laid_events) in events.iter().enumerate() {
        let x = col_x(i);
        let mut top = events_top;
        let mut rects = Vec::new();
        for laid in laid_events {
            let h = laid.height + 2.0 * pad[1];
            rects.push(Rect::new(x, top, col_w, h));
            top += h + gap;
        }
        if let Some(last) = rects.last() {
            let cx_ = x + col_w / 2.0;
            out.stroke(
                Geom::path(PathData::polyline(&[[cx_, period_top + period_h], [cx_, last.y]])),
                Stroke::hairline(series(color(i), Tone::Solid)).dash(Dash::Dashed),
            );
        }
        card_rects.push(rects);
    }

    // The axis, with an arrow past the last period.
    let right = col_x(model.periods.len() - 1) + col_w;
    out.edge(
        &[[-col_gap / 2.0, axis_y], [right + col_gap, axis_y]],
        PathData::polyline,
        Stroke::new(style::EDGE, StrokeWidth::Thick),
        None,
        Some(Marker::Arrow),
        cx.marker_size(),
    );

    for (i, laid) in periods.iter().enumerate() {
        let rect = Rect::new(col_x(i), period_top, col_w, period_h);
        let c = color(i);
        out.shape(
            Geom::rounded(rect, RadiusTier::Sm),
            Some(series(c, Tone::Soft)),
            Some(Stroke::hairline(series(c, Tone::Solid))),
        );
        out.text_centered(laid, rect.center(), style::TEXT);
        out.fill(Geom::Ellipse(Rect::centered([rect.center()[0], axis_y], [9.0 * k, 9.0 * k])), series(c, Tone::Solid));
        for (rect, laid) in card_rects[i].iter().zip(&events[i]) {
            out.shape(
                Geom::rounded(*rect, RadiusTier::Sm),
                Some(style::NODE_FILL),
                Some(Stroke::hairline(series(c, Tone::Solid))),
            );
            out.text_centered(laid, rect.center(), style::TEXT);
        }
    }

    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: (model.periods.len() + model.periods.iter().map(|p| p.events.len()).sum::<usize>()) as u32,
        edges: 0,
        groups: model.sections.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::fields;

    #[test]
    fn colons_split_only_before_whitespace() {
        assert_eq!(fields("2004 : Facebook : Google"), vec!["2004", "Facebook", "Google"]);
        assert_eq!(fields(": 10:30 standup"), vec!["", "10:30 standup"]);
        assert_eq!(fields("2005 :"), vec!["2005", ""]);
    }
}
