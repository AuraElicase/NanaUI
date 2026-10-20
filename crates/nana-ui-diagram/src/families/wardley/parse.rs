//! The `wardley-beta` grammar.

use std::collections::HashMap;

use super::{Annotation, Component, Link, LinkKind, Marked, Model, Sourcing, Stage};
use crate::families::ParseCx;
use crate::families::kit::{self, keyword};
use crate::{DiagramError, LimitKind};

struct RawLink {
    line: usize,
    from: String,
    to: String,
    kind: LinkKind,
    label: Option<String>,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        cx.warn(header, "unexpected text after `wardley-beta`");
    }
    let mut model = Model::default();
    let mut names: HashMap<String, usize> = HashMap::new();
    let mut links: Vec<RawLink> = Vec::new();
    let mut evolves: Vec<(usize, String, f32)> = Vec::new();
    let mut trends: Vec<(usize, String, f32, f32)> = Vec::new();
    let mut pipeline: Option<usize> = None;
    let mut elements = 0usize;
    for (line, stmt) in kit::statements(body, header) {
        elements += 1;
        cx.limits.check_count(elements, cx.limits.elements, LimitKind::Elements)?;
        if stmt == "}" {
            if pipeline.take().is_none() {
                cx.warn(line, "`}` without a pipeline");
            }
            continue;
        }
        if let Some(owner) = pipeline {
            // Inside `pipeline X { ... }`: `component Name [evolution]`.
            let Some(def) = keyword(stmt, "component") else {
                cx.warn(line, format!("pipelines hold components, not `{stmt}`"));
                continue;
            };
            let (name, rest) = split_name(def);
            let Some(evolution) = single(rest) else {
                cx.warn(line, format!("pipeline component `{name}` needs `[evolution]`"));
                continue;
            };
            let visibility = model.components[owner].visibility;
            add(cx, &mut model, &mut names, line, name, visibility, evolution, false, rest)?;
            if let Some(c) = model.components.last_mut() {
                c.pipeline = Some(owner);
            }
            continue;
        }
        if let Some(title) = keyword(stmt, "title") {
            let title = kit::unquote(title).trim();
            if !title.is_empty() {
                cx.meta.title = Some(title.to_string());
            }
        } else if let Some(size) = keyword(stmt, "size") {
            match kit::pair(size) {
                Some((w, h)) if w > 0.0 && h > 0.0 => model.size = Some([w as f32, h as f32]),
                _ => cx.warn(line, format!("unreadable size `{size}`")),
            }
        } else if let Some(stages) = keyword(stmt, "evolution") {
            model.stages = parse_stages(cx, stages)?;
        } else if let Some((word, def)) = ["anchor", "component"].into_iter().find_map(|w| Some((w, keyword(stmt, w)?))) {
            let (name, rest) = split_name(def);
            match kit::pair(rest_coords(rest)) {
                Some((v, e)) => add(cx, &mut model, &mut names, line, name, v as f32, e as f32, word == "anchor", rest)?,
                None => cx.warn(line, format!("{word} `{name}` needs `[visibility, evolution]`")),
            }
        } else if let Some(def) = keyword(stmt, "pipeline") {
            let def = def.trim_end();
            let Some(name) = def.strip_suffix('{') else {
                cx.warn(line, "only the `pipeline Name { ... }` form is drawn");
                continue;
            };
            let name = kit::unquote(name.trim());
            match names.get(name) {
                Some(&owner) => {
                    if !model.pipelines.contains(&owner) {
                        model.pipelines.push(owner);
                    }
                    pipeline = Some(owner);
                }
                None => {
                    cx.warn(line, format!("pipeline names an undefined component `{name}`"));
                    // Swallow the block so its lines do not read as statements.
                    pipeline = None;
                }
            }
        } else if let Some(def) = keyword(stmt, "evolve") {
            let def = def.trim();
            let split = def.rfind(char::is_whitespace).unwrap_or(0);
            match kit::number(&def[split..]) {
                Some(e) if split > 0 => evolves.push((line, kit::unquote(&def[..split]).trim().to_string(), e as f32)),
                _ => cx.warn(line, format!("`evolve {def}` needs a component and an evolution")),
            }
        } else if let Some(def) = keyword(stmt, "note") {
            match marked(cx, def)? {
                Some(note) => model.notes.push(note),
                None => cx.warn(line, "a note needs \"text\" [visibility, evolution]"),
            }
        } else if let Some(def) = keyword(stmt, "accelerator") {
            match marked(cx, def)? {
                Some(mark) => model.accelerators.push((mark, true)),
                None => cx.warn(line, "an accelerator needs a name and [visibility, evolution]"),
            }
        } else if let Some(def) = keyword(stmt, "deaccelerator") {
            match marked(cx, def)? {
                Some(mark) => model.accelerators.push((mark, false)),
                None => cx.warn(line, "a deaccelerator needs a name and [visibility, evolution]"),
            }
        } else if let Some(at) = keyword(stmt, "annotations") {
            match kit::pair(at) {
                Some((v, e)) => model.annotations_at = Some([v as f32, e as f32]),
                None => cx.warn(line, format!("unreadable annotations position `{at}`")),
            }
        } else if let Some(def) = keyword(stmt, "annotation") {
            match annotation(cx, def)? {
                Some(a) => model.annotations.push(a),
                None => cx.warn(line, "an annotation needs N,[visibility, evolution] \"text\""),
            }
        } else if let Some((from, x, y)) = trend(stmt) {
            trends.push((line, from, x, y));
        } else if let Some(link) = link(stmt, line) {
            cx.limits.check_count(links.len() + 1, cx.limits.edges, LimitKind::Edges)?;
            links.push(link);
        } else {
            cx.warn(line, format!("unreadable wardley statement `{stmt}`"));
        }
    }
    if pipeline.is_some() {
        cx.warn(header, "a pipeline block is missing its `}`");
    }
    for l in links {
        match (names.get(&l.from), names.get(&l.to)) {
            (Some(&from), Some(&to)) => {
                let label = l.label.map(|t| cx.label(&t)).transpose()?.filter(|l| !l.is_empty());
                model.links.push(Link {
                    from,
                    to,
                    kind: l.kind,
                    label,
                });
            }
            _ => cx.warn(l.line, format!("link `{} -> {}` names an undefined component", l.from, l.to)),
        }
    }
    for (line, name, e) in evolves {
        match names.get(&name) {
            Some(&c) => model.evolves.push((c, e)),
            None => cx.warn(line, format!("`evolve` names an undefined component `{name}`")),
        }
    }
    for (line, name, x, y) in trends {
        match names.get(&name) {
            Some(&c) => model.trends.push((c, x, y)),
            None => cx.warn(line, format!("trend names an undefined component `{name}`")),
        }
    }
    Ok(model)
}

#[allow(clippy::too_many_arguments)]
fn add(
    cx: &mut ParseCx<'_>,
    model: &mut Model,
    names: &mut HashMap<String, usize>,
    line: usize,
    name: &str,
    visibility: f32,
    evolution: f32,
    anchor: bool,
    rest: &str,
) -> Result<(), DiagramError> {
    if name.is_empty() {
        cx.warn(line, "a component needs a name");
        return Ok(());
    }
    if names.contains_key(name) {
        cx.warn(line, format!("`{name}` is already on the map"));
        return Ok(());
    }
    cx.limits.check_count(model.components.len() + 1, cx.limits.nodes, LimitKind::Nodes)?;
    let label = cx.label(name)?;
    let mut component = Component {
        label,
        visibility: visibility.clamp(0.0, 1.0),
        evolution: evolution.clamp(0.0, 1.0),
        anchor,
        label_offset: None,
        sourcing: None,
        inertia: false,
        pipeline: None,
    };
    // Trailing decorations: `label [dx, dy]` and `(build)` style marks.
    let after = rest.find(']').map_or("", |i| &rest[i + 1..]);
    if let Some(i) = after.find("label") {
        let tail = &after[i + 5..];
        if let (Some(a), Some(b)) = (tail.find('['), tail.find(']'))
            && a < b
            && let Some((dx, dy)) = kit::pair(&tail[a..=b])
        {
            component.label_offset = Some([dx as f32, dy as f32]);
        }
    }
    let mut tail = after;
    while let Some(open) = tail.find('(') {
        let Some(close) = tail[open..].find(')').map(|c| c + open) else {
            break;
        };
        match tail[open + 1..close].trim() {
            "build" => component.sourcing = Some(Sourcing::Build),
            "buy" => component.sourcing = Some(Sourcing::Buy),
            "outsource" => component.sourcing = Some(Sourcing::Outsource),
            "market" => component.sourcing = Some(Sourcing::Market),
            "inertia" => component.inertia = true,
            other => cx.warn(line, format!("unknown decorator `({other})`")),
        }
        tail = &tail[close + 1..];
    }
    names.insert(name.to_string(), model.components.len());
    model.components.push(component);
    Ok(())
}

/// A quoted name, or everything before the first `[`.
fn split_name(def: &str) -> (&str, &str) {
    let def = def.trim();
    if let Some(inner) = def.strip_prefix('"')
        && let Some(close) = inner.find('"')
    {
        return (&inner[..close], &inner[close + 1..]);
    }
    match def.find('[') {
        Some(i) => (def[..i].trim(), &def[i..]),
        None => (def, ""),
    }
}

/// The first `[...]` group of `rest`.
fn rest_coords(rest: &str) -> &str {
    match (rest.find('['), rest.find(']')) {
        (Some(a), Some(b)) if a < b => &rest[a..=b],
        _ => "",
    }
}

fn single(rest: &str) -> Option<f32> {
    let inner = rest_coords(rest).strip_prefix('[')?.strip_suffix(']')?;
    kit::number(inner).map(|v| v.clamp(0.0, 1.0) as f32)
}

/// `"text" [v, e]` or `Name [v, e]`.
fn marked(cx: &ParseCx<'_>, def: &str) -> Result<Option<Marked>, DiagramError> {
    let (name, rest) = split_name(def);
    let Some((v, e)) = kit::pair(rest_coords(rest)) else {
        return Ok(None);
    };
    Ok(Some(Marked {
        label: cx.label(name)?,
        visibility: v as f32,
        evolution: e as f32,
    }))
}

/// `N,[v, e] "text"` or `N,[[v, e],[v, e]] "text"`.
fn annotation(cx: &ParseCx<'_>, def: &str) -> Result<Option<Annotation>, DiagramError> {
    let Some((number, rest)) = def.split_once(',') else {
        return Ok(None);
    };
    let Ok(number) = number.trim().parse::<u32>() else {
        return Ok(None);
    };
    let rest = rest.trim();
    let Some(text_at) = rest.rfind(']') else {
        return Ok(None);
    };
    let coords = &rest[..=text_at];
    let text = kit::unquote(rest[text_at + 1..].trim());
    let mut points = Vec::new();
    let inner = coords.strip_prefix('[').and_then(|c| c.strip_suffix(']')).unwrap_or("");
    if inner.trim_start().starts_with('[') {
        for part in kit::split_top(inner, ',') {
            if let Some((v, e)) = kit::pair(part) {
                points.push([v as f32, e as f32]);
            }
        }
    } else if let Some((v, e)) = kit::pair(coords) {
        points.push([v as f32, e as f32]);
    }
    if points.is_empty() || points.len() > 16 {
        return Ok(None);
    }
    Ok(Some(Annotation {
        number,
        points,
        text: cx.label(text)?,
    }))
}

/// `Genesis@0.25 -> Custom / Emerging -> ...`.
fn parse_stages(cx: &ParseCx<'_>, text: &str) -> Result<Vec<Stage>, DiagramError> {
    let parts: Vec<&str> = text.split("->").map(str::trim).filter(|p| !p.is_empty()).take(12).collect();
    let n = parts.len().max(1);
    let mut out = Vec::with_capacity(n);
    let mut prev = 0.0f32;
    for (i, part) in parts.iter().enumerate() {
        let (name, end) = match part.rsplit_once('@') {
            Some((name, at)) => (name.trim(), kit::number(at).map(|v| v as f32)),
            None => (*part, None),
        };
        let end = end.unwrap_or((i + 1) as f32 / n as f32).clamp(prev, 1.0);
        let text = name.split(" / ").map(str::trim).collect::<Vec<_>>().join("<br>");
        out.push(Stage {
            label: cx.label(&text)?,
            end,
        });
        prev = end;
    }
    if let Some(last) = out.last_mut() {
        last.end = 1.0;
    }
    Ok(out)
}

/// `Component -.- (x, y)`.
fn trend(stmt: &str) -> Option<(String, f32, f32)> {
    let (name, rest) = stmt.split_once("-.-")?;
    let rest = rest.trim();
    let inner = rest.strip_prefix('(')?.strip_suffix(')')?;
    let (x, y) = inner.split_once(',')?;
    Some((
        kit::unquote(name.trim()).to_string(),
        kit::number(x)? as f32,
        kit::number(y)? as f32,
    ))
}

fn link(stmt: &str, line: usize) -> Option<RawLink> {
    let bytes = stmt.as_bytes();
    let mut quote = false;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'"' {
            quote = !quote;
            continue;
        }
        if quote || !(b == b'-' || b == b'+') {
            continue;
        }
        let rest = &stmt[i..];
        let (kind, len, mut label) = if rest.starts_with("+<>") {
            (LinkKind::Flow { forward: true, back: true }, 3, None)
        } else if rest.starts_with("+<") {
            (LinkKind::Flow { forward: false, back: true }, 2, None)
        } else if rest.starts_with("+>") {
            (LinkKind::Flow { forward: true, back: false }, 2, None)
        } else if let Some(text) = rest.strip_prefix("+'") {
            let close = text.find("'>")?;
            (LinkKind::Flow { forward: true, back: false }, close + 4, Some(text[..close].to_string()))
        } else if rest.starts_with("-.->") {
            (LinkKind::Dashed, 4, None)
        } else if rest.starts_with("-->") {
            (LinkKind::Plain, 3, None)
        } else if rest.starts_with("->") {
            (LinkKind::Plain, 2, None)
        } else {
            continue;
        };
        let from = kit::unquote(stmt[..i].trim()).trim().to_string();
        let mut to = stmt[i + len..].trim();
        if let Some(semi) = to.find(';') {
            let text = to[semi + 1..].trim();
            if !text.is_empty() {
                label = Some(text.to_string());
            }
            to = to[..semi].trim();
        }
        let to = kit::unquote(to).trim().to_string();
        if from.is_empty() || to.is_empty() {
            return None;
        }
        return Some(RawLink {
            line,
            from,
            to,
            kind,
            label,
        });
    }
    None
}
