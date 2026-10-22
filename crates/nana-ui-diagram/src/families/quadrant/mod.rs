//! Quadrant charts: `quadrantChart` with axis captions, four quadrant
//! titles and points in the unit square, styled inline or by `classDef`.

use std::collections::HashMap;

use super::kit::{self, keyword};
use super::{LayoutCx, ParseCx};
use crate::geom::{Marker, PathData};
use crate::ir::{Align, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::source::color;
use crate::text::Label;
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

/// Point labels look for a free spot only up to this many points.
const AVOID_LIMIT: usize = 300;

/// Point styling as authored: `radius`, `color`, `stroke-color`,
/// `stroke-width`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct PointStyle {
    pub radius: Option<f32>,
    pub color: Option<[f32; 4]>,
    pub stroke_color: Option<[f32; 4]>,
    pub stroke_width: Option<f32>,
}

impl PointStyle {
    fn parse(text: &str) -> Self {
        let mut style = Self::default();
        for decl in text.split(',') {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            let value = value.trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "radius" => style.radius = kit::number(value).map(|r| r.clamp(0.0, 60.0) as f32),
                "color" => style.color = color::parse(value),
                "stroke-color" => style.stroke_color = color::parse(value),
                "stroke-width" => {
                    style.stroke_width =
                        kit::number(value.trim_end_matches("px")).map(|w| w.clamp(0.0, 12.0) as f32)
                }
                _ => {}
            }
        }
        style
    }

    /// `self` wins wherever it says something.
    fn over(self, base: Self) -> Self {
        Self {
            radius: self.radius.or(base.radius),
            color: self.color.or(base.color),
            stroke_color: self.stroke_color.or(base.stroke_color),
            stroke_width: self.stroke_width.or(base.stroke_width),
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Point {
    pub label: Label,
    pub x: f32,
    pub y: f32,
    pub style: PointStyle,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// Left and right captions.
    pub x_axis: [Option<Label>; 2],
    /// Bottom and top captions.
    pub y_axis: [Option<Label>; 2],
    /// Quadrants 1 (top right) to 4 (bottom right), counter-clockwise.
    pub quadrants: [Option<Label>; 4],
    pub points: Vec<Point>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        cx.warn(header, "unexpected text after `quadrantChart`");
    }
    let mut model = Model::default();
    let mut classes: HashMap<String, PointStyle> = HashMap::new();
    let mut pending: Vec<(Point, Vec<String>)> = Vec::new();
    for (line, stmt) in kit::statements(body, header) {
        if let Some(title) = keyword(stmt, "title") {
            let title = kit::unquote(title).trim();
            if !title.is_empty() {
                cx.meta.title = Some(title.to_string());
            }
        } else if let Some(axis) = keyword(stmt, "x-axis") {
            model.x_axis = axis_pair(cx, axis)?;
        } else if let Some(axis) = keyword(stmt, "y-axis") {
            model.y_axis = axis_pair(cx, axis)?;
        } else if let Some(n) = quadrant_number(stmt) {
            let text = stmt["quadrant-1".len()..].trim();
            model.quadrants[n] = label(cx, text)?;
        } else if let Some(def) = keyword(stmt, "classDef") {
            let (name, decls) = def.split_once(char::is_whitespace).unwrap_or((def, ""));
            classes.insert(name.to_string(), PointStyle::parse(decls));
        } else if let Some((point, names)) = point(cx, stmt)? {
            cx.limits
                .check_count(pending.len() + 1, cx.limits.elements, LimitKind::Elements)?;
            pending.push((point, names));
        } else {
            cx.warn(line, format!("unreadable quadrant statement `{stmt}`"));
        }
    }
    for (mut point, names) in pending {
        for name in names.iter().rev() {
            if let Some(class) = classes.get(name) {
                point.style = point.style.over(*class);
            }
        }
        model.points.push(point);
    }
    Ok(model)
}

fn quadrant_number(stmt: &str) -> Option<usize> {
    let rest = stmt.strip_prefix("quadrant-")?;
    let digit = rest.chars().next()?.to_digit(10)?;
    let after = &rest[1..];
    ((1..=4).contains(&digit) && (after.is_empty() || after.starts_with(char::is_whitespace)))
        .then_some(digit as usize - 1)
}

fn label(cx: &ParseCx<'_>, text: &str) -> Result<Option<Label>, DiagramError> {
    let label = cx.label(text.trim())?;
    Ok((!label.is_empty()).then_some(label))
}

fn axis_pair(cx: &ParseCx<'_>, text: &str) -> Result<[Option<Label>; 2], DiagramError> {
    match text.split_once("-->") {
        Some((a, b)) => Ok([label(cx, a)?, label(cx, b)?]),
        None => Ok([label(cx, text)?, None]),
    }
}

/// `Name[:::class]: [x, y] style...`.
fn point(cx: &mut ParseCx<'_>, stmt: &str) -> Result<Option<(Point, Vec<String>)>, DiagramError> {
    // The coordinates follow the last `:` before the first `[` outside quotes.
    let Some(open) = find_unquoted(stmt, '[') else {
        return Ok(None);
    };
    let Some(head) = stmt[..open].trim_end().strip_suffix(':') else {
        return Ok(None);
    };
    let Some(close) = stmt[open..].find(']').map(|i| i + open) else {
        return Ok(None);
    };
    let Some((x, y)) = kit::pair(&stmt[open..=close]) else {
        return Ok(None);
    };
    let (name, class_list) = match head.find(":::") {
        Some(i) if !head.trim_start().starts_with('"') || head[..i].trim_end().ends_with('"') => {
            (&head[..i], Some(&head[i + 3..]))
        }
        _ => (head, None),
    };
    let names: Vec<String> = class_list
        .map(|c| {
            c.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    let label = cx.label(name.trim())?;
    let style = PointStyle::parse(&stmt[close + 1..]);
    let clamp = |v: f64| v.clamp(0.0, 1.0) as f32;
    Ok(Some((
        Point {
            label,
            x: clamp(x),
            y: clamp(y),
            style,
        },
        names,
    )))
}

fn find_unquoted(s: &str, needle: char) -> Option<usize> {
    let mut quote = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            quote = !quote;
        } else if c == needle && !quote {
            return Some(i);
        }
    }
    None
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let strong = font.weight(m.strong_weight);
    let side = m.font_size * 30.0;
    let half = side / 2.0;
    let pad = m.font_size * 0.6;
    let mut out = SceneBuilder::new();

    // Quadrant washes: 2 | 1 over 3 | 4.
    let origins = [[half, 0.0], [0.0, 0.0], [0.0, half], [half, half]];
    for (q, o) in origins.iter().enumerate() {
        out.fill(
            Geom::rect(Rect::new(o[0], o[1], half, half)),
            style::series(q, Tone::Soft),
        );
    }
    let mut cross = PathData::new();
    cross
        .move_to([half, 0.0])
        .line_to([half, side])
        .move_to([0.0, half])
        .line_to([side, half]);
    out.stroke(
        Geom::path(cross),
        Stroke::new(style::NODE_FILL, StrokeWidth::Thick),
    );
    out.stroke(
        Geom::rect(Rect::new(0.0, 0.0, side, side)),
        Stroke::hairline(style::AXIS),
    );

    // Quadrant titles: centred, or along the top edge once points share
    // the space.
    let has_points = !model.points.is_empty();
    let mut taken: Vec<Rect> = Vec::new();
    for (q, o) in origins.iter().enumerate() {
        let Some(title) = &model.quadrants[q] else {
            continue;
        };
        let laid = cx.text.lay(title, strong, Some(half - 2.0 * pad));
        let center = if has_points {
            [o[0] + half / 2.0, o[1] + pad + laid.height / 2.0]
        } else {
            [o[0] + half / 2.0, o[1] + half / 2.0]
        };
        out.text_centered(&laid, center, style::TEXT);
        taken.push(Rect::centered(center, laid.size()));
    }

    // Axis captions: x under each half, y left of each half.
    for (i, l) in model.x_axis.iter().enumerate() {
        if let Some(l) = l {
            let laid = cx.text.lay(l, small, Some(half - 4.0 * pad));
            let cxp = half * i as f32 + half / 2.0;
            out.text(
                &laid,
                [cxp - laid.width / 2.0, side + pad * 0.6],
                laid.width,
                Align::Center,
                style::MUTED,
            );
        }
    }
    let y_wrap = m.font_size * 9.0;
    for (i, l) in model.y_axis.iter().enumerate() {
        if let Some(l) = l {
            let laid = cx.text.lay(l, small, Some(y_wrap));
            // Bottom caption on the lower half, top caption on the upper.
            let cy = if i == 0 {
                half + half / 2.0
            } else {
                half / 2.0
            };
            let x = -pad * 0.8 - laid.width;
            out.text(
                &laid,
                [x, cy - laid.height / 2.0],
                laid.width,
                Align::End,
                style::MUTED,
            );
        }
    }
    if model.x_axis.iter().all(Option::is_some) {
        // A small arrow between the two captions reads as "low → high".
        let y = side + pad * 0.6 + cx.text.line_advance(small) / 2.0;
        arrow(
            &mut out,
            [half - pad * 1.5, y],
            [half + pad * 1.5, y],
            cx.marker_size() * 0.8,
        );
    }
    if model.y_axis.iter().all(Option::is_some) {
        let x = -pad * 0.4;
        arrow(
            &mut out,
            [x, half + pad * 1.5],
            [x, half - pad * 1.5],
            cx.marker_size() * 0.8,
        );
    }

    // Points over everything; each label takes the first free spot of
    // below, above, right and left of its point.
    let default_r = m.font_size * 0.38;
    let radii: Vec<f32> = model
        .points
        .iter()
        .map(|p| p.style.radius.unwrap_or(default_r).max(1.0))
        .collect();
    let centers: Vec<[f32; 2]> = model
        .points
        .iter()
        .map(|p| [p.x * side, (1.0 - p.y) * side])
        .collect();
    for (i, p) in model.points.iter().enumerate() {
        let (at, r) = (centers[i], radii[i]);
        let fill = p
            .style
            .color
            .map_or(style::series(0, Tone::Solid), Ink::Rgba);
        let stroke = match (p.style.stroke_color, p.style.stroke_width) {
            (None, None) => Stroke::hairline(style::NODE_FILL),
            (c, w) => Stroke::new(
                c.map_or(style::NODE_FILL, Ink::Rgba),
                w.map_or(StrokeWidth::Hairline, StrokeWidth::Px),
            ),
        };
        let disc = Rect::centered(at, [r * 2.0, r * 2.0]);
        out.shape(Geom::Ellipse(disc), Some(fill), Some(stroke));
        taken.push(disc);
    }
    for (i, p) in model.points.iter().enumerate() {
        let laid = cx.text.lay(&p.label, small, Some(half - pad));
        if laid.is_empty() {
            continue;
        }
        let (at, r) = (centers[i], radii[i] + 2.0);
        let [w, h] = laid.size();
        let fit = |x: f32, y: f32| {
            Rect::new(
                x.clamp(2.0, (side - w - 2.0).max(2.0)),
                y.clamp(2.0, (side - h - 2.0).max(2.0)),
                w,
                h,
            )
        };
        let mut candidates = vec![
            fit(at[0] - w / 2.0, at[1] + r),
            fit(at[0] - w / 2.0, at[1] - r - h),
            fit(at[0] + r + 2.0, at[1] - h / 2.0),
            fit(at[0] - r - 2.0 - w, at[1] - h / 2.0),
        ];
        // Crowded corners: step further away, down or up, before giving in.
        for k in 1..=4 {
            let step = k as f32 * h;
            candidates.push(fit(at[0] - w / 2.0, at[1] + r + step));
            candidates.push(fit(at[0] - w / 2.0, at[1] - r - h - step));
        }
        let free = |c: &Rect| {
            !taken
                .iter()
                .any(|t| t.inset(0.5, 0.5).intersects(&c.inset(0.5, 0.5)))
        };
        // Past a few hundred points the chart is a cloud; skip the search.
        let rect = if model.points.len() > AVOID_LIMIT {
            candidates[0]
        } else {
            candidates
                .iter()
                .copied()
                .find(free)
                .unwrap_or(candidates[0])
        };
        out.text(&laid, [rect.x, rect.y], w, Align::Center, style::TEXT);
        taken.push(rect);
    }

    let content = Rect::new(0.0, 0.0, side, side).union(&out.bounds());
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

fn arrow(out: &mut SceneBuilder, from: [f32; 2], to: [f32; 2], size: f32) {
    out.edge(
        &[from, to],
        PathData::polyline,
        Stroke::hairline(style::MUTED),
        None,
        Some(Marker::Arrow),
        size,
    );
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.points.len() as u32,
        edges: 0,
        groups: 4,
    }
}
