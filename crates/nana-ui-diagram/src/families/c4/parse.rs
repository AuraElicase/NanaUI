//! The C4-PlantUML subset Mermaid reads: element, boundary and
//! relationship macros with positional or `$named` arguments, `{ ... }`
//! nesting, `title`, and the `Update*Style` / `UpdateLayoutConfig` calls.

use std::collections::HashMap;

use super::{Boundary, BoundaryKind, Colors, ElemKind, Element, End, Model, Rel, Variant};
use crate::families::ParseCx;
use crate::families::kit::{self, balanced, keyword, split_top, truncate, unquote};
use crate::source::color;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

/// Arguments of one macro call.
struct Args {
    positional: Vec<String>,
    named: HashMap<String, String>,
}

impl Args {
    /// The `$name` argument, or the positional one at `index`.
    fn get(&self, index: usize, name: &str) -> Option<&str> {
        self.named
            .get(&name.to_ascii_lowercase())
            .or_else(|| self.positional.get(index))
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
    }
}

/// Split `a, "b, c", $d="e"` into arguments.
fn arguments(text: &str) -> Args {
    let mut args = Args {
        positional: Vec::new(),
        named: HashMap::new(),
    };
    for part in split_top(text, ',') {
        let part = part.trim();
        if let Some(named) = part.strip_prefix('$')
            && let Some((key, value)) = named.split_once('=')
        {
            args.named.insert(key.trim().to_ascii_lowercase(), unquote(value).to_string());
            continue;
        }
        args.positional.push(unquote(part).to_string());
    }
    if args.positional.len() == 1 && args.positional[0].is_empty() {
        args.positional.clear();
    }
    args
}

/// `Name(args)` and whatever follows the closing parenthesis.
fn call(stmt: &str) -> Option<(&str, &str, &str)> {
    let name_end = stmt.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
    let name = &stmt[..name_end];
    let rest = stmt[name_end..].trim_start();
    if !rest.starts_with('(') {
        return None;
    }
    let close = balanced(rest, '(', ')')?;
    Some((name, &rest[1..close], rest[close + 1..].trim()))
}

fn element_kind(name: &str) -> Option<(ElemKind, Variant, bool)> {
    let (kind, rest) = if let Some(r) = name.strip_prefix("Person") {
        (ElemKind::Person, r)
    } else if let Some(r) = name.strip_prefix("System") {
        (ElemKind::System, r)
    } else if let Some(r) = name.strip_prefix("Container") {
        (ElemKind::Container, r)
    } else {
        (ElemKind::Component, name.strip_prefix("Component")?)
    };
    let (variant, rest) = if let Some(r) = rest.strip_prefix("Db") {
        (Variant::Db, r)
    } else if let Some(r) = rest.strip_prefix("Queue") {
        (Variant::Queue, r)
    } else {
        (Variant::Plain, rest)
    };
    if kind == ElemKind::Person && variant != Variant::Plain {
        return None;
    }
    match rest {
        "" => Some((kind, variant, false)),
        "_Ext" => Some((kind, variant, true)),
        _ => None,
    }
}

fn boundary_kind(name: &str) -> Option<BoundaryKind> {
    Some(match name {
        "Boundary" => BoundaryKind::Generic,
        "Enterprise_Boundary" => BoundaryKind::Enterprise,
        "System_Boundary" => BoundaryKind::System,
        "Container_Boundary" => BoundaryKind::Container,
        "Deployment_Node" | "Node" | "Node_L" | "Node_R" => BoundaryKind::Node,
        _ => return None,
    })
}

fn colors(args: &Args, first: usize) -> Colors {
    Colors {
        bg: args.get(first, "bgColor").and_then(color::parse),
        font: args.get(first + 1, "fontColor").and_then(color::parse),
        border: args.get(first + 2, "borderColor").and_then(color::parse),
    }
}

/// `UpdateRelStyle`: from, to, text colour, line colour.
type RelStyle = (String, String, Option<[f32; 4]>, Option<[f32; 4]>);

struct PendingRel {
    line: usize,
    from: String,
    to: String,
    written: (String, String),
    label: Option<Label>,
    techn: Option<Label>,
    both: bool,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model::default();
    let mut stack: Vec<usize> = Vec::new();
    let mut opened: Option<usize> = None;
    let mut aliases: HashMap<String, End> = HashMap::new();
    let mut rels: Vec<PendingRel> = Vec::new();
    let mut element_styles: Vec<(String, Colors)> = Vec::new();
    let mut rel_styles: Vec<RelStyle> = Vec::new();
    let (header, _) = kit::header(body)?;
    for (line_no, mut stmt) in kit::statements(body, header) {
        // Closing braces, possibly several, possibly before a statement.
        while let Some(rest) = stmt.strip_prefix('}') {
            if stack.pop().is_none() {
                cx.warn(line_no, "`}` without a boundary");
            }
            stmt = rest.trim_start();
        }
        if stmt == "{" {
            match opened.take() {
                Some(b) => stack.push(b),
                None => cx.warn(line_no, "`{` without a boundary"),
            }
            continue;
        }
        if stmt.is_empty() {
            continue;
        }
        opened = None;
        if let Some(title) = keyword(stmt, "title").filter(|t| !t.is_empty()) {
            cx.meta.title = Some(title.to_string());
            continue;
        }
        let Some((name, inner, tail)) = call(stmt) else {
            cx.warn(line_no, format!("cannot read `{}`", truncate(stmt)));
            continue;
        };
        let args = arguments(inner);
        let opens = tail.starts_with('{');
        if !tail.is_empty() && !opens {
            cx.warn(line_no, format!("ignored `{}`", truncate(tail)));
        }
        let parent = stack.last().copied();
        let lower = name.to_ascii_lowercase();
        if let Some((kind, variant, external)) = element_kind(name) {
            let (Some(alias), Some(label)) = (args.get(0, "alias"), args.get(1, "label")) else {
                cx.warn(line_no, format!("`{name}` needs an alias and a label"));
                continue;
            };
            cx.limits.check_count(model.elements.len() + 1, cx.limits.nodes.min(crate::layout::grid::MAX_ROUTED), LimitKind::Nodes)?;
            let with_techn = matches!(kind, ElemKind::Container | ElemKind::Component);
            let techn = if with_techn { args.get(2, "techn") } else { args.named.get("techn").map(String::as_str) };
            let descr = args.get(if with_techn { 3 } else { 2 }, "descr");
            let label = cx.label(label)?;
            let techn = techn.map(|t| cx.label(t)).transpose()?;
            let descr = descr.map(|t| cx.label(t)).transpose()?;
            aliases.insert(alias.to_string(), End::Element(model.elements.len()));
            model.elements.push(Element {
                label,
                kind,
                variant,
                external,
                techn,
                descr,
                parent,
                colors: Colors::default(),
                link: args.named.get("link").cloned(),
            });
            if opens {
                cx.warn(line_no, "an element cannot hold others");
            }
            continue;
        }
        if let Some(kind) = boundary_kind(name) {
            let (Some(alias), Some(label)) = (args.get(0, "alias"), args.get(1, "label")) else {
                cx.warn(line_no, format!("`{name}` needs an alias and a label"));
                continue;
            };
            cx.limits.check_count(model.boundaries.len() + 1, cx.limits.clusters, LimitKind::Clusters)?;
            if stack.len() + 1 > cx.limits.cluster_depth {
                return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
            }
            let type_text = match kind {
                BoundaryKind::Generic | BoundaryKind::Node => args.get(2, "type").map(str::to_string),
                BoundaryKind::Enterprise => Some("Enterprise".to_string()),
                BoundaryKind::System => Some("System".to_string()),
                BoundaryKind::Container => Some("Container".to_string()),
            };
            let descr = match kind {
                BoundaryKind::Node => args.get(3, "descr").map(|d| cx.label(d)).transpose()?,
                _ => None,
            };
            let index = model.boundaries.len();
            aliases.insert(alias.to_string(), End::Boundary(index));
            model.boundaries.push(Boundary {
                label: cx.label(label)?,
                kind,
                type_text,
                descr,
                parent,
                colors: Colors::default(),
            });
            if opens {
                stack.push(index);
            } else {
                opened = Some(index);
            }
            continue;
        }
        match lower.as_str() {
            "rel" | "birel" | "rel_u" | "rel_up" | "rel_d" | "rel_down" | "rel_l" | "rel_left" | "rel_r" | "rel_right" | "rel_back" | "rel_neighbor" | "relindex" => {
                let shift = usize::from(lower == "relindex");
                let (Some(a), Some(b)) = (args.get(shift, "from"), args.get(shift + 1, "to")) else {
                    cx.warn(line_no, format!("`{name}` needs two aliases"));
                    continue;
                };
                cx.limits.check_count(rels.len() + 1, cx.limits.edges.min(crate::layout::grid::MAX_ROUTED), LimitKind::Edges)?;
                let label = args.get(shift + 2, "label").map(|l| cx.label(l)).transpose()?;
                let techn = args.get(shift + 3, "techn").map(|l| cx.label(l)).transpose()?;
                let written = (a.to_string(), b.to_string());
                let (from, to) = if lower == "rel_back" { (b, a) } else { (a, b) };
                rels.push(PendingRel {
                    line: line_no,
                    from: from.to_string(),
                    to: to.to_string(),
                    written,
                    label,
                    techn,
                    both: lower == "birel",
                });
            }
            "updateelementstyle" | "updateboundarystyle" => {
                if let Some(alias) = args.get(0, "elementName").or_else(|| args.get(0, "boundaryName")) {
                    element_styles.push((alias.to_string(), colors(&args, 1)));
                }
            }
            "updaterelstyle" => {
                if let (Some(a), Some(b)) = (args.get(0, "from"), args.get(1, "to")) {
                    let text = args.get(2, "textColor").and_then(color::parse);
                    let line = args.get(3, "lineColor").and_then(color::parse);
                    rel_styles.push((a.to_string(), b.to_string(), text, line));
                }
            }
            "updatelayoutconfig" => {
                let read = |v: Option<&str>| v.and_then(|v| v.trim().parse::<usize>().ok()).filter(|&n| n > 0).map(|n| n.min(64));
                if let Some(n) = read(args.get(0, "c4ShapeInRow")) {
                    model.shape_in_row = n;
                }
                if let Some(n) = read(args.get(1, "c4BoundaryInRow")) {
                    model.boundary_in_row = n;
                }
            }
            _ => cx.warn(line_no, format!("`{name}` is not supported")),
        }
    }
    for rel in rels {
        let (Some(&from), Some(&to)) = (aliases.get(&rel.from), aliases.get(&rel.to)) else {
            cx.warn(rel.line, format!("a relationship to an unknown alias `{}` / `{}`", truncate(&rel.from), truncate(&rel.to)));
            continue;
        };
        model.rels.push(Rel {
            from,
            to,
            written: rel.written,
            label: rel.label,
            techn: rel.techn,
            both: rel.both,
            text_color: None,
            line_color: None,
        });
    }
    for (alias, c) in element_styles {
        match aliases.get(&alias) {
            Some(End::Element(i)) => merge(&mut model.elements[*i].colors, c),
            Some(End::Boundary(i)) => merge(&mut model.boundaries[*i].colors, c),
            None => {}
        }
    }
    for (a, b, text, line) in rel_styles {
        for rel in model.rels.iter_mut().filter(|r| r.written.0 == a && r.written.1 == b) {
            rel.text_color = text.or(rel.text_color);
            rel.line_color = line.or(rel.line_color);
        }
    }
    Ok(model)
}

fn merge(into: &mut Colors, c: Colors) {
    into.bg = c.bg.or(into.bg);
    into.font = c.font.or(into.font);
    into.border = c.border.or(into.border);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Meta;

    fn parse_src(src: &str) -> (Model, Meta) {
        let (model, warnings, meta) = crate::families::parse_for_test(src, parse);
        assert!(warnings.is_empty(), "{warnings:?}");
        (model.unwrap(), meta)
    }

    #[test]
    fn elements_boundaries_and_relationships() {
        let (m, meta) = parse_src(
            "C4Container\ntitle Containers\nPerson(customer, Customer, \"A customer, with accounts\", $tags=\"v1.0\")\nContainer_Boundary(c1, \"Banking\") {\n  Container(spa, \"SPA\", \"JavaScript, Angular\", \"The app\")\n  ContainerDb_Ext(db, \"DB\", \"SQL\")\n  Boundary(b2, \"Inner\", \"group\")\n  {\n    ComponentQueue(q, \"Queue\")\n  }\n}\nSystem_Ext(mail, \"Mail\")\nRel(customer, spa, \"Uses\", \"HTTPS\")\nRel_Back(db, spa, \"Reads\")\nBiRel(spa, mail, \"Talks\")\nRelIndex(1, q, db, \"Stores\")\nUpdateElementStyle(customer, $bgColor=\"grey\", $fontColor=\"red\")\nUpdateRelStyle(db, spa, $lineColor=\"blue\", $offsetX=\"5\")\nUpdateLayoutConfig($c4ShapeInRow=\"3\", $c4BoundaryInRow=\"1\")",
        );
        assert_eq!(meta.title.as_deref(), Some("Containers"));
        assert_eq!(m.elements.len(), 5);
        assert_eq!(m.boundaries.len(), 2);
        assert_eq!(m.boundaries[1].parent, Some(0));
        assert_eq!(m.elements[3].parent, Some(1));
        assert_eq!(m.elements[1].techn.as_ref().map(|t| t.text()), Some("JavaScript, Angular".into()));
        assert_eq!(m.elements[0].descr.as_ref().map(|t| t.text()), Some("A customer, with accounts".into()));
        assert_eq!(m.elements[2].stereotype(), "<<external_container_db>>");
        assert_eq!(m.elements[3].stereotype(), "<<component_queue>>");
        assert_eq!(m.rels.len(), 4);
        assert_eq!((m.rels[1].from, m.rels[1].to), (End::Element(1), End::Element(2)));
        assert!(m.rels[1].line_color.is_some());
        assert!(m.rels[2].both);
        assert_eq!(m.rels[3].label.as_ref().map(|l| l.text()), Some("Stores".into()));
        assert!(m.elements[0].colors.bg.is_some() && m.elements[0].colors.font.is_some());
        assert_eq!((m.shape_in_row, m.boundary_in_row), (3, 1));
    }
}
