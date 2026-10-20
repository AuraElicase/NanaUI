//! Pieces the layered box-and-line families (class, state, ER,
//! requirement, use case, agent flow) share: statement words, `@{ ... }`
//! attributes, author classes and styles, group boxes, edges with end
//! labels and outlined node shapes.

use std::collections::HashMap;

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use crate::geom::path::{dist, rounded_polyline};
use crate::geom::{Marker, PathData, Shape};
use crate::ir::{Align, Corner, Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth};
use crate::source::directives::AuthorStyle;
use crate::style::{self, Paint};
use crate::text::LaidText;

/// The header band of a compartment box.
pub(crate) const HEADER_FILL: Ink = Ink::Role(R::Subtle);
/// Lines between table rows.
pub(crate) const ROW_RULE: Ink = Ink::Mix(SemanticColorMix::alpha(R::BorderSoft, 0.8));

/// The first whitespace-separated word and the rest, trimmed.
pub(crate) fn first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// The first word, or a whole `"quoted words"` run without its quotes,
/// and the rest.
pub(crate) fn first_name(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    if let Some(body) = s.strip_prefix('"')
        && let Some(end) = body.find('"')
    {
        return (&body[..end], body[end + 1..].trim_start());
    }
    first_word(s)
}

/// The end of a quoted label at the start of `s`: past the closing `"`,
/// or past `` `" `` for a markdown string.
pub(crate) fn quoted_end(s: &str) -> Option<usize> {
    if let Some(body) = s.strip_prefix("\"`") {
        body.find("`\"").map(|e| e + 4)
    } else {
        s.strip_prefix('"')?.find('"').map(|e| e + 2)
    }
}

/// `"text"` without its quotes; anything else trimmed.
pub(crate) fn unquote(s: &str) -> &str {
    let s = s.trim();
    if s.len() >= 2
        && ((s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')))
    {
        &s[1..s.len() - 1]
    } else {
        s
    }
}

/// Byte length of the identifier at the start of `s`: letters, digits,
/// `_` and any of `extra`.
pub(crate) fn ident_len(s: &str, extra: &str) -> usize {
    s.char_indices()
        .find(|&(_, c)| !(c.is_alphanumeric() || c == '_' || extra.contains(c)))
        .map_or(s.len(), |(i, _)| i)
}

/// The byte index of the `close` that balances the `open` at index 0,
/// skipping quoted text.
pub(crate) fn balanced(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quote = !quote,
            c if c == open && !quote => depth += 1,
            c if c == close && !quote => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether braces in `s` (outside quotes) leave a block open.
pub(crate) fn brace_depth(s: &str) -> i32 {
    let mut depth = 0i32;
    let mut quote = false;
    for c in s.chars() {
        match c {
            '"' => quote = !quote,
            '{' if !quote => depth += 1,
            '}' if !quote => depth -= 1,
            _ => {}
        }
    }
    depth
}

/// Source lines joined into statements: a statement runs on while a `{`
/// block or a `` "`...`" `` markdown string is still open. Each comes with
/// the index of its first line; blank lines are skipped.
pub(crate) fn statements(body: &str) -> Vec<(usize, String)> {
    let mut out: Vec<(usize, String)> = Vec::new();
    let mut open = false;
    let mut depth = 0i32;
    let mut ticks = 0usize;
    for (index, raw) in body.lines().enumerate() {
        let line = raw.trim();
        if open {
            if let Some((_, text)) = out.last_mut() {
                text.push('\n');
                text.push_str(line);
            }
        } else {
            if line.is_empty() {
                continue;
            }
            out.push((index, line.to_string()));
            depth = 0;
            ticks = 0;
        }
        depth += brace_depth(line);
        ticks += line.matches('`').count();
        open = depth > 0 || ticks % 2 == 1;
    }
    out
}

/// `key: value` pairs from an `@{ ... }` body, split on commas and line
/// breaks outside quotes and brackets. Values lose their quotes.
pub(crate) fn attributes(body: &str) -> Vec<(String, String)> {
    let mut parts = Vec::new();
    let mut quote = false;
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in body.char_indices() {
        match c {
            '"' => quote = !quote,
            '[' | '{' if !quote => depth += 1,
            ']' | '}' if !quote => depth -= 1,
            ',' | '\n' if !quote && depth <= 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
        .into_iter()
        .filter_map(|part| {
            let (k, v) = part.split_once(':')?;
            let k = unquote(k).trim();
            (!k.is_empty()).then(|| (k.to_string(), unquote(v).to_string()))
        })
        .collect()
}

/// `classDef`, `class`, `cssClass`, `:::` and `style` statements, resolved
/// per element in Mermaid's order: the `default` class, the element's
/// classes in assignment order, then its direct styles.
#[derive(Debug, Clone, Default)]
pub(crate) struct Styles {
    defs: HashMap<String, AuthorStyle>,
    assigned: Vec<(String, String)>,
    direct: Vec<(String, String)>,
}

impl Styles {
    /// `classDef a,b fill:#f00,...`.
    pub fn class_def(&mut self, rest: &str) {
        let (names, decls) = first_word(rest);
        for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            self.defs.entry(name.to_string()).or_default().apply(decls);
        }
    }

    /// Assign every class in the comma list `classes` to every id in `ids`.
    pub fn assign(&mut self, ids: &str, classes: &str) {
        for id in ids.split(',').map(str::trim).filter(|n| !n.is_empty()) {
            for class in classes.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                self.assigned.push((id.to_string(), class.to_string()));
            }
        }
    }

    /// `style id decls`.
    pub fn style(&mut self, rest: &str) {
        let quoted = rest.trim_start().starts_with('"');
        let (ids, decls) = first_name(rest);
        let ids: Vec<&str> = if quoted {
            vec![ids]
        } else {
            ids.split(',').collect()
        };
        for id in ids.into_iter().map(str::trim).filter(|n| !n.is_empty()) {
            self.direct.push((id.to_string(), decls.to_string()));
        }
    }

    /// The style an element ends up with. `inline` are its `:::` classes.
    pub fn resolve(&self, id: &str, inline: &[String]) -> AuthorStyle {
        let mut out = self.defs.get("default").copied().unwrap_or_default();
        let assigned = self
            .assigned
            .iter()
            .filter(|(i, _)| i == id)
            .map(|(_, c)| c);
        for class in inline.iter().chain(assigned) {
            if let Some(def) = self.defs.get(class) {
                out = out.merged(def);
            }
        }
        for (i, decls) in &self.direct {
            if i == id {
                out.apply(decls);
            }
        }
        out
    }
}

/// The first `:` that is not part of `:::`, outside quotes.
pub(crate) fn plain_colon(s: &str) -> Option<usize> {
    let bytes = s.as_bytes();
    let mut quote = false;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => quote = !quote,
            b':' if !quote => {
                if bytes[i..].starts_with(b":::") {
                    i += 3;
                    while i < bytes.len() && bytes[i] == b':' {
                        i += 1;
                    }
                    continue;
                }
                return Some(i);
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Split `name:::a,b` into the name and its classes.
pub(crate) fn split_classes(s: &str) -> (&str, Vec<String>) {
    match s.find(":::") {
        Some(i) => (
            s[..i].trim(),
            s[i + 3..]
                .split(',')
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect(),
        ),
        None => (s.trim(), Vec::new()),
    }
}

/// Fill, stroke and text inks for a node body after an author's style.
pub(crate) fn node_paint(author: &AuthorStyle) -> Paint {
    Paint::node().with_author(author)
}

/// Notes: the warm note colours unless the author says otherwise.
pub(crate) fn note_paint(author: &AuthorStyle) -> Paint {
    Paint {
        fill: style::NOTE_FILL,
        stroke: Stroke::hairline(style::NOTE_STROKE),
        text: style::TEXT,
        weight: None,
        italic: false,
    }
    .with_author(author)
}

/// A line's stroke after an author's `style` / `linkStyle`.
pub(crate) fn line_stroke(base: Stroke, author: &AuthorStyle) -> Stroke {
    let mut stroke = base;
    if let Some(c) = author.stroke {
        stroke.ink = Ink::Rgba(c);
    }
    if let Some(w) = author.stroke_width {
        stroke.width = StrokeWidth::Px(w);
    }
    if let Some(d) = author.dashed {
        stroke.dash = if d { Dash::Dashed } else { Dash::Solid };
    }
    stroke
}

/// A group box and its title.
pub(crate) fn cluster(
    out: &mut SceneBuilder,
    rect: Rect,
    title: Option<(&LaidText, Rect)>,
    author: &AuthorStyle,
) {
    let fill = author.fill.map_or(style::CLUSTER_FILL, Ink::Rgba);
    let stroke = line_stroke(style::cluster_stroke(), author);
    out.shape(
        Geom::Rect {
            rect,
            corner: Corner::Tier(style::CLUSTER_CORNER),
        },
        Some(fill),
        Some(stroke),
    );
    if let Some((laid, at)) = title
        && !laid.is_empty()
    {
        let ink = author.color.map_or(style::MUTED, Ink::Rgba);
        out.text(laid, [at.x, at.y], at.w, Align::Center, ink);
    }
}

/// Clusters drawn outermost first: indices sorted by nesting depth.
pub(crate) fn outermost_first(parents: &[Option<usize>]) -> Vec<usize> {
    let depth = |mut c: usize| {
        let mut d = 0;
        while let Some(p) = parents.get(c).copied().flatten() {
            d += 1;
            c = p;
            if d > parents.len() {
                break;
            }
        }
        d
    };
    let mut order: Vec<usize> = (0..parents.len()).collect();
    order.sort_by_key(|&c| (depth(c), c));
    order
}

/// Move a route's end onto `outline` along its last segment.
pub(crate) fn attach(points: &mut [[f32; 2]], outline: &PathData, rect: Rect, at_end: bool) {
    let n = points.len();
    if n < 2 {
        return;
    }
    let (outside, end) = if at_end {
        (points[n - 2], points[n - 1])
    } else {
        (points[1], points[0])
    };
    let d = dist(outside, end);
    if d < 1e-3 {
        return;
    }
    let dir = [(end[0] - outside[0]) / d, (end[1] - outside[1]) / d];
    let reach = rect.w + rect.h;
    let beyond = [end[0] + dir[0] * reach, end[1] + dir[1] * reach];
    if let Some(p) = outline.exit_point(beyond, outside) {
        if at_end {
            points[n - 1] = p;
        } else {
            points[0] = p;
        }
    }
}

/// A rounded orthogonal edge with optional end markers.
pub(crate) fn edge(
    out: &mut SceneBuilder,
    points: &[[f32; 2]],
    stroke: Stroke,
    start: Option<Marker>,
    end: Option<Marker>,
    marker: f32,
    radius: f32,
) {
    out.edge(
        points,
        move |p| rounded_polyline(p, radius),
        stroke,
        start,
        end,
        marker,
    );
}

fn unit(from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    let d = dist(from, to);
    if d < 1e-6 {
        [0.0, 1.0]
    } else {
        [(to[0] - from[0]) / d, (to[1] - from[1]) / d]
    }
}

/// Where a small label at one end of a route goes (multiplicities,
/// cardinalities): just past the marker, beside the line, on whichever side
/// stays clear of `avoid`. Returns the label's top-left.
pub(crate) fn end_label_origin(
    points: &[[f32; 2]],
    size: [f32; 2],
    at_start: bool,
    clear: f32,
    avoid: &[Rect],
) -> Option<[f32; 2]> {
    let n = points.len();
    if n < 2 {
        return None;
    }
    let (p, q) = if at_start {
        (points[0], points[1])
    } else {
        (points[n - 1], points[n - 2])
    };
    let d = unit(p, q);
    let vertical = d[1].abs() >= d[0].abs();
    let (along, across, side) = if vertical {
        // Beside a vertical run: to its right, else its left.
        (size[1] / 2.0, size[0] / 2.0, [1.0, 0.0])
    } else {
        // Beside a horizontal run: above it, else below.
        (size[0] / 2.0, size[1] / 2.0, [0.0, -1.0])
    };
    let gap = 3.0;
    let place = |sign: f32| {
        let c = [
            p[0] + d[0] * (clear + along + gap) + sign * side[0] * (across + gap),
            p[1] + d[1] * (clear + along + gap) + sign * side[1] * (across + gap),
        ];
        Rect::new(c[0] - size[0] / 2.0, c[1] - size[1] / 2.0, size[0], size[1])
    };
    let first = place(1.0);
    let hit = |r: &Rect| avoid.iter().any(|a| a.intersects(r));
    let rect = if hit(&first) {
        let second = place(-1.0);
        if hit(&second) { first } else { second }
    } else {
        first
    };
    Some([rect.x, rect.y])
}

/// A flowchart-style shape with its decorations.
pub(crate) fn shape_node(out: &mut SceneBuilder, shape: Shape, rect: Rect, paint: &Paint) {
    let (behind, over) = shape.decorations(rect);
    for geom in behind {
        out.shape(geom, Some(paint.fill), Some(paint.stroke));
    }
    if shape.solid() {
        let ink = match paint.fill {
            Ink::Rgba(_) => paint.fill,
            _ => style::TEXT,
        };
        out.shape(shape.outline(rect), Some(ink), None);
    } else if shape.has_body() {
        out.shape(shape.outline(rect), Some(paint.fill), Some(paint.stroke));
    }
    let line = Stroke {
        dash: Dash::Solid,
        ..paint.stroke
    };
    for path in over {
        out.stroke(Geom::path(path), line);
    }
}

/// A box with a tinted header band: the body, then the band clipped to the
/// rounded outline, then a divider under it and the outline again on top so
/// the band never covers it.
pub(crate) fn header_box(
    out: &mut SceneBuilder,
    rect: Rect,
    header: f32,
    corner: RadiusTier,
    paint: &Paint,
    band: Option<Ink>,
) {
    let outline = Geom::Rect {
        rect,
        corner: Corner::Tier(corner),
    };
    out.shape(outline.clone(), Some(paint.fill), None);
    if let Some(band) = band
        && header > 0.0
    {
        out.clip(outline.clone());
        out.fill(
            Geom::rect(Rect::new(rect.x, rect.y, rect.w, header.min(rect.h))),
            band,
        );
        out.unclip();
    }
    if header > 0.0 && header < rect.h - 0.5 {
        divider(out, rect.x, rect.right(), rect.y + header, paint.stroke.ink);
    }
    out.stroke(outline, paint.stroke);
}

/// A horizontal hairline from `x0` to `x1`.
pub(crate) fn divider(out: &mut SceneBuilder, x0: f32, x1: f32, y: f32, ink: Ink) {
    let mut path = PathData::new();
    path.move_to([x0, y]).line_to([x1, y]);
    out.stroke(Geom::path(path), Stroke::hairline(ink));
}

/// A straight hairline between two points.
pub(crate) fn line(out: &mut SceneBuilder, a: [f32; 2], b: [f32; 2], stroke: Stroke) {
    let mut path = PathData::new();
    path.move_to(a).line_to(b);
    out.stroke(Geom::path(path), stroke);
}

/// A note box with its text, top-left aligned inside the padding.
pub(crate) fn note(
    out: &mut SceneBuilder,
    rect: Rect,
    laid: &LaidText,
    pad: [f32; 2],
    paint: &Paint,
) {
    out.shape(
        Geom::Rect {
            rect,
            corner: Corner::Tier(style::NOTE_CORNER),
        },
        Some(paint.fill),
        Some(paint.stroke),
    );
    out.text(
        laid,
        [rect.x + pad[0], rect.y + pad[1]],
        rect.w - 2.0 * pad[0],
        Align::Start,
        paint.text,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_quotes_and_attributes() {
        assert_eq!(first_word("  class  A {"), ("class", "A {"));
        assert_eq!(unquote(" \"a b\" "), "a b");
        assert_eq!(unquote("\""), "\"");
        assert_eq!(ident_len("ab_1-x y", "-"), 6);
        assert_eq!(balanced("{a{b}\"}\"}x", '{', '}'), Some(8));
        let attrs = attributes(" shape: tool, label: \"a, b\"\n returns: X ");
        assert_eq!(
            attrs,
            vec![
                ("shape".into(), "tool".into()),
                ("label".into(), "a, b".into()),
                ("returns".into(), "X".into()),
            ]
        );
    }

    #[test]
    fn styles_resolve_in_mermaid_order() {
        let mut s = Styles::default();
        s.class_def("default stroke:#111");
        s.class_def("hot,warm fill:#f00");
        s.assign("A,B", "hot");
        s.style("A stroke:#00f");
        let a = s.resolve("A", &[]);
        assert!(a.fill.is_some());
        assert_eq!(a.stroke.map(|c| c[2]), Some(1.0));
        let c = s.resolve("C", &["warm".into()]);
        assert!(c.fill.is_some() && c.stroke.is_some());
        assert_eq!(split_classes("X:::a,b").1.len(), 2);
    }
}
