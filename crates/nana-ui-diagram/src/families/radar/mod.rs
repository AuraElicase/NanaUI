//! Radar charts: `radar-beta` / `radar` with `axis` and `curve` lists and
//! the `max`, `min`, `graticule`, `ticks` and `showLegend` options.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, TAU};

use super::kit::{self, LegendRow, keyword, split_top};
use super::{LayoutCx, ParseCx};
use crate::geom::PathData;
use crate::ir::{Align, Geom, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Graticule {
    Circle,
    Polygon,
}

#[derive(Debug, Clone)]
pub(crate) struct Curve {
    pub label: Label,
    /// One value per axis, in axis order.
    pub values: Vec<f64>,
}

#[derive(Debug, Clone)]
pub(crate) struct Model {
    /// Axis labels, clockwise from twelve o'clock.
    pub axes: Vec<Label>,
    pub curves: Vec<Curve>,
    pub max: Option<f64>,
    pub min: Option<f64>,
    pub graticule: Graticule,
    pub ticks: usize,
    pub show_legend: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            axes: Vec::new(),
            curves: Vec::new(),
            max: None,
            min: None,
            graticule: Graticule::Circle,
            ticks: 5,
            show_legend: true,
        }
    }
}

enum Values {
    List(Vec<f64>),
    Keyed(Vec<(String, f64)>),
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        cx.warn(header, "unexpected text after the radar header");
    }
    let mut model = Model::default();
    let mut ids: HashMap<String, usize> = HashMap::new();
    let mut raw: Vec<(usize, Label, Values)> = Vec::new();
    for (line, stmt) in kit::statements(body, header) {
        if let Some(title) = keyword(stmt, "title") {
            let title = kit::unquote(title).trim();
            if !title.is_empty() {
                cx.meta.title = Some(title.to_string());
            }
        } else if let Some(list) = keyword(stmt, "axis") {
            for item in items(list) {
                let (id, label) = id_label(cx, item)?;
                cx.limits.check_count(
                    model.axes.len() + 1,
                    cx.limits.elements,
                    LimitKind::Elements,
                )?;
                ids.entry(id).or_insert(model.axes.len());
                model.axes.push(label);
            }
        } else if let Some(list) = keyword(stmt, "curve") {
            for item in items(list) {
                let Some(open) = item.find('{') else {
                    cx.warn(line, format!("curve `{item}` has no values"));
                    continue;
                };
                let Some(inner) = item[open + 1..].trim_end().strip_suffix('}') else {
                    cx.warn(line, format!("curve `{item}` is missing `}}`"));
                    continue;
                };
                let (_, label) = id_label(cx, &item[..open])?;
                match values(inner) {
                    Some(v) => {
                        cx.limits.check_count(
                            raw.len() + 1,
                            cx.limits.elements,
                            LimitKind::Elements,
                        )?;
                        raw.push((line, label, v));
                    }
                    None => cx.warn(line, format!("unreadable values in curve `{item}`")),
                }
            }
        } else if let Some(v) = keyword(stmt, "max") {
            model.max = option_number(cx, line, v);
        } else if let Some(v) = keyword(stmt, "min") {
            model.min = option_number(cx, line, v);
        } else if let Some(v) = keyword(stmt, "ticks") {
            if let Some(n) = option_number(cx, line, v) {
                model.ticks = (n.round() as i64).clamp(1, 20) as usize;
            }
        } else if let Some(v) = keyword(stmt, "graticule") {
            match v {
                "circle" => model.graticule = Graticule::Circle,
                "polygon" => model.graticule = Graticule::Polygon,
                _ => cx.warn(line, format!("unknown graticule `{v}`")),
            }
        } else if let Some(v) = keyword(stmt, "showLegend") {
            match v {
                "true" | "" => model.show_legend = true,
                "false" => model.show_legend = false,
                _ => cx.warn(line, format!("showLegend takes true or false, not `{v}`")),
            }
        } else {
            cx.warn(line, format!("unreadable radar statement `{stmt}`"));
        }
    }
    let n = model.axes.len();
    cx.limits
        .check_count(n * raw.len(), cx.limits.elements * 4, LimitKind::Elements)?;
    for (line, label, values) in raw {
        let values = match values {
            Values::List(list) => {
                if list.len() != n {
                    cx.warn(
                        line,
                        format!("curve has {} values for {n} axes", list.len()),
                    );
                }
                (0..n)
                    .map(|i| list.get(i).copied().unwrap_or(0.0))
                    .collect()
            }
            Values::Keyed(pairs) => {
                let mut out = vec![0.0; n];
                for (key, v) in pairs {
                    match ids.get(&key) {
                        Some(&i) => out[i] = v,
                        None => cx.warn(line, format!("no axis `{key}`")),
                    }
                }
                out
            }
        };
        model.curves.push(Curve { label, values });
    }
    Ok(model)
}

/// The non-empty items of a comma list.
fn items(list: &str) -> impl Iterator<Item = &str> {
    split_top(list, ',')
        .into_iter()
        .map(str::trim)
        .filter(|i| !i.is_empty())
}

fn option_number(cx: &mut ParseCx<'_>, line: usize, v: &str) -> Option<f64> {
    let n = kit::number(v);
    if n.is_none() {
        cx.warn(line, format!("expected a number, found `{v}`"));
    }
    n
}

/// `id["Label"]` or `id`.
fn id_label(cx: &ParseCx<'_>, item: &str) -> Result<(String, Label), DiagramError> {
    let item = item.trim();
    if let Some(open) = item.find('[')
        && let Some(inner) = item[open + 1..].trim_end().strip_suffix(']')
    {
        let id = item[..open].trim().to_string();
        return Ok((id, cx.label(inner)?));
    }
    Ok((item.to_string(), cx.label(item)?))
}

fn values(inner: &str) -> Option<Values> {
    let parts: Vec<&str> = inner
        .split(',')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.iter().any(|p| p.contains(':')) {
        let mut out = Vec::with_capacity(parts.len());
        for p in parts {
            let (k, v) = p.split_once(':')?;
            out.push((k.trim().to_string(), kit::number(v)?));
        }
        Some(Values::Keyed(out))
    } else {
        parts
            .into_iter()
            .map(kit::number)
            .collect::<Option<Vec<_>>>()
            .map(Values::List)
    }
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let small = cx.small_font();
    let radius = m.font_size * 9.5;
    let center = [0.0f32, 0.0f32];
    let n = model.axes.len();
    let mut out = SceneBuilder::new();

    let data_max = model
        .curves
        .iter()
        .flat_map(|c| c.values.iter().copied())
        .fold(f64::NEG_INFINITY, f64::max);
    let min = model.min.unwrap_or(0.0);
    let mut max = model
        .max
        .unwrap_or(if data_max.is_finite() { data_max } else { 1.0 });
    if max <= min {
        max = min + 1.0;
    }
    let angle = |i: usize| -FRAC_PI_2 + TAU * i as f32 / n.max(1) as f32;
    let at = |i: usize, r: f32| {
        [
            center[0] + r * angle(i).cos(),
            center[1] + r * angle(i).sin(),
        ]
    };

    let grid = |out: &mut SceneBuilder| {
        let stroke = Stroke::hairline(style::GRID);
        for t in 1..=model.ticks {
            let r = radius * t as f32 / model.ticks as f32;
            if model.graticule == Graticule::Polygon && n >= 3 {
                let pts: Vec<[f32; 2]> = (0..n).map(|i| at(i, r)).collect();
                out.stroke(Geom::path(PathData::polygon(&pts)), stroke);
            } else {
                out.stroke(
                    Geom::Ellipse(Rect::centered(center, [r * 2.0, r * 2.0])),
                    stroke,
                );
            }
        }
        let mut spokes = PathData::new();
        for i in 0..n {
            spokes.move_to(center).line_to(at(i, radius));
        }
        if !spokes.is_empty() {
            out.stroke(Geom::path(spokes), Stroke::hairline(style::AXIS));
        }
    };

    // Curves: washes first, largest underneath so every one stays visible,
    // then the grid again over the washes, then the outlines.
    let tension = cx
        .config
        .get_f32("radar.curveTension")
        .unwrap_or(0.17)
        .clamp(0.0, 0.5);
    let shapes: Vec<(f32, Vec<[f32; 2]>)> = model
        .curves
        .iter()
        .map(|c| {
            let pts: Vec<[f32; 2]> = (0..n)
                .map(|i| {
                    let v = ((c.values[i] - min) / (max - min)).clamp(0.0, 1.0) as f32;
                    at(i, radius * v)
                })
                .collect();
            (polygon_area(&pts), pts)
        })
        .collect();
    grid(&mut out);
    let mut order: Vec<usize> = (0..shapes.len()).collect();
    order.sort_by(|&a, &b| shapes[b].0.total_cmp(&shapes[a].0).then(a.cmp(&b)));
    if n >= 3 {
        for &c in &order {
            out.fill(
                Geom::path(closed_curve(&shapes[c].1, tension)),
                style::series(c, Tone::Soft),
            );
        }
        grid(&mut out);
    }
    for (c, (_, pts)) in shapes.iter().enumerate() {
        let stroke = Stroke::new(style::series(c, Tone::Solid), StrokeWidth::Thick);
        if n >= 3 {
            out.stroke(Geom::path(closed_curve(pts, tension)), stroke);
        } else {
            out.stroke(Geom::path(PathData::polyline(pts)), stroke);
        }
        let dot = m.font_size * 0.22;
        for p in pts {
            out.fill(
                Geom::Ellipse(Rect::centered(*p, [dot * 2.0, dot * 2.0])),
                style::series(c, Tone::Solid),
            );
        }
    }

    // Axis labels just outside the rim, anchored by direction.
    let gap = m.font_size * 0.6;
    let wrap = cx
        .wrap_width("radar.axisLabelWidth")
        .min(m.font_size * 10.0);
    // How much of the label sits back toward the centre along one axis.
    let back = |d: f32| {
        if d > 0.3 {
            0.0
        } else if d < -0.3 {
            1.0
        } else {
            0.5
        }
    };
    for (i, label) in model.axes.iter().enumerate() {
        let laid = cx.text.lay(label, small, Some(wrap));
        let a = angle(i);
        let (c, s) = (a.cos(), a.sin());
        let p = [
            center[0] + (radius + gap) * c,
            center[1] + (radius + gap) * s,
        ];
        let align = if c > 0.3 {
            Align::Start
        } else if c < -0.3 {
            Align::End
        } else {
            Align::Center
        };
        let origin = [p[0] - laid.width * back(c), p[1] - laid.height * back(s)];
        out.text(&laid, origin, laid.width, align, style::MUTED);
    }

    let mut content = Rect::centered(center, [radius * 2.0, radius * 2.0]).union(&out.bounds());
    if model.show_legend && !model.curves.is_empty() {
        let font = cx.font();
        let rows: Vec<LegendRow> = model
            .curves
            .iter()
            .enumerate()
            .map(|(i, c)| LegendRow {
                index: i as u16,
                name: cx
                    .text
                    .lay(&c.label, font, Some(cx.wrap_width("radar.legendWidth"))),
                value: None,
            })
            .collect();
        let legend = kit::legend(
            &mut out,
            cx,
            &rows,
            content.right() + m.font_size * 2.0,
            center[1],
        );
        content = content.union(&legend);
    }
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

fn polygon_area(pts: &[[f32; 2]]) -> f32 {
    let n = pts.len();
    (0..n)
        .map(|i| {
            let (a, b) = (pts[i], pts[(i + 1) % n]);
            a[0] * b[1] - b[0] * a[1]
        })
        .sum::<f32>()
        .abs()
        / 2.0
}

/// A closed cardinal spline through `pts`; `tension` 0 is the polygon.
fn closed_curve(pts: &[[f32; 2]], tension: f32) -> PathData {
    let n = pts.len();
    if tension <= 0.001 || n < 3 {
        return PathData::polygon(pts);
    }
    let mut path = PathData::new();
    path.move_to(pts[0]);
    for i in 0..n {
        let p0 = pts[(i + n - 1) % n];
        let p1 = pts[i];
        let p2 = pts[(i + 1) % n];
        let p3 = pts[(i + 2) % n];
        let c1 = [
            p1[0] + (p2[0] - p0[0]) * tension,
            p1[1] + (p2[1] - p0[1]) * tension,
        ];
        let c2 = [
            p2[0] - (p3[0] - p1[0]) * tension,
            p2[1] - (p3[1] - p1[1]) * tension,
        ];
        path.cubic_to(c1, c2, p2);
    }
    path.close();
    path
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.axes.len() as u32,
        edges: 0,
        groups: model.curves.len() as u32,
    }
}
