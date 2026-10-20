//! Gantt drawing: a section column, one row per task (or packed rows in
//! compact mode), a calendar axis from the chart crate's time ticks and
//! status-coloured bars.

use nana_ui_charts::scale::time_ticks;
use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use super::time::{self, DAY};
use super::{Model, Task};
use crate::families::LayoutCx;
use crate::geom::PathData;
use crate::ir::{Align, Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene, style};

const DEFAULT_AXIS_FORMAT: &str = "%Y-%m-%d";

/// Section bands alternate between these.
const BAND: [Ink; 2] = [
    Ink::Mix(SemanticColorMix::alpha(R::Subtle, 0.95)),
    Ink::Mix(SemanticColorMix::alpha(R::Subtle, 0.4)),
];
const EXCLUDED: Ink = Ink::Mix(SemanticColorMix::alpha(R::Border, 0.3));

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let k = m.font_size / 14.0;
    let font = cx.font();
    let small = cx.small_font();
    let mut out = SceneBuilder::new();

    let bars: Vec<usize> = (0..model.tasks.len()).filter(|&i| !model.tasks[i].vert).collect();
    let verts: Vec<usize> = (0..model.tasks.len()).filter(|&i| model.tasks[i].vert).collect();
    if model.tasks.is_empty() {
        let content = cx.title_above(&mut out, Rect::default());
        return Ok(out.finish(content, m.margin));
    }

    // Rows: one per bar, or packed per section in compact mode.
    let mut row_of = vec![0usize; model.tasks.len()];
    let mut section_rows = vec![0usize; model.sections.len()];
    for (s, rows) in section_rows.iter_mut().enumerate() {
        let mut ends: Vec<i64> = Vec::new();
        for &i in bars.iter().filter(|&&i| model.tasks[i].section == s) {
            let task = &model.tasks[i];
            let row = if model.compact {
                match ends.iter().position(|&e| e <= task.start) {
                    Some(r) => r,
                    None => {
                        ends.push(task.end);
                        ends.len() - 1
                    }
                }
            } else {
                ends.push(task.end);
                ends.len() - 1
            };
            ends[row] = ends[row].max(task.end);
            row_of[i] = row;
        }
        *rows = ends.len();
    }

    // Section titles and the column they sit in.
    let title_font = FontSpec::new(m.font_size).weight(m.strong_weight);
    let titles: Vec<LaidText> = model
        .sections
        .iter()
        .map(|s| cx.text.lay(s, title_font, Some(160.0 * k)))
        .collect();
    let pad = 12.0 * k;
    let column = titles
        .iter()
        .zip(&section_rows)
        .filter(|(t, rows)| !t.is_empty() && **rows > 0)
        .map(|(t, _)| t.width + 2.0 * pad)
        .fold(0.0_f32, f32::max);

    // The time domain.
    let mut t_min = i64::MAX;
    let mut t_max = i64::MIN;
    for task in &model.tasks {
        t_min = t_min.min(task.start);
        t_max = t_max.max(if task.vert { task.start } else { task.end });
    }
    if let Some(today) = model.today {
        t_min = t_min.min(today);
        t_max = t_max.max(today);
    }
    if t_max <= t_min {
        t_max = t_min.saturating_add(DAY);
    }
    let span = (t_max as f64) - (t_min as f64);

    // Wide enough that the shortest bar still reads as a bar.
    let shortest = bars
        .iter()
        .map(|&i| &model.tasks[i])
        .filter(|t| !t.milestone && t.end > t.start)
        .map(|t| (t.end - t.start) as f64)
        .fold(f64::INFINITY, f64::min);
    let base = 640.0 * k;
    let need = if shortest.is_finite() { (28.0 * k) as f64 * span / shortest } else { 0.0 };
    let width = (need as f32).clamp(base, 2_000.0 * k);
    let x0 = column;
    let x_of = |t: i64| x0 + (((t as f64) - (t_min as f64)) / span * f64::from(width)) as f32;

    // Axis ticks and their labels.
    let format = model.axis_format.as_deref().unwrap_or(DEFAULT_AXIS_FORMAT);
    let ticks = ticks(model, cx, small, format, t_min, t_max, width, k);
    let tick_labels: Vec<LaidText> = ticks
        .iter()
        .map(|&t| cx.text.lay_plain(&time::format(t, format), small))
        .collect();
    let label_h = cx.text.line_advance(small);

    let bar_h = cx.text.line_advance(font) + 6.0 * k;
    let row_h = bar_h + 8.0 * k;
    let gap = 6.0 * k;

    // Vertical strips from the top: vert labels, top axis, rows, bottom axis.
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let mut vert_chips: Vec<(usize, LaidText, f32, usize)> = Vec::new();
    let mut levels: Vec<f32> = Vec::new();
    for &i in &verts {
        let laid = cx.text.lay(&model.tasks[i].name, small, Some(160.0 * k));
        if laid.is_empty() {
            continue;
        }
        let x = x_of(model.tasks[i].start);
        let half = laid.width / 2.0 + chip_pad[0];
        let level = match levels.iter().position(|&right| right + gap <= x - half) {
            Some(l) => l,
            None => {
                levels.push(f32::NEG_INFINITY);
                levels.len() - 1
            }
        };
        levels[level] = x + half;
        vert_chips.push((i, laid, x, level));
    }
    let chip_h = vert_chips
        .iter()
        .map(|(_, l, ..)| l.height + 2.0 * chip_pad[1])
        .fold(0.0_f32, f32::max);
    let mut y = 0.0;
    let strip_top = y;
    if !levels.is_empty() {
        y += levels.len() as f32 * (chip_h + gap);
    }
    let top_axis_y = y + label_h + gap;
    if model.top_axis {
        y = top_axis_y;
    }
    let rows_top = y;

    // Task labels, inside their bar when it fits, else to its right; the
    // bands reach past the furthest outside label.
    let label_gap = 6.0 * k;
    let mut names: Vec<Option<(LaidText, bool)>> = vec![None; model.tasks.len()];
    let mut reach = x0 + width;
    for &i in &bars {
        let task = &model.tasks[i];
        let laid = cx.text.lay(&task.name, font, None);
        if laid.is_empty() {
            continue;
        }
        let (left, right) = bar_span(task, &x_of, bar_h, k);
        let inside = !task.milestone && laid.width + 2.0 * label_gap <= right - left;
        if !inside {
            reach = reach.max(right + label_gap + laid.width + pad);
        }
        names[i] = Some((laid, inside));
    }

    // Section bands.
    let full = Rect::new(0.0, 0.0, reach, 0.0);
    let mut band_rects = Vec::new();
    let mut index = 0;
    for (s, title) in titles.iter().enumerate() {
        if section_rows[s] == 0 {
            band_rects.push(None);
            continue;
        }
        let h = (section_rows[s] as f32 * row_h).max(title.height + 2.0 * gap);
        let rect = Rect::new(full.x, y, full.w, h);
        out.fill(Geom::rect(rect), BAND[index % 2]);
        band_rects.push(Some(rect));
        index += 1;
        y += h;
    }
    let rows_bottom = y;
    let rows_h = rows_bottom - rows_top;

    // Excluded days, merged into runs.
    if !model.excludes.is_empty() && span <= (3_660 * DAY) as f64 {
        let mut day = time::day_start(t_min);
        let mut run: Option<i64> = None;
        while day <= t_max {
            let excluded = model.excludes.excludes(day);
            match (excluded, run) {
                (true, None) => run = Some(day),
                (false, Some(from)) => {
                    shade(&mut out, x_of(from.max(t_min)), x_of(day.min(t_max)), rows_top, rows_h);
                    run = None;
                }
                _ => {}
            }
            day += DAY;
        }
        if let Some(from) = run {
            shade(&mut out, x_of(from.max(t_min)), x_of(day.min(t_max)), rows_top, rows_h);
        }
    }

    // Grid, axis and tick labels.
    let grid = Stroke::hairline(style::GRID);
    for &t in &ticks {
        let x = x_of(t);
        out.stroke(Geom::path(PathData::polyline(&[[x, rows_top], [x, rows_bottom]])), grid);
    }
    let axis_y = if model.top_axis { rows_top } else { rows_bottom };
    let axis = Stroke::hairline(style::AXIS);
    out.stroke(Geom::path(PathData::polyline(&[[x0, axis_y], [x0 + width, axis_y]])), axis);
    let tick_len = 4.0 * k;
    for (&t, laid) in ticks.iter().zip(&tick_labels) {
        let x = x_of(t);
        let (a, b, label_y) = if model.top_axis {
            (axis_y - tick_len, axis_y, axis_y - tick_len - laid.height)
        } else {
            (axis_y, axis_y + tick_len, axis_y + tick_len + 2.0 * k)
        };
        out.stroke(Geom::path(PathData::polyline(&[[x, a], [x, b]])), axis);
        out.text(laid, [x - laid.width / 2.0, label_y], laid.width, Align::Center, style::MUTED);
    }

    // Section titles.
    for (s, title) in titles.iter().enumerate() {
        if let Some(rect) = band_rects[s]
            && !title.is_empty()
        {
            let top = rect.y + (rect.h - title.height) / 2.0;
            out.text(title, [pad, top], column - 2.0 * pad, Align::Start, style::TEXT);
        }
    }

    // "Today", only when the source names the date.
    if let Some(today) = model.today {
        let x = x_of(today);
        out.stroke(
            Geom::path(PathData::polyline(&[[x, rows_top], [x, rows_bottom]])),
            Stroke::new(style::DANGER, StrokeWidth::Thick),
        );
    }

    // Bars and milestones.
    let mut band_top = vec![0.0_f32; model.sections.len()];
    for (s, rect) in band_rects.iter().enumerate() {
        band_top[s] = rect.map_or(0.0, |r| r.y);
    }
    for &i in &bars {
        let task = &model.tasks[i];
        let cy = band_top[task.section] + row_of[i] as f32 * row_h + row_h / 2.0;
        let paint = paint(task);
        let (left, right) = bar_span(task, &x_of, bar_h, k);
        let shape = if task.milestone {
            let h = (right - left) / 2.0;
            let c = left + h;
            let diamond = PathData::polygon(&[[c, cy - h], [c + h, cy], [c, cy + h], [c - h, cy]]);
            out.fill(Geom::path(diamond), paint.solid);
            Rect::new(left, cy - h, right - left, 2.0 * h)
        } else {
            let rect = Rect::new(left, cy - bar_h / 2.0, right - left, bar_h);
            out.shape(Geom::rounded(rect, RadiusTier::Xs), Some(paint.fill), Some(paint.stroke));
            rect
        };
        match &names[i] {
            Some((laid, true)) => out.text_centered(laid, shape.center(), style::TEXT),
            Some((laid, false)) => {
                let origin = [shape.right() + label_gap, cy - laid.height / 2.0];
                out.text(laid, origin, laid.width, Align::Start, style::TEXT);
            }
            None => {}
        }
        if let Some(url) = &task.link {
            out.link(shape, url, None);
        }
    }

    // Vertical markers over everything else.
    for (i, laid, x, level) in &vert_chips {
        let top = strip_top + *level as f32 * (chip_h + gap);
        out.stroke(
            Geom::path(PathData::polyline(&[[*x, top + chip_h], [*x, rows_bottom]])),
            Stroke::hairline(style::EDGE).dash(Dash::Dashed),
        );
        out.chip(laid, [*x, top + chip_h / 2.0], chip_pad);
        if let Some(url) = &model.tasks[*i].link {
            out.link(Rect::centered([*x, top + chip_h / 2.0], [laid.width, chip_h]), url, None);
        }
    }
    // A vert without a label still marks its date.
    for &i in &verts {
        if vert_chips.iter().any(|(j, ..)| *j == i) {
            continue;
        }
        let x = x_of(model.tasks[i].start);
        out.stroke(
            Geom::path(PathData::polyline(&[[x, rows_top], [x, rows_bottom]])),
            Stroke::hairline(style::EDGE).dash(Dash::Dashed),
        );
    }

    let content = out.bounds().union(&Rect::new(0.0, 0.0, reach, rows_bottom));
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// The drawn left and right edge of a bar or milestone.
fn bar_span(task: &Task, x_of: &impl Fn(i64) -> f32, bar_h: f32, k: f32) -> (f32, f32) {
    if task.milestone {
        let c = (x_of(task.start) + x_of(task.end)) / 2.0;
        let h = bar_h * 0.36;
        return (c - h, c + h);
    }
    let x = x_of(task.start);
    (x, x + (x_of(task.end) - x).max(3.0 * k))
}

fn shade(out: &mut SceneBuilder, a: f32, b: f32, top: f32, h: f32) {
    if b - a > 0.05 {
        out.fill(Geom::rect(Rect::new(a, top, b - a, h)), EXCLUDED);
    }
}

struct Paint {
    fill: Ink,
    stroke: Stroke,
    /// Milestones.
    solid: Ink,
}

fn paint(task: &Task) -> Paint {
    let series = task.section.min(u16::MAX as usize) as u16;
    let (fill, line, solid) = if task.done {
        (
            Ink::Mix(SemanticColorMix::alpha(R::Muted, 0.2)),
            Ink::Mix(SemanticColorMix::alpha(R::Muted, 0.6)),
            style::MUTED,
        )
    } else if task.active {
        (Ink::Mix(SemanticColorMix::alpha(R::Accent, 0.22)), style::ACCENT, style::ACCENT)
    } else if task.crit {
        (Ink::Mix(SemanticColorMix::alpha(R::Danger, 0.18)), style::DANGER, style::DANGER)
    } else {
        (
            Ink::Series {
                index: series,
                tone: Tone::Soft,
            },
            Ink::Series {
                index: series,
                tone: Tone::Solid,
            },
            Ink::Series {
                index: series,
                tone: Tone::Solid,
            },
        )
    };
    let stroke = if task.crit {
        Stroke::new(style::DANGER, StrokeWidth::Thick)
    } else {
        Stroke::hairline(line)
    };
    Paint {
        fill,
        stroke,
        solid: if task.crit { style::DANGER } else { solid },
    }
}

/// Tick instants: the author's `tickInterval` when it fits, else calendar
/// ticks from the chart crate; thinned until labels no longer collide.
#[allow(clippy::too_many_arguments)]
fn ticks(
    model: &Model,
    cx: &LayoutCx<'_>,
    font: FontSpec,
    format: &str,
    t_min: i64,
    t_max: i64,
    width: f32,
    k: f32,
) -> Vec<i64> {
    let label_w = |t: i64| cx.text.width(&time::format(t, format), font);
    let gap = 14.0 * k;
    let mut ticks: Vec<i64> = model
        .tick_interval
        .and_then(|(n, unit)| time::interval_ticks(t_min, t_max, n, unit, model.week_start, 400))
        .filter(|t| !t.is_empty())
        .unwrap_or_default();
    if ticks.is_empty() {
        let sample = label_w(t_min).max(label_w(t_max));
        // Ask for twice what fits: the calendar steps are coarse, and the
        // thinning below brings a crowded axis back down.
        let count = ((width / (sample + gap)).floor() as usize).clamp(1, 16);
        ticks = time_ticks(t_min, t_max, 2 * count, 0).into_iter().map(|t| t.value).collect();
    }
    if ticks.is_empty() {
        ticks.push(t_min);
    }
    // A format coarser than the step names several ticks alike: keep the
    // first of each run.
    let mut last = None;
    ticks.retain(|&t| {
        let label = time::format(t, format);
        let keep = last.as_ref() != Some(&label);
        last = Some(label);
        keep
    });
    let span = (t_max - t_min).max(1) as f64;
    let px = |t: i64| ((t - t_min) as f64 / span * f64::from(width)) as f32;
    let widest = ticks.iter().map(|&t| label_w(t)).fold(0.0_f32, f32::max);
    let closest = ticks
        .windows(2)
        .map(|w| px(w[1]) - px(w[0]))
        .fold(f32::INFINITY, f32::min);
    if closest.is_finite() && closest > 0.0 && widest + gap > closest {
        let stride = ((widest + gap) / closest).ceil().max(1.0) as usize;
        ticks = ticks.into_iter().step_by(stride).collect();
    }
    ticks
}
