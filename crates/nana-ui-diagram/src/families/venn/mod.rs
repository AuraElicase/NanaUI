//! Venn diagrams (`venn-beta`): `set`, `union`, indented `text` notes and
//! `style`. One to three sets are circles; a fourth turns every set into
//! a rotated ellipse so all fifteen regions exist.
//!
//! Region labels go to the deepest point of each region, found on a grid
//! with the exact signed distance to every outline.

use std::collections::HashMap;

use super::kit::{self, keyword, split_top};
use super::{LayoutCx, ParseCx};
use crate::geom::PathData;
use crate::ir::{Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::source::directives::AuthorStyle;
use crate::text::{FontSpec, LaidText, Label};
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};
use nana_ui_core::{SemanticColorMix, SemanticColorRole};


/// At most this many sets are drawn; later ones are skipped with a warning.
const MAX_SETS: usize = 4;

#[derive(Debug, Clone)]
pub(crate) struct Note {
    pub id: String,
    pub label: Label,
}

#[derive(Debug, Clone)]
pub(crate) struct Area {
    pub id: String,
    pub label: Option<Label>,
    /// Bit `i` set: inside set `i`.
    pub mask: u8,
    pub size: Option<f64>,
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct AreaStyle {
    pub author: AuthorStyle,
    pub fill_opacity: Option<f32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    /// Sets first (mask with one bit), then unions in source order.
    pub areas: Vec<Area>,
    pub sets: usize,
    pub styles: HashMap<String, AreaStyle>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        cx.warn(header, "unexpected text after `venn-beta`");
    }
    let mut model = Model::default();
    let mut set_ids: HashMap<String, usize> = HashMap::new();
    let mut current: Option<usize> = None;
    let mut notes = 0usize;
    for (line, stmt) in kit::statements(body, header) {
        if let Some(title) = keyword(stmt, "title") {
            let title = kit::unquote(title).trim();
            if !title.is_empty() {
                cx.meta.title = Some(title.to_string());
            }
        } else if let Some(def) = keyword(stmt, "set") {
            current = None;
            let (ids, label, size) = area_head(cx, def)?;
            let [id] = ids.as_slice() else {
                cx.warn(line, "`set` names one set");
                continue;
            };
            if set_ids.contains_key(id) {
                cx.warn(line, format!("set `{id}` is already defined"));
                continue;
            }
            if model.sets >= MAX_SETS {
                cx.warn(line, format!("only {MAX_SETS} sets are drawn; `{id}` is skipped"));
                continue;
            }
            set_ids.insert(id.clone(), model.sets);
            model.areas.push(Area {
                id: id.clone(),
                label: Some(label.unwrap_or_else(|| Label::plain(id))),
                mask: 1 << model.sets,
                size,
                notes: Vec::new(),
            });
            model.sets += 1;
            current = Some(model.areas.len() - 1);
        } else if let Some(def) = keyword(stmt, "union") {
            current = None;
            let (ids, label, size) = area_head(cx, def)?;
            let mut mask = 0u8;
            let mut ok = ids.len() >= 2;
            for id in &ids {
                match set_ids.get(id) {
                    Some(&i) => mask |= 1 << i,
                    None => {
                        cx.warn(line, format!("union names an undefined set `{id}`"));
                        ok = false;
                    }
                }
            }
            if !ok || mask.count_ones() < 2 {
                continue;
            }
            if let Some(existing) = model.areas.iter().position(|a| a.mask == mask) {
                // One region per combination: a repeat renames it and
                // collects its notes.
                if label.is_some() {
                    model.areas[existing].label = label;
                }
                model.areas[existing].size = size.or(model.areas[existing].size);
                current = Some(existing);
                continue;
            }
            cx.limits.check_count(model.areas.len() + 1, cx.limits.elements, LimitKind::Elements)?;
            model.areas.push(Area {
                id: ids.join(","),
                label,
                mask,
                size,
                notes: Vec::new(),
            });
            current = Some(model.areas.len() - 1);
        } else if let Some(def) = keyword(stmt, "text") {
            let Some(area) = current else {
                cx.warn(line, "`text` needs a preceding set or union");
                continue;
            };
            let (ids, label, _) = area_head(cx, def)?;
            let id = ids.first().cloned().unwrap_or_default();
            notes += 1;
            cx.limits.check_count(notes, cx.limits.elements, LimitKind::Elements)?;
            let label = label.unwrap_or_else(|| Label::plain(&id));
            model.areas[area].notes.push(Note { id, label });
        } else if let Some(def) = keyword(stmt, "style") {
            let (targets, decls) = kit::first_word(def);
            let style = AreaStyle {
                author: AuthorStyle::parse(decls),
                fill_opacity: fill_opacity(decls),
            };
            let targets: Vec<String> = split_top(targets, ',').iter().map(|t| kit::unquote(t).to_string()).collect();
            // `style A,B` styles the union of A and B when one exists,
            // otherwise each named set or note.
            let joined = targets.join(",");
            if targets.len() > 1 && model.areas.iter().any(|a| a.id == joined) {
                merge(&mut model.styles, joined, style);
            } else {
                for t in targets {
                    merge(&mut model.styles, t, style);
                }
            }
        } else {
            cx.warn(line, format!("unreadable venn statement `{stmt}`"));
        }
    }
    Ok(model)
}

fn merge(styles: &mut HashMap<String, AreaStyle>, key: String, style: AreaStyle) {
    let entry = styles.entry(key).or_default();
    entry.author = entry.author.merged(&style.author);
    entry.fill_opacity = style.fill_opacity.or(entry.fill_opacity);
}

fn fill_opacity(decls: &str) -> Option<f32> {
    decls.split([',', ';']).find_map(|d| {
        let (k, v) = d.split_once(':')?;
        (k.trim() == "fill-opacity").then(|| kit::number(v).map(|v| v.clamp(0.0, 1.0) as f32))?
    })
}

/// Ids, display label and size of a `set` / `union` / `text` head.
type Head = (Vec<String>, Option<Label>, Option<f64>);

/// `A`, `"Foo Bar"`, `A,B["Label"]:3` → ids, label and size.
fn area_head(cx: &ParseCx<'_>, def: &str) -> Result<Head, DiagramError> {
    let mut def = def.trim();
    let mut size = None;
    // A trailing `:N` outside quotes and brackets.
    if let Some(colon) = def.rfind(':')
        && !def[colon..].contains([']', '"'])
        && let Some(n) = kit::number(&def[colon + 1..])
    {
        size = Some(n);
        def = def[..colon].trim_end();
    }
    let mut label = None;
    let mut ids_part = def;
    if def.ends_with(']')
        && let Some(open) = bracket_open(def)
    {
        let inner = &def[open + 1..def.len() - 1];
        label = Some(cx.label(inner)?).filter(|l| !l.is_empty());
        ids_part = &def[..open];
    }
    let ids = split_top(ids_part, ',')
        .into_iter()
        .map(|s| kit::unquote(s).trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    Ok((ids, label, size))
}

/// The `[` that opens the trailing bracket group, outside quotes.
fn bracket_open(s: &str) -> Option<usize> {
    let mut quote = false;
    let mut found = None;
    let mut depth = 0i32;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quote = !quote,
            '[' if !quote => {
                if depth == 0 {
                    found = Some(i);
                }
                depth += 1;
            }
            ']' if !quote => depth -= 1,
            _ => {}
        }
    }
    found
}

/// A set outline: a circle, or an ellipse rotated by `angle`.
#[derive(Debug, Clone, Copy)]
struct Outline {
    center: [f32; 2],
    rx: f32,
    ry: f32,
    angle: f32,
}

impl Outline {
    /// Signed distance, negative inside (exact for circles, a close
    /// approximation for ellipses).
    fn distance(&self, p: [f32; 2]) -> f32 {
        let (s, c) = self.angle.sin_cos();
        let d = [p[0] - self.center[0], p[1] - self.center[1]];
        let local = [d[0] * c + d[1] * s, -d[0] * s + d[1] * c];
        if (self.rx - self.ry).abs() < 1e-3 {
            return (local[0].hypot(local[1])) - self.rx;
        }
        let k = ((local[0] / self.rx).powi(2) + (local[1] / self.ry).powi(2)).sqrt();
        if k < 1e-6 {
            return -self.rx.min(self.ry);
        }
        let grad = ((local[0] / (self.rx * self.rx)).powi(2) + (local[1] / (self.ry * self.ry)).powi(2)).sqrt();
        k * (k - 1.0) / grad.max(1e-6)
    }

    fn geom(&self) -> Geom {
        let rect = Rect::centered(self.center, [self.rx * 2.0, self.ry * 2.0]);
        if self.angle.abs() < 1e-6 {
            return Geom::Ellipse(rect);
        }
        let (s, c) = self.angle.sin_cos();
        let o = self.center;
        Geom::path(PathData::ellipse(rect).mapped(|p| {
            let d = [p[0] - o[0], p[1] - o[1]];
            [o[0] + d[0] * c - d[1] * s, o[1] + d[0] * s + d[1] * c]
        }))
    }

    fn bounds(&self) -> Rect {
        let (s, c) = self.angle.sin_cos();
        let w = ((self.rx * c).powi(2) + (self.ry * s).powi(2)).sqrt();
        let h = ((self.rx * s).powi(2) + (self.ry * c).powi(2)).sqrt();
        Rect::centered(self.center, [w * 2.0, h * 2.0])
    }
}

fn outlines(sets: usize, r: f32, scale: &[f32]) -> Vec<Outline> {
    let circle = |c: [f32; 2], i: usize| Outline {
        center: c,
        rx: r * scale[i],
        ry: r * scale[i],
        angle: 0.0,
    };
    match sets {
        0 => Vec::new(),
        1 => vec![circle([0.0, 0.0], 0)],
        2 => vec![circle([-0.62 * r, 0.0], 0), circle([0.62 * r, 0.0], 1)],
        3 => vec![
            circle([-0.58 * r, -0.36 * r], 0),
            circle([0.58 * r, -0.36 * r], 1),
            circle([0.0, 0.62 * r], 2),
        ],
        _ => {
            // The classic four-ellipse arrangement.
            let (a, b) = (r * 1.45, r * 0.78);
            let tilt = 0.75f32;
            (0..4)
                .map(|i| {
                    let (dx, dy, angle) = match i {
                        0 => (-0.62, 0.32, tilt),
                        1 => (-0.2, -0.05, tilt),
                        2 => (0.2, -0.05, -tilt),
                        _ => (0.62, 0.32, -tilt),
                    };
                    Outline {
                        center: [dx * r, dy * r],
                        rx: a * scale[i],
                        ry: b * scale[i],
                        angle,
                    }
                })
                .collect()
        }
    }
}

/// The deepest point of the region inside exactly the sets in `mask`, and
/// its depth.
fn region_anchor(shapes: &[Outline], mask: u8, area: Rect) -> Option<([f32; 2], f32)> {
    const STEPS: usize = 72;
    let mut best: Option<([f32; 2], f32)> = None;
    for j in 0..=STEPS {
        for i in 0..=STEPS {
            let p = [
                area.x + area.w * i as f32 / STEPS as f32,
                area.y + area.h * j as f32 / STEPS as f32,
            ];
            let mut depth = f32::INFINITY;
            let mut inside_all = true;
            for (k, s) in shapes.iter().enumerate() {
                let d = s.distance(p);
                let want_in = mask & (1 << k) != 0;
                if want_in != (d < 0.0) {
                    inside_all = false;
                    break;
                }
                depth = depth.min(d.abs());
            }
            if inside_all && best.is_none_or(|(_, b)| depth > b + 1e-3) {
                best = Some((p, depth));
            }
        }
    }
    best
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let strong = font.weight(m.strong_weight);
    let small = cx.small_font();
    let r = m.font_size * if model.sets >= 4 { 8.0 } else { 9.0 };
    let mut out = SceneBuilder::new();

    // Author sizes shrink a set a little, never enough to lose a region.
    let max_size = model.areas[..model.sets].iter().filter_map(|a| a.size).fold(0.0f64, f64::max);
    let scale: Vec<f32> = model.areas[..model.sets]
        .iter()
        .map(|a| match a.size {
            Some(s) if max_size > 0.0 => ((s / max_size).sqrt() as f32).clamp(0.85, 1.0),
            _ => 1.0,
        })
        .collect();
    let shapes = outlines(model.sets, r, &scale);
    let style_of = |id: &str| model.styles.get(id).copied().unwrap_or_default();

    // Washes, then a neutral overlay on every pairwise overlap so deeper
    // overlaps read darker, then outlines on top.
    for (i, s) in shapes.iter().enumerate() {
        let st = style_of(&model.areas[i].id);
        let fill = match st.author.fill {
            Some(c) => Ink::Rgba([c[0], c[1], c[2], c[3] * st.fill_opacity.unwrap_or(0.35)]),
            None => style::series(i, Tone::Soft),
        };
        out.fill(s.geom(), fill);
    }
    let overlay = Ink::Mix(SemanticColorMix::alpha(SemanticColorRole::Text, 0.07));
    for i in 0..shapes.len() {
        for j in i + 1..shapes.len() {
            out.clip(shapes[i].geom());
            out.fill(shapes[j].geom(), overlay);
            out.unclip();
        }
    }
    for (i, s) in shapes.iter().enumerate() {
        let st = style_of(&model.areas[i].id);
        let stroke = Stroke::new(
            st.author.stroke.map_or(style::series(i, Tone::Solid), Ink::Rgba),
            st.author.stroke_width.map_or(StrokeWidth::Thick, StrokeWidth::Px),
        );
        out.stroke(s.geom(), stroke);
    }

    // Labels: each area's name over its notes at the region's deepest point.
    let bounds = shapes.iter().map(Outline::bounds).reduce(|a, b| a.union(&b)).unwrap_or_default();
    for area in &model.areas {
        if area.label.is_none() && area.notes.is_empty() {
            continue;
        }
        let Some((at, depth)) = region_anchor(&shapes, area.mask, bounds) else {
            continue;
        };
        let wrap = (depth * 2.2).max(m.font_size * 7.0).min(r * 1.2);
        let st = style_of(&area.id);
        let mut block: Vec<(LaidText, Ink)> = Vec::new();
        if let Some(label) = &area.label {
            let face = if area.mask.count_ones() == 1 { strong } else { font };
            let face = FontSpec {
                weight: st.author.font_weight.unwrap_or(face.weight),
                ..face
            };
            block.push((cx.text.lay(label, face, Some(wrap)), st.author.color.map_or(style::TEXT, Ink::Rgba)));
        }
        for note in &area.notes {
            let nst = style_of(&note.id);
            let ink = nst.author.color.or(st.author.color).map_or(style::MUTED, Ink::Rgba);
            block.push((cx.text.lay(&note.label, small, Some(wrap)), ink));
        }
        let gap = m.font_size * 0.2;
        let total: f32 = block.iter().map(|(l, _)| l.height).sum::<f32>() + gap * (block.len() - 1) as f32;
        let mut y = at[1] - total / 2.0;
        for (laid, ink) in &block {
            out.text_centered(laid, [at[0], y + laid.height / 2.0], *ink);
            y += laid.height + gap;
        }
    }

    let content = bounds.union(&out.bounds());
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.sets as u32,
        edges: 0,
        groups: model.areas.len().saturating_sub(model.sets) as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_region_of_four_ellipses_exists() {
        let shapes = outlines(4, 100.0, &[1.0; 4]);
        let bounds = shapes.iter().map(Outline::bounds).reduce(|a, b| a.union(&b)).unwrap();
        for mask in 1u8..16 {
            assert!(region_anchor(&shapes, mask, bounds).is_some(), "region {mask:04b} missing");
        }
    }

    #[test]
    fn every_region_of_three_circles_exists() {
        let shapes = outlines(3, 100.0, &[0.85, 1.0, 0.85]);
        let bounds = shapes.iter().map(Outline::bounds).reduce(|a, b| a.union(&b)).unwrap();
        for mask in 1u8..8 {
            assert!(region_anchor(&shapes, mask, bounds).is_some(), "region {mask:03b} missing");
        }
    }
}
