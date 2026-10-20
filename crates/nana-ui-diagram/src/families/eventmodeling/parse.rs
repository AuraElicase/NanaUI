//! The event-modeling grammar: `tf|timeframe NN type Name`, `rf|resetframe`
//! for a frame that starts a new flow, `->> NN` relations, `[[Data]]`
//! references, inline `` `type`{ ... } `` data and `data Name { ... }`
//! blocks.

use std::collections::HashMap;

use super::{EntityKind, Frame, Model};
use crate::families::ParseCx;
use crate::families::kit::{self, first_word, truncate};
use crate::{DiagramError, LimitKind};

/// Data lines shown per frame.
const MAX_DATA_LINES: usize = 24;

struct Pending {
    line: usize,
    number: u64,
    reset: bool,
    kind: EntityKind,
    entity: String,
    sources: Vec<u64>,
    data_ref: Option<String>,
    inline: Vec<String>,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let lines: Vec<&str> = body.lines().collect();
    let mut pending: Vec<Pending> = Vec::new();
    let mut blocks: HashMap<String, Vec<String>> = HashMap::new();
    // Data blocks may consume the lines after their statement.
    let mut i = kit::header(body)?.0 + 1;
    while i < lines.len() {
        let line_no = i;
        let stmt = lines[i].trim();
        i += 1;
        if stmt.is_empty() {
            continue;
        }
        let (word, rest) = first_word(stmt);
        match word {
            "tf" | "timeframe" | "rf" | "resetframe" => {
                let (number, rest) = first_word(rest);
                let (kind, rest) = first_word(rest);
                let (entity, mut rest) = first_word(rest);
                let (Ok(number), Some(kind)) = (number.parse::<u64>(), EntityKind::parse(kind)) else {
                    cx.warn(line_no, format!("cannot read `{}`", truncate(stmt)));
                    continue;
                };
                if entity.is_empty() || entity.starts_with(['[', '{', '`', '-']) {
                    cx.warn(line_no, "a frame needs an entity name");
                    continue;
                }
                cx.limits.check_label(entity)?;
                cx.limits.check_count(pending.len() + 1, cx.limits.elements.min(cx.limits.nodes), LimitKind::Elements)?;
                let mut frame = Pending {
                    line: line_no,
                    number,
                    reset: matches!(word, "rf" | "resetframe"),
                    kind,
                    entity: entity.to_string(),
                    sources: Vec::new(),
                    data_ref: None,
                    inline: Vec::new(),
                };
                loop {
                    rest = rest.trim_start();
                    if rest.is_empty() {
                        break;
                    }
                    if let Some(after) = rest.strip_prefix("->>") {
                        let (n, tail) = first_word(after);
                        match n.parse::<u64>() {
                            Ok(n) => frame.sources.push(n),
                            Err(_) => cx.warn(line_no, format!("cannot read relation `{}`", truncate(n))),
                        }
                        rest = tail;
                    } else if let Some(after) = rest.strip_prefix("[[") {
                        let Some(end) = after.find("]]") else {
                            cx.warn(line_no, "unclosed `[[`");
                            break;
                        };
                        frame.data_ref = Some(after[..end].trim().to_string());
                        rest = &after[end + 2..];
                    } else if rest.starts_with('`') || rest.starts_with('{') {
                        let body = skip_type(rest);
                        let Some(inner) = body.strip_prefix('{') else {
                            cx.warn(line_no, format!("ignored `{}`", truncate(rest)));
                            break;
                        };
                        match inner.rfind('}') {
                            Some(end) => frame.inline = data_lines(&inner[..end]),
                            None => {
                                // Data running over several lines.
                                let mut text = inner.to_string();
                                while i < lines.len() {
                                    let l = lines[i];
                                    i += 1;
                                    if let Some(end) = l.rfind('}') {
                                        text.push('\n');
                                        text.push_str(&l[..end]);
                                        break;
                                    }
                                    text.push('\n');
                                    text.push_str(l);
                                }
                                frame.inline = data_lines(&text);
                            }
                        }
                        break;
                    } else {
                        cx.warn(line_no, format!("ignored `{}`", truncate(rest)));
                        break;
                    }
                }
                for d in &frame.inline {
                    cx.limits.check_label(d)?;
                }
                pending.push(frame);
            }
            "data" => {
                let (name, rest) = first_word(rest);
                let name = name.split(['{', '`']).next().unwrap_or("").to_string();
                let after_name = stmt.find(&name).map_or(rest, |p| &stmt[p + name.len()..]);
                let body = skip_type(after_name.trim_start());
                let Some(inner) = body.strip_prefix('{') else {
                    cx.warn(line_no, format!("cannot read `{}`", truncate(stmt)));
                    continue;
                };
                let text = match inner.rfind('}') {
                    Some(end) => inner[..end].to_string(),
                    None => {
                        let mut text = inner.to_string();
                        while i < lines.len() {
                            let l = lines[i];
                            i += 1;
                            if l.trim() == "}" || l.trim_end().ends_with('}') {
                                if let Some(end) = l.rfind('}') {
                                    text.push('\n');
                                    text.push_str(&l[..end]);
                                }
                                break;
                            }
                            text.push('\n');
                            text.push_str(l);
                        }
                        text
                    }
                };
                let lines = data_lines(&text);
                for d in &lines {
                    cx.limits.check_label(d)?;
                }
                if name.is_empty() {
                    cx.warn(line_no, "a data block needs a name");
                } else {
                    blocks.insert(name, lines);
                }
            }
            _ => cx.warn(line_no, format!("cannot read `{}`", truncate(stmt))),
        }
    }
    // Time order, one frame per number.
    pending.sort_by_key(|p| p.number);
    let mut seen: HashMap<u64, usize> = HashMap::new();
    let mut kept: Vec<Pending> = Vec::new();
    for p in pending {
        if seen.contains_key(&p.number) {
            cx.warn(p.line, format!("frame {} is used twice", p.number));
            continue;
        }
        seen.insert(p.number, kept.len());
        kept.push(p);
    }
    let mut model = Model::default();
    for p in kept {
        let mut sources = Vec::new();
        for n in &p.sources {
            match seen.get(n) {
                Some(&s) => sources.push(s),
                None => cx.warn(p.line, format!("no frame {n}")),
            }
        }
        let mut data = p.inline;
        if let Some(r) = &p.data_ref {
            match blocks.get(r) {
                Some(lines) => data.extend(lines.iter().cloned()),
                None => cx.warn(p.line, format!("no data block `{}`", truncate(r))),
            }
        }
        data.truncate(MAX_DATA_LINES);
        let (namespace, name) = match p.entity.split_once('.') {
            Some((ns, name)) if !ns.is_empty() && !name.is_empty() => (Some(ns.to_string()), name.to_string()),
            _ => (None, p.entity.clone()),
        };
        model.frames.push(Frame {
            reset: p.reset,
            kind: p.kind,
            namespace,
            name,
            sources,
            data,
        });
    }
    Ok(model)
}

/// Skip a `` `type` `` prefix.
fn skip_type(s: &str) -> &str {
    if let Some(after) = s.strip_prefix('`')
        && let Some(end) = after.find('`')
    {
        return after[end + 1..].trim_start();
    }
    s
}

fn data_lines(text: &str) -> Vec<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty()).map(str::to_string).collect()
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
    fn frames_data_and_relations() {
        let m = parse_src(
            "eventmodeling\n\ntf 01 ui CartUI\ntf 02 cmd AddItem [[AddItem01]]\ntimeframe 03 event ItemAdded { description: string }\nrf 04 evt External.InventoryChanged\ntf 05 pcr InventoryProcessor\n\ndata AddItem01 {\n  description: 'john'\n  price: 20.4\n}\n",
        );
        assert_eq!(m.frames.len(), 5);
        assert_eq!(m.frames[1].data, vec!["description: 'john'", "price: 20.4"]);
        assert_eq!(m.frames[2].data, vec!["description: string"]);
        assert_eq!(m.frames[3].namespace.as_deref(), Some("External"));
        assert!(m.frames[3].reset);
        assert_eq!(m.relations(), vec![(0, 1), (1, 2), (3, 4)]);
    }

    #[test]
    fn explicit_relations_reach_back_and_forward_in_time() {
        let m = parse_src("eventmodeling\nrf 02 evt CartCreated\nrf 03 evt ItemAdded\ntf 01 rmo CartUI ->> 02 ->> 03");
        assert_eq!(m.frames[0].name, "CartUI");
        assert_eq!(m.relations(), vec![(1, 0), (2, 0)]);
    }
}
