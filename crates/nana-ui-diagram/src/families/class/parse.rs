//! The class diagram grammar: `class` declarations with `{ ... }` bodies,
//! `Name : member` lines, annotations, generics, namespaces, every relation
//! form with multiplicities and labels, notes, `direction`, `classDef` /
//! `cssClass` / `class` / `:::`, `style` and `click` / `link`.

use std::collections::HashMap;

use super::{Class, Member, Model, Namespace, Note, Relation};
use crate::families::ParseCx;
use crate::families::graphkit::{Styles, first_word, ident_len, plain_colon, split_classes, unquote};
use crate::geom::Marker;
use crate::kind;
use crate::layout::Direction;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

enum Block {
    Namespace(usize),
    Class(usize),
    /// The body of a class declaration that could not be read.
    Skip,
}

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    classes: Vec<Class>,
    index: HashMap<String, usize>,
    namespaces: Vec<Namespace>,
    stack: Vec<Block>,
    relations: Vec<Relation>,
    notes: Vec<Note>,
    styles: Styles,
    direction: Direction,
    members: usize,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut p = Parser {
        cx,
        classes: Vec::new(),
        index: HashMap::new(),
        namespaces: Vec::new(),
        stack: Vec::new(),
        relations: Vec::new(),
        notes: Vec::new(),
        styles: Styles::default(),
        direction: Direction::TB,
        members: 0,
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
                return Err(DiagramError::syntax(index as u32 + 1, 1, "expected `classDiagram`"));
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
    if !p.stack.is_empty() {
        p.cx.warn(p.line, "a `{` block is not closed");
    }
    Ok(p.finish())
}

/// `~T~` generics as `<T>`: a tilde after a word character that opens
/// something opens, every other tilde closes.
pub(super) fn generics(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    for (i, &c) in chars.iter().enumerate() {
        if c != '~' {
            out.push(c);
            continue;
        }
        let after_word = i > 0 && (chars[i - 1].is_alphanumeric() || chars[i - 1] == '_');
        let opens = chars
            .get(i + 1)
            .is_some_and(|n| *n != '~' && !n.is_whitespace() && *n != ')' && *n != ',');
        out.push(if after_word && opens { '<' } else { '>' });
    }
    out
}

/// One member line as written in a body or after `Name :`.
pub(super) fn member(raw: &str) -> Member {
    let mut text = raw.trim().to_string();
    let mut is_static = false;
    let mut is_abstract = false;
    let method = text.contains('(');
    if method {
        // A classifier right after the closing parenthesis or at the end.
        if let Some(close) = text.rfind(')') {
            match text[close + 1..].chars().next() {
                Some('$') => {
                    is_static = true;
                    text.remove(close + 1);
                }
                Some('*') => {
                    is_abstract = true;
                    text.remove(close + 1);
                }
                _ => {}
            }
        }
    }
    if text.ends_with('$') {
        is_static = true;
        text.pop();
    } else if text.ends_with('*') && !text.ends_with("**") {
        is_abstract = true;
        text.pop();
    }
    let mut display = generics(text.trim());
    if method
        && let Some(close) = display.rfind(')')
    {
        let ret = display[close + 1..].trim().trim_start_matches(':').trim().to_string();
        display.truncate(close + 1);
        if !ret.is_empty() {
            display.push_str(" : ");
            display.push_str(&ret);
        }
    }
    Member {
        text: display,
        method,
        is_static,
        is_abstract,
    }
}

/// Split a class reference into its id and display name: `` `A B` ``,
/// `Name~T~`, `Name`.
fn class_ref(raw: &str) -> Option<(String, String)> {
    let raw = raw.trim();
    if let Some(inner) = raw.strip_prefix('`').and_then(|r| r.strip_suffix('`')) {
        let inner = inner.trim();
        return (!inner.is_empty()).then(|| (inner.to_string(), inner.to_string()));
    }
    let len = ident_len(raw, "-");
    if len == 0 {
        return None;
    }
    let id = &raw[..len];
    let rest = &raw[len..];
    if rest.is_empty() {
        return Some((id.to_string(), id.to_string()));
    }
    if rest.starts_with('~') && rest.len() > 1 && !rest.contains(char::is_whitespace) {
        return Some((id.to_string(), generics(raw)));
    }
    None
}

struct RelationParts<'s> {
    left: &'s str,
    right: &'s str,
    start: Option<Marker>,
    end: Option<Marker>,
    dashed: bool,
    label: Option<&'s str>,
}

/// `A "1" <|-- "*" B : label` split into its parts.
fn relation_parts(stmt: &str) -> Option<RelationParts<'_>> {
    let head_end = plain_colon(stmt).unwrap_or(stmt.len());
    let head = &stmt[..head_end];
    let bytes = head.as_bytes();
    let mut quote = false;
    let mut at = None;
    for i in 0..bytes.len().saturating_sub(1) {
        match bytes[i] {
            b'"' => quote = !quote,
            b'-' | b'.' if !quote && bytes[i + 1] == bytes[i] => {
                at = Some(i);
                break;
            }
            _ => {}
        }
    }
    let i = at?;
    let dashed = bytes[i] == b'.';
    let before = &head[..i];
    let (start, left) = if let Some(l) = before.strip_suffix("<|") {
        (Some(Marker::Triangle), l)
    } else if let Some(l) = before.strip_suffix("()") {
        (Some(Marker::Lollipop), l)
    } else if let Some(l) = before.strip_suffix('*') {
        (Some(Marker::Diamond), l)
    } else if let Some(l) = before.strip_suffix('<') {
        (Some(Marker::OpenArrow), l)
    } else if let Some(l) = before
        .strip_suffix('o')
        .filter(|l| l.is_empty() || l.ends_with(char::is_whitespace) || l.ends_with('"'))
    {
        (Some(Marker::DiamondHollow), l)
    } else {
        (None, before)
    };
    let after = &head[i + 2..];
    let ends_word = |r: &str| r.is_empty() || r.starts_with(char::is_whitespace) || r.starts_with('"');
    let (end, right) = if let Some(r) = after.strip_prefix("|>") {
        (Some(Marker::Triangle), r)
    } else if let Some(r) = after.strip_prefix("()") {
        (Some(Marker::Lollipop), r)
    } else if let Some(r) = after.strip_prefix('*') {
        (Some(Marker::Diamond), r)
    } else if let Some(r) = after.strip_prefix('>') {
        (Some(Marker::OpenArrow), r)
    } else if let Some(r) = after.strip_prefix('o').filter(|r| ends_word(r)) {
        (Some(Marker::DiamondHollow), r)
    } else {
        (None, after)
    };
    let label = (head_end < stmt.len()).then(|| stmt[head_end + 1..].trim());
    Some(RelationParts {
        left: left.trim(),
        right: right.trim(),
        start,
        end,
        dashed,
        label,
    })
}

/// `Name "card"` → (`Name`, card); `"card" Name` with `leading`.
fn split_card(s: &str, leading: bool) -> (&str, Option<String>) {
    let s = s.trim();
    if leading {
        if let Some(body) = s.strip_prefix('"')
            && let Some(end) = body.find('"')
        {
            return (body[end + 1..].trim(), Some(body[..end].to_string()));
        }
    } else if let Some(body) = s.strip_suffix('"')
        && let Some(open) = body.rfind('"')
    {
        return (body[..open].trim(), Some(body[open + 1..].to_string()));
    }
    (s, None)
}

impl Parser<'_, '_> {
    fn current_namespace(&self) -> Option<usize> {
        self.stack.iter().rev().find_map(|b| match b {
            Block::Namespace(n) => Some(*n),
            Block::Class(_) | Block::Skip => None,
        })
    }

    /// The class `raw` names, created on first mention.
    fn class(&mut self, raw: &str) -> Result<Option<usize>, DiagramError> {
        let (raw, inline) = split_classes(raw);
        let Some((id, name)) = class_ref(raw) else {
            return Ok(None);
        };
        let c = match self.index.get(&id) {
            Some(&c) => {
                if name != id && self.classes[c].name == self.classes[c].id {
                    self.classes[c].name = name;
                }
                c
            }
            None => {
                self.cx.limits.check_count(self.classes.len() + 1, self.cx.limits.nodes, LimitKind::Nodes)?;
                self.cx.limits.check_label(&name)?;
                let c = self.classes.len();
                self.index.insert(id.clone(), c);
                self.classes.push(Class {
                    id,
                    name,
                    annotations: Vec::new(),
                    attributes: Vec::new(),
                    methods: Vec::new(),
                    namespace: self.current_namespace(),
                    classes: Vec::new(),
                    style: Default::default(),
                    link: None,
                });
                c
            }
        };
        self.classes[c].classes.extend(inline);
        Ok(Some(c))
    }

    fn add_member(&mut self, c: usize, raw: &str) -> Result<(), DiagramError> {
        let raw = raw.trim();
        if raw.is_empty() {
            return Ok(());
        }
        if let Some(a) = raw.strip_prefix("<<").and_then(|r| r.strip_suffix(">>")) {
            self.annotate(c, a);
            return Ok(());
        }
        self.cx.limits.check_label(raw)?;
        self.members += 1;
        self.cx.limits.check_count(self.members, self.cx.limits.elements, LimitKind::Elements)?;
        let m = member(raw);
        let class = &mut self.classes[c];
        if m.method {
            class.methods.push(m);
        } else {
            class.attributes.push(m);
        }
        Ok(())
    }

    fn annotate(&mut self, c: usize, a: &str) {
        let a = a.trim();
        if !a.is_empty() && self.classes[c].annotations.len() < 8 {
            self.classes[c].annotations.push(a.to_string());
        }
    }

    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        if let Some(Block::Skip) = self.stack.last() {
            if stmt.ends_with('}') {
                self.stack.pop();
            }
            return Ok(());
        }
        if let Some(Block::Class(c)) = self.stack.last() {
            let c = *c;
            if stmt == "}" {
                self.stack.pop();
                return Ok(());
            }
            if let Some(body) = stmt.strip_suffix('}') {
                self.add_member(c, body)?;
                self.stack.pop();
                return Ok(());
            }
            return self.add_member(c, stmt);
        }
        if stmt == "}" {
            if self.stack.pop().is_none() {
                self.cx.warn(self.line, "`}` without an open block");
            }
            return Ok(());
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
            "cssClass" => {
                let rest = rest.trim();
                let (ids, class) = match rest.strip_prefix('"').and_then(|r| r.split_once('"')) {
                    Some((ids, class)) => (ids, class.trim()),
                    None => first_word(rest),
                };
                self.styles.assign(ids, class);
                return Ok(());
            }
            "style" => {
                self.styles.style(rest);
                return Ok(());
            }
            "click" | "link" => {
                self.click(word, rest);
                return Ok(());
            }
            "callback" => return Ok(()),
            "note" => return self.note(rest),
            "namespace" => return self.namespace(rest),
            "class" if !rest.is_empty() && relation_parts(stmt).is_none() => return self.declare(rest),
            _ => {}
        }
        if let Some(body) = stmt.strip_prefix("<<")
            && let Some((a, name)) = body.split_once(">>")
        {
            if let Some(c) = self.class(name)? {
                self.annotate(c, a);
            } else {
                self.cx.warn(self.line, "an annotation without a class");
            }
            return Ok(());
        }
        if let Some(parts) = relation_parts(stmt) {
            return self.relation(parts);
        }
        if let Some(colon) = plain_colon(stmt) {
            let (name, member) = (&stmt[..colon], &stmt[colon + 1..]);
            if let Some(c) = self.class(name)? {
                return self.add_member(c, member);
            }
        }
        if self.class(stmt)?.is_none() {
            self.cx.warn(self.line, format!("cannot read `{}`", truncate(stmt)));
        }
        Ok(())
    }

    fn declare(&mut self, rest: &str) -> Result<(), DiagramError> {
        let mut rest = rest.trim();
        let mut open = false;
        let mut inline_body = None;
        if let Some(i) = rest.find('{') {
            let body = &rest[i + 1..];
            match body.rfind('}') {
                Some(end) => inline_body = Some(body[..end].to_string()),
                None => open = true,
            }
            rest = rest[..i].trim();
        }
        // `Name["Label"]`: Mermaid's display label.
        let mut label = None;
        let (head, inline) = split_classes(rest);
        let mut head = head;
        if let Some(i) = head.find('[')
            && head.ends_with(']')
        {
            label = Some(unquote(&head[i + 1..head.len() - 1]).to_string());
            head = head[..i].trim();
        }
        let joined = if inline.is_empty() {
            head.to_string()
        } else {
            format!("{head}:::{}", inline.join(","))
        };
        let Some(c) = self.class(&joined)? else {
            self.cx.warn(self.line, format!("cannot read class `{}`", truncate(rest)));
            if open {
                // Swallow the body so its members do not read as statements.
                self.stack.push(Block::Skip);
            }
            return Ok(());
        };
        if let Some(ns) = self.current_namespace() {
            self.classes[c].namespace = Some(ns);
        }
        if let Some(label) = label.filter(|l| !l.trim().is_empty()) {
            self.cx.limits.check_label(&label)?;
            self.classes[c].name = label;
        }
        if let Some(body) = inline_body {
            for m in body.split(';') {
                self.add_member(c, m)?;
            }
        }
        if open {
            self.stack.push(Block::Class(c));
        }
        Ok(())
    }

    fn namespace(&mut self, rest: &str) -> Result<(), DiagramError> {
        let name = rest.trim().trim_end_matches('{').trim();
        if name.is_empty() {
            self.cx.warn(self.line, "a namespace without a name");
            return Ok(());
        }
        self.cx.limits.check_count(self.namespaces.len() + 1, self.cx.limits.clusters, LimitKind::Clusters)?;
        self.cx.limits.check_label(name)?;
        let depth = self.stack.iter().filter(|b| matches!(b, Block::Namespace(_))).count();
        if depth + 1 > self.cx.limits.cluster_depth {
            return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
        }
        let n = self.namespaces.len();
        self.namespaces.push(Namespace {
            name: unquote(name).to_string(),
            parent: self.current_namespace(),
        });
        if rest.trim_end().ends_with('{') {
            self.stack.push(Block::Namespace(n));
        } else {
            self.cx.warn(self.line, "a namespace needs `{`");
        }
        Ok(())
    }

    fn note(&mut self, rest: &str) -> Result<(), DiagramError> {
        let rest = rest.trim();
        let (target, text) = match rest.strip_prefix("for ") {
            Some(tail) => {
                let (name, text) = first_word(tail);
                (self.class(name)?, text)
            }
            None => (None, rest),
        };
        let text = unquote(text).replace("\\n", "\n");
        if text.trim().is_empty() {
            self.cx.warn(self.line, "an empty note");
            return Ok(());
        }
        self.cx.limits.check_label(&text)?;
        self.cx.limits.check_count(self.classes.len() + self.notes.len() + 1, self.cx.limits.nodes, LimitKind::Nodes)?;
        self.notes.push(Note {
            text: Label::plain(&text),
            target,
        });
        Ok(())
    }

    fn click(&mut self, word: &str, rest: &str) {
        let (id, mut tail) = first_word(rest);
        if word == "click" {
            let (kind, t) = first_word(tail);
            match kind {
                "href" => tail = t,
                "call" => return,
                _ => {}
            }
        }
        let mut quoted = tail.split('"').skip(1).step_by(2);
        let Some(url) = quoted.next().filter(|u| !u.trim().is_empty()) else {
            return;
        };
        let tooltip = quoted.next().map(str::to_string);
        if let Some(&c) = self.index.get(id) {
            self.classes[c].link = Some((url.to_string(), tooltip));
        }
    }

    fn relation(&mut self, parts: RelationParts<'_>) -> Result<(), DiagramError> {
        let (left, from_card) = split_card(parts.left, false);
        let (right, to_card) = split_card(parts.right, true);
        let (Some(from), Some(to)) = (self.class(left)?, self.class(right)?) else {
            self.cx.warn(self.line, "a relation needs a class on each side");
            return Ok(());
        };
        self.cx.limits.check_count(self.relations.len() + 1, self.cx.limits.edges, LimitKind::Edges)?;
        let label = match parts.label.map(unquote).filter(|l| !l.is_empty()) {
            Some(l) => Some(self.cx.label(l)?),
            None => None,
        };
        for card in [&from_card, &to_card].into_iter().flatten() {
            self.cx.limits.check_label(card)?;
        }
        self.relations.push(Relation {
            from,
            to,
            start: parts.start,
            end: parts.end,
            dashed: parts.dashed,
            label,
            from_card: from_card.filter(|c| !c.trim().is_empty()),
            to_card: to_card.filter(|c| !c.trim().is_empty()),
        });
        Ok(())
    }

    fn finish(mut self) -> Model {
        for class in &mut self.classes {
            class.style = self.styles.resolve(&class.id, &class.classes);
        }
        Model {
            direction: self.direction,
            classes: self.classes,
            namespaces: self.namespaces,
            relations: self.relations,
            notes: self.notes,
        }
    }
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
    fn members_generics_and_classifiers() {
        let m = member("+getItems(List~int~ ids)$ List~Item~");
        assert_eq!(m.text, "+getItems(List<int> ids) : List<Item>");
        assert!(m.method && m.is_static);
        let a = member("-List~List~int~~ grid");
        assert_eq!(a.text, "-List<List<int>> grid");
        assert!(member("+area()* double").is_abstract);
        assert!(member("#count$").is_static);
    }

    #[test]
    fn bodies_member_lines_annotations_and_namespaces() {
        let m = parse_src(
            "classDiagram\nnamespace Shapes {\n  class Square~Shape~ {\n    <<interface>>\n    -int id\n    +area() double\n  }\n}\nSquare : +String name\n<<abstract>> Base\nclass Label[\"Shown name\"]:::hot\nclassDef hot fill:#f00",
        );
        let sq = &m.classes[0];
        assert_eq!(sq.name, "Square<Shape>");
        assert_eq!(sq.annotations, vec!["interface".to_string()]);
        assert_eq!(sq.attributes.len(), 2);
        assert_eq!(sq.methods.len(), 1);
        assert_eq!(sq.namespace, Some(0));
        assert_eq!(m.classes[1].annotations, vec!["abstract".to_string()]);
        assert_eq!(m.classes[2].name, "Shown name");
        assert!(m.classes[2].style.fill.is_some());
    }

    #[test]
    fn every_relation_form() {
        let m = parse_src(
            "classDiagram\nA <|-- B\nC *-- D\nE o-- F\nG --> H\nI ..> J\nK ..|> L\nM -- N\nO .. P\nQ ()-- R\nS \"1\" --> \"*\" T : owns\nU<|--|>V\nW --() X",
        );
        let r = &m.relations;
        assert_eq!(r.len(), 12);
        assert_eq!((r[0].start, r[0].end), (Some(Marker::Triangle), None));
        assert_eq!(r[1].start, Some(Marker::Diamond));
        assert_eq!(r[2].start, Some(Marker::DiamondHollow));
        assert_eq!(r[3].end, Some(Marker::OpenArrow));
        assert!(r[4].dashed);
        assert_eq!((r[5].end, r[5].dashed), (Some(Marker::Triangle), true));
        assert_eq!((r[6].start, r[6].end, r[6].dashed), (None, None, false));
        assert!(r[7].dashed);
        assert_eq!(r[8].start, Some(Marker::Lollipop));
        assert_eq!(r[9].from_card.as_deref(), Some("1"));
        assert_eq!(r[9].to_card.as_deref(), Some("*"));
        assert_eq!(r[9].label.as_ref().map(|l| l.text()), Some("owns".into()));
        assert_eq!((r[10].start, r[10].end), (Some(Marker::Triangle), Some(Marker::Triangle)));
        assert_eq!(r[11].end, Some(Marker::Lollipop));
        assert_eq!(m.classes.len(), 24);
    }

    #[test]
    fn notes_links_styles_and_direction() {
        let m = parse_src(
            "classDiagram\ndirection RL\nclass A\nnote for A \"line one\\nline two\"\nnote \"free\"\nclick A href \"https://example.com\" \"tip\"\ncssClass \"A\" warm\nclassDef warm stroke:#f00\nstyle A stroke-width:3px",
        );
        assert_eq!(m.direction, Direction::RL);
        assert_eq!(m.notes.len(), 2);
        assert_eq!(m.notes[0].target, Some(0));
        assert_eq!(m.notes[0].text.lines.len(), 2);
        assert_eq!(m.classes[0].link.as_ref().map(|l| l.0.as_str()), Some("https://example.com"));
        assert!(m.classes[0].style.stroke.is_some());
        assert_eq!(m.classes[0].style.stroke_width, Some(3.0));
    }
}
