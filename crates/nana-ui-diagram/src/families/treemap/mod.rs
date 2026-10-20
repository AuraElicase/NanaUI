//! Treemaps: nested rectangles sized by value.
//!
//! Sections are `Subtle` boxes with a title bar; their children fill the
//! rest by the squarified algorithm (`layout::squarify`). Leaves take the
//! series colour of their top-level branch as a soft wash; labels that do
//! not fit their leaf are left out.

use std::collections::HashMap;

use nana_ui_core::{RadiusTier, SemanticColorRole as R};

use super::{LayoutCx, ParseCx};
use crate::ir::{Align, Corner, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::kind;
use crate::layout::squarify::squarify;
use crate::layout::tree::{Outline, indent_width, preorder};
use crate::source::directives::AuthorStyle;
use crate::style::{self, Paint};
use crate::text::{FontSpec, Label};
use crate::{DiagramError, DiagramScene, LimitKind, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub name: Label,
    /// A leaf's own value; sections sum their children.
    pub value: Option<f64>,
    pub style: AuthorStyle,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model::default();
    let mut outline = Outline::new();
    let mut classes: Vec<(usize, String)> = Vec::new();
    let mut defs: HashMap<String, AuthorStyle> = HashMap::new();
    let mut header_seen = false;
    for (index, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if !header_seen {
            if kind::header(trimmed).is_none() {
                return Err(DiagramError::syntax(
                    index as u32 + 1,
                    1,
                    "expected `treemap`",
                ));
            }
            header_seen = true;
            continue;
        }
        if let Some(title) = trimmed
            .strip_prefix("title ")
            .or_else(|| (trimmed == "title").then_some(""))
        {
            cx.limits.check_label(title)?;
            cx.meta.title = Some(title.trim().trim_matches('"').to_string());
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("classDef ") {
            let rest = rest.trim().trim_end_matches(';');
            match rest.split_once(char::is_whitespace) {
                Some((name, decls)) => defs.entry(name.to_string()).or_default().apply(decls),
                None => cx.warn(index, "`classDef` needs a name and styles"),
            }
            continue;
        }
        let Some((name, value, class)) = node_line(trimmed) else {
            cx.warn(index, "unreadable treemap line");
            continue;
        };
        let name = cx.label(name)?;
        let value = match value {
            Some(text) => match text.trim().parse::<f64>() {
                Ok(v) if v.is_finite() && v >= 0.0 => Some(v),
                _ => {
                    cx.warn(index, format!("`{}` is not a value", text.trim()));
                    None
                }
            },
            None => None,
        };
        let id = model.nodes.len();
        cx.limits
            .check_count(id + 1, cx.limits.nodes, LimitKind::Nodes)?;
        let parent = outline.push(indent_width(line), id);
        if let Some(p) = parent {
            if model.nodes[p].value.is_some() {
                cx.warn(
                    index,
                    "a node with a value has children; the value is ignored",
                );
                model.nodes[p].value = None;
            }
            model.nodes[p].children.push(id);
        } else {
            model.roots.push(id);
        }
        if let Some(class) = class {
            classes.extend(
                class
                    .split([',', ' '])
                    .filter(|c| !c.is_empty())
                    .map(|c| (id, c.to_string())),
            );
        }
        model.nodes.push(Node {
            name,
            value,
            style: AuthorStyle::default(),
            children: Vec::new(),
        });
    }
    if !header_seen {
        return Err(DiagramError::Empty);
    }
    for (id, class) in classes {
        if let Some(def) = defs.get(&class) {
            model.nodes[id].style = model.nodes[id].style.merged(def);
        }
    }
    Ok(model)
}

/// `"name"`, `"name": 12`, `"name":::class`, `"name": 12:::class`; bare
/// names end at the first colon.
fn node_line(text: &str) -> Option<(&str, Option<&str>, Option<&str>)> {
    let (name, rest) = if let Some(quoted) = text.strip_prefix('"') {
        let end = quoted.find('"')?;
        (&quoted[..end], quoted[end + 1..].trim())
    } else {
        let end = text.find(':').unwrap_or(text.len());
        (text[..end].trim(), text[end..].trim())
    };
    if name.is_empty() {
        return None;
    }
    if let Some(class) = rest.strip_prefix(":::") {
        return Some((name, None, Some(class.trim())));
    }
    let Some(value) = rest.strip_prefix(':') else {
        return rest.is_empty().then_some((name, None, None));
    };
    match value.split_once(":::") {
        Some((v, class)) => Some((name, Some(v), Some(class.trim()))),
        None => Some((name, Some(value), None)),
    }
}

/// A d3-style subset: `$` currency, `,` grouping, `.N` precision, `%`.
fn format_value(value: f64, format: &str) -> String {
    let currency = format.contains('$');
    let percent = format.ends_with('%');
    let group = format.contains(',') || format.is_empty();
    let precision = format
        .split_once('.')
        .and_then(|(_, p)| {
            p.trim_end_matches(|c: char| !c.is_ascii_digit())
                .parse::<usize>()
                .ok()
        })
        .map(|p| p.min(6));
    let v = if percent { value * 100.0 } else { value };
    let digits = precision.unwrap_or(if v.fract().abs() < 1e-9 { 0 } else { 2 });
    let mut text = format!("{:.*}", digits, v.abs());
    if precision.is_none() && text.contains('.') {
        text = text.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    if group {
        let (int, frac) = text
            .split_once('.')
            .map_or((text.as_str(), None), |(i, f)| (i, Some(f)));
        let mut grouped = String::new();
        for (i, c) in int.chars().enumerate() {
            if i > 0 && (int.len() - i) % 3 == 0 {
                grouped.push(',');
            }
            grouped.push(c);
        }
        if let Some(f) = frac {
            grouped.push('.');
            grouped.push_str(f);
        }
        text = grouped;
    }
    let sign = if value < 0.0 { "-" } else { "" };
    format!(
        "{sign}{}{text}{}",
        if currency { "$" } else { "" },
        if percent { "%" } else { "" }
    )
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let mut out = SceneBuilder::new();
    let n = model.nodes.len();
    let children: Vec<Vec<usize>> = model.nodes.iter().map(|n| n.children.clone()).collect();
    let order = preorder(&children, &model.roots);

    // Sums, children before parents.
    let mut sums = vec![0.0f64; n];
    for &(v, _) in order.iter().rev() {
        let node = &model.nodes[v];
        sums[v] = if node.children.is_empty() {
            node.value.unwrap_or(0.0)
        } else {
            node.children.iter().map(|&c| sums[c]).sum()
        };
    }
    let leaves = order
        .iter()
        .filter(|&&(v, _)| model.nodes[v].children.is_empty())
        .count();
    let width = cx
        .config
        .get_f32("treemap.width")
        .filter(|w| *w >= 120.0)
        .unwrap_or(m.wrap_width * 3.2)
        .min(4000.0);
    let cell = m.font_size * m.font_size * 32.0;
    let height = (width * 0.62)
        .max(leaves as f32 * cell / width)
        .min(width * 3.0);
    let format = cx
        .config
        .get("treemap.valueFormat")
        .unwrap_or("")
        .trim()
        .trim_matches(['\'', '"'])
        .to_string();
    let show_values = cx.config.get_bool("treemap.showValues").unwrap_or(true);

    // Colour branches: the first level with siblings.
    let mut branch = vec![0u16; n];
    let colour_roots: Vec<usize> = match model.roots.as_slice() {
        [single] => model.nodes[*single].children.clone(),
        roots => roots.to_vec(),
    };
    for (b, &r) in colour_roots.iter().enumerate() {
        for (v, _) in preorder(&children, &[r]) {
            branch[v] = b as u16;
        }
    }

    let gap = 3.0;
    let font = cx.font();
    let small = cx.small_font();
    let head_font = FontSpec::new(m.small_size).weight(m.strong_weight);
    let head_h = cx.text.line_advance(head_font) + 6.0;
    let area = Rect::new(0.0, 0.0, width, height);
    let mut rects = vec![Rect::default(); n];
    let mut placed = vec![false; n];
    let roots: Vec<f64> = model.roots.iter().map(|&r| sums[r]).collect();
    for (&r, rect) in model.roots.iter().zip(squarify(&roots, area)) {
        rects[r] = rect;
        placed[r] = rect.w > 0.0 && rect.h > 0.0;
    }
    let section_stroke = Stroke::hairline(Ink::Role(R::Border));
    for &(v, _) in &order {
        if !placed[v] {
            continue;
        }
        let node = &model.nodes[v];
        let rect = rects[v].inset(gap / 2.0, gap / 2.0);
        if rect.w < 2.0 || rect.h < 2.0 {
            continue;
        }
        let shown_value = show_values.then(|| format_value(sums[v], &format));
        if node.children.is_empty() {
            let base = Paint {
                fill: Ink::Series {
                    index: branch[v],
                    tone: crate::ir::Tone::Soft,
                },
                stroke: Stroke::hairline(Ink::Series {
                    index: branch[v],
                    tone: crate::ir::Tone::Solid,
                }),
                ..Paint::node()
            };
            let paint = base.with_author(&node.style);
            out.shape(
                Geom::Rect {
                    rect,
                    corner: Corner::Tier(RadiusTier::Xs),
                },
                Some(paint.fill),
                Some(paint.stroke),
            );
            let face = FontSpec {
                weight: paint.weight.unwrap_or(font.weight),
                italic: paint.italic,
                ..font
            };
            let room = rect.inset(6.0, 4.0);
            let name = cx.text.lay(&node.name, face, Some(room.w));
            if name.is_empty() || name.width > room.w + 0.01 || name.height > room.h {
                continue;
            }
            let value = shown_value
                .map(|t| cx.text.lay_plain(&t, small))
                .filter(|t| t.width <= room.w && name.height + t.height <= room.h);
            let total = name.height + value.as_ref().map_or(0.0, |t| t.height);
            let c = rect.center();
            let top = c[1] - total / 2.0;
            out.text(
                &name,
                [c[0] - name.width / 2.0, top],
                name.width,
                Align::Center,
                paint.text,
            );
            if let Some(t) = value {
                let ink = if matches!(paint.fill, Ink::Rgba(_)) {
                    paint.text
                } else {
                    style::MUTED
                };
                out.text(
                    &t,
                    [c[0] - t.width / 2.0, top + name.height],
                    t.width,
                    Align::Center,
                    ink,
                );
            }
            continue;
        }

        // A section: box, title bar, children below it.
        let paint = Paint {
            fill: style::CLUSTER_FILL,
            stroke: section_stroke,
            ..Paint::node()
        }
        .with_author(&node.style);
        out.shape(
            Geom::Rect {
                rect,
                corner: Corner::Tier(RadiusTier::Sm),
            },
            Some(paint.fill),
            Some(paint.stroke),
        );
        let mut inner = rect.inset(gap, gap);
        if rect.h > head_h * 2.0 && rect.w > m.font_size * 3.0 {
            let room = rect.w - 12.0;
            let title = cx.text.lay(&node.name, head_font, None);
            let value = shown_value.map(|t| cx.text.lay_plain(&t, small));
            let vw = value.as_ref().map_or(0.0, |t| t.width + 8.0);
            let y = rect.y + (head_h - title.line_height) / 2.0;
            if title.lines.len() == 1 && title.width <= room {
                out.text(
                    &title,
                    [rect.x + 6.0, y],
                    title.width,
                    Align::Start,
                    paint.text,
                );
                if let Some(t) = value.filter(|_| title.width + vw <= room) {
                    let ink = if matches!(paint.fill, Ink::Rgba(_)) {
                        paint.text
                    } else {
                        style::MUTED
                    };
                    let vy = rect.y + (head_h - t.line_height) / 2.0;
                    out.text(
                        &t,
                        [rect.right() - 6.0 - t.width, vy],
                        t.width,
                        Align::Start,
                        ink,
                    );
                }
            }
            inner = Rect::new(
                rect.x + gap,
                rect.y + head_h,
                rect.w - 2.0 * gap,
                rect.h - head_h - gap,
            );
        }
        let values: Vec<f64> = node.children.iter().map(|&c| sums[c]).collect();
        for (&c, r) in node.children.iter().zip(squarify(&values, inner)) {
            rects[c] = r;
            placed[c] = r.w > 0.0 && r.h > 0.0;
        }
    }

    let content = area;
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.nodes.iter().filter(|n| n.children.is_empty()).count() as u32,
        edges: 0,
        groups: model
            .nodes
            .iter()
            .filter(|n| !n.children.is_empty())
            .count() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_split_name_value_and_class() {
        assert_eq!(
            node_line("\"A\": 20:::hot"),
            Some(("A", Some(" 20"), Some("hot")))
        );
        assert_eq!(
            node_line("\"B\":::important"),
            Some(("B", None, Some("important")))
        );
        assert_eq!(
            node_line("\"Men's\": 40"),
            Some(("Men's", Some(" 40"), None))
        );
        assert_eq!(node_line("Bare: 3"), Some(("Bare", Some(" 3"), None)));
        assert_eq!(node_line("\"Section\""), Some(("Section", None, None)));
        assert_eq!(node_line("\"x\" junk"), None);
    }

    #[test]
    fn values_format_like_d3() {
        assert_eq!(format_value(700000.0, "$0,0"), "$700,000");
        assert_eq!(format_value(0.35, "$.1%"), "$35.0%");
        assert_eq!(format_value(0.35, ".0%"), "35%");
        assert_eq!(format_value(1234.5, ""), "1,234.5");
        assert_eq!(format_value(12.0, ""), "12");
    }
}
