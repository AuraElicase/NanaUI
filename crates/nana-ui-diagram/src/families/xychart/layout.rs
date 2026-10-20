//! XY chart drawing. The category axis is a band scale; the value axis a
//! linear scale with round ticks and grid lines. Bars of several series
//! share each band side by side; lines run through the band centres.

use nana_ui_core::RadiusTier;

use super::{Model, Plot, XAxis};
use crate::families::LayoutCx;
use crate::geom::PathData;
use crate::ir::{Align, Geom, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::layout::axis::{BandScale, LinearScale};
use crate::style::{self, series};
use crate::text::{Label, LaidText};
use crate::{DiagramError, DiagramScene};

/// Keep tick labels that clear the previous kept one along the axis:
/// widths on a horizontal axis, heights on a vertical one.
fn thin(ticks: Vec<(f32, LaidText)>, along_x: bool, gap: f32) -> Vec<(f32, LaidText)> {
    let mut out: Vec<(f32, LaidText)> = Vec::with_capacity(ticks.len());
    for (at, laid) in ticks {
        let size = |l: &LaidText| if along_x { l.width } else { l.height };
        let clear = out
            .last()
            .is_none_or(|(prev, p)| (at - prev).abs() >= (size(p) + size(&laid)) / 2.0 + gap / 2.0);
        if clear {
            out.push((at, laid));
        }
    }
    out
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let k = m.font_size / 14.0;
    let small = cx.small_font();
    let font = cx.font();
    let gap = 8.0 * k;
    let mut out = SceneBuilder::new();
    let n = model.points();

    // Value domain.
    let has_bars = model.series.iter().any(|s| s.plot == Plot::Bar);
    let values = || {
        model
            .series
            .iter()
            .flat_map(|s| s.values.iter().take(n.max(1)).copied())
    };
    let (lo, hi) = match model.y_range {
        Some((a, b)) => (a.min(b), a.max(b)),
        None => {
            let (mut lo, mut hi) = values()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                    (l.min(v), h.max(v))
                });
            if !lo.is_finite() {
                (lo, hi) = (0.0, 1.0);
            }
            if has_bars {
                lo = lo.min(0.0);
                hi = hi.max(0.0);
            }
            (lo, hi)
        }
    };

    // Category labels along the band axis.
    let categories: Vec<Option<&Label>> = (0..n)
        .map(|i| match &model.x {
            XAxis::Categories(c) => c.get(i),
            _ => None,
        })
        .collect();
    let horizontal = model.horizontal;

    // Sizes: bands wide enough for their labels.
    let natural: Vec<LaidText> = categories
        .iter()
        .map(|c| {
            c.map_or_else(
                || cx.text.lay_plain("", small),
                |l| cx.text.lay(l, small, None),
            )
        })
        .collect();
    let (band_len, plot_w, plot_h) = if horizontal {
        let label_h = natural.iter().map(|l| l.height).fold(0.0_f32, f32::max);
        let band = (label_h + 10.0 * k).max(26.0 * k);
        let along = (band * n as f32).max(240.0 * k);
        (along / n.max(1) as f32, 480.0 * k, along)
    } else {
        let widest = natural.iter().map(|l| l.width).fold(0.0_f32, f32::max);
        let band = (widest + 10.0 * k).clamp(28.0 * k, 110.0 * k);
        let along = (band * n as f32).max(420.0 * k);
        (along / n.max(1) as f32, along, 300.0 * k)
    };
    let label_wrap = if horizontal {
        140.0 * k
    } else {
        (band_len - 6.0 * k).max(20.0 * k)
    };
    let labels: Vec<LaidText> = categories
        .iter()
        .map(|c| {
            c.map_or_else(
                || cx.text.lay_plain("", small),
                |l| cx.text.lay(l, small, Some(label_wrap)),
            )
        })
        .collect();

    let value_len = if horizontal { plot_w } else { plot_h };
    let count = ((value_len / (48.0 * k)) as usize).clamp(2, 10);
    let value_range = if horizontal {
        [0.0, plot_w]
    } else {
        [plot_h, 0.0]
    };
    let scale = match model.y_range {
        Some(_) => LinearScale::fixed(lo, hi, count, value_range),
        None => LinearScale::nice(lo, hi, count, value_range),
    };
    let band = BandScale {
        count: n.max(1),
        range: if horizontal {
            [0.0, plot_h]
        } else {
            [0.0, plot_w]
        },
        padding: 0.3,
    };

    // A numeric x axis: points spread evenly over min..max, ticks from a
    // linear scale through the band centres.
    let x_scale = if let XAxis::Range(a, b) = model.x {
        let ends = [band.center(0), band.center(n.max(1) - 1)];
        let along = (ends[1] - ends[0]).abs();
        let count = ((along / (60.0 * k)) as usize).clamp(1, 10);
        // Round ticks inside the author's range, without its odd ends.
        let round = LinearScale::nice(a, b, count, ends);
        let mut scale = LinearScale::fixed(a, b, count, ends);
        let (lo, hi) = (a.min(b), a.max(b));
        scale.ticks.ticks = round
            .ticks
            .ticks
            .into_iter()
            .filter(|t| (lo..=hi).contains(t))
            .collect();
        scale.ticks.step = round.ticks.step;
        Some(scale)
    } else {
        None
    };

    // Value tick labels.
    let value_labels: Vec<(f32, LaidText)> = scale
        .ticks
        .ticks
        .iter()
        .map(|&t| (scale.map(t), cx.text.lay_plain(&scale.label(t), small)))
        .collect();
    let x_ticks: Vec<(f32, LaidText)> = match (&x_scale, &model.x) {
        (Some(s), _) if n > 0 => s
            .ticks
            .ticks
            .iter()
            .map(|&t| (s.map(t), cx.text.lay_plain(&s.label(t), small)))
            .collect(),
        // Unlabelled points are numbered from 1.
        (None, XAxis::Auto) => (0..n)
            .map(|i| {
                (
                    band.center(i),
                    cx.text.lay_plain(&(i + 1).to_string(), small),
                )
            })
            .collect(),
        _ => Vec::new(),
    };

    // Fixed ranges add their end points to the round ticks: drop a label
    // that would touch the one before it.
    let value_labels = thin(value_labels, horizontal, gap);
    let x_ticks = thin(x_ticks, !horizontal, gap);

    // Titles and legend above the plot.
    let x_title = model
        .x_title
        .as_ref()
        .map(|t| cx.text.lay(t, font, Some(plot_w.max(plot_h))));
    let y_title = model
        .y_title
        .as_ref()
        .map(|t| cx.text.lay(t, font, Some(plot_w.max(plot_h))));
    // Vertical: y (value) title above the left axis, x (category) title below.
    // Horizontal: x (category) title above the left axis, y (value) below.
    let (top_title, bottom_title) = if horizontal {
        (x_title, y_title)
    } else {
        (y_title, x_title)
    };

    let swatch = 12.0 * k;
    let legend: Vec<(usize, LaidText)> = model
        .series
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.name
                .as_ref()
                .map(|name| (i, cx.text.lay(name, small, Some(160.0 * k))))
        })
        .filter(|(_, l)| !l.is_empty())
        .collect();

    let mut y = 0.0;
    if !legend.is_empty() {
        let mut x = 0.0;
        let row = legend.iter().map(|(_, l)| l.height).fold(swatch, f32::max);
        for (i, laid) in &legend {
            let mid = y + row / 2.0;
            match model.series[*i].plot {
                Plot::Bar => out.fill(
                    Geom::rounded(
                        Rect::new(x, mid - swatch / 2.0, swatch, swatch),
                        RadiusTier::Xs,
                    ),
                    series(*i, Tone::Solid),
                ),
                Plot::Line => out.stroke(
                    Geom::path(PathData::polyline(&[[x, mid], [x + swatch, mid]])),
                    Stroke::new(series(*i, Tone::Solid), StrokeWidth::Thick),
                ),
            }
            out.text(
                laid,
                [x + swatch + gap / 2.0, mid - laid.height / 2.0],
                laid.width,
                Align::Start,
                style::TEXT,
            );
            x += swatch + gap / 2.0 + laid.width + 2.0 * gap;
        }
        y += row + gap;
    }
    if let Some(t) = top_title.as_ref() {
        y += t.height + gap / 2.0;
    }
    let plot_top = y;

    // Left labels decide where the plot starts.
    let left_w = if horizontal {
        labels.iter().map(|l| l.width).fold(0.0_f32, f32::max)
    } else {
        value_labels
            .iter()
            .map(|(_, l)| l.width)
            .fold(0.0_f32, f32::max)
    };
    let plot = Rect::new(left_w + gap, plot_top, plot_w, plot_h);
    let px = |v: f32| plot.x + v;
    let py = |v: f32| plot.y + v;

    if let Some(t) = top_title.as_ref() {
        out.text(
            t,
            [0.0, plot_top - gap / 2.0 - t.height],
            t.width,
            Align::Start,
            style::MUTED,
        );
    }

    // Grid along the value ticks, then the axes.
    let grid = Stroke::hairline(style::GRID);
    let axis = Stroke::hairline(style::AXIS);
    for (v, laid) in &value_labels {
        if horizontal {
            let x = px(*v);
            out.stroke(
                Geom::path(PathData::polyline(&[[x, plot.y], [x, plot.bottom()]])),
                grid,
            );
            out.text(
                laid,
                [x - laid.width / 2.0, plot.bottom() + gap / 2.0],
                laid.width,
                Align::Center,
                style::MUTED,
            );
        } else {
            let yv = py(*v);
            out.stroke(
                Geom::path(PathData::polyline(&[[plot.x, yv], [plot.right(), yv]])),
                grid,
            );
            out.text(
                laid,
                [plot.x - gap - laid.width, yv - laid.height / 2.0],
                laid.width,
                Align::End,
                style::MUTED,
            );
        }
    }
    out.stroke(
        Geom::path(PathData::polyline(&[
            [plot.x, plot.y],
            [plot.x, plot.bottom()],
            [plot.right(), plot.bottom()],
        ])),
        axis,
    );

    // Category or numeric labels on the band axis.
    let tick = 4.0 * k;
    let mut band_axis_extent: f32 = 0.0;
    // Category labels centre in a wrap-wide box; numeric ones in their own.
    let band_labels: Vec<(f32, &LaidText, f32)> = if x_ticks.is_empty() {
        labels
            .iter()
            .enumerate()
            .map(|(i, l)| (band.center(i), l, l.width.max(label_wrap)))
            .collect()
    } else {
        x_ticks.iter().map(|(v, l)| (*v, l, l.width)).collect()
    };
    for (v, laid, w) in band_labels {
        if horizontal {
            let yc = py(v);
            out.stroke(
                Geom::path(PathData::polyline(&[[plot.x - tick, yc], [plot.x, yc]])),
                axis,
            );
            out.text(
                laid,
                [plot.x - gap - laid.width, yc - laid.height / 2.0],
                laid.width,
                Align::End,
                style::MUTED,
            );
        } else {
            let xc = px(v);
            out.stroke(
                Geom::path(PathData::polyline(&[
                    [xc, plot.bottom()],
                    [xc, plot.bottom() + tick],
                ])),
                axis,
            );
            out.text(
                laid,
                [xc - w / 2.0, plot.bottom() + tick + 2.0 * k],
                w,
                Align::Center,
                style::MUTED,
            );
            band_axis_extent = band_axis_extent.max(laid.height);
        }
    }

    // Bars, grouped within each band.
    let (lo_d, hi_d) = (scale.ticks.min, scale.ticks.max);
    let clamp = |v: f64| v.clamp(lo_d.min(hi_d), lo_d.max(hi_d));
    let base = scale.map(clamp(0.0));
    let bar_series: Vec<usize> = (0..model.series.len())
        .filter(|&i| model.series[i].plot == Plot::Bar)
        .collect();
    for (slot, &si) in bar_series.iter().enumerate() {
        for (i, &v) in model.series[si].values.iter().take(n).enumerate() {
            let (start, width) = band.slot(i);
            let w = width / bar_series.len() as f32;
            let a = start + slot as f32 * w;
            let end = scale.map(clamp(v));
            let (v0, v1) = (base.min(end), base.max(end));
            let rect = if horizontal {
                Rect::new(px(v0), py(a), v1 - v0, w)
            } else {
                Rect::new(px(a), py(v0), w, v1 - v0)
            };
            out.fill(Geom::rounded(rect, RadiusTier::Xs), series(si, Tone::Solid));
            if model.data_labels {
                let laid = cx.text.lay_plain(&scale.label(v), small);
                if horizontal {
                    out.text(
                        &laid,
                        [
                            rect.right() + 3.0 * k,
                            rect.y + (rect.h - laid.height) / 2.0,
                        ],
                        laid.width,
                        Align::Start,
                        style::TEXT,
                    );
                } else {
                    out.text(
                        &laid,
                        [rect.x + (rect.w - laid.width) / 2.0, rect.y - laid.height],
                        laid.width,
                        Align::Center,
                        style::TEXT,
                    );
                }
            }
        }
    }

    // Lines through the band centres.
    for (si, s) in model
        .series
        .iter()
        .enumerate()
        .filter(|(_, s)| s.plot == Plot::Line)
    {
        let points: Vec<[f32; 2]> = s
            .values
            .iter()
            .take(n)
            .enumerate()
            .map(|(i, &v)| {
                let c = band.center(i);
                let val = scale.map(clamp(v));
                if horizontal {
                    [px(val), py(c)]
                } else {
                    [px(c), py(val)]
                }
            })
            .collect();
        if points.len() > 1 {
            out.stroke(
                Geom::path(PathData::polyline(&points)),
                Stroke::new(series(si, Tone::Solid), StrokeWidth::Thick),
            );
        }
        let r = 3.0 * k;
        for p in &points {
            out.shape(
                Geom::Ellipse(Rect::centered(*p, [2.0 * r, 2.0 * r])),
                Some(series(si, Tone::Solid)),
                Some(Stroke::new(style::NODE_FILL, StrokeWidth::Px(1.0 * k))),
            );
        }
    }

    // The title under the bottom axis.
    if let Some(t) = bottom_title.as_ref() {
        let below = if horizontal {
            value_labels
                .iter()
                .map(|(_, l)| l.height)
                .fold(0.0_f32, f32::max)
                + gap / 2.0
        } else {
            band_axis_extent + tick + 2.0 * k
        };
        let top = plot.bottom() + below + gap;
        out.text(t, [plot.x, top], plot.w, Align::Center, style::MUTED);
    }

    let content = out.bounds().union(&plot);
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}
