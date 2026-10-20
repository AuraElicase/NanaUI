//! User journeys: `journey`. Tasks run left to right under their
//! sections; each task's score places a drawn face on a five-level scale
//! below it, and the actors taking part show as series-coloured dots.

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use super::kit::{self, first_word};
use super::{LayoutCx, ParseCx};
use crate::geom::PathData;
use crate::ir::{Align, Cap, Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::style::{self, series};
use crate::text::{FontSpec, LaidText, Label};
use crate::{DiagramError, DiagramScene, LimitKind, Summary};


#[derive(Debug, Clone)]
pub(crate) struct Task {
    pub name: Label,
    pub section: usize,
    /// The written score, clamped to 1..=5.
    pub score: f32,
    /// Indices into `Model::actors`.
    pub actors: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub sections: Vec<Label>,
    pub tasks: Vec<Task>,
    pub actors: Vec<String>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model::default();
    let (header, _) = kit::header(body)?;
    for (index, line) in kit::statements(body, header) {
        let line = line.trim_end_matches(';').trim_end();
        if line.is_empty() {
            continue;
        }
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
        let mut parts = line.splitn(3, ':');
        let name = parts.next().unwrap_or("").trim();
        let Some(score) = parts.next() else {
            cx.warn(index, format!("unknown statement `{word}`"));
            continue;
        };
        let Some(score) = score.trim().parse::<f32>().ok().filter(|s| s.is_finite()) else {
            cx.warn(index, format!("`{}` is not a score", score.trim()));
            continue;
        };
        if model.tasks.len() >= cx.limits.elements {
            return Err(DiagramError::TooLarge(LimitKind::Elements));
        }
        let mut actors = Vec::new();
        for actor in parts.next().unwrap_or("").split(',').map(str::trim).filter(|a| !a.is_empty()) {
            cx.limits.check_label(actor)?;
            let i = match model.actors.iter().position(|a| a == actor) {
                Some(i) => i,
                None => {
                    if model.actors.len() >= cx.limits.elements {
                        return Err(DiagramError::TooLarge(LimitKind::Elements));
                    }
                    model.actors.push(actor.to_string());
                    model.actors.len() - 1
                }
            };
            if !actors.contains(&i) {
                actors.push(i);
            }
        }
        if model.sections.is_empty() {
            model.sections.push(Label::default());
        }
        model.tasks.push(Task {
            name: cx.label(name)?,
            section: model.sections.len() - 1,
            score: score.clamp(1.0, 5.0),
            actors,
        });
    }
    Ok(model)
}

/// Fill and outline of a face by mood.
fn mood(level: u8) -> (Ink, Ink) {
    let role = match level {
        4 | 5 => R::Success,
        3 => R::Muted,
        _ => R::Danger,
    };
    (Ink::Mix(SemanticColorMix::alpha(role, 0.18)), Ink::Role(role))
}

/// A face of radius `r` at `c`: eyes and a mouth for one of five levels.
fn face(out: &mut SceneBuilder, c: [f32; 2], r: f32, level: u8) {
    let (fill, line) = mood(level);
    out.shape(Geom::Ellipse(Rect::centered(c, [2.0 * r, 2.0 * r])), Some(fill), Some(Stroke::hairline(line)));
    let p = |x: f32, y: f32| [c[0] + x * r, c[1] + y * r];
    for x in [-0.36, 0.36] {
        out.fill(Geom::Ellipse(Rect::centered(p(x, -0.22), [0.24 * r, 0.24 * r])), style::TEXT);
    }
    let pen = Stroke::new(style::TEXT, StrokeWidth::Px((r * 0.12).max(1.0))).cap(Cap::Round);
    let mut mouth = PathData::new();
    match level {
        5 => {
            // An open grin, filled.
            mouth.move_to(p(-0.46, 0.14)).quad_to(p(0.0, 0.86), p(0.46, 0.14)).close();
            out.fill(Geom::path(mouth), style::TEXT);
            return;
        }
        4 => {
            mouth.move_to(p(-0.42, 0.24)).quad_to(p(0.0, 0.66), p(0.42, 0.24));
        }
        3 => {
            mouth.move_to(p(-0.36, 0.38)).line_to(p(0.36, 0.38));
        }
        2 => {
            mouth.move_to(p(-0.38, 0.52)).quad_to(p(0.0, 0.26), p(0.38, 0.52));
        }
        _ => {
            mouth.move_to(p(-0.44, 0.6)).quad_to(p(0.0, 0.08), p(0.44, 0.6));
        }
    }
    out.stroke(Geom::path(mouth), pen);
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let k = m.font_size / 14.0;
    let font = cx.font();
    let strong = FontSpec::new(m.font_size).weight(m.strong_weight);
    let pad = [m.node_pad_x * 0.75, m.node_pad_y * 0.8];
    let gap = 10.0 * k;
    let col_gap = 14.0 * k;
    let mut out = SceneBuilder::new();
    if model.tasks.is_empty() {
        let content = cx.title_above(&mut out, Rect::default());
        return Ok(out.finish(content, m.margin));
    }

    // Legend: one series dot per actor.
    let dot = 10.0 * k;
    let names: Vec<LaidText> = model.actors.iter().map(|a| cx.text.lay(&Label::plain(a), font, Some(140.0 * k))).collect();
    let legend_w = names.iter().map(|n| n.width).fold(0.0_f32, f32::max);
    let x0 = if names.is_empty() { 0.0 } else { dot + gap * 0.6 + legend_w + 2.0 * gap };

    // Columns.
    let wrap = 140.0 * k;
    let labels: Vec<LaidText> = model.tasks.iter().map(|t| cx.text.lay(&t.name, font, Some(wrap))).collect();
    let sections: Vec<LaidText> = model.sections.iter().map(|s| cx.text.lay(s, strong, Some(wrap * 2.0))).collect();
    let col_w = labels.iter().map(|l| l.width).fold(0.0_f32, f32::max).max(80.0 * k) + 2.0 * pad[0];
    let col_x = |i: usize| x0 + i as f32 * (col_w + col_gap);

    let mut y = 0.0;
    let has_titles = sections.iter().any(|s| !s.is_empty());
    if has_titles {
        let band_h = sections.iter().map(|s| s.height).fold(cx.text.line_advance(strong), f32::max) + 2.0 * pad[1];
        for (s, laid) in sections.iter().enumerate().filter(|(_, l)| !l.is_empty()) {
            let in_section = |t: &Task| t.section == s;
            let (Some(first), Some(last)) = (model.tasks.iter().position(in_section), model.tasks.iter().rposition(in_section)) else {
                continue;
            };
            let rect = Rect::new(col_x(first), y, col_x(last) + col_w - col_x(first), band_h);
            out.fill(Geom::rounded(rect, RadiusTier::Sm), series(s, Tone::Solid));
            out.text(laid, [rect.x, rect.y + (rect.h - laid.height) / 2.0], rect.w, Align::Center, series(s, Tone::On));
        }
        y += band_h + gap;
    }

    // Task boxes: a row of actor dots over the name.
    let actor_dot = 8.0 * k;
    let dots_h = if model.tasks.iter().any(|t| !t.actors.is_empty()) { actor_dot + gap * 0.5 } else { 0.0 };
    let box_h = labels.iter().map(|l| l.height).fold(0.0_f32, f32::max) + dots_h + 2.0 * pad[1];
    let box_top = y;
    let face_r = m.font_size * 0.95;
    let level_h = 2.0 * face_r + 10.0 * k;
    let scale_top = box_top + box_h + gap;
    let scale_bottom = scale_top + 5.0 * level_h;
    let level_y = |score: f32| scale_top + (5.0 - score) * level_h + level_h / 2.0;

    // Level guides behind everything.
    let right = col_x(model.tasks.len() - 1) + col_w;
    for level in 1..=5 {
        let gy = level_y(level as f32);
        out.stroke(Geom::path(PathData::polyline(&[[x0, gy], [right, gy]])), Stroke::hairline(style::GRID));
    }

    for (i, task) in model.tasks.iter().enumerate() {
        let x = col_x(i);
        let center = x + col_w / 2.0;
        out.stroke(
            Geom::path(PathData::polyline(&[[center, box_top + box_h], [center, scale_bottom]])),
            Stroke::hairline(style::AXIS).dash(Dash::Dashed),
        );
        let rect = Rect::new(x, box_top, col_w, box_h);
        out.shape(
            Geom::rounded(rect, RadiusTier::Sm),
            Some(series(task.section, Tone::Soft)),
            Some(Stroke::hairline(series(task.section, Tone::Solid))),
        );
        for (n, &a) in task.actors.iter().enumerate() {
            let at = [x + pad[0] + actor_dot / 2.0 + n as f32 * (actor_dot + 3.0 * k), box_top + pad[1] + actor_dot / 2.0];
            if at[0] + actor_dot / 2.0 > rect.right() - pad[0] {
                break;
            }
            out.shape(
                Geom::Ellipse(Rect::centered(at, [actor_dot, actor_dot])),
                Some(series(a, Tone::Solid)),
                Some(Stroke::new(style::NODE_FILL, StrokeWidth::Px(1.0 * k))),
            );
        }
        let laid = &labels[i];
        let text_top = box_top + pad[1] + dots_h + (box_h - 2.0 * pad[1] - dots_h - laid.height) / 2.0;
        out.text(laid, [x, text_top], col_w, Align::Center, style::TEXT);
        let level = task.score.round().clamp(1.0, 5.0) as u8;
        face(&mut out, [center, level_y(task.score)], face_r, level);
    }

    // Legend rows, top-aligned with the task boxes.
    let mut ly = box_top;
    for (a, laid) in names.iter().enumerate() {
        let row = laid.height.max(dot);
        out.fill(Geom::Ellipse(Rect::centered([dot / 2.0, ly + cx.text.line_advance(font) / 2.0], [dot, dot])), series(a, Tone::Solid));
        out.text(laid, [dot + gap * 0.6, ly], laid.width, Align::Start, style::TEXT);
        ly += row + gap * 0.6;
    }

    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.tasks.len() as u32,
        edges: 0,
        groups: model.sections.iter().filter(|s| !s.is_empty()).count() as u32,
    }
}
