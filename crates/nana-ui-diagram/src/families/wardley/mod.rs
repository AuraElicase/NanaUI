//! Wardley maps (`wardley-beta`): components placed by `[visibility,
//! evolution]` on a value chain × evolution canvas, with dependencies,
//! flows, evolution arrows, pipelines, sourcing and inertia marks,
//! accelerators, notes and numbered annotations.

mod parse;

use super::{LayoutCx, ParseCx};
use crate::geom::path::{dist, lerp};
use crate::geom::{Marker, PathData};
use crate::ir::{Align, Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth};
use crate::text::{FontSpec, Label, LaidText};
use crate::{DiagramError, DiagramScene, Summary, style};
use nana_ui_core::{SemanticColorMix, SemanticColorRole};


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sourcing {
    Build,
    Buy,
    Outsource,
    Market,
}

#[derive(Debug, Clone)]
pub(crate) struct Component {
    pub label: Label,
    pub visibility: f32,
    pub evolution: f32,
    pub anchor: bool,
    pub label_offset: Option<[f32; 2]>,
    pub sourcing: Option<Sourcing>,
    pub inertia: bool,
    /// Index of the pipeline this component sits on.
    pub pipeline: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LinkKind {
    Plain,
    Dashed,
    /// Flow with arrowheads at the target, the source, or both.
    Flow { forward: bool, back: bool },
}

#[derive(Debug, Clone)]
pub(crate) struct Link {
    pub from: usize,
    pub to: usize,
    pub kind: LinkKind,
    pub label: Option<Label>,
}

#[derive(Debug, Clone)]
pub(crate) struct Stage {
    /// One or two lines (`Genesis / Concept`).
    pub label: Label,
    /// Right edge of the stage on the evolution axis.
    pub end: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct Marked {
    pub label: Label,
    pub visibility: f32,
    pub evolution: f32,
}

#[derive(Debug, Clone)]
pub(crate) struct Annotation {
    pub number: u32,
    pub points: Vec<[f32; 2]>,
    pub text: Label,
}

#[derive(Debug, Clone)]
pub(crate) struct Model {
    pub size: Option<[f32; 2]>,
    pub stages: Vec<Stage>,
    pub components: Vec<Component>,
    pub links: Vec<Link>,
    /// `(component, target evolution)`.
    pub evolves: Vec<(usize, f32)>,
    /// `(component, x, y)` in plain evolution/visibility order.
    pub trends: Vec<(usize, f32, f32)>,
    /// Pipeline owner components.
    pub pipelines: Vec<usize>,
    pub notes: Vec<Marked>,
    pub accelerators: Vec<(Marked, bool)>,
    pub annotations: Vec<Annotation>,
    pub annotations_at: Option<[f32; 2]>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            size: None,
            stages: ["Genesis", "Custom-Built", "Product<br>(+rental)", "Commodity<br>(+utility)"]
                .iter()
                .enumerate()
                .map(|(i, s)| Stage {
                    label: Label::plain(s),
                    end: (i + 1) as f32 * 0.25,
                })
                .collect(),
            components: Vec::new(),
            links: Vec::new(),
            evolves: Vec::new(),
            trends: Vec::new(),
            pipelines: Vec::new(),
            notes: Vec::new(),
            accelerators: Vec::new(),
            annotations: Vec::new(),
            annotations_at: None,
        }
    }
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let strong = font.weight(m.strong_weight);
    let (w, h) = match model.size {
        Some([w, h]) => {
            let k = (m.font_size * 60.0 / w.max(1.0)).min(m.font_size * 44.0 / h.max(1.0));
            ((w * k).clamp(m.font_size * 24.0, m.font_size * 64.0), (h * k).clamp(m.font_size * 18.0, m.font_size * 48.0))
        }
        None => (m.font_size * 60.0, m.font_size * 34.0),
    };
    let place = |visibility: f32, evolution: f32| [evolution.clamp(0.0, 1.0) * w, (1.0 - visibility.clamp(0.0, 1.0)) * h];
    let mut out = SceneBuilder::new();
    let gap = m.font_size * 0.5;

    // Canvas, stage dividers and axes.
    out.fill(Geom::rect(Rect::new(0.0, 0.0, w, h)), style::NODE_FILL);
    let mut prev = 0.0f32;
    for (i, stage) in model.stages.iter().enumerate() {
        let x0 = prev * w;
        let x1 = stage.end.clamp(prev, 1.0) * w;
        if i > 0 {
            let mut line = PathData::new();
            line.move_to([x0, 0.0]).line_to([x0, h]);
            out.stroke(Geom::path(line), Stroke::hairline(style::GRID).dash(Dash::Dashed));
        }
        let laid = cx.text.lay(&stage.label, small, Some((x1 - x0 - gap).max(m.font_size * 3.0)));
        out.text(&laid, [(x0 + x1) / 2.0 - laid.width / 2.0, h + gap], laid.width, Align::Center, style::MUTED);
        prev = stage.end.clamp(prev, 1.0);
    }
    let marker = cx.marker_size();
    let axis = Stroke::hairline(style::AXIS);
    out.edge(&[[0.0, h], [w + marker, h]], PathData::polyline, axis, None, Some(Marker::Arrow), marker);
    out.edge(&[[0.0, h], [0.0, -marker]], PathData::polyline, axis, None, Some(Marker::Arrow), marker);
    // Right-aligned to `end`.
    let caption = |out: &mut SceneBuilder, text: &str, end: [f32; 2]| {
        let laid = cx.text.lay_plain(text, small);
        out.text(&laid, [end[0] - laid.width, end[1]], laid.width, Align::End, style::MUTED);
    };
    let stage_h = model
        .stages
        .iter()
        .map(|s| cx.text.lay(&s.label, small, None).height)
        .fold(0.0, f32::max);
    caption(&mut out, "Evolution", [w + marker, h + gap * 2.0 + stage_h]);
    caption(&mut out, "Value Chain", [-gap, -marker]);
    caption(&mut out, "Visible", [-gap, m.font_size * 2.0]);
    caption(&mut out, "Invisible", [-gap, h - cx.text.line_advance(small)]);

    let r = m.font_size * 0.36;
    // Pipeline members ride on a band just under their owner.
    let mut pos: Vec<[f32; 2]> = model.components.iter().map(|c| place(c.visibility, c.evolution)).collect();
    for (i, c) in model.components.iter().enumerate() {
        if let Some(owner) = c.pipeline {
            pos[i][1] = pos[owner][1] + r * 3.5;
        }
    }

    // Accelerators: broad arrows behind everything else on the map.
    for (mark, accelerate) in &model.accelerators {
        let at = place(mark.visibility, mark.evolution);
        let (len, thick) = (m.font_size * 4.0, m.font_size * 1.2);
        let dir = if *accelerate { 1.0 } else { -1.0 };
        let head = thick * 0.9;
        let pts = [
            [at[0] - dir * len / 2.0, at[1] - thick / 4.0],
            [at[0] + dir * (len / 2.0 - head), at[1] - thick / 4.0],
            [at[0] + dir * (len / 2.0 - head), at[1] - thick / 2.0],
            [at[0] + dir * len / 2.0, at[1]],
            [at[0] + dir * (len / 2.0 - head), at[1] + thick / 2.0],
            [at[0] + dir * (len / 2.0 - head), at[1] + thick / 4.0],
            [at[0] - dir * len / 2.0, at[1] + thick / 4.0],
        ];
        let role = if *accelerate { SemanticColorRole::Success } else { SemanticColorRole::Danger };
        out.shape(
            Geom::path(PathData::polygon(&pts)),
            Some(Ink::Mix(SemanticColorMix::alpha(role, 0.18))),
            Some(Stroke::hairline(Ink::Role(role))),
        );
        let laid = cx.text.lay(&mark.label, small, Some(m.font_size * 9.0));
        out.text(&laid, [at[0] - laid.width / 2.0, at[1] + thick / 2.0 + 2.0], laid.width, Align::Center, style::MUTED);
    }

    // Pipelines: a band under the owner spanning its members.
    for &owner in &model.pipelines {
        let (lo, hi) = (0..model.components.len())
            .filter(|&i| model.components[i].pipeline == Some(owner))
            .map(|i| pos[i][0])
            .fold((pos[owner][0], pos[owner][0]), |(a, b), x| (a.min(x), b.max(x)));
        let y = pos[owner][1] + r * 3.5;
        let rect = Rect::new(lo - r * 2.5, y - r * 1.4, hi - lo + r * 5.0, r * 2.8);
        out.shape(Geom::rect(rect), Some(style::CLUSTER_FILL), Some(style::cluster_stroke()));
    }

    // Dependencies and flows.
    let link_inset = r + 1.0;
    for link in &model.links {
        let (a, b) = (pos[link.from], pos[link.to]);
        let Some((a, b)) = trim(a, b, link_inset) else {
            continue;
        };
        let mut stroke = style::edge_stroke();
        let (start, end) = match link.kind {
            LinkKind::Plain => (None, None),
            LinkKind::Dashed => {
                stroke = stroke.dash(Dash::Dashed);
                (None, None)
            }
            LinkKind::Flow { forward, back } => {
                stroke.width = StrokeWidth::Thick;
                (back.then_some(Marker::Arrow), forward.then_some(Marker::Arrow))
            }
        };
        out.edge(&[a, b], PathData::polyline, stroke, start, end, marker);
        if let Some(label) = &link.label {
            let laid = cx.text.lay(label, small, Some(m.font_size * 9.0));
            out.chip(&laid, [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0], [m.small_size * 0.4, m.small_size * 0.1]);
        }
    }

    // Evolution: a dashed arrow to a ghost of the component.
    let evolve_ink = style::DANGER;
    for &(c, target) in &model.evolves {
        let from = pos[c];
        let to = place(model.components[c].visibility, target);
        if let Some((a, b)) = trim(from, to, r + 2.0) {
            out.edge(&[a, b], PathData::polyline, Stroke::hairline(evolve_ink).dash(Dash::Dashed), None, Some(Marker::Arrow), marker);
        }
        out.shape(Geom::Ellipse(Rect::centered(to, [r * 2.0, r * 2.0])), Some(style::NODE_FILL), Some(Stroke::hairline(evolve_ink)));
        let laid = cx.text.lay(&model.components[c].label, small, Some(m.font_size * 10.0));
        out.text(&laid, [to[0] + r + 4.0, to[1] - laid.height / 2.0], laid.width, Align::Start, evolve_ink);
    }
    for &(c, x, y) in &model.trends {
        let to = place(y, x);
        if let Some((a, b)) = trim(pos[c], to, r + 2.0) {
            out.stroke(Geom::path(PathData::polyline(&[a, b])), Stroke::hairline(style::MUTED).dash(Dash::Dotted));
        }
        out.stroke(Geom::Ellipse(Rect::centered(to, [r * 2.0, r * 2.0])), Stroke::hairline(style::MUTED).dash(Dash::Dashed));
    }

    // Components with their sourcing halo and inertia bar.
    for (i, c) in model.components.iter().enumerate() {
        let at = pos[i];
        if let Some(s) = c.sourcing {
            let halo = r * 2.6;
            let fill = Some(Ink::Mix(SemanticColorMix::alpha(SemanticColorRole::Text, 0.08)));
            let stroke = Some(Stroke::hairline(style::NODE_STROKE));
            let geom = match s {
                Sourcing::Build => Geom::path(PathData::polygon(&[
                    [at[0], at[1] - halo],
                    [at[0] + halo * 0.95, at[1] + halo * 0.7],
                    [at[0] - halo * 0.95, at[1] + halo * 0.7],
                ])),
                Sourcing::Buy => Geom::path(PathData::polygon(&[
                    [at[0], at[1] - halo],
                    [at[0] + halo, at[1]],
                    [at[0], at[1] + halo],
                    [at[0] - halo, at[1]],
                ])),
                Sourcing::Outsource => Geom::rect(Rect::centered(at, [halo * 1.5, halo * 1.5])),
                Sourcing::Market => Geom::Ellipse(Rect::centered(at, [halo * 2.0, halo * 2.0])),
            };
            out.shape(geom, fill, stroke);
        }
        if c.inertia {
            let x = at[0] + r * 3.2;
            out.fill(Geom::rect(Rect::new(x, at[1] - r * 2.2, r * 0.9, r * 4.4)), style::TEXT);
        }
        if c.anchor {
            continue;
        }
        if c.pipeline.is_some() {
            out.shape(Geom::rect(Rect::centered(at, [r * 1.8, r * 1.8])), Some(style::NODE_FILL), Some(Stroke::hairline(style::TEXT)));
        } else {
            out.shape(Geom::Ellipse(Rect::centered(at, [r * 2.0, r * 2.0])), Some(style::NODE_FILL), Some(Stroke::new(style::TEXT, StrokeWidth::Thick)));
        }
    }

    // Notes, annotation badges and component marks claim their space
    // first; each component label then takes the first free spot around
    // its point.
    let mut taken: Vec<Rect> = pos.iter().map(|p| Rect::centered(*p, [r * 2.4, r * 2.4])).collect();
    for note in &model.notes {
        let at = place(note.visibility, note.evolution);
        let laid = cx.text.lay(&note.label, small.weight(m.strong_weight), Some(m.font_size * 12.0));
        let origin = [at[0], at[1] - laid.height / 2.0];
        out.text(&laid, origin, laid.width, Align::Start, style::TEXT);
        taken.push(Rect::new(origin[0], origin[1], laid.width, laid.height));
    }
    let badge = m.small_size * 1.5;
    for a in &model.annotations {
        for p in &a.points {
            taken.push(Rect::centered(place(p[0], p[1]), [badge, badge]));
        }
    }
    for (i, c) in model.components.iter().enumerate() {
        let at = pos[i];
        let face = if c.anchor { strong } else { small };
        let laid = cx.text.lay(&c.label, face, Some(m.font_size * 10.0));
        let [lw, lh] = laid.size();
        let origin = if c.anchor {
            [at[0] - lw / 2.0, at[1] - lh / 2.0]
        } else if let Some([dx, dy]) = c.label_offset {
            [at[0] + dx, at[1] + dy - lh / 2.0]
        } else if c.pipeline.is_some() {
            [at[0] - lw / 2.0, at[1] + r * 1.8]
        } else {
            let d = r + 3.0;
            let candidates = [
                [at[0] + d, at[1] - lh - 1.0],
                [at[0] + d, at[1] + 1.0],
                [at[0] - d - lw, at[1] - lh - 1.0],
                [at[0] - d - lw, at[1] + 1.0],
                [at[0] - lw / 2.0, at[1] - d - lh],
                [at[0] - lw / 2.0, at[1] + d],
                [at[0] + d, at[1] - lh / 2.0],
                [at[0] - d - lw, at[1] - lh / 2.0],
            ];
            // The spot with the least overlap; the first wins ties.
            let overlap = |o: &[f32; 2]| -> f32 {
                let rect = Rect::new(o[0], o[1], lw, lh);
                taken
                    .iter()
                    .enumerate()
                    .filter(|&(k, _)| k != i)
                    .map(|(_, t)| {
                        let w = (rect.right().min(t.right()) - rect.x.max(t.x)).max(0.0);
                        let h = (rect.bottom().min(t.bottom()) - rect.y.max(t.y)).max(0.0);
                        w * h
                    })
                    .sum()
            };
            let mut best = (candidates[0], overlap(&candidates[0]));
            let crowded = model.components.len() > 300;
            for c in candidates.iter().skip(1).filter(|_| !crowded) {
                if best.1 <= 0.25 {
                    break;
                }
                let o = overlap(c);
                if o + 0.25 < best.1 {
                    best = (*c, o);
                }
            }
            best.0
        };
        if c.anchor {
            let pill = Rect::new(origin[0] - 4.0, origin[1], lw + 8.0, lh);
            out.fill(Geom::rect(pill), style::NODE_FILL);
        }
        out.text(&laid, origin, lw, Align::Start, style::TEXT);
        taken.push(Rect::new(origin[0], origin[1], lw, lh));
    }

    // Numbered annotations and their key.
    let badge_font = FontSpec::new(m.small_size * 0.85).weight(m.strong_weight);
    for a in &model.annotations {
        let pts: Vec<[f32; 2]> = a.points.iter().map(|p| place(p[0], p[1])).collect();
        if pts.len() > 1 {
            out.stroke(Geom::path(PathData::polyline(&pts)), Stroke::hairline(style::MUTED).dash(Dash::Dotted));
        }
        for p in &pts {
            out.shape(Geom::Ellipse(Rect::centered(*p, [badge, badge])), Some(style::NODE_FILL), Some(Stroke::hairline(style::TEXT)));
            let laid = cx.text.lay_plain(&a.number.to_string(), badge_font);
            out.text_centered(&laid, *p, style::TEXT);
        }
    }
    if !model.annotations.is_empty() {
        let rows: Vec<LaidText> = model
            .annotations
            .iter()
            .map(|a| {
                let mut label = Label::plain(&format!("{}. ", a.number));
                if let (Some(first), Some(rest)) = (label.lines.first_mut(), a.text.lines.first()) {
                    first.extend(rest.iter().cloned());
                }
                label.lines.extend(a.text.lines.iter().skip(1).cloned());
                cx.text.lay(&label, small, Some(m.font_size * 16.0))
            })
            .collect();
        let pad = m.small_size * 0.6;
        let title = cx.text.lay_plain("Annotations", small.weight(m.strong_weight));
        let width = rows.iter().map(|r| r.width).fold(title.width, f32::max) + 2.0 * pad;
        let height = title.height + rows.iter().map(|r| r.height).sum::<f32>() + 2.0 * pad;
        let origin = match model.annotations_at {
            Some([v, e]) => {
                let p = place(v, e);
                [p[0].min(w - width).max(0.0), p[1].min(h - height).max(0.0)]
            }
            None => [w - width, h + gap * 3.0 + stage_h + cx.text.line_advance(small)],
        };
        let rect = Rect::new(origin[0], origin[1], width, height);
        out.shape(Geom::rounded(rect, style::NOTE_CORNER), Some(style::NODE_FILL), Some(Stroke::hairline(style::CLUSTER_STROKE)));
        let mut y = origin[1] + pad;
        out.text(&title, [origin[0] + pad, y], title.width, Align::Start, style::TEXT);
        y += title.height;
        for row in &rows {
            out.text(row, [origin[0] + pad, y], row.width, Align::Start, style::TEXT);
            y += row.height;
        }
    }

    // A thin frame on top.
    out.stroke(Geom::rect(Rect::new(0.0, 0.0, w, h)), Stroke::hairline(style::GRID));
    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// Pull both ends of a segment in by `by`; `None` when nothing is left.
fn trim(a: [f32; 2], b: [f32; 2], by: f32) -> Option<([f32; 2], [f32; 2])> {
    let d = dist(a, b);
    if d <= 2.0 * by + 1.0 {
        return None;
    }
    let t = by / d;
    Some((lerp(a, b, t), lerp(b, a, t)))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.components.len() as u32,
        edges: model.links.len() as u32,
        groups: model.pipelines.len() as u32,
    }
}
