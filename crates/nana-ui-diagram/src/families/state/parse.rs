//! The state diagram grammar: states with labels and descriptions, `[*]`,
//! transitions with labels, composite states (nested, with their own
//! `direction` and `--` concurrent regions), `<<fork>>` / `<<join>>` /
//! `<<choice>>`, notes, `classDef` / `class` / `:::` and `style`.

use std::collections::HashMap;

use super::{Composite, Kind, Model, Note, Scope, State, Transition};
use crate::families::ParseCx;
use crate::families::graphkit::{Styles, plain_colon, split_classes};
use crate::families::kit::{self, first_word, ident_len, truncate, unquote};
use crate::layout::Direction;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

struct PendingNote {
    target: Option<usize>,
    left: bool,
    lines: Vec<String>,
}

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    states: Vec<State>,
    index: HashMap<String, usize>,
    pseudo: HashMap<(Scope, bool), usize>,
    composites: Vec<Composite>,
    stack: Vec<Scope>,
    transitions: Vec<Transition>,
    notes: Vec<Note>,
    note: Option<PendingNote>,
    styles: Styles,
    direction: Direction,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut p = Parser {
        cx,
        states: Vec::new(),
        index: HashMap::new(),
        pseudo: HashMap::new(),
        composites: Vec::new(),
        stack: Vec::new(),
        transitions: Vec::new(),
        notes: Vec::new(),
        note: None,
        styles: Styles::default(),
        direction: Direction::TB,
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
    if p.note.is_some() {
        p.cx.warn(p.line, "a note is not closed with `end note`");
        p.close_note()?;
    }
    if !p.stack.is_empty() {
        p.cx.warn(p.line, "a composite state is not closed");
    }
    Ok(p.finish())
}

impl Parser<'_, '_> {
    fn scope(&self) -> Scope {
        self.stack.last().copied().unwrap_or_default()
    }

    fn check_nodes(&self) -> Result<(), DiagramError> {
        let count = self.states.len() + self.notes.len() + 1;
        self.cx.limits.check_count(count, self.cx.limits.nodes, LimitKind::Nodes)
    }

    /// The state `raw` names (`[*]` is the scope's start or end).
    fn state(&mut self, raw: &str, as_target: bool) -> Result<Option<usize>, DiagramError> {
        let (raw, classes) = split_classes(raw);
        if raw == "[*]" {
            let scope = self.scope();
            if let Some(&s) = self.pseudo.get(&(scope, as_target)) {
                return Ok(Some(s));
            }
            self.check_nodes()?;
            let s = self.states.len();
            let suffix = if as_target { "end" } else { "start" };
            self.states.push(State {
                id: format!("[*]{suffix}{s}"),
                label: None,
                descriptions: Vec::new(),
                kind: if as_target { Kind::End } else { Kind::Start },
                scope,
                composite: None,
                classes,
                style: Default::default(),
            });
            self.pseudo.insert((scope, as_target), s);
            return Ok(Some(s));
        }
        let len = ident_len(raw, "-.");
        if len == 0 || len != raw.len() {
            return Ok(None);
        }
        let s = match self.index.get(raw) {
            Some(&s) => s,
            None => {
                self.check_nodes()?;
                self.cx.limits.check_label(raw)?;
                let s = self.states.len();
                self.index.insert(raw.to_string(), s);
                self.states.push(State {
                    id: raw.to_string(),
                    label: None,
                    descriptions: Vec::new(),
                    kind: Kind::Normal,
                    scope: self.scope(),
                    composite: None,
                    classes: Vec::new(),
                    style: Default::default(),
                });
                s
            }
        };
        self.states[s].classes.extend(classes);
        Ok(Some(s))
    }

    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        if let Some(note) = &mut self.note {
            if stmt == "end note" || stmt == "endnote" {
                return self.close_note();
            }
            note.lines.push(stmt.to_string());
            return Ok(());
        }
        if stmt == "}" {
            if self.stack.pop().is_none() {
                self.cx.warn(self.line, "`}` without a composite state");
            }
            return Ok(());
        }
        if stmt == "--" {
            match self.stack.last_mut() {
                Some(scope) => {
                    if let Some(c) = scope.composite {
                        scope.region += 1;
                        self.composites[c].regions = scope.region;
                    }
                }
                None => self.cx.warn(self.line, "`--` outside a composite state"),
            }
            return Ok(());
        }
        let (word, rest) = first_word(stmt);
        match word {
            "direction" => {
                let Some(d) = Direction::parse(rest) else {
                    self.cx.warn(self.line, "unknown direction");
                    return Ok(());
                };
                match self.scope().composite {
                    Some(c) => self.composites[c].direction = Some(d),
                    None => self.direction = d,
                }
                return Ok(());
            }
            "classDef" | "class" | "style" => {
                self.styles.statement(word, rest);
                return Ok(());
            }
            "hide" | "scale" | "click" => return Ok(()),
            "state" => return self.declare(rest),
            "note" => return self.open_note(rest),
            _ => {}
        }
        if let Some(arrow) = find_outside_quotes(stmt, "-->") {
            return self.transition(&stmt[..arrow], &stmt[arrow + 3..]);
        }
        if let Some(colon) = plain_colon(stmt) {
            let (id, desc) = (&stmt[..colon], &stmt[colon + 1..]);
            let Some(s) = self.state(id.trim(), false)? else {
                self.cx.warn(self.line, format!("cannot read `{}`", truncate(stmt)));
                return Ok(());
            };
            return self.describe(s, desc);
        }
        if self.state(stmt, false)?.is_none() {
            self.cx.warn(self.line, format!("cannot read `{}`", truncate(stmt)));
        }
        Ok(())
    }

    fn describe(&mut self, s: usize, desc: &str) -> Result<(), DiagramError> {
        let desc = desc.trim();
        if desc.is_empty() {
            return Ok(());
        }
        let label = self.cx.label(desc)?;
        if self.states[s].descriptions.len() < 32 {
            self.states[s].descriptions.push(label);
        }
        Ok(())
    }

    fn transition(&mut self, left: &str, right: &str) -> Result<(), DiagramError> {
        let (right, label) = match plain_colon(right) {
            Some(i) => (&right[..i], Some(right[i + 1..].trim())),
            None => (right, None),
        };
        let (Some(from), Some(to)) = (self.state(left.trim(), false)?, self.state(right.trim(), true)?)
        else {
            self.cx.warn(self.line, "a transition needs a state on each side");
            return Ok(());
        };
        self.cx.limits.check_count(self.transitions.len() + 1, self.cx.limits.edges, LimitKind::Edges)?;
        let label = label.filter(|l| !l.is_empty()).map(|l| self.cx.label(l)).transpose()?;
        self.transitions.push(Transition { from, to, label });
        Ok(())
    }

    /// `state X`, `state "Label" as X`, `state X <<fork>>`, `state X {`,
    /// `state X : description`.
    fn declare(&mut self, rest: &str) -> Result<(), DiagramError> {
        let mut rest = rest.trim();
        let open = rest.ends_with('{');
        if open {
            rest = rest[..rest.len() - 1].trim();
        }
        let mut label = None;
        let mut desc = None;
        let id_part = if let Some(body) = rest.strip_prefix('"') {
            // `"Label" as id`
            let Some(end) = body.find('"') else {
                self.cx.warn(self.line, "an unclosed state label");
                return Ok(());
            };
            label = Some(body[..end].to_string());
            let tail = body[end + 1..].trim();
            match tail.strip_prefix("as") {
                Some(id) if id.starts_with(char::is_whitespace) => id.trim(),
                _ => {
                    self.cx.warn(self.line, "expected `as` after a state label");
                    return Ok(());
                }
            }
        } else {
            rest
        };
        let (id_part, kind) = match id_part.find("<<") {
            Some(i) => {
                let tag = id_part[i + 2..].trim_end_matches(">>").trim().to_ascii_lowercase();
                let kind = match tag.as_str() {
                    "fork" => Some(Kind::Fork),
                    "join" => Some(Kind::Join),
                    "choice" => Some(Kind::Choice),
                    _ => None,
                };
                (id_part[..i].trim(), kind)
            }
            None => (id_part, None),
        };
        let id_part = match plain_colon(id_part) {
            Some(i) => {
                desc = Some(id_part[i + 1..].to_string());
                id_part[..i].trim()
            }
            None => id_part,
        };
        // `state id as "Label"`, an older spelling.
        let id_part = match id_part.split_once(" as ") {
            Some((id, l)) if label.is_none() => {
                label = Some(unquote(l).to_string());
                id.trim()
            }
            _ => id_part,
        };
        let Some(s) = self.state(id_part, false)? else {
            self.cx.warn(self.line, format!("cannot read state `{}`", truncate(rest)));
            if open {
                // Still open a scope so the matching `}` closes something.
                self.stack.push(self.scope());
            }
            return Ok(());
        };
        if let Some(label) = label {
            self.states[s].label = Some(self.cx.label(&label)?);
        }
        if let Some(kind) = kind {
            self.states[s].kind = kind;
        }
        if let Some(desc) = desc {
            self.describe(s, &desc)?;
        }
        if open {
            let c = match self.states[s].composite {
                Some(c) => c,
                None => {
                    if self.stack.len() + 1 > self.cx.limits.cluster_depth {
                        return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
                    }
                    self.cx.limits.check_count(self.composites.len() + 1, self.cx.limits.clusters, LimitKind::Clusters)?;
                    let c = self.composites.len();
                    self.composites.push(Composite {
                        state: s,
                        scope: self.states[s].scope,
                        direction: None,
                        regions: 0,
                    });
                    self.states[s].composite = Some(c);
                    self.states[s].kind = Kind::Normal;
                    c
                }
            };
            if self.stack.iter().any(|scope| scope.composite == Some(c)) {
                return Err(DiagramError::syntax(self.line as u32 + 1, 1, "a state cannot contain itself"));
            }
            self.stack.push(Scope {
                composite: Some(c),
                region: 0,
            });
        }
        Ok(())
    }

    fn open_note(&mut self, rest: &str) -> Result<(), DiagramError> {
        let (side, tail) = first_word(rest);
        let left = match side {
            "left" => true,
            "right" => false,
            _ => {
                // `note "text" as N`: a free note.
                let text = rest.split(" as ").next().unwrap_or(rest);
                self.note = Some(PendingNote {
                    target: None,
                    left: false,
                    lines: vec![unquote(text).to_string()],
                });
                return self.close_note();
            }
        };
        let tail = tail.strip_prefix("of").map(str::trim).unwrap_or(tail);
        let (target, text) = match tail.split_once(':') {
            Some((t, text)) => (t.trim(), Some(text.trim())),
            None => (tail.trim(), None),
        };
        let target = self.state(target, false)?;
        if target.is_none() {
            self.cx.warn(self.line, "a note needs a state");
        }
        self.note = Some(PendingNote {
            target,
            left,
            lines: text.map(|t| vec![t.to_string()]).unwrap_or_default(),
        });
        if text.is_some() {
            self.close_note()?;
        }
        Ok(())
    }

    fn close_note(&mut self) -> Result<(), DiagramError> {
        let Some(note) = self.note.take() else {
            return Ok(());
        };
        let text = note.lines.join("\n").replace("\\n", "\n");
        if text.trim().is_empty() {
            return Ok(());
        }
        self.cx.limits.check_label(&text)?;
        self.check_nodes()?;
        self.notes.push(Note {
            text: Label::plain(&text),
            target: note.target,
            left: note.left,
        });
        Ok(())
    }

    fn finish(mut self) -> Model {
        for s in &mut self.states {
            s.style = self.styles.resolve(&s.id, &s.classes);
        }
        Model {
            direction: self.direction,
            states: self.states,
            composites: self.composites,
            transitions: self.transitions,
            notes: self.notes,
        }
    }
}

fn find_outside_quotes(s: &str, pat: &str) -> Option<usize> {
    let mut quote = false;
    for (i, c) in s.char_indices() {
        if c == '"' {
            quote = !quote;
        } else if !quote && s[i..].starts_with(pat) {
            return Some(i);
        }
    }
    None
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
    fn states_transitions_and_pseudo_states() {
        let m = parse_src(
            "stateDiagram-v2\n[*] --> Still\nStill --> [*]\nStill --> Moving : push\nMoving --> Crash\nCrash --> [*]\nstate \"Long name\" as LN\nLN : first\nLN : second",
        );
        let kinds: Vec<Kind> = m.states.iter().map(|s| s.kind).collect();
        assert_eq!(kinds[0], Kind::Start);
        assert_eq!(kinds[2], Kind::End);
        assert_eq!(m.states.iter().filter(|s| s.kind == Kind::End).count(), 1);
        assert_eq!(m.transitions.len(), 5);
        assert_eq!(m.transitions[2].label.as_ref().map(|l| l.text()), Some("push".into()));
        let ln = m.states.iter().find(|s| s.id == "LN").unwrap();
        assert_eq!(ln.label.as_ref().map(|l| l.text()), Some("Long name".into()));
        assert_eq!(ln.descriptions.len(), 2);
    }

    #[test]
    fn composites_regions_and_special_states() {
        let m = parse_src(
            "stateDiagram\ndirection LR\nstate Active {\n  direction TB\n  [*] --> A\n  --\n  [*] --> B\n  state Inner {\n    X\n  }\n}\nstate f <<fork>>\nstate j <<join>>\nstate c <<choice>>\nnote right of A : hi\nnote left of B\n  two\n  lines\nend note\nclassDef bad fill:#f00\nclass B bad\nA:::bad --> Active",
        );
        assert_eq!(m.direction, Direction::LR);
        assert_eq!(m.composites.len(), 2);
        assert_eq!(m.composites[0].regions, 1);
        assert_eq!(m.composites[0].direction, Some(Direction::TB));
        assert_eq!(m.composites[1].scope, Scope { composite: Some(0), region: 1 });
        let starts = m.states.iter().filter(|s| s.kind == Kind::Start).count();
        assert_eq!(starts, 2, "each region has its own start");
        let kind = |id: &str| m.states.iter().find(|s| s.id == id).unwrap().kind;
        assert_eq!((kind("f"), kind("j"), kind("c")), (Kind::Fork, Kind::Join, Kind::Choice));
        assert_eq!(m.notes.len(), 2);
        assert!(m.notes[1].left);
        assert_eq!(m.notes[1].text.lines.len(), 2);
        let b = m.states.iter().find(|s| s.id == "B").unwrap();
        assert!(b.style.fill.is_some());
    }
}
