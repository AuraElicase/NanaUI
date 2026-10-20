//! Tree views: a file or outline tree drawn as an indented list with
//! connector rails, from indentation or `tree`-style box-drawing prefixes.

use std::collections::HashMap;

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use super::{LayoutCx, ParseCx, graphkit, kit};
use crate::geom::PathData;
use crate::geom::path::rounded_polyline;
use crate::ir::{Align, Corner, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth};
use crate::layout::tree::{Outline, preorder};
use crate::source::directives::AuthorStyle;
use crate::style::{self, Paint};
use crate::text::{FontSpec, Label, LaidText};
use crate::{DiagramError, DiagramScene, LimitKind, Summary};

#[derive(Debug, Clone)]
pub(crate) struct Node {
    pub label: Label,
    /// Ends in `/`: a directory even without children.
    pub folder: bool,
    pub comment: Option<Label>,
    pub icon: Option<String>,
    pub classes: Vec<String>,
    pub children: Vec<usize>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub nodes: Vec<Node>,
    pub roots: Vec<usize>,
    pub class_defs: HashMap<String, AuthorStyle>,
}

/// Characters a `tree`-style prefix is drawn with.
fn is_prefix(c: char) -> bool {
    c.is_whitespace() || "│├└─┃┣┗━┬┼╰╭|`+".contains(c)
}

/// The prefix width and the item text. ASCII trees (`|-- a`, `` `-- b ``)
/// count their dashes as prefix too.
fn split_prefix(line: &str) -> (usize, &str) {
    let mut width = 0;
    let mut rest = line;
    let mut marker = false;
    while let Some(c) = rest.chars().next() {
        if is_prefix(c) {
            marker |= !c.is_whitespace();
            width += if c == '\t' { 4 } else { 1 };
        } else if c == '-' && marker {
            width += 1;
        } else {
            break;
        }
        rest = &rest[c.len_utf8()..];
    }
    (width, rest)
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model::default();
    let mut outline = Outline::new();
    let (header, _) = kit::header(body)?;
    for (index, line) in body.lines().enumerate().skip(header + 1) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("classDef ") {
            let rest = rest.trim().trim_end_matches(';');
            match rest.split_once(char::is_whitespace) {
                Some((name, decls)) => model
                    .class_defs
                    .entry(name.to_string())
                    .or_default()
                    .apply(decls),
                None => cx.warn(index, "`classDef` needs a name and styles"),
            }
            continue;
        }
        let (indent, text) = split_prefix(line);
        let text = text.trim_end();
        if text.is_empty() {
            continue;
        }
        let (mut text, comment) = match text.find("##") {
            Some(at) => (text[..at].trim_end(), Some(text[at + 2..].trim())),
            None => (text, None),
        };
        let mut classes = Vec::new();
        let mut icon = None;
        let mut name = String::new();
        // Pull `:::class` and `icon(...)` tokens out of the name.
        while !text.is_empty() {
            let token_end = text.find(char::is_whitespace).unwrap_or(text.len());
            let token = &text[..token_end];
            if let Some(class) = token.strip_prefix(":::") {
                classes.extend(
                    class
                        .split(',')
                        .filter(|c| !c.is_empty())
                        .map(str::to_string),
                );
                text = text[token_end..].trim_start();
            } else if let Some(arg) = text.strip_prefix("icon(")
                && let Some(end) = arg.find(')')
            {
                icon = Some(arg[..end].trim().to_string());
                text = arg[end + 1..].trim_start();
            } else {
                if !name.is_empty() {
                    name.push(' ');
                }
                name.push_str(token);
                text = text[token_end..].trim_start();
            }
        }
        if name.is_empty() {
            cx.warn(index, "an item needs a name");
            continue;
        }
        let label = cx.label(&name)?;
        let comment = match comment.filter(|c| !c.is_empty()) {
            Some(c) => Some(cx.label(c)?),
            None => None,
        };
        let id = model.nodes.len();
        cx.limits
            .check_count(id + 1, cx.limits.nodes, LimitKind::Nodes)?;
        match outline.push(indent, id) {
            Some(p) => model.nodes[p].children.push(id),
            None => model.roots.push(id),
        }
        model.nodes.push(Node {
            folder: name.trim_end_matches('"').ends_with('/'),
            label,
            comment,
            icon,
            classes,
            children: Vec::new(),
        });
    }
    Ok(model)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let mut out = SceneBuilder::new();
    let font = cx.font();
    let bold = font.weight(m.strong_weight);
    let small = cx.small_font();
    let indent = cx
        .config
        .get_f32("treeView.rowIndent")
        .filter(|v| (4.0..=400.0).contains(v))
        .unwrap_or(m.font_size * 1.6);
    let line_width = cx
        .config
        .get_f32("treeView.lineThickness")
        .filter(|v| *v > 0.0)
        .map_or(StrokeWidth::Hairline, |v| StrokeWidth::Px(v.min(8.0)));
    let show_icons = cx.config.get_bool("treeView.showIcons").unwrap_or(false);
    let row_gap = m.font_size * 0.45;
    let icon = m.font_size;
    let icon_gap = m.font_size * 0.4;
    let chip_pad = [m.small_size * 0.35, 1.0];
    let children: Vec<Vec<usize>> = model.nodes.iter().map(|n| n.children.clone()).collect();
    let rows = preorder(&children, &model.roots);

    // Rows: label at `depth × indent`, comments in one column after the
    // widest name.
    struct Row {
        node: usize,
        depth: usize,
        y: f32,
        h: f32,
        label: LaidText,
        text_x: f32,
        folder: bool,
    }
    let mut laid_rows = Vec::with_capacity(rows.len());
    let mut y = 0.0;
    let mut name_end: f32 = 0.0;
    for &(node, depth) in &rows {
        let n = &model.nodes[node];
        let folder = n.folder || !n.children.is_empty();
        let label = cx
            .text
            .lay(&n.label, if folder { bold } else { font }, None);
        let has_icon = show_icons || n.icon.is_some();
        let x = depth as f32 * indent;
        let text_x = x + if has_icon { icon + icon_gap } else { 0.0 };
        let h = label.height.max(cx.text.line_advance(font));
        name_end = name_end.max(text_x + label.width + chip_pad[0]);
        laid_rows.push(Row {
            node,
            depth,
            y,
            h,
            label,
            text_x,
            folder,
        });
        y += h + row_gap;
    }
    let comment_x = name_end + m.font_size * 1.5;
    let mut row_of = vec![usize::MAX; model.nodes.len()];
    for (r, row) in laid_rows.iter().enumerate() {
        row_of[row.node] = r;
    }

    // Rails: from under a parent's first column down to each child.
    let rail = Stroke::new(style::EDGE, line_width);
    for row in &laid_rows {
        let kids = &model.nodes[row.node].children;
        let Some(&last) = kids.last() else { continue };
        let x = row.depth as f32 * indent + indent * 0.4;
        let top = row.y + row.h;
        for &kid in kids {
            let k = &laid_rows[row_of[kid]];
            let cy = k.y + k.h / 2.0;
            let end = k.depth as f32 * indent - indent * 0.15;
            if kid == last {
                out.stroke(
                    Geom::path(rounded_polyline(
                        &[[x, top], [x, cy], [end, cy]],
                        indent * 0.3,
                    )),
                    rail,
                );
            } else {
                graphkit::line(&mut out, [x, cy], [end, cy], rail);
            }
        }
    }

    let highlight = Ink::Mix(SemanticColorMix::alpha(R::Highlight, 0.28));
    for row in &laid_rows {
        let n = &model.nodes[row.node];
        let x = row.depth as f32 * indent;
        if show_icons || n.icon.is_some() {
            let cy = row.y + row.label.line_height / 2.0;
            draw_icon(
                &mut out,
                Rect::new(x, cy - icon / 2.0, icon, icon),
                row.folder,
            );
        }
        let mut ink = style::TEXT;
        if !n.classes.is_empty() {
            let author = n
                .classes
                .iter()
                .filter_map(|c| model.class_defs.get(c))
                .fold(AuthorStyle::default(), |acc, s| acc.merged(s));
            let paint = Paint {
                fill: highlight,
                ..Paint::node()
            }
            .with_author(&author);
            ink = paint.text;
            let chip = Rect::new(
                row.text_x - chip_pad[0],
                row.y - chip_pad[1],
                row.label.width + 2.0 * chip_pad[0],
                row.label.height + 2.0 * chip_pad[1],
            );
            let stroke = author.stroke.map(|_| paint.stroke);
            out.shape(
                Geom::Rect {
                    rect: chip,
                    corner: Corner::Tier(RadiusTier::Xs),
                },
                Some(paint.fill),
                stroke,
            );
        }
        out.text(
            &row.label,
            [row.text_x, row.y],
            row.label.width,
            Align::Start,
            ink,
        );
        if let Some(comment) = &n.comment {
            let laid = cx.text.lay(
                comment,
                FontSpec {
                    italic: true,
                    ..small
                },
                None,
            );
            let cy = row.y + (row.label.line_height - laid.line_height) / 2.0;
            out.text(
                &laid,
                [comment_x, cy],
                laid.width,
                Align::Start,
                style::MUTED,
            );
        }
    }

    let content = if laid_rows.is_empty() {
        Rect::new(0.0, 0.0, m.font_size, m.font_size)
    } else {
        out.bounds()
    };
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// A placeholder glyph: a folder tab or a page with a folded corner.
fn draw_icon(out: &mut SceneBuilder, r: Rect, folder: bool) {
    let r = r.inset(r.w * 0.08, r.h * 0.12);
    if folder {
        let tab = r.w * 0.42;
        let mut p = PathData::new();
        p.move_to([r.x, r.bottom()])
            .line_to([r.x, r.y])
            .line_to([r.x + tab, r.y])
            .line_to([r.x + tab + r.h * 0.18, r.y + r.h * 0.2])
            .line_to([r.right(), r.y + r.h * 0.2])
            .line_to([r.right(), r.bottom()])
            .close();
        out.shape(
            Geom::path(p),
            Some(Ink::Mix(SemanticColorMix::alpha(R::Accent, 0.22))),
            Some(Stroke::hairline(style::ACCENT)),
        );
    } else {
        let w = r.h * 0.78;
        let x = r.x + (r.w - w) / 2.0;
        let fold = w * 0.32;
        let mut p = PathData::new();
        p.move_to([x, r.y])
            .line_to([x + w - fold, r.y])
            .line_to([x + w, r.y + fold])
            .line_to([x + w, r.bottom()])
            .line_to([x, r.bottom()])
            .close();
        out.shape(
            Geom::path(p),
            Some(style::NODE_FILL),
            Some(Stroke::hairline(style::MUTED)),
        );
    }
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.nodes.len() as u32,
        edges: model.nodes.len().saturating_sub(model.roots.len()) as u32,
        groups: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_measure_depth_in_both_styles() {
        assert_eq!(split_prefix("├── src/"), (4, "src/"));
        assert_eq!(split_prefix("│   └── index.ts"), (8, "index.ts"));
        assert_eq!(split_prefix("|-- a"), (4, "a"));
        assert_eq!(split_prefix("    -flag"), (4, "-flag"));
    }
}
