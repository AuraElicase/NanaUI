//! The ER grammar: entities with aliases and attribute blocks (type, name,
//! `PK` / `FK` / `UK`, comment), relationships in the symbolic
//! (`||--o{`) and word (`1 to zero or more`) forms with labels,
//! `direction`, `classDef` / `class` / `:::` and `style`.

use std::collections::HashMap;

use super::{Attribute, Cardinality, Entity, Model, Relationship};
use crate::families::ParseCx;
use crate::families::graphkit::{Styles, plain_colon, split_classes};
use crate::families::kit::{self, first_word, ident_len, truncate, unquote};
use crate::layout::Direction;
use crate::{DiagramError, LimitKind};

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    entities: Vec<Entity>,
    index: HashMap<String, usize>,
    relationships: Vec<Relationship>,
    styles: Styles,
    direction: Direction,
    open: Option<usize>,
    attributes: usize,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut p = Parser {
        cx,
        entities: Vec::new(),
        index: HashMap::new(),
        relationships: Vec::new(),
        styles: Styles::default(),
        direction: Direction::TB,
        open: None,
        attributes: 0,
        line: 0,
    };
    let (header, rest) = kit::header(body)?;
    p.line = header;
    if !rest.is_empty() {
        p.statement(rest)?;
    }
    for (line, stmt) in kit::statements(body, header) {
        p.line = line;
        p.statement(stmt)?;
    }
    if p.open.is_some() {
        p.cx.warn(p.line, "an entity block is not closed");
    }
    for e in &mut p.entities {
        e.style = p.styles.resolve(&e.id, &e.classes);
    }
    Ok(Model {
        direction: p.direction,
        entities: p.entities,
        relationships: p.relationships,
    })
}

fn left_card(s: &str) -> Option<Cardinality> {
    Some(match s {
        "||" => Cardinality::ExactlyOne,
        "|o" | "o|" => Cardinality::ZeroOrOne,
        "}|" => Cardinality::OneOrMore,
        "}o" => Cardinality::ZeroOrMore,
        _ => return None,
    })
}

fn right_card(s: &str) -> Option<Cardinality> {
    Some(match s {
        "||" => Cardinality::ExactlyOne,
        "o|" | "|o" => Cardinality::ZeroOrOne,
        "|{" => Cardinality::OneOrMore,
        "o{" => Cardinality::ZeroOrMore,
        _ => return None,
    })
}

/// `||--o{` inside `head`: (left end, right start, from, to, identifying).
fn symbolic(head: &str) -> Option<(usize, usize, Cardinality, Cardinality, bool)> {
    let b = head.as_bytes();
    for i in 2..b.len().saturating_sub(3) {
        let line = &b[i..i + 2];
        let identifying = match line {
            b"--" => true,
            b".." | b".-" | b"-." => false,
            _ => continue,
        };
        if !head.is_char_boundary(i - 2) || !head.is_char_boundary(i + 4) {
            continue;
        }
        let (Some(from), Some(to)) = (left_card(&head[i - 2..i]), right_card(&head[i + 2..i + 4])) else {
            continue;
        };
        return Some((i - 2, i + 4, from, to, identifying));
    }
    None
}

/// `A only one to zero or more B`, with `optionally` before `to` for a
/// non-identifying one: (left, right, from, to, identifying).
fn worded(head: &str) -> Option<(&str, &str, Cardinality, Cardinality, bool)> {
    let words: Vec<&str> = head.split_whitespace().collect();
    let t = words.iter().position(|w| *w == "to").filter(|&t| t >= 2 && words.len() >= t + 3)?;
    let optional = words[t - 1] == "optionally";
    let from = word_card(&words[1..if optional { t - 1 } else { t }])?;
    let to = word_card(&words[t + 1..words.len() - 1])?;
    Some((words[0], words[words.len() - 1], from, to, !optional))
}

/// A cardinality in words: `only one`, `zero or more`, `1+`, ...
fn word_card(words: &[&str]) -> Option<Cardinality> {
    let phrase = words.join(" ").to_ascii_lowercase();
    Some(match phrase.as_str() {
        "only one" | "1" | "one" | "exactly one" => Cardinality::ExactlyOne,
        "zero or one" | "one or zero" => Cardinality::ZeroOrOne,
        "one or more" | "one or many" | "many(1)" | "1+" => Cardinality::OneOrMore,
        "zero or more" | "zero or many" | "many(0)" | "0+" => Cardinality::ZeroOrMore,
        _ => return None,
    })
}

/// Whitespace-separated tokens, `"..."` kept whole with its quotes.
fn tokens(s: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = s.trim_start();
    while !rest.is_empty() {
        let end = if let Some(body) = rest.strip_prefix('"') {
            body.find('"').map_or(rest.len(), |e| e + 2)
        } else {
            rest.find(char::is_whitespace).unwrap_or(rest.len())
        };
        out.push(&rest[..end]);
        rest = rest[end..].trim_start();
    }
    out
}

impl Parser<'_, '_> {
    /// The entity `raw` names: `NAME`, `NAME["alias"]`, `"quoted"`, with
    /// optional `:::classes`.
    fn entity(&mut self, raw: &str) -> Result<Option<usize>, DiagramError> {
        let (raw, classes) = split_classes(raw);
        let (id, alias) = if let Some(open) = raw.find('[').filter(|_| raw.ends_with(']')) {
            (raw[..open].trim(), Some(unquote(&raw[open + 1..raw.len() - 1]).to_string()))
        } else {
            (raw, None)
        };
        let id = if let Some(quoted) = id.strip_prefix('"').and_then(|q| q.strip_suffix('"')) {
            quoted
        } else {
            let len = ident_len(id, "-");
            if len == 0 || len != id.len() {
                return Ok(None);
            }
            id
        };
        if id.trim().is_empty() {
            return Ok(None);
        }
        let e = match self.index.get(id) {
            Some(&e) => e,
            None => {
                self.cx.limits.check_count(self.entities.len() + 1, self.cx.limits.nodes, LimitKind::Nodes)?;
                self.cx.limits.check_label(id)?;
                let e = self.entities.len();
                self.index.insert(id.to_string(), e);
                self.entities.push(Entity {
                    id: id.to_string(),
                    label: id.to_string(),
                    attributes: Vec::new(),
                    classes: Vec::new(),
                    style: Default::default(),
                });
                e
            }
        };
        if let Some(alias) = alias.filter(|a| !a.trim().is_empty()) {
            self.cx.limits.check_label(&alias)?;
            self.entities[e].label = alias;
        }
        self.entities[e].classes.extend(classes);
        Ok(Some(e))
    }

    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        if let Some(e) = self.open {
            let body = stmt.strip_suffix('}');
            if body.is_some() {
                self.open = None;
            }
            return self.attribute(e, body.unwrap_or(stmt));
        }
        let (word, rest) = first_word(stmt);
        match word {
            "direction" => {
                match Direction::parse(rest) {
                    Some(d) => self.direction = d,
                    None => self.cx.warn(self.line, "unknown direction"),
                }
                return Ok(());
            }
            "classDef" | "class" | "style" => {
                self.styles.statement(word, rest);
                return Ok(());
            }
            _ => {}
        }
        if let Some(brace) = stmt.find('{')
            && symbolic(stmt).is_none()
        {
            let Some(e) = self.entity(stmt[..brace].trim())? else {
                self.cx.warn(self.line, format!("cannot read entity `{}`", truncate(stmt)));
                return Ok(());
            };
            let body = &stmt[brace + 1..];
            match body.rfind('}') {
                Some(end) => {
                    let inner = body[..end].trim();
                    if !inner.is_empty() {
                        self.attribute(e, inner)?;
                    }
                }
                None => self.open = Some(e),
            }
            return Ok(());
        }
        if self.relationship(stmt)? {
            return Ok(());
        }
        if self.entity(stmt)?.is_none() {
            self.cx.warn(self.line, format!("cannot read `{}`", truncate(stmt)));
        }
        Ok(())
    }

    fn attribute(&mut self, e: usize, line: &str) -> Result<(), DiagramError> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }
        let toks = tokens(line);
        if toks.len() < 2 || toks[0].starts_with('"') {
            self.cx.warn(self.line, format!("cannot read attribute `{}`", truncate(line)));
            return Ok(());
        }
        self.cx.limits.check_label(line)?;
        self.attributes += 1;
        self.cx.limits.check_count(self.attributes, self.cx.limits.elements, LimitKind::Elements)?;
        let mut keys = Vec::new();
        let mut comment = None;
        for tok in &toks[2..] {
            if tok.starts_with('"') {
                comment = Some(unquote(tok).to_string());
                continue;
            }
            for k in tok.split(',').map(str::trim).filter(|k| !k.is_empty()) {
                let k = k.to_ascii_uppercase();
                if matches!(k.as_str(), "PK" | "FK" | "UK") && !keys.contains(&k) {
                    keys.push(k);
                }
            }
        }
        self.entities[e].attributes.push(Attribute {
            ty: toks[0].to_string(),
            name: toks[1].to_string(),
            keys,
            comment: comment.filter(|c| !c.is_empty()),
        });
        Ok(())
    }

    /// A relationship statement. Returns whether `stmt` was one.
    fn relationship(&mut self, stmt: &str) -> Result<bool, DiagramError> {
        let colon = plain_colon(stmt);
        let head = &stmt[..colon.unwrap_or(stmt.len())];
        let parsed = match symbolic(head) {
            Some((a, b, from, to, identifying)) => Some((head[..a].trim(), head[b..].trim(), from, to, identifying)),
            None => worded(head),
        };
        let Some((left, right, from_card, to_card, identifying)) = parsed else {
            return Ok(false);
        };
        let (Some(from), Some(to)) = (self.entity(left)?, self.entity(right)?) else {
            self.cx.warn(self.line, "a relationship needs an entity on each side");
            return Ok(true);
        };
        self.cx.limits.check_count(self.relationships.len() + 1, self.cx.limits.edges, LimitKind::Edges)?;
        let label = colon.map(|c| unquote(&stmt[c + 1..])).filter(|l| !l.is_empty()).map(|l| self.cx.label(l)).transpose()?;
        self.relationships.push(Relationship {
            from,
            to,
            from_card,
            to_card,
            identifying,
            label,
        });
        Ok(true)
    }
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
    fn entities_attributes_and_aliases() {
        let m = parse_src(
            "erDiagram\nCUSTOMER[\"Customer account\"] {\n  string name \"full name\"\n  int id PK, FK\n  varchar(255) email UK \"login\"\n}\nLINE-ITEM {}\np[Person]:::warm",
        );
        let c = &m.entities[0];
        assert_eq!(c.label, "Customer account");
        assert_eq!(c.attributes.len(), 3);
        assert_eq!(c.attributes[0].comment.as_deref(), Some("full name"));
        assert_eq!(c.attributes[1].keys, vec!["PK".to_string(), "FK".to_string()]);
        assert_eq!(c.attributes[2].ty, "varchar(255)");
        assert_eq!(m.entities[1].id, "LINE-ITEM");
        assert_eq!(m.entities[2].label, "Person");
        assert_eq!(m.entities[2].classes, vec!["warm".to_string()]);
    }

    #[test]
    fn symbolic_and_word_relationships() {
        let m = parse_src(
            "erDiagram\nA ||--o{ B : places\nB }|..|| C : \"is in\"\nC |o--|{ D : x\nD }o--o| E : y\nCAR 1 to zero or more DRIVER : allows\nP only one optionally to one or more Q : has",
        );
        let r = &m.relationships;
        assert_eq!(r.len(), 6);
        assert_eq!((r[0].from_card, r[0].to_card, r[0].identifying), (Cardinality::ExactlyOne, Cardinality::ZeroOrMore, true));
        assert_eq!((r[1].from_card, r[1].to_card, r[1].identifying), (Cardinality::OneOrMore, Cardinality::ExactlyOne, false));
        assert_eq!(r[1].label.as_ref().map(|l| l.text()), Some("is in".into()));
        assert_eq!((r[2].from_card, r[2].to_card), (Cardinality::ZeroOrOne, Cardinality::OneOrMore));
        assert_eq!((r[3].from_card, r[3].to_card), (Cardinality::ZeroOrMore, Cardinality::ZeroOrOne));
        assert_eq!((r[4].from_card, r[4].to_card, r[4].identifying), (Cardinality::ExactlyOne, Cardinality::ZeroOrMore, true));
        assert!(!r[5].identifying);
        assert_eq!(r[5].to_card, Cardinality::OneOrMore);
    }
}
