//! The requirement diagram grammar: the six requirement kinds and
//! `element` blocks with their fields, the seven relationship kinds in
//! both arrow directions, `direction`, `classDef` / `class` / `:::` and
//! `style`.

use std::collections::HashMap;

use super::{Block, BlockKind, Model, RelKind, Relation};
use crate::families::ParseCx;
use crate::families::graphkit::{Styles, first_name, first_word, split_classes, unquote};
use crate::kind;
use crate::layout::Direction;
use crate::{DiagramError, LimitKind};

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    blocks: Vec<Block>,
    index: HashMap<String, usize>,
    pending: Vec<(String, String, RelKind, usize)>,
    styles: Styles,
    direction: Direction,
    open: Option<usize>,
    fields: usize,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut p = Parser {
        cx,
        blocks: Vec::new(),
        index: HashMap::new(),
        pending: Vec::new(),
        styles: Styles::default(),
        direction: Direction::TB,
        open: None,
        fields: 0,
        line: 0,
    };
    let mut header = false;
    for (index, raw) in body.lines().enumerate() {
        p.line = index;
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if !header {
            let Some((_, rest)) = kind::header(line) else {
                return Err(DiagramError::syntax(index as u32 + 1, 1, "expected `requirementDiagram`"));
            };
            header = true;
            if !rest.is_empty() {
                p.statement(rest)?;
            }
            continue;
        }
        p.statement(line)?;
    }
    if !header {
        return Err(DiagramError::Empty);
    }
    if p.open.is_some() {
        p.cx.warn(p.line, "a block is not closed");
    }
    p.finish()
}

fn capitalised(value: &str) -> String {
    let lower = value.to_ascii_lowercase();
    let mut chars = lower.chars();
    match chars.next() {
        Some(c) => c.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `a - satisfies -> b` or `b <- satisfies - a`, as (source, kind, target).
fn relation(stmt: &str) -> Option<(&str, &str, &str)> {
    if let Some(open) = stmt.find("<-") {
        let left = &stmt[..open];
        let rest = &stmt[open + 2..];
        let close = rest.find(" -").or_else(|| rest.rfind('-'))?;
        let kind = &rest[..close];
        let right = rest[close..].trim_start().strip_prefix('-')?;
        return Some((right.trim(), kind.trim(), left.trim()));
    }
    let arrow = stmt.find("->")?;
    let (head, right) = (&stmt[..arrow], &stmt[arrow + 2..]);
    let dash = head.find(" - ").or_else(|| head.find('-'))?;
    let left = &head[..dash];
    let kind = head[dash..].trim_start().strip_prefix('-')?;
    Some((left.trim(), kind.trim(), right.trim()))
}

impl Parser<'_, '_> {
    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        if let Some(b) = self.open {
            if stmt == "}" {
                self.open = None;
                return Ok(());
            }
            if let Some(body) = stmt.strip_suffix('}') {
                self.open = None;
                return self.field(b, body);
            }
            return self.field(b, stmt);
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
            "classDef" => {
                self.styles.class_def(rest);
                return Ok(());
            }
            "class" => {
                let (ids, classes) = first_name(rest);
                self.styles.assign(ids, classes);
                return Ok(());
            }
            "style" => {
                self.styles.style(rest);
                return Ok(());
            }
            _ => {}
        }
        if let Some(kind) = BlockKind::keyword(word) {
            return self.block(kind, rest);
        }
        if let Some((from, kind, to)) = relation(stmt) {
            let Some(kind) = RelKind::parse(kind) else {
                self.cx.warn(self.line, format!("unknown relationship `{}`", truncate(kind)));
                return Ok(());
            };
            self.cx.limits.check_count(self.pending.len() + 1, self.cx.limits.edges, LimitKind::Edges)?;
            self.pending.push((name_of(from).0, name_of(to).0, kind, self.line));
            return Ok(());
        }
        // `name:::class` on its own assigns classes.
        let (name, classes) = split_classes(stmt);
        if !classes.is_empty() {
            self.styles.assign(unquote(name), &classes.join(","));
            return Ok(());
        }
        self.cx.warn(self.line, format!("cannot read `{}`", truncate(stmt)));
        Ok(())
    }

    fn block(&mut self, kind: BlockKind, rest: &str) -> Result<(), DiagramError> {
        let rest = rest.trim();
        let (head, body) = match rest.find('{') {
            Some(i) => (&rest[..i], Some(&rest[i + 1..])),
            None => (rest, None),
        };
        let (name, classes) = name_of(head);
        if name.is_empty() {
            self.cx.warn(self.line, "a block without a name");
            return Ok(());
        }
        self.cx.limits.check_label(&name)?;
        let b = match self.index.get(&name) {
            Some(&b) => {
                self.blocks[b].kind = kind;
                b
            }
            None => {
                self.cx.limits.check_count(self.blocks.len() + 1, self.cx.limits.nodes, LimitKind::Nodes)?;
                let b = self.blocks.len();
                self.index.insert(name.clone(), b);
                self.blocks.push(Block {
                    id: name,
                    kind,
                    fields: Vec::new(),
                    classes: Vec::new(),
                    style: Default::default(),
                });
                b
            }
        };
        self.blocks[b].classes.extend(classes);
        match body {
            Some(body) => match body.rfind('}') {
                Some(end) => {
                    for f in body[..end].split(';') {
                        self.field(b, f)?;
                    }
                }
                None => {
                    self.field(b, body)?;
                    self.open = Some(b);
                }
            },
            None => self.cx.warn(self.line, "a block needs `{`"),
        }
        Ok(())
    }

    fn field(&mut self, b: usize, line: &str) -> Result<(), DiagramError> {
        let line = line.trim();
        if line.is_empty() {
            return Ok(());
        }
        let Some((key, value)) = line.split_once(':') else {
            self.cx.warn(self.line, format!("cannot read field `{}`", truncate(line)));
            return Ok(());
        };
        let value = unquote(value);
        self.cx.limits.check_label(value)?;
        let is_element = self.blocks[b].kind == BlockKind::Element;
        let (name, value) = match (key.trim().to_ascii_lowercase().as_str(), is_element) {
            ("id", false) => ("ID", value.to_string()),
            ("text", false) => ("Text", value.to_string()),
            ("risk", false) => ("Risk", capitalised(value)),
            ("verifymethod", false) => ("Verification", capitalised(value)),
            ("type", true) => ("Type", value.to_string()),
            ("docref", true) => ("Doc Ref", value.to_string()),
            _ => {
                self.cx.warn(self.line, format!("unknown field `{}`", truncate(key)));
                return Ok(());
            }
        };
        self.fields += 1;
        self.cx.limits.check_count(self.fields, self.cx.limits.elements, LimitKind::Elements)?;
        let fields = &mut self.blocks[b].fields;
        match fields.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value,
            None => fields.push((name.to_string(), value)),
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Model, DiagramError> {
        let mut relations = Vec::with_capacity(self.pending.len());
        for (from, to, kind, line) in std::mem::take(&mut self.pending) {
            match (self.index.get(&from), self.index.get(&to)) {
                (Some(&from), Some(&to)) => relations.push(Relation { from, to, kind }),
                _ => self.cx.warn(line, format!("`{}` names an undeclared block", kind.name())),
            }
        }
        for b in &mut self.blocks {
            b.style = self.styles.resolve(&b.id, &b.classes);
        }
        Ok(Model {
            direction: self.direction,
            blocks: self.blocks,
            relations,
        })
    }
}

/// A block name, unquoted, and its `:::` classes.
fn name_of(raw: &str) -> (String, Vec<String>) {
    let (name, classes) = split_classes(raw.trim());
    (unquote(name).trim().to_string(), classes)
}

fn truncate(s: &str) -> String {
    s.chars().take(40).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Config;
    use crate::{Limits, Meta};

    fn parse_src(src: &str) -> Model {
        let limits = Limits::DEFAULT;
        let config = Config::default();
        let mut cx = ParseCx {
            limits: &limits,
            config: &config,
            meta: Meta::default(),
            warnings: Vec::new(),
        };
        let model = parse(src, &mut cx).unwrap();
        assert!(cx.warnings.is_empty(), "{:?}", cx.warnings);
        model
    }

    #[test]
    fn blocks_fields_and_relations() {
        let m = parse_src(
            "requirementDiagram\nrequirement \"test req\":::hot {\n  id: 1\n  text: \"the test text.\"\n  risk: high\n  verifymethod: test\n}\nelement test_entity {\n  type: simulation\n  docref: reqs/test_entity\n}\ndesignConstraint dc { id: 5; text: x }\ntest_entity - satisfies -> \"test req\"\n\"test req\" <- contains - dc\nclassDef hot fill:#f00",
        );
        assert_eq!(m.blocks.len(), 3);
        let r = &m.blocks[0];
        assert_eq!(r.id, "test req");
        assert_eq!(r.fields[2], ("Risk".to_string(), "High".to_string()));
        assert_eq!(r.fields[3], ("Verification".to_string(), "Test".to_string()));
        assert!(r.style.fill.is_some());
        assert_eq!(m.blocks[1].fields[1].0, "Doc Ref");
        assert_eq!(m.blocks[2].kind, BlockKind::DesignConstraint);
        assert_eq!(m.relations.len(), 2);
        assert_eq!((m.relations[0].from, m.relations[0].to, m.relations[0].kind), (1, 0, RelKind::Satisfies));
        assert_eq!((m.relations[1].from, m.relations[1].to, m.relations[1].kind), (2, 0, RelKind::Contains));
    }
}
