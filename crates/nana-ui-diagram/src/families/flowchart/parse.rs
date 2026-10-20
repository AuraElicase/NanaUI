//! The flowchart grammar: node shapes, every link form and length, `&`
//! chains, subgraphs, `direction`, `classDef` / `class` / `:::`, `style`,
//! `linkStyle`, `click` and v11 `@{ ... }` attributes on nodes and edges.

use std::collections::HashMap;

use super::{Curve, Edge, EndRef, LineKind, Model, Node, Subgraph};
use crate::families::ParseCx;
use crate::geom::{Marker, Shape};
use crate::kind;
use crate::layout::Direction;
use crate::source::directives::AuthorStyle;
use crate::source::split_statements;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

struct NodeRef {
    name: String,
    shape: Option<(Shape, String)>,
    classes: Vec<String>,
    attrs: Vec<(String, String)>,
}

struct Link {
    line: LineKind,
    start: Option<Marker>,
    end: Option<Marker>,
    len: u32,
    label: Option<String>,
    id: Option<String>,
}

struct PendingEdge {
    from: String,
    to: String,
    edge: Edge,
    id: Option<String>,
}

#[derive(Default)]
struct Draft {
    explicit: bool,
    label: Option<Label>,
    shape: Option<Shape>,
    classes: Vec<String>,
    subgraph: Option<usize>,
}

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    order: Vec<String>,
    drafts: HashMap<String, Draft>,
    edges: Vec<PendingEdge>,
    subgraphs: Vec<Subgraph>,
    stack: Vec<usize>,
    direction: Direction,
    class_defs: HashMap<String, AuthorStyle>,
    class_assign: Vec<(String, String)>,
    styles: Vec<(String, String)>,
    link_styles: Vec<(Option<Vec<usize>>, String, Option<Curve>)>,
    edge_attrs: Vec<(String, Vec<(String, String)>)>,
    clicks: Vec<(String, String, Option<String>)>,
    default_curve: Option<Curve>,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let default_curve = cx.config.get("flowchart.curve").and_then(Curve::parse);
    let mut p = Parser {
        cx,
        order: Vec::new(),
        drafts: HashMap::new(),
        edges: Vec::new(),
        subgraphs: Vec::new(),
        stack: Vec::new(),
        direction: Direction::TB,
        class_defs: HashMap::new(),
        class_assign: Vec::new(),
        styles: Vec::new(),
        link_styles: Vec::new(),
        edge_attrs: Vec::new(),
        clicks: Vec::new(),
        default_curve,
        line: 0,
    };
    let mut header_seen = false;
    for (index, line) in body.lines().enumerate() {
        p.line = index;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mut rest = trimmed;
        if !header_seen {
            let Some((_, after)) = kind::header(trimmed) else {
                return Err(DiagramError::syntax(index as u32 + 1, 1, "expected a flowchart header"));
            };
            header_seen = true;
            let after = after.trim_start_matches(':').trim();
            let split = after.find(|c: char| c.is_whitespace() || c == ';').unwrap_or(after.len());
            let (first, tail) = after.split_at(split);
            if let Some(d) = Direction::parse(first) {
                p.direction = d;
                rest = tail.trim_start_matches(|c: char| c.is_whitespace() || c == ';');
            } else {
                rest = after.trim_start_matches(';');
            }
            if rest.is_empty() {
                continue;
            }
        }
        for (_, stmt) in split_statements(rest) {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                p.statement(stmt)?;
            }
        }
    }
    if !header_seen {
        return Err(DiagramError::Empty);
    }
    p.finish()
}

fn first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

impl Parser<'_, '_> {
    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let (word, rest) = first_word(stmt);
        match word {
            "subgraph" => return self.open_subgraph(rest),
            "end" if rest.is_empty() => {
                if self.stack.pop().is_none() {
                    self.cx.warn(self.line, "`end` without a subgraph");
                }
                return Ok(());
            }
            "direction" => {
                if let Some(d) = Direction::parse(rest) {
                    match self.stack.last() {
                        Some(&s) => self.subgraphs[s].direction = Some(d),
                        None => self.direction = d,
                    }
                }
                return Ok(());
            }
            "classDef" => {
                let (names, decls) = first_word(rest);
                for name in names.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    self.class_defs.entry(name.to_string()).or_default().apply(decls);
                }
                return Ok(());
            }
            "class" => {
                let (ids, class) = first_word(rest);
                let class = class.trim();
                for id in ids.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    self.class_assign.push((id.to_string(), class.to_string()));
                }
                return Ok(());
            }
            "style" => {
                let (id, decls) = first_word(rest);
                self.styles.push((id.to_string(), decls.to_string()));
                return Ok(());
            }
            "linkStyle" => {
                let (which, mut decls) = first_word(rest);
                let targets = if which == "default" {
                    None
                } else {
                    Some(which.split(',').filter_map(|n| n.trim().parse::<usize>().ok()).collect())
                };
                let mut curve = None;
                if let Some(after) = decls.strip_prefix("interpolate") {
                    let (name, tail) = first_word(after);
                    curve = Curve::parse(name);
                    decls = tail;
                }
                self.link_styles.push((targets, decls.to_string(), curve));
                return Ok(());
            }
            "click" => {
                self.click(rest);
                return Ok(());
            }
            _ => {}
        }
        self.chain(stmt)
    }

    fn open_subgraph(&mut self, rest: &str) -> Result<(), DiagramError> {
        self.cx.limits.check_count(self.subgraphs.len() + 1, self.cx.limits.clusters, LimitKind::Clusters)?;
        let rest = rest.trim();
        let auto = || format!("subGraph{}", self.subgraphs.len());
        let (id, title) = if rest.is_empty() {
            (auto(), String::new())
        } else if let Some(open) = rest.find('[').filter(|&i| i > 0 && rest.ends_with(']')) {
            (rest[..open].trim().to_string(), rest[open + 1..rest.len() - 1].to_string())
        } else if rest.starts_with('"') {
            (auto(), rest.to_string())
        } else {
            (rest.to_string(), rest.to_string())
        };
        let title = self.cx.label(&title)?;
        let index = self.subgraphs.len();
        self.subgraphs.push(Subgraph {
            id,
            title,
            parent: self.stack.last().copied(),
            direction: None,
            classes: Vec::new(),
            style: AuthorStyle::default(),
        });
        self.stack.push(index);
        Ok(())
    }

    fn click(&mut self, rest: &str) {
        let (id, tail) = first_word(rest);
        let mut tokens = Words::new(tail);
        let mut url = None;
        let mut tooltip = None;
        match tokens.next() {
            Some(t) if t == "href" => {
                url = tokens.next();
                tooltip = tokens.next().filter(|t| !t.starts_with('_'));
            }
            Some(t) if tokens.was_quoted => {
                url = Some(t);
                tooltip = tokens.next().filter(|t| !t.starts_with('_'));
            }
            _ => {}
        }
        if let Some(url) = url {
            self.clicks.push((id.to_string(), url, tooltip));
        }
    }

    fn mention(&mut self, node: &NodeRef) -> Result<(), DiagramError> {
        if !self.drafts.contains_key(&node.name) {
            self.cx.limits.check_count(self.order.len() + 1, self.cx.limits.nodes, LimitKind::Nodes)?;
            self.order.push(node.name.clone());
        }
        let top = self.stack.last().copied();
        let draft = self.drafts.entry(node.name.clone()).or_default();
        if draft.subgraph.is_none() {
            draft.subgraph = top;
        }
        if let Some((shape, text)) = &node.shape {
            draft.explicit = true;
            draft.shape = Some(*shape);
            self.cx.limits.check_label(text)?;
            draft.label = Some(Label::parse(text));
        }
        for (key, value) in &node.attrs {
            match key.as_str() {
                "shape" => {
                    if let Some(shape) = Shape::from_name(value) {
                        draft.explicit = true;
                        draft.shape = Some(shape);
                    }
                }
                "label" => {
                    self.cx.limits.check_label(value)?;
                    draft.explicit = true;
                    draft.label = Some(Label::parse(value));
                }
                _ => {}
            }
        }
        draft.classes.extend(node.classes.iter().cloned());
        Ok(())
    }

    fn chain(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let mut cur = Cur { s: stmt, i: 0 };
        let Some(first) = cur.group() else {
            self.cx.warn(self.line, format!("cannot read `{}`", truncate(stmt)));
            return Ok(());
        };
        // `id@{ curve: ... }` on its own names an edge.
        if first.len() == 1 && cur.at_end() && !first[0].attrs.is_empty() && first[0].shape.is_none() {
            let r = &first[0];
            let is_edge = self.edges.iter().any(|e| e.id.as_deref() == Some(r.name.as_str()));
            let is_node_attr = r.attrs.iter().any(|(k, _)| k == "shape" || k == "label");
            if is_edge && !is_node_attr {
                self.edge_attrs.push((r.name.clone(), r.attrs.clone()));
                return Ok(());
            }
        }
        for n in &first {
            self.mention(n)?;
        }
        let mut left = first;
        loop {
            cur.ws();
            if cur.at_end() {
                break;
            }
            let Some(link) = cur.link() else {
                self.cx.warn(self.line, format!("cannot read link at `{}`", truncate(cur.rest())));
                break;
            };
            cur.ws();
            let Some(right) = cur.group() else {
                self.cx.warn(self.line, "a link without a target");
                break;
            };
            for n in &right {
                self.mention(n)?;
            }
            let label = match &link.label {
                Some(t) => Some(self.cx.label(t)?).filter(|l| !l.is_empty()),
                None => None,
            };
            for a in &left {
                for b in &right {
                    self.cx.limits.check_count(self.edges.len() + 1, self.cx.limits.edges, LimitKind::Edges)?;
                    self.edges.push(PendingEdge {
                        from: a.name.clone(),
                        to: b.name.clone(),
                        id: link.id.clone(),
                        edge: Edge {
                            from: EndRef::Node(0),
                            to: EndRef::Node(0),
                            line: link.line,
                            start: link.start,
                            end: link.end,
                            label: label.clone(),
                            min_len: link.len.clamp(1, 12),
                            style: AuthorStyle::default(),
                            curve: None,
                        },
                    });
                }
            }
            left = right;
        }
        if !cur.at_end() {
            self.cx.warn(self.line, format!("ignored `{}`", truncate(cur.rest())));
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Model, DiagramError> {
        let sub_index: HashMap<String, usize> = self.subgraphs.iter().enumerate().map(|(i, s)| (s.id.clone(), i)).collect();
        // Names that are subgraph ids are cluster ends unless a node with a
        // shape claimed them.
        let mut index: HashMap<String, usize> = HashMap::new();
        let mut nodes = Vec::new();
        for name in &self.order {
            let draft = &self.drafts[name];
            if sub_index.contains_key(name) && !draft.explicit {
                continue;
            }
            index.insert(name.clone(), nodes.len());
            nodes.push(Node {
                id: name.clone(),
                label: draft.label.clone().unwrap_or_else(|| Label::plain(name)),
                shape: draft.shape.unwrap_or(Shape::Rect),
                classes: draft.classes.clone(),
                style: AuthorStyle::default(),
                link: None,
                subgraph: draft.subgraph,
            });
        }
        let resolve = |name: &str| -> EndRef {
            match index.get(name) {
                Some(&n) => EndRef::Node(n),
                None => EndRef::Subgraph(sub_index[name]),
            }
        };
        let mut edges: Vec<Edge> = Vec::with_capacity(self.edges.len());
        let mut edge_by_id: HashMap<String, usize> = HashMap::new();
        for pending in &self.edges {
            let mut edge = pending.edge.clone();
            edge.from = resolve(&pending.from);
            edge.to = resolve(&pending.to);
            if let Some(id) = &pending.id {
                edge_by_id.entry(id.clone()).or_insert(edges.len());
            }
            edges.push(edge);
        }
        for (id, attrs) in &self.edge_attrs {
            if let Some(&e) = edge_by_id.get(id) {
                for (k, v) in attrs {
                    if k == "curve" {
                        edges[e].curve = Curve::parse(v);
                    }
                }
            }
        }
        for (targets, decls, curve) in &self.link_styles {
            let style = AuthorStyle::parse(decls);
            let apply = |e: &mut Edge| {
                e.style = e.style.merged(&style);
                if curve.is_some() {
                    e.curve = *curve;
                }
            };
            match targets {
                None => edges.iter_mut().for_each(apply),
                Some(list) => {
                    for &i in list {
                        if let Some(e) = edges.get_mut(i) {
                            apply(e);
                        }
                    }
                }
            }
        }
        for (id, class) in &self.class_assign {
            if let Some(&n) = index.get(id) {
                nodes[n].classes.push(class.clone());
            } else if let Some(&s) = sub_index.get(id) {
                self.subgraphs[s].classes.push(class.clone());
            }
        }
        let default = self.class_defs.get("default").copied().unwrap_or_default();
        for node in &mut nodes {
            let mut style = default;
            for class in &node.classes {
                if let Some(def) = self.class_defs.get(class) {
                    style = style.merged(def);
                }
            }
            node.style = style;
        }
        for sub in &mut self.subgraphs {
            let mut style = AuthorStyle::default();
            for class in &sub.classes {
                if let Some(def) = self.class_defs.get(class) {
                    style = style.merged(def);
                }
            }
            sub.style = style;
        }
        for (id, decls) in &self.styles {
            if let Some(&n) = index.get(id) {
                nodes[n].style.apply(decls);
            } else if let Some(&s) = sub_index.get(id) {
                self.subgraphs[s].style.apply(decls);
            }
        }
        for (id, url, tooltip) in self.clicks {
            if let Some(&n) = index.get(&id) {
                nodes[n].link = Some((url, tooltip));
            }
        }
        Ok(Model {
            direction: self.direction,
            nodes,
            edges,
            subgraphs: self.subgraphs,
            default_curve: self.default_curve,
        })
    }
}

fn truncate(s: &str) -> String {
    s.chars().take(40).collect()
}

/// Whitespace-separated words where `"..."` is one word.
struct Words<'s> {
    rest: &'s str,
    was_quoted: bool,
}

impl<'s> Words<'s> {
    fn new(rest: &'s str) -> Self {
        Self { rest, was_quoted: false }
    }

    fn next(&mut self) -> Option<String> {
        let s = self.rest.trim_start();
        if s.is_empty() {
            return None;
        }
        if let Some(body) = s.strip_prefix('"') {
            let end = body.find('"').unwrap_or(body.len());
            self.rest = body.get(end + 1..).unwrap_or("");
            self.was_quoted = true;
            return Some(body[..end].to_string());
        }
        let end = s.find(char::is_whitespace).unwrap_or(s.len());
        self.rest = &s[end..];
        self.was_quoted = false;
        Some(s[..end].to_string())
    }
}

/// A cursor over one statement.
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
    ("[/", &[("/]", Shape::LeanRight), ("\\]", Shape::TrapezoidBottom)]),
    ("[\\", &[("\\]", Shape::LeanLeft), ("/]", Shape::TrapezoidTop)]),
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

    fn arrow_ahead(&self) -> bool {
        let r = self.rest();
        r.starts_with("--") || r.starts_with("-.") || r.starts_with("==") || r.starts_with("~~")
    }

    fn group(&mut self) -> Option<Vec<NodeRef>> {
        let mut out = vec![self.node()?];
        loop {
            let save = self.i;
            self.ws();
            if self.eat("&") {
                self.ws();
                match self.node() {
                    Some(n) => out.push(n),
                    None => {
                        self.i = save;
                        break;
                    }
                }
            } else {
                self.i = save;
                break;
            }
        }
        Some(out)
    }

    fn node(&mut self) -> Option<NodeRef> {
        self.ws();
        let start = self.i;
        while let Some(c) = self.peek() {
            if c.is_whitespace() || "[](){}|&;\"<>".contains(c) {
                break;
            }
            let r = self.rest();
            if r.starts_with(":::") || r.starts_with("@{") || self.arrow_ahead() {
                break;
            }
            self.i += c.len_utf8();
        }
        if self.i == start {
            return None;
        }
        let name = self.s[start..self.i].to_string();
        let mut node = NodeRef {
            name,
            shape: None,
            classes: Vec::new(),
            attrs: Vec::new(),
        };
        for (open, closes) in OPENERS {
            if self.rest().starts_with(open) {
                let save = self.i;
                self.i += open.len();
                match self.shape_text(closes) {
                    Some((shape, text)) => {
                        node.shape = Some((shape, text));
                        break;
                    }
                    None => {
                        self.i = save;
                        continue;
                    }
                }
            }
        }
        if self.rest().starts_with("@{") {
            self.i += 1;
            if let Some(body) = self.braced() {
                node.attrs = attributes(body);
            }
        }
        while self.eat(":::") {
            let start = self.i;
            while let Some(c) = self.peek() {
                if c.is_whitespace() || "&;".contains(c) || self.arrow_ahead() {
                    break;
                }
                self.i += c.len_utf8();
            }
            node.classes.extend(self.s[start..self.i].split(',').map(|s| s.to_string()));
        }
        Some(node)
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
                    let consumed = rest.len() - after.len() + close.len();
                    self.i += consumed;
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

    /// The contents of a `{ ... }` block starting at the cursor.
    fn braced(&mut self) -> Option<&'s str> {
        let rest = self.rest();
        if !rest.starts_with('{') {
            return None;
        }
        let mut depth = 0;
        let mut quote = false;
        for (i, c) in rest.char_indices() {
            match c {
                '"' => quote = !quote,
                '{' if !quote => depth += 1,
                '}' if !quote => {
                    depth -= 1;
                    if depth == 0 {
                        self.i += i + 1;
                        return Some(&rest[1..i]);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn link(&mut self) -> Option<Link> {
        self.ws();
        let save = self.i;
        let mut id = None;
        // `e1@-->`: an edge id.
        {
            let r = self.rest();
            let ident_len = r.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-')).unwrap_or(r.len());
            if ident_len > 0 && r[ident_len..].starts_with('@') {
                let after = &r[ident_len + 1..];
                if after.starts_with("--") || after.starts_with("==") || after.starts_with("-.") || after.starts_with("~~") || after.starts_with('<') {
                    id = Some(r[..ident_len].to_string());
                    self.i += ident_len + 1;
                }
            }
        }
        let head = |c: char| match c {
            '>' => Some(Marker::Arrow),
            'o' => Some(Marker::Circle),
            'x' => Some(Marker::Cross),
            '<' => Some(Marker::Arrow),
            _ => None,
        };
        let mut start = None;
        {
            let r = self.rest();
            let mut chars = r.chars();
            if let (Some(c), Some(n)) = (chars.next(), chars.next())
                && matches!(c, '<' | 'o' | 'x')
                && matches!(n, '-' | '=')
                && (r[1..].starts_with("--") || r[1..].starts_with("==") || r[1..].starts_with("-."))
            {
                start = head(c);
                self.i += 1;
            }
        }
        let r = self.rest();
        let link = if r.starts_with("~~~") {
            let n = r.chars().take_while(|&c| c == '~').count();
            self.i += n;
            Link {
                line: LineKind::Invisible,
                start: None,
                end: None,
                len: (n as u32).saturating_sub(2).max(1),
                label: None,
                id,
            }
        } else if r.starts_with("-.") {
            self.i += 1;
            let dots = self.rest().chars().take_while(|&c| c == '.').count();
            self.i += dots;
            if self.eat("-") {
                let end = self.peek().and_then(|c| if matches!(c, '>' | 'o' | 'x') { head(c) } else { None });
                if end.is_some() {
                    self.i += 1;
                }
                Link {
                    line: LineKind::Dotted,
                    start,
                    end,
                    len: dots as u32,
                    label: None,
                    id,
                }
            } else {
                // `-. text .->`
                let r = self.rest();
                let close = r.find(".-")?;
                let text = r[..close].trim().to_string();
                self.i += close;
                let dots = self.rest().chars().take_while(|&c| c == '.').count();
                self.i += dots;
                if !self.eat("-") {
                    self.i = save;
                    return None;
                }
                let end = self.peek().and_then(|c| if matches!(c, '>' | 'o' | 'x') { head(c) } else { None });
                if end.is_some() {
                    self.i += 1;
                }
                Link {
                    line: LineKind::Dotted,
                    start,
                    end,
                    len: dots as u32,
                    label: Some(text),
                    id,
                }
            }
        } else if r.starts_with("--") || r.starts_with("==") {
            let ch = r.chars().next()?;
            let line = if ch == '-' { LineKind::Solid } else { LineKind::Thick };
            let n = r.chars().take_while(|&c| c == ch).count();
            self.i += n;
            let next = self.peek();
            match next {
                Some(c) if matches!(c, '>' | 'o' | 'x') => {
                    self.i += 1;
                    Link {
                        line,
                        start,
                        end: head(c),
                        len: (n as u32).saturating_sub(1).max(1),
                        label: None,
                        id,
                    }
                }
                _ if n == 2 => {
                    // `-- text -->`: find the closing run.
                    let r = self.rest();
                    let pair: String = [ch, ch].iter().collect();
                    let close = r.find(&pair)?;
                    let text = r[..close].trim().to_string();
                    self.i += close;
                    let m = self.rest().chars().take_while(|&c| c == ch).count();
                    self.i += m;
                    let end = self.peek().and_then(|c| if matches!(c, '>' | 'o' | 'x') { head(c) } else { None });
                    if end.is_some() {
                        self.i += 1;
                    }
                    let len = if end.is_some() { m.saturating_sub(1) } else { m.saturating_sub(2) };
                    Link {
                        line,
                        start,
                        end,
                        len: (len as u32).max(1),
                        label: Some(text).filter(|t| !t.is_empty()),
                        id,
                    }
                }
                _ => Link {
                    line,
                    start,
                    end: None,
                    len: (n as u32).saturating_sub(2).max(1),
                    label: None,
                    id,
                },
            }
        } else {
            self.i = save;
            return None;
        };
        let mut link = link;
        // `|label|`
        let save = self.i;
        self.ws();
        if self.eat("|") {
            let r = self.rest();
            match r.find('|') {
                Some(end) => {
                    link.label = Some(r[..end].trim().to_string()).filter(|t| !t.is_empty());
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

/// `key: value, key: "value"` pairs.
fn attributes(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut quote = false;
    let mut start = 0;
    let mut parts = Vec::new();
    for (i, c) in body.char_indices() {
        match c {
            '"' => quote = !quote,
            ',' if !quote => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    for part in parts {
        if let Some((k, v)) = part.split_once(':') {
            let v = v.trim();
            out.push((k.trim().to_string(), v.to_string()));
        }
    }
    out
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
    fn shapes_and_labels() {
        let m = parse_src(
            "flowchart LR\nA[Rect] --> B(Round) --> C([Stadium]) --> D[[Sub]] --> E[(DB)] --> F((Circle))\nG>Odd] --> H{Diamond} --> I{{Hex}} --> J[/Lean/] --> K[\\Left\\] --> L[/Trap\\] --> M[\\Inv/] --> N(((Dbl)))\nO@{ shape: doc, label: \"Doc\" }",
        );
        assert_eq!(m.direction, Direction::LR);
        let shapes: Vec<Shape> = m.nodes.iter().map(|n| n.shape).collect();
        assert_eq!(
            shapes,
            vec![
                Shape::Rect,
                Shape::Rounded,
                Shape::Stadium,
                Shape::Subroutine,
                Shape::Cylinder,
                Shape::Circle,
                Shape::Odd,
                Shape::Diamond,
                Shape::Hexagon,
                Shape::LeanRight,
                Shape::LeanLeft,
                Shape::TrapezoidBottom,
                Shape::TrapezoidTop,
                Shape::DoubleCircle,
                Shape::Document,
            ]
        );
        assert_eq!(m.nodes[14].label.text(), "Doc");
    }

    #[test]
    fn link_forms_lengths_and_labels() {
        let m = parse_src(
            "graph TD\nA-->B\nA---C\nA-.->D\nA==>E\nA~~~F\nA--o G\nA--x H\nA<-->I\nA-- text -->J\nA-->|pipe|K\nA-. dotted .->L\nA== thick ==>M\nA---->N\nA-..->O",
        );
        let e = &m.edges;
        assert_eq!(e.len(), 14);
        assert_eq!((e[0].line, e[0].end, e[0].min_len), (LineKind::Solid, Some(Marker::Arrow), 1));
        assert_eq!((e[1].line, e[1].end), (LineKind::Solid, None));
        assert_eq!((e[2].line, e[2].end), (LineKind::Dotted, Some(Marker::Arrow)));
        assert_eq!(e[3].line, LineKind::Thick);
        assert_eq!(e[4].line, LineKind::Invisible);
        assert_eq!(e[5].end, Some(Marker::Circle));
        assert_eq!(e[6].end, Some(Marker::Cross));
        assert_eq!((e[7].start, e[7].end), (Some(Marker::Arrow), Some(Marker::Arrow)));
        assert_eq!(e[8].label.as_ref().map(|l| l.text()), Some("text".into()));
        assert_eq!(e[9].label.as_ref().map(|l| l.text()), Some("pipe".into()));
        assert_eq!(e[10].label.as_ref().map(|l| l.text()), Some("dotted".into()));
        assert_eq!((e[11].line, e[11].label.as_ref().map(|l| l.text())), (LineKind::Thick, Some("thick".into())));
        assert_eq!(e[12].min_len, 3);
        assert_eq!(e[13].min_len, 2);
    }

    #[test]
    fn chains_with_ampersands_fan_out() {
        let m = parse_src("flowchart TB\nA & B --> C & D --> E");
        assert_eq!(m.edges.len(), 6);
    }

    #[test]
    fn subgraphs_nest_and_take_members_and_edges() {
        let m = parse_src(
            "flowchart TB\nc1-->a2\nsubgraph one\n  a1-->a2\nend\nsubgraph two [Second]\n  direction LR\n  b1-->b2\n  subgraph inner\n    x\n  end\nend\none --> two\nc1 --> inner",
        );
        assert_eq!(m.subgraphs.len(), 3);
        assert_eq!(m.subgraphs[1].title.text(), "Second");
        assert_eq!(m.subgraphs[1].direction, Some(Direction::LR));
        assert_eq!(m.subgraphs[2].parent, Some(1));
        let a2 = m.nodes.iter().find(|n| n.id == "a2").unwrap();
        assert_eq!(a2.subgraph, Some(0), "a mention inside a subgraph moves the node in");
        let x = m.nodes.iter().find(|n| n.id == "x").unwrap();
        assert_eq!(x.subgraph, Some(2));
        let last = &m.edges[m.edges.len() - 2];
        assert_eq!((last.from, last.to), (EndRef::Subgraph(0), EndRef::Subgraph(1)));
        assert!(!m.nodes.iter().any(|n| n.id == "one"));
    }

    #[test]
    fn styles_classes_links_and_edge_attributes() {
        let m = parse_src(
            "flowchart LR\nA:::hot --> B\nB e1@--> C\ne1@{ curve: linear }\nclassDef hot fill:#f00,color:#fff\nclass B hot\nstyle C stroke:#00f\nlinkStyle 0 stroke:#0f0,stroke-width:3px\nclick A href \"https://example.com\" \"Go\"",
        );
        assert!(m.nodes[0].style.fill.is_some());
        assert!(m.nodes[1].style.fill.is_some());
        assert!(m.nodes[2].style.stroke.is_some());
        assert_eq!(m.edges[0].style.stroke_width, Some(3.0));
        assert_eq!(m.edges[1].curve, Some(Curve::Linear));
        assert_eq!(m.nodes[0].link.as_ref().map(|l| l.0.as_str()), Some("https://example.com"));
    }

    #[test]
    fn quoted_and_markdown_labels() {
        let m = parse_src("flowchart TD\nA[\"Quoted (with) [brackets]\"] --> B[\"`**md** text`\"]\nC[中文标签] --> D");
        assert_eq!(m.nodes[0].label.text(), "Quoted (with) [brackets]");
        assert!(m.nodes[1].label.markdown);
        assert_eq!(m.nodes[2].label.text(), "中文标签");
    }
}
