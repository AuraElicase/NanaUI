//! The block grammar: `columns`, blocks with the flowchart brackets and a
//! `:N` width, `space` / `space:N`, composite `block[:id[:N]] ... end`,
//! block arrows `id<["label"]>(right)`, links with labels, `classDef`,
//! `class` and `style`.

use std::collections::HashMap;

use super::{Block, Edge, Heads, Kind, Model};
use crate::families::ParseCx;
use crate::families::flowchart::LineKind;
use crate::families::kit::{self, first_word, truncate};
use crate::geom::{Marker, Shape};
use crate::source::directives::AuthorStyle;
use crate::source::split_statements;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

/// Widths and column counts past this are typing accidents.
const MAX_SPAN: usize = 64;

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    model: Model,
    index: HashMap<String, usize>,
    stack: Vec<usize>,
    class_defs: HashMap<String, AuthorStyle>,
    class_assign: Vec<(String, String)>,
    styles: Vec<(String, String)>,
    pending: Vec<(String, String, Edge)>,
    groups: usize,
    spaces: usize,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, after) = kit::header(body)?;
    let mut p = Parser {
        cx,
        model: Model::default(),
        index: HashMap::new(),
        stack: Vec::new(),
        class_defs: HashMap::new(),
        class_assign: Vec::new(),
        styles: Vec::new(),
        pending: Vec::new(),
        groups: 0,
        spaces: 0,
        line: 0,
    };
    // Statements may follow the header on its own line.
    for (index, line) in std::iter::once((header, after)).chain(kit::statements(body, header)) {
        p.line = index;
        for (_, stmt) in split_statements(line) {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                p.statement(stmt)?;
            }
        }
    }
    p.finish()
}

impl Parser<'_, '_> {
    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let (word, rest) = first_word(stmt);
        match word {
            "columns" => {
                let value = rest.trim();
                let columns = if value == "auto" {
                    None
                } else if let Ok(n) = value.parse::<usize>() {
                    Some(n.clamp(1, MAX_SPAN))
                } else {
                    self.cx.warn(
                        self.line,
                        format!("cannot read `columns {}`", truncate(value)),
                    );
                    return Ok(());
                };
                match self.stack.last() {
                    Some(&g) => self.model.blocks[g].kind = Kind::Group { columns },
                    None => self.model.columns = columns,
                }
                return Ok(());
            }
            "classDef" => {
                let (names, decls) = first_word(rest);
                for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    self.class_defs
                        .entry(name.to_string())
                        .or_default()
                        .apply(decls);
                }
                return Ok(());
            }
            "class" => {
                let (ids, class) = first_word(rest);
                for id in ids.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    self.class_assign
                        .push((id.to_string(), class.trim().to_string()));
                }
                return Ok(());
            }
            "style" => {
                let (id, decls) = first_word(rest);
                self.styles.push((id.to_string(), decls.to_string()));
                return Ok(());
            }
            _ => {}
        }
        self.items(stmt)
    }

    fn container(&self) -> Option<usize> {
        self.stack.last().copied()
    }

    fn add(&mut self, block: Block) -> Result<usize, DiagramError> {
        let limits = self.cx.limits;
        limits.check_count(
            self.model.blocks.len() + 1,
            limits.nodes.min(crate::layout::grid::MAX_ROUTED),
            LimitKind::Nodes,
        )?;
        let i = self.model.blocks.len();
        match self.container() {
            Some(g) => self.model.blocks[g].children.push(i),
            None => self.model.root.push(i),
        }
        self.model.blocks.push(block);
        Ok(i)
    }

    /// A line of blocks, spaces, group openers and links.
    fn items(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let mut cur = Cur { s: stmt, i: 0 };
        loop {
            cur.ws();
            if cur.at_end() {
                break;
            }
            if let Some(span) = cur.space() {
                self.spaces += 1;
                self.add(block(
                    format!("space-{}", self.spaces),
                    Label::default(),
                    Kind::Space,
                    span,
                ))?;
                continue;
            }
            if cur.word_is("end") {
                if self.stack.pop().is_none() {
                    self.cx.warn(self.line, "`end` without a block");
                }
                continue;
            }
            if cur.word_is_prefix("block") {
                self.open_group(&mut cur)?;
                continue;
            }
            let Some(first) = cur.node() else {
                self.cx
                    .warn(self.line, format!("cannot read `{}`", truncate(cur.rest())));
                break;
            };
            let mut left = self.mention(&first, true)?;
            // Links chain from the block just read.
            loop {
                let save = cur.i;
                cur.ws();
                let Some(link) = cur.link() else {
                    cur.i = save;
                    break;
                };
                cur.ws();
                let Some(target) = cur.node() else {
                    self.cx.warn(self.line, "a link without a target");
                    break;
                };
                let right = self.mention(&target, false)?;
                let label = match &link.label {
                    Some(t) => Some(self.cx.label(t)?).filter(|l| !l.is_empty()),
                    None => None,
                };
                let limits = self.cx.limits;
                limits.check_count(
                    self.pending.len() + 1,
                    limits.edges.min(crate::layout::grid::MAX_ROUTED),
                    LimitKind::Edges,
                )?;
                self.pending.push((
                    left.clone(),
                    right.clone(),
                    Edge {
                        from: 0,
                        to: 0,
                        label,
                        line: link.line,
                        start: link.start,
                        end: link.end,
                    },
                ));
                left = right;
            }
        }
        Ok(())
    }

    fn open_group(&mut self, cur: &mut Cur<'_>) -> Result<(), DiagramError> {
        let limits = self.cx.limits;
        self.groups += 1;
        limits.check_count(self.groups, limits.clusters, LimitKind::Clusters)?;
        if self.stack.len() + 1 > limits.cluster_depth {
            return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
        }
        let mut id = format!("block-{}", self.groups);
        let mut span = 1;
        if cur.eat(":") {
            let start = cur.i;
            while let Some(c) = cur.peek() {
                if c.is_whitespace() || c == ':' {
                    break;
                }
                cur.i += c.len_utf8();
            }
            let name = &cur.s[start..cur.i];
            if !name.is_empty() {
                id = name.to_string();
            }
            if cur.eat(":") {
                span = cur.number().unwrap_or(1);
            }
        }
        let g = self.add(block(
            id.clone(),
            Label::default(),
            Kind::Group { columns: None },
            span,
        ))?;
        self.index.entry(id).or_insert(g);
        self.stack.push(g);
        Ok(())
    }

    /// Record a block reference; a new id becomes a block in the current
    /// container. Returns its id.
    fn mention(&mut self, node: &NodeRef, declares: bool) -> Result<String, DiagramError> {
        let label = match &node.text {
            Some(t) => Some(self.cx.label(t)?),
            None => None,
        };
        if let Some(&i) = self.index.get(&node.id) {
            let block = &mut self.model.blocks[i];
            if let Some(kind) = &node.kind
                && !matches!(block.kind, Kind::Group { .. })
            {
                block.kind = kind.clone();
            }
            if let Some(label) = label {
                block.label = label;
            }
            if declares && let Some(span) = node.span {
                block.span = span.clamp(1, MAX_SPAN);
            }
            block.classes.extend(node.classes.iter().cloned());
            return Ok(node.id.clone());
        }
        let i = self.add(Block {
            classes: node.classes.clone(),
            ..block(
                node.id.clone(),
                label.unwrap_or_else(|| Label::plain(&node.id)),
                node.kind.clone().unwrap_or(Kind::Node(Shape::Rect)),
                node.span.unwrap_or(1),
            )
        })?;
        self.index.insert(node.id.clone(), i);
        Ok(node.id.clone())
    }

    fn finish(mut self) -> Result<Model, DiagramError> {
        for (from, to, mut edge) in std::mem::take(&mut self.pending) {
            match (self.index.get(&from), self.index.get(&to)) {
                (Some(&a), Some(&b)) => {
                    edge.from = a;
                    edge.to = b;
                    self.model.edges.push(edge);
                }
                _ => self.cx.warn(
                    self.line,
                    format!("a link to an unknown block `{from}` / `{to}`"),
                ),
            }
        }
        for (id, class) in &self.class_assign {
            if let Some(&i) = self.index.get(id) {
                self.model.blocks[i].classes.push(class.clone());
            }
        }
        let default = self.class_defs.get("default").copied().unwrap_or_default();
        for block in &mut self.model.blocks {
            let mut style = if matches!(block.kind, Kind::Group { .. } | Kind::Space) {
                AuthorStyle::default()
            } else {
                default
            };
            for class in &block.classes {
                if let Some(def) = self.class_defs.get(class) {
                    style = style.merged(def);
                }
            }
            block.style = style;
        }
        for (id, decls) in &self.styles {
            if let Some(&i) = self.index.get(id) {
                self.model.blocks[i].style.apply(decls);
            }
        }
        Ok(self.model)
    }
}

/// A block with no classes or style yet; `span` is clamped.
fn block(id: String, label: Label, kind: Kind, span: usize) -> Block {
    Block {
        id,
        label,
        kind,
        span: span.clamp(1, MAX_SPAN),
        children: Vec::new(),
        classes: Vec::new(),
        style: AuthorStyle::default(),
    }
}

struct NodeRef {
    id: String,
    kind: Option<Kind>,
    text: Option<String>,
    span: Option<usize>,
    classes: Vec<String>,
}

struct LinkRef {
    line: LineKind,
    start: Option<Marker>,
    end: Option<Marker>,
    label: Option<String>,
}

struct Cur<'s> {
    s: &'s str,
    i: usize,
}

const OPENERS: &[(&str, &[(&str, Shape)])] = &[
    ("(((", &[(")))", Shape::DoubleCircle)]),
    ("((", &[("))", Shape::Circle)]),
    ("([", &[("])", Shape::Stadium)]),
    ("[[", &[("]]", Shape::Subroutine)]),
    ("[(", &[(")]", Shape::Cylinder)]),
    (
        "[/",
        &[("/]", Shape::LeanRight), ("\\]", Shape::TrapezoidBottom)],
    ),
    (
        "[\\",
        &[("\\]", Shape::LeanLeft), ("/]", Shape::TrapezoidTop)],
    ),
    ("{{", &[("}}", Shape::Hexagon)]),
    ("[", &[("]", Shape::Rect)]),
    ("(", &[(")", Shape::Rounded)]),
    ("{", &[("}", Shape::Diamond)]),
    (">", &[("]", Shape::Odd)]),
];

impl<'s> Cur<'s> {
    fn rest(&self) -> &'s str {
        &self.s[self.i..]
    }

    fn at_end(&self) -> bool {
        self.rest().trim().is_empty()
    }

    fn peek(&self) -> Option<char> {
        self.rest().chars().next()
    }

    fn ws(&mut self) {
        while let Some(c) = self.peek() {
            if !c.is_whitespace() {
                break;
            }
            self.i += c.len_utf8();
        }
    }

    fn eat(&mut self, pat: &str) -> bool {
        if self.rest().starts_with(pat) {
            self.i += pat.len();
            true
        } else {
            false
        }
    }

    fn number(&mut self) -> Option<usize> {
        let digits = self.rest().chars().take_while(char::is_ascii_digit).count();
        if digits == 0 {
            return None;
        }
        let n = self.rest()[..digits]
            .parse::<usize>()
            .ok()
            .map(|n| n.min(MAX_SPAN));
        self.i += digits;
        n
    }

    /// `word` as a whole word at the cursor.
    fn word_is(&mut self, word: &str) -> bool {
        let r = self.rest();
        if r.starts_with(word)
            && r[word.len()..]
                .chars()
                .next()
                .is_none_or(char::is_whitespace)
        {
            self.i += word.len();
            true
        } else {
            false
        }
    }

    /// `word` followed by the end, whitespace or `:`.
    fn word_is_prefix(&mut self, word: &str) -> bool {
        let r = self.rest();
        if r.starts_with(word)
            && r[word.len()..]
                .chars()
                .next()
                .is_none_or(|c| c.is_whitespace() || c == ':')
        {
            self.i += word.len();
            true
        } else {
            false
        }
    }

    /// `space` or `space:N`: the width, 1 for a bare space.
    fn space(&mut self) -> Option<usize> {
        let save = self.i;
        if !self.word_is_prefix("space") {
            return None;
        }
        if !self.eat(":") {
            return Some(1);
        }
        if !self.rest().starts_with(|c: char| c.is_ascii_digit()) {
            self.i = save;
            return None;
        }
        Some(self.number().unwrap_or(1))
    }

    fn arrow_ahead(&self) -> bool {
        let r = self.rest();
        r.starts_with("--") || r.starts_with("-.") || r.starts_with("==") || r.starts_with("~~")
    }

    fn node(&mut self) -> Option<NodeRef> {
        self.ws();
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || "[](){}<>\":;|&".contains(c) || self.arrow_ahead() {
                break;
            }
            self.i += c.len_utf8();
        }
        if self.i == start {
            return None;
        }
        let mut node = NodeRef {
            id: self.s[start..self.i].to_string(),
            kind: None,
            text: None,
            span: None,
            classes: Vec::new(),
        };
        if self.rest().starts_with("<[") {
            let save = self.i;
            self.i += 2;
            match self.shape_text(&[("]>", Shape::Rect)]) {
                Some((_, text)) => {
                    node.text = Some(text);
                    node.kind = Some(Kind::Arrow(self.heads()));
                }
                None => self.i = save,
            }
        } else {
            for (open, closes) in OPENERS {
                if self.rest().starts_with(open) {
                    let save = self.i;
                    self.i += open.len();
                    match self.shape_text(closes) {
                        Some((shape, text)) => {
                            node.kind = Some(Kind::Node(shape));
                            node.text = Some(text);
                            break;
                        }
                        None => self.i = save,
                    }
                }
            }
        }
        while self.eat(":::") {
            let start = self.i;
            while let Some(c) = self.peek() {
                if c.is_whitespace() || c == ':' || self.arrow_ahead() {
                    break;
                }
                self.i += c.len_utf8();
            }
            node.classes.extend(
                self.s[start..self.i]
                    .split(',')
                    .filter(|c| !c.is_empty())
                    .map(str::to_string),
            );
        }
        if self.rest().starts_with(':')
            && self.rest()[1..].starts_with(|c: char| c.is_ascii_digit())
        {
            self.i += 1;
            node.span = self.number();
        }
        Some(node)
    }

    /// `(right)`, `(x, down)` after a block arrow.
    fn heads(&mut self) -> Heads {
        let mut heads = Heads::default();
        let r = self.rest();
        if let Some(body) = r.strip_prefix('(')
            && let Some(end) = body.find(')')
        {
            for part in body[..end].split(',') {
                match part.trim() {
                    "right" => heads.right = true,
                    "left" => heads.left = true,
                    "up" => heads.up = true,
                    "down" => heads.down = true,
                    "x" => {
                        heads.left = true;
                        heads.right = true;
                    }
                    "y" => {
                        heads.up = true;
                        heads.down = true;
                    }
                    _ => {}
                }
            }
            self.i += end + 2;
        }
        if heads == Heads::default() {
            heads.right = true;
        }
        heads
    }

    /// Text up to one of `closes`; a leading `"` quotes it.
    fn shape_text(&mut self, closes: &[(&str, Shape)]) -> Option<(Shape, String)> {
        let rest = self.rest();
        if let Some(quoted) = rest.strip_prefix('"') {
            let end = quoted.find('"')? + 1;
            let text = &rest[..=end];
            let after = rest[end + 1..].trim_start();
            for &(close, shape) in closes {
                if after.starts_with(close) {
                    self.i += rest.len() - after.len() + close.len();
                    return Some((shape, text.to_string()));
                }
            }
            return None;
        }
        let mut best: Option<(usize, &str, Shape)> = None;
        for &(close, shape) in closes {
            if let Some(pos) = rest.find(close)
                && best.is_none_or(|(b, ..)| pos < b)
            {
                best = Some((pos, close, shape));
            }
        }
        let (pos, close, shape) = best?;
        let text = rest[..pos].to_string();
        self.i += pos + close.len();
        Some((shape, text))
    }

    fn link(&mut self) -> Option<LinkRef> {
        let save = self.i;
        let head = |c: char| match c {
            '>' | '<' => Some(Marker::Arrow),
            'o' => Some(Marker::Circle),
            'x' => Some(Marker::Cross),
            _ => None,
        };
        let mut start = None;
        {
            let r = self.rest();
            let mut chars = r.chars();
            if let Some(c) = chars.next()
                && matches!(c, '<' | 'o' | 'x')
                && (r[1..].starts_with("--")
                    || r[1..].starts_with("==")
                    || r[1..].starts_with("-."))
            {
                start = head(c);
                self.i += 1;
            }
        }
        let r = self.rest();
        let end_head = |cur: &mut Self| {
            let h = cur
                .peek()
                .filter(|c| matches!(c, '>' | 'o' | 'x'))
                .and_then(head);
            if h.is_some() {
                cur.i += 1;
            }
            h
        };
        let mut link = if r.starts_with("~~~") {
            self.i += r.chars().take_while(|&c| c == '~').count();
            LinkRef {
                line: LineKind::Invisible,
                start: None,
                end: None,
                label: None,
            }
        } else if r.starts_with("-.") {
            self.i += 1;
            self.i += self.rest().chars().take_while(|&c| c == '.').count();
            if self.eat("-") {
                let end = end_head(self);
                LinkRef {
                    line: LineKind::Dotted,
                    start,
                    end,
                    label: None,
                }
            } else {
                let r = self.rest();
                let close = r.find(".-")?;
                let text = r[..close].trim().to_string();
                self.i += close;
                self.i += self.rest().chars().take_while(|&c| c == '.').count();
                if !self.eat("-") {
                    self.i = save;
                    return None;
                }
                let end = end_head(self);
                LinkRef {
                    line: LineKind::Dotted,
                    start,
                    end,
                    label: Some(text),
                }
            }
        } else if r.starts_with("--") || r.starts_with("==") {
            let ch = r.chars().next()?;
            let line = if ch == '-' {
                LineKind::Solid
            } else {
                LineKind::Thick
            };
            let n = r.chars().take_while(|&c| c == ch).count();
            self.i += n;
            let head_next = match self.peek() {
                Some('>') => true,
                Some('o' | 'x') => {
                    !self.rest()[1..].starts_with(|c: char| c.is_alphanumeric() || c == '_')
                }
                _ => false,
            };
            match self.peek() {
                _ if head_next => {
                    let end = end_head(self);
                    LinkRef {
                        line,
                        start,
                        end,
                        label: None,
                    }
                }
                _ if n == 2 => {
                    let r = self.rest();
                    let pair: String = [ch, ch].iter().collect();
                    let Some(close) = r.find(&pair) else {
                        self.i = save;
                        return None;
                    };
                    let text = r[..close].trim().to_string();
                    self.i += close;
                    self.i += self.rest().chars().take_while(|&c| c == ch).count();
                    let end = end_head(self);
                    LinkRef {
                        line,
                        start,
                        end,
                        label: Some(text).filter(|t| !t.is_empty()),
                    }
                }
                _ => LinkRef {
                    line,
                    start,
                    end: None,
                    label: None,
                },
            }
        } else {
            self.i = save;
            return None;
        };
        // `|label|`
        let save = self.i;
        self.ws();
        if self.eat("|") {
            match self.rest().find('|') {
                Some(end) => {
                    link.label =
                        Some(self.rest()[..end].trim().to_string()).filter(|t| !t.is_empty());
                    self.i += end + 1;
                }
                None => self.i = save,
            }
        } else {
            self.i = save;
        }
        Some(link)
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
    fn columns_widths_spaces_and_groups() {
        let m = parse_src(
            "block-beta\ncolumns 3\na[\"A label\"] b:2 c:2 d\nspace:2 e\nblock:g1:2\n  columns 2\n  h i\nend\nblock\n  x\nend",
        );
        assert_eq!(m.columns, Some(3));
        let names: Vec<&str> = m.root.iter().map(|&i| m.blocks[i].id.as_str()).collect();
        assert_eq!(
            names,
            vec!["a", "b", "c", "d", "space-1", "e", "g1", "block-2"]
        );
        let b = &m.blocks[m.root[1]];
        assert_eq!(b.span, 2);
        assert_eq!(m.blocks[m.root[4]].span, 2);
        let g = &m.blocks[m.root[6]];
        assert_eq!((g.span, g.children.len()), (2, 2));
        assert_eq!(g.kind, Kind::Group { columns: Some(2) });
        assert_eq!(m.blocks[m.root[0]].label.text(), "A label");
    }

    #[test]
    fn shapes_arrows_and_links() {
        let m = parse_src(
            "block-beta\nA((\"DB\")) B[(\"Cyl\")] C{{\"Hex\"}} D>\"Odd\"] E[/\"Trap\"\\]\nar<[\"Go\"]>(down) ar2<[\" \"]>(x)\nA --> B\nB-- \"X\" -->C\nC --- D\nid1(\"Start\")-->id2(\"Stop\")",
        );
        let kind = |id: &str| m.blocks.iter().find(|b| b.id == id).unwrap().kind.clone();
        assert_eq!(kind("A"), Kind::Node(Shape::Circle));
        assert_eq!(kind("B"), Kind::Node(Shape::Cylinder));
        assert_eq!(kind("C"), Kind::Node(Shape::Hexagon));
        assert_eq!(kind("D"), Kind::Node(Shape::Odd));
        assert_eq!(kind("E"), Kind::Node(Shape::TrapezoidBottom));
        assert_eq!(
            kind("ar"),
            Kind::Arrow(Heads {
                down: true,
                ..Heads::default()
            })
        );
        assert_eq!(
            kind("ar2"),
            Kind::Arrow(Heads {
                left: true,
                right: true,
                ..Heads::default()
            })
        );
        assert_eq!(kind("id1"), Kind::Node(Shape::Rounded));
        assert_eq!(m.edges.len(), 4);
        assert_eq!(
            m.edges[1].label.as_ref().map(|l| l.text()),
            Some("X".into())
        );
        assert_eq!(m.edges[2].end, None);
        assert_eq!(m.edges[0].end, Some(Marker::Arrow));
    }

    #[test]
    fn styles_and_classes() {
        let m = parse_src(
            "block-beta\nA B\nclassDef blue fill:#6e6ce6,stroke:#333;\nclass A blue\nstyle B fill:#bbf,stroke-dasharray: 5 5",
        );
        assert!(m.blocks[0].style.fill.is_some());
        assert_eq!(m.blocks[1].style.dashed, Some(true));
    }
}
