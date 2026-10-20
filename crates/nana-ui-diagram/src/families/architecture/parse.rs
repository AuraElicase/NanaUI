//! The architecture grammar: `group id(icon)[title] in parent`,
//! `service id(icon)[title] in parent`, `junction id in parent`, edges
//! `a{group}:R <--> L:b{group}` with an optional `-[label]-`, and
//! `align row|column a b ...`.

use std::collections::HashMap;

use super::{Align, Edge, Group, Model, Service};
use crate::families::ParseCx;
use crate::families::kit::{self, first_word, truncate};
use crate::layout::grid::Side;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model::default();
    let mut groups: HashMap<String, usize> = HashMap::new();
    let mut services: HashMap<String, usize> = HashMap::new();
    let mut pending: Vec<(usize, PendingEdge)> = Vec::new();
    let mut aligns: Vec<(usize, bool, Vec<String>)> = Vec::new();
    let (header, _) = kit::header(body)?;
    for (line_no, trimmed) in kit::statements(body, header) {
        if let Some(title) = trimmed.strip_prefix("title ") {
            cx.meta.title = Some(title.trim().to_string());
            continue;
        }
        let (word, rest) = first_word(trimmed);
        match word {
            "group" | "service" | "junction" => {
                let Some(decl) = declaration(rest, word == "junction") else {
                    cx.warn(line_no, format!("cannot read `{}`", truncate(trimmed)));
                    continue;
                };
                let parent = match &decl.parent {
                    Some(p) => match groups.get(p) {
                        Some(&g) => Some(g),
                        None => {
                            cx.warn(line_no, format!("unknown group `{}`", truncate(p)));
                            None
                        }
                    },
                    None => None,
                };
                let title = match &decl.title {
                    Some(t) => cx.label(t)?,
                    None => Label::plain(&decl.id),
                };
                if word == "group" {
                    cx.limits.check_count(
                        model.groups.len() + 1,
                        cx.limits.clusters,
                        LimitKind::Clusters,
                    )?;
                    let mut depth = 1;
                    let mut at = parent;
                    while let Some(p) = at {
                        depth += 1;
                        at = model.groups[p].parent;
                    }
                    if depth > cx.limits.cluster_depth {
                        return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
                    }
                    if groups.contains_key(&decl.id) {
                        cx.warn(
                            line_no,
                            format!("group `{}` declared twice", truncate(&decl.id)),
                        );
                        continue;
                    }
                    groups.insert(decl.id, model.groups.len());
                    model.groups.push(Group {
                        title,
                        icon: decl.icon,
                        parent,
                    });
                } else {
                    cx.limits.check_count(
                        model.services.len() + 1,
                        cx.limits.nodes,
                        LimitKind::Nodes,
                    )?;
                    if services.contains_key(&decl.id) {
                        cx.warn(line_no, format!("`{}` declared twice", truncate(&decl.id)));
                        continue;
                    }
                    services.insert(decl.id, model.services.len());
                    let junction = word == "junction";
                    model.services.push(Service {
                        title: if junction { Label::default() } else { title },
                        icon: decl.icon,
                        junction,
                        parent,
                    });
                }
            }
            "align" => {
                let mut words = rest.split_whitespace();
                let row = match words.next() {
                    Some("row") => true,
                    Some("column") => false,
                    _ => {
                        cx.warn(line_no, "`align` needs `row` or `column`");
                        continue;
                    }
                };
                aligns.push((line_no, row, words.map(str::to_string).collect()));
            }
            _ => match edge(trimmed) {
                Some(e) => {
                    cx.limits
                        .check_count(pending.len() + 1, cx.limits.edges, LimitKind::Edges)?;
                    pending.push((line_no, e));
                }
                None => cx.warn(line_no, format!("cannot read `{}`", truncate(trimmed))),
            },
        }
    }
    for (line_no, e) in pending {
        let (Some(&from), Some(&to)) = (services.get(&e.from), services.get(&e.to)) else {
            cx.warn(
                line_no,
                format!(
                    "an edge between unknown services `{}` and `{}`",
                    truncate(&e.from),
                    truncate(&e.to)
                ),
            );
            continue;
        };
        let label = match &e.label {
            Some(t) => Some(cx.label(t)?).filter(|l| !l.is_empty()),
            None => None,
        };
        model.edges.push(Edge {
            from,
            to,
            from_side: e.from_side,
            to_side: e.to_side,
            from_group: e.from_group && model.services[from].parent.is_some(),
            to_group: e.to_group && model.services[to].parent.is_some(),
            start_arrow: e.start_arrow,
            end_arrow: e.end_arrow,
            label,
        });
    }
    for (line_no, row, names) in aligns {
        let members: Vec<usize> = names
            .iter()
            .filter_map(|n| services.get(n).copied())
            .collect();
        if members.len() < 2 || members.len() != names.len() {
            cx.warn(line_no, "`align` needs at least two declared services");
            continue;
        }
        model.aligns.push(Align { row, members });
    }
    Ok(model)
}

struct Decl {
    id: String,
    icon: Option<String>,
    title: Option<String>,
    parent: Option<String>,
}

/// `id(icon)[title] in parent`; junctions take neither icon nor title.
fn declaration(rest: &str, junction: bool) -> Option<Decl> {
    let rest = rest.trim();
    let end = rest
        .find(|c: char| c.is_whitespace() || c == '(' || c == '[')
        .unwrap_or(rest.len());
    let id = &rest[..end];
    if id.is_empty() {
        return None;
    }
    let mut tail = &rest[end..];
    let mut icon = None;
    let mut title = None;
    if !junction {
        if let Some(body) = tail.strip_prefix('(') {
            let close = body.find(')')?;
            icon =
                Some(body[..close].trim().trim_matches('"').to_string()).filter(|s| !s.is_empty());
            tail = &body[close + 1..];
        }
        if let Some(body) = tail.strip_prefix('[') {
            let close = body.rfind(']')?;
            title = Some(body[..close].to_string());
            tail = &body[close + 1..];
        }
    }
    let tail = tail.trim();
    let parent = if tail.is_empty() {
        None
    } else {
        let p = tail.strip_prefix("in")?;
        if !p.starts_with(char::is_whitespace) {
            return None;
        }
        Some(p.trim().to_string()).filter(|p| !p.is_empty() && !p.contains(char::is_whitespace))
    };
    if !tail.is_empty() && parent.is_none() {
        return None;
    }
    Some(Decl {
        id: id.to_string(),
        icon,
        title,
        parent,
    })
}

struct PendingEdge {
    from: String,
    to: String,
    from_side: Side,
    to_side: Side,
    from_group: bool,
    to_group: bool,
    start_arrow: bool,
    end_arrow: bool,
    label: Option<String>,
}

/// `a{group}:R <-[label]-> L:b{group}`.
fn edge(stmt: &str) -> Option<PendingEdge> {
    let left_colon = stmt.find(':')?;
    let right_colon = stmt.rfind(':')?;
    if right_colon <= left_colon + 1 {
        return None;
    }
    let (from, from_group) = endpoint(&stmt[..left_colon])?;
    let (to, to_group) = endpoint(&stmt[right_colon + 1..])?;
    let middle = stmt[left_colon + 1..right_colon].trim();
    let mut chars = middle.chars();
    let from_side = Side::parse(chars.next()?)?;
    let to_side = Side::parse(chars.next_back()?)?;
    let arrow = chars.as_str().trim();
    let start_arrow = arrow.starts_with('<');
    let end_arrow = arrow.ends_with('>');
    let inner = arrow.trim_start_matches('<').trim_end_matches('>');
    if !inner.starts_with('-') || !inner.ends_with('-') || inner.len() < 2 {
        return None;
    }
    let label = if let Some(body) = inner.strip_prefix("-[").and_then(|b| b.strip_suffix("]-")) {
        Some(body.to_string())
    } else if inner.chars().all(|c| c == '-') {
        None
    } else {
        return None;
    };
    Some(PendingEdge {
        from,
        to,
        from_side,
        to_side,
        from_group,
        to_group,
        start_arrow,
        end_arrow,
        label,
    })
}

fn endpoint(text: &str) -> Option<(String, bool)> {
    let text = text.trim();
    let (id, group) = match text.strip_suffix("{group}") {
        Some(id) => (id.trim(), true),
        None => (text, false),
    };
    if id.is_empty() || id.contains(|c: char| c.is_whitespace() || "{}[]()<>".contains(c)) {
        return None;
    }
    Some((id.to_string(), group))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_src(src: &str) -> Model {
        let (model, warnings, _) = crate::families::parse_for_test(src, parse);
        assert!(warnings.is_empty(), "{warnings:?}");
        model.unwrap()
    }

    #[test]
    fn declarations_edges_and_aligns() {
        let m = parse_src(
            "architecture-beta\ngroup api(cloud)[Public API]\ngroup inner[Inner] in api\nservice db(database)[My Database] in inner\nservice server[Server] in api\njunction j in api\ndb:L -- R:server\nserver{group}:B <--> T:j\ndb:T -[reads]-> B:j\nalign row db server",
        );
        assert_eq!(m.groups.len(), 2);
        assert_eq!(m.groups[1].parent, Some(0));
        assert_eq!(m.services[0].icon.as_deref(), Some("database"));
        assert_eq!(m.services[0].title.text(), "My Database");
        assert_eq!(m.services[1].icon, None);
        assert!(m.services[2].junction);
        assert_eq!(m.edges.len(), 3);
        let e = &m.edges[1];
        assert_eq!(
            (
                e.from_side,
                e.to_side,
                e.from_group,
                e.start_arrow,
                e.end_arrow
            ),
            (Side::Bottom, Side::Top, true, true, true)
        );
        assert_eq!(
            m.edges[2].label.as_ref().map(|l| l.text()),
            Some("reads".into())
        );
        assert!(!m.edges[0].start_arrow && !m.edges[0].end_arrow);
        assert_eq!(m.aligns.len(), 1);
    }
}
