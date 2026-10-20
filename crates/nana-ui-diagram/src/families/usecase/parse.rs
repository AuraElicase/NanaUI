//! The use case grammar (Mermaid's `usecase-beta`): actors and their
//! variants, ellipse and rectangle use cases, quoted declarations,
//! stereotypes, `business`, system boundaries (with `type: package`),
//! notes, JSON tables, the association operators with labels and lengths,
//! include / extend / generalization, edge ids, `direction`, `classDef` /
//! `class` / `:::` and `style`.

use std::collections::HashMap;

use super::{ActorLook, Boundary, Edge, EdgeKind, Model, Node, NodeKind, json};
use crate::families::ParseCx;
use crate::families::graphkit::{Styles, attributes, balanced, first_word, ident_len, statements};
use crate::geom::Marker;
use crate::kind;
use crate::layout::Direction;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

/// A declaration: `id("label")@{...} <<stereo>>:::classes`.
#[derive(Debug, Default)]
struct Decl {
    id: String,
    label: Option<String>,
    rect: bool,
    meta: Vec<(String, String)>,
    stereotype: Option<String>,
    classes: Vec<String>,
}

#[derive(Default)]
struct Draft {
    kind: Option<NodeKind>,
    label: Option<Label>,
    stereotype: Option<String>,
    business: bool,
    boundary: Option<usize>,
    classes: Vec<String>,
}

struct PendingEdge {
    from: String,
    to: String,
    edge: Edge,
    line: usize,
}

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    order: Vec<String>,
    drafts: HashMap<String, Draft>,
    boundaries: Vec<Boundary>,
    boundary_index: HashMap<String, usize>,
    open: Option<usize>,
    edges: Vec<PendingEdge>,
    styles: Styles,
    direction: Direction,
    notes: usize,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut p = Parser {
        cx,
        order: Vec::new(),
        drafts: HashMap::new(),
        boundaries: Vec::new(),
        boundary_index: HashMap::new(),
        open: None,
        edges: Vec::new(),
        styles: Styles::default(),
        direction: Direction::TB,
        notes: 0,
        line: 0,
    };
    let mut header = false;
    for (index, stmt) in statements(body) {
        p.line = index;
        if !header {
            let Some((_, rest)) = kind::header(&stmt) else {
                return Err(DiagramError::syntax(index as u32 + 1, 1, "expected `usecase-beta`"));
            };
            header = true;
            if !rest.is_empty() {
                p.statement(rest)?;
            }
            continue;
        }
        p.statement(&stmt)?;
    }
    if !header {
        return Err(DiagramError::Empty);
    }
    if p.open.is_some() {
        p.cx.warn(p.line, "a system boundary is not closed with `end`");
    }
    p.finish()
}

/// `"Reset password"` → `Reset_password`.
fn derived_id(label: &str) -> String {
    label
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' })
        .collect()
}

/// The end of a quoted label starting at `s[0] == '"'`: past the closing
/// quote, or past "`\"" for a markdown string.
fn quoted_end(s: &str) -> Option<usize> {
    if s.starts_with("\"`") {
        s[2..].find("`\"").map(|e| e + 4)
    } else {
        s[1..].find('"').map(|e| e + 2)
    }
}

/// Read a declaration that must span all of `s`.
fn declaration(s: &str) -> Option<Decl> {
    let s = s.trim();
    let mut d = Decl::default();
    let mut rest;
    if s.starts_with('"') {
        let end = quoted_end(s)?;
        let raw = &s[..end];
        d.label = Some(raw.to_string());
        d.id = derived_id(Label::parse(raw).text().trim());
        rest = &s[end..];
    } else {
        let len = ident_len(s, "");
        if len == 0 {
            return None;
        }
        d.id = s[..len].to_string();
        rest = &s[len..];
        let close = match rest.chars().next() {
            Some('(') => Some(')'),
            Some('[') => Some(']'),
            _ => None,
        };
        if let Some(close) = close {
            d.rect = close == ']';
            let inner = &rest[1..];
            let t = inner.trim_start();
            let (label, after) = if t.starts_with('"') {
                let end = quoted_end(t)?;
                let after = t[end..].trim_start();
                (t[..end].to_string(), after.strip_prefix(close)?)
            } else {
                let end = inner.find(close)?;
                (inner[..end].trim().to_string(), &inner[end + 1..])
            };
            d.label = Some(label);
            rest = after;
        }
    }
    rest = rest.trim_start();
    if rest.starts_with("@{") {
        let end = balanced(&rest[1..], '{', '}')? + 1;
        d.meta = attributes(&rest[2..end]);
        rest = rest[end + 1..].trim_start();
    }
    if let Some(body) = rest.strip_prefix("<<") {
        let end = body.find(">>")?;
        d.stereotype = Some(body[..end].trim().to_string()).filter(|s| !s.is_empty());
        rest = body[end + 2..].trim_start();
    }
    if let Some(classes) = rest.strip_prefix(":::") {
        d.classes = classes
            .split(',')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();
        rest = "";
    }
    rest.trim().is_empty().then_some(d)
}

struct Rel {
    from: String,
    to: String,
    kind: EdgeKind,
    start: Option<Marker>,
    end: Option<Marker>,
    label: Option<String>,
    min_len: u32,
    id: Option<String>,
}

fn dash_run(s: &str) -> usize {
    s.bytes().take_while(|&b| b == b'-').count()
}

/// A relationship statement, if `stmt` is one.
fn relation(stmt: &str) -> Option<Rel> {
    let from_len = ident_len(stmt, "");
    if from_len == 0 {
        return None;
    }
    let from = stmt[..from_len].to_string();
    let mut r = stmt[from_len..].trim_start();
    let mut id = None;
    let id_len = ident_len(r, "");
    if id_len > 0 && r[id_len..].starts_with('@') {
        let after = &r[id_len + 1..];
        if after.starts_with(['-', '.', '<']) || after.starts_with("o-") || after.starts_with("x-") {
            id = Some(r[..id_len].to_string());
            r = after;
        }
    }
    let whole = |t: &str| {
        let t = t.trim();
        (!t.is_empty() && ident_len(t, "") == t.len()).then(|| t.to_string())
    };
    if let Some(t) = r.strip_prefix("..>") {
        let t = t.trim_start().strip_prefix(':')?;
        let (word, target) = first_word(t);
        let kind = match word {
            "include" => EdgeKind::Include,
            "extend" => EdgeKind::Extend,
            _ => return None,
        };
        return Some(Rel {
            from,
            to: whole(target)?,
            kind,
            start: None,
            end: Some(Marker::OpenArrow),
            label: None,
            min_len: 1,
            id,
        });
    }
    let mut start = None;
    if r.starts_with("<--") {
        start = Some(Marker::OpenArrow);
        r = &r[1..];
    } else if r.starts_with("o--") {
        start = Some(Marker::Circle);
        r = &r[1..];
    } else if r.starts_with("x--") {
        start = Some(Marker::Cross);
        r = &r[1..];
    }
    let n = dash_run(r);
    if n < 2 {
        return None;
    }
    r = &r[n..];
    let mut kind = EdgeKind::Association;
    let mut label = None;
    let mut dashes = n;
    let ends_word = |t: &str| t.chars().nth(1).is_none_or(char::is_whitespace);
    let end = if let Some(t) = r.strip_prefix("|>") {
        r = t;
        kind = EdgeKind::Generalization;
        Some(Marker::Triangle)
    } else if let Some(t) = r.strip_prefix('>') {
        r = t;
        Some(Marker::OpenArrow)
    } else if (r.starts_with('o') || r.starts_with('x')) && ends_word(r) && start.is_none() {
        let m = if r.starts_with('o') { Marker::Circle } else { Marker::Cross };
        r = &r[1..];
        Some(m)
    } else if n == 2 && start.is_none() && r.starts_with(char::is_whitespace) && r.contains("--") {
        // `-- label -->`
        let t = r.trim_start();
        let (text, after) = if t.starts_with('"') {
            let e = quoted_end(t)?;
            (t[..e].to_string(), &t[e..])
        } else {
            let e = t.find("--")?;
            (t[..e].trim().to_string(), &t[e..])
        };
        label = Some(text);
        let after = after.trim_start();
        let m = dash_run(after);
        if m < 2 {
            return None;
        }
        dashes = m;
        let tail = &after[m..];
        match tail.strip_prefix('>') {
            Some(t) => {
                r = t;
                Some(Marker::OpenArrow)
            }
            None => {
                r = tail;
                None
            }
        }
    } else {
        None
    };
    Some(Rel {
        from,
        to: whole(r)?,
        kind,
        start,
        end,
        label,
        min_len: (dashes as u32).saturating_sub(1).clamp(1, 12),
        id,
    })
}

impl Parser<'_, '_> {
    fn check_nodes(&self) -> Result<(), DiagramError> {
        self.cx.limits.check_count(self.order.len() + 1, self.cx.limits.nodes, LimitKind::Nodes)
    }

    fn draft(&mut self, id: &str) -> Result<&mut Draft, DiagramError> {
        if !self.drafts.contains_key(id) {
            self.check_nodes()?;
            self.cx.limits.check_label(id)?;
            self.order.push(id.to_string());
        }
        Ok(self.drafts.entry(id.to_string()).or_default())
    }

    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let (word, rest) = first_word(stmt);
        match word {
            "end" if rest.is_empty() => {
                if self.open.take().is_none() {
                    self.cx.warn(self.line, "`end` without a system boundary");
                }
                return Ok(());
            }
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
                let (ids, classes) = first_word(rest);
                self.styles.assign(ids, classes);
                return Ok(());
            }
            "style" => {
                self.styles.style(rest);
                return Ok(());
            }
            "actor" => return self.declare(rest, true),
            "systemBoundary" => return self.boundary(rest),
            "note" => return self.note(rest),
            "json" => return self.json(rest),
            _ => {}
        }
        if let Some(rel) = relation(stmt) {
            return self.relation(rel);
        }
        self.declare(stmt, false)
    }

    fn declare(&mut self, text: &str, actor: bool) -> Result<(), DiagramError> {
        let Some(d) = declaration(text) else {
            self.cx.warn(self.line, format!("cannot read `{}`", truncate(text)));
            return Ok(());
        };
        // `id@{ ... }` on its own may configure a boundary or an edge.
        if !actor && d.label.is_none() && !d.meta.is_empty() && !self.drafts.contains_key(&d.id) {
            if let Some(&b) = self.boundary_index.get(&d.id) {
                self.boundary_meta(b, &d.meta);
                return Ok(());
            }
            if self.edges.iter().any(|e| e.edge.id.as_deref() == Some(d.id.as_str())) {
                return Ok(());
            }
        }
        let label = match &d.label {
            Some(raw) => Some(self.cx.label(raw)?),
            None => None,
        };
        let boundary = self.open;
        let line = self.line;
        let mut look = ActorLook::Normal;
        let mut business = false;
        for (k, v) in &d.meta {
            match (k.as_str(), v.as_str()) {
                ("type", "hollow") if actor => look = ActorLook::Hollow,
                ("type", "awesome") if actor => look = ActorLook::Awesome,
                ("type", _) => {}
                ("icon", _) if actor => look = ActorLook::Icon,
                ("business", "true") => business = true,
                ("business", _) => {}
                _ => self.cx.warn(line, format!("unknown key `{k}`")),
            }
        }
        let draft = self.draft(&d.id)?;
        draft.kind = Some(if actor {
            NodeKind::Actor(look)
        } else if d.rect {
            NodeKind::Rect
        } else {
            NodeKind::Ellipse
        });
        if label.is_some() {
            draft.label = label;
        }
        if d.stereotype.is_some() {
            draft.stereotype = d.stereotype;
        }
        draft.business |= business;
        if boundary.is_some() {
            draft.boundary = boundary;
        }
        draft.classes.extend(d.classes);
        Ok(())
    }

    fn boundary_meta(&mut self, b: usize, meta: &[(String, String)]) {
        for (k, v) in meta {
            if k == "type" {
                self.boundaries[b].package = v == "package";
            }
        }
    }

    fn boundary(&mut self, rest: &str) -> Result<(), DiagramError> {
        if self.open.is_some() {
            self.cx.warn(self.line, "system boundaries do not nest");
        }
        let Some(d) = declaration(rest) else {
            self.cx.warn(self.line, format!("cannot read boundary `{}`", truncate(rest)));
            return Ok(());
        };
        self.cx.limits.check_count(self.boundaries.len() + 1, self.cx.limits.clusters, LimitKind::Clusters)?;
        let title = match &d.label {
            Some(raw) => self.cx.label(raw)?,
            None => Label::plain(&d.id),
        };
        let b = match self.boundary_index.get(&d.id) {
            Some(&b) => b,
            None => {
                let b = self.boundaries.len();
                self.boundary_index.insert(d.id.clone(), b);
                self.boundaries.push(Boundary {
                    id: d.id.clone(),
                    title,
                    package: false,
                    classes: Vec::new(),
                    style: Default::default(),
                });
                b
            }
        };
        self.boundaries[b].classes.extend(d.classes);
        self.boundary_meta(b, &d.meta);
        self.open = Some(b);
        Ok(())
    }

    fn note(&mut self, rest: &str) -> Result<(), DiagramError> {
        let Some(tail) = rest.strip_prefix("for") else {
            self.cx.warn(self.line, "a note needs `for <id>`");
            return Ok(());
        };
        let (target, text) = first_word(tail);
        let text = text.trim();
        if ident_len(target, "") != target.len() || target.is_empty() || !text.starts_with('"') {
            self.cx.warn(self.line, "a note needs `for <id> \"text\"`");
            return Ok(());
        }
        let label = self.cx.label(text)?;
        let id = format!("__note{}", self.notes);
        self.notes += 1;
        let draft = self.draft(&id)?;
        draft.kind = Some(NodeKind::Note);
        draft.label = Some(label);
        self.push_edge(id, target.to_string(), Edge {
            from: 0,
            to: 0,
            kind: EdgeKind::Note,
            start: None,
            end: None,
            label: None,
            min_len: 1,
            id: None,
            style: Default::default(),
        })
    }

    fn json(&mut self, rest: &str) -> Result<(), DiagramError> {
        let rest = rest.trim();
        let len = ident_len(rest, "");
        let tail = &rest[len..];
        let Some(body) = tail.strip_prefix('@').filter(|b| b.starts_with('{') && len > 0) else {
            self.cx.warn(self.line, "a JSON table needs `id@{ ... }`");
            return Ok(());
        };
        let Some(end) = balanced(body, '{', '}') else {
            self.cx.warn(self.line, "an unclosed JSON object");
            return Ok(());
        };
        self.cx.limits.check_label(&body[..=end])?;
        let Some(rows) = json::rows(&body[..=end], self.cx.limits.elements.min(500)) else {
            self.cx.warn(self.line, "not a JSON object");
            return Ok(());
        };
        let classes: Vec<String> = body[end + 1..]
            .trim()
            .strip_prefix(":::")
            .map(|c| c.split(',').map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect())
            .unwrap_or_default();
        let id = rest[..len].to_string();
        let draft = self.draft(&id)?;
        draft.kind = Some(NodeKind::Json(rows));
        draft.classes.extend(classes);
        Ok(())
    }

    fn push_edge(&mut self, from: String, to: String, edge: Edge) -> Result<(), DiagramError> {
        self.cx.limits.check_count(self.edges.len() + 1, self.cx.limits.edges, LimitKind::Edges)?;
        self.edges.push(PendingEdge {
            from,
            to,
            edge,
            line: self.line,
        });
        Ok(())
    }

    fn relation(&mut self, rel: Rel) -> Result<(), DiagramError> {
        let label = match rel.label.as_deref() {
            Some(raw) => Some(self.cx.label(raw)?).filter(|l| !l.is_empty()),
            None => None,
        };
        let label = match rel.kind {
            EdgeKind::Include => Some(Label::plain("«include»")),
            EdgeKind::Extend => Some(Label::plain("«extend»")),
            _ => label,
        };
        self.push_edge(rel.from, rel.to, Edge {
            from: 0,
            to: 0,
            kind: rel.kind,
            start: rel.start,
            end: rel.end,
            label,
            min_len: rel.min_len,
            id: rel.id,
            style: Default::default(),
        })
    }

    fn finish(mut self) -> Result<Model, DiagramError> {
        // Endpoints never declared become ellipse use cases.
        let pending = std::mem::take(&mut self.edges);
        let mut kept = Vec::with_capacity(pending.len());
        for e in pending {
            let mut ok = true;
            for id in [&e.from, &e.to] {
                if !self.drafts.contains_key(id.as_str()) && self.boundary_index.contains_key(id.as_str()) {
                    self.cx.warn(e.line, format!("`{id}` is a system boundary"));
                    ok = false;
                }
            }
            if ok {
                self.draft(&e.from)?;
                self.draft(&e.to)?;
                kept.push(e);
            }
        }
        let index: HashMap<&str, usize> = self.order.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
        let mut nodes = Vec::with_capacity(self.order.len());
        for id in &self.order {
            let d = self.drafts.remove(id).unwrap_or_default();
            nodes.push(Node {
                id: id.clone(),
                label: d.label.unwrap_or_else(|| Label::plain(id)),
                kind: d.kind.unwrap_or(NodeKind::Ellipse),
                stereotype: d.stereotype,
                business: d.business,
                boundary: d.boundary,
                style: self.styles.resolve(id, &d.classes),
                classes: d.classes,
            });
        }
        let mut edges = Vec::with_capacity(kept.len());
        for e in kept {
            let (Some(&from), Some(&to)) = (index.get(e.from.as_str()), index.get(e.to.as_str())) else {
                continue;
            };
            let mut edge = e.edge;
            edge.from = from;
            edge.to = to;
            if let Some(id) = &edge.id {
                edge.style = self.styles.resolve(id, &[]);
            }
            edges.push(edge);
        }
        for b in &mut self.boundaries {
            b.style = self.styles.resolve(&b.id, &b.classes);
        }
        Ok(Model {
            direction: self.direction,
            nodes,
            boundaries: self.boundaries,
            edges,
        })
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

    fn node<'m>(m: &'m Model, id: &str) -> &'m Node {
        m.nodes.iter().find(|n| n.id == id).unwrap()
    }

    #[test]
    fn declarations_boundaries_and_variants() {
        let m = parse_src(
            "usecase-beta\ndirection LR\nactor Customer\nactor Staff(\"Order staff\")@{ type: hollow, business: true } <<Employee>>\nsystemBoundary ordering[\"Ordering System\"]@{ type: package }:::system\n  Browse(\"Browse products\")\n  Review[Review order]\nend\n\"Reset password\"\nCheckout(\"`Two\nlines`\") <<Core>>:::critical",
        );
        assert_eq!(m.direction, Direction::LR);
        assert_eq!(node(&m, "Customer").kind, NodeKind::Actor(ActorLook::Normal));
        let staff = node(&m, "Staff");
        assert_eq!(staff.kind, NodeKind::Actor(ActorLook::Hollow));
        assert!(staff.business);
        assert_eq!(staff.stereotype.as_deref(), Some("Employee"));
        assert_eq!(node(&m, "Browse").boundary, Some(0));
        assert_eq!(node(&m, "Review").kind, NodeKind::Rect);
        assert!(m.boundaries[0].package);
        assert_eq!(m.boundaries[0].title.text(), "Ordering System");
        assert_eq!(node(&m, "Reset_password").label.text(), "Reset password");
        let checkout = node(&m, "Checkout");
        assert_eq!(checkout.label.lines.len(), 2);
        assert_eq!(checkout.classes, vec!["critical".to_string()]);
    }

    #[test]
    fn every_relation_form() {
        let m = parse_src(
            "usecase-beta\nactor User\nUser --> Start\nStart <-- User\nStart -- Finish\nUser --o Start\nStart o-- User\nUser --x Finish\nFinish x-- User\nUser -- \"include details\" --> Login\nCheckout ..> : include Payment\nCoupon ..> : extend Checkout\nCoupon --|> Checkout\nUser opens@-- \"starts\" ---> Checkout\nUser ----> Far\nnote for Login \"Requires a session\"\nstyle opens stroke:#06c",
        );
        let e = &m.edges;
        assert_eq!(e.len(), 14);
        assert_eq!(e[0].end, Some(Marker::OpenArrow));
        assert_eq!(e[1].start, Some(Marker::OpenArrow));
        assert_eq!((e[2].start, e[2].end), (None, None));
        assert_eq!(e[3].end, Some(Marker::Circle));
        assert_eq!(e[4].start, Some(Marker::Circle));
        assert_eq!(e[5].end, Some(Marker::Cross));
        assert_eq!(e[6].start, Some(Marker::Cross));
        assert_eq!(e[7].label.as_ref().map(|l| l.text()), Some("include details".into()));
        assert_eq!(e[8].kind, EdgeKind::Include);
        assert_eq!(e[9].kind, EdgeKind::Extend);
        assert_eq!(e[10].kind, EdgeKind::Generalization);
        assert_eq!((e[11].id.as_deref(), e[11].min_len), (Some("opens"), 2));
        assert!(e[11].style.stroke.is_some());
        assert_eq!(e[12].min_len, 3);
        assert_eq!(e[13].kind, EdgeKind::Note);
        assert_eq!(node(&m, "Start").kind, NodeKind::Ellipse);
    }

    #[test]
    fn json_tables() {
        let m = parse_src("usecase-beta\njson Payload@{\n  \"a\": 1,\n  \"b\": { \"c\": true }\n}:::data\nInspect --> Payload");
        let p = node(&m, "Payload");
        assert_eq!(p.kind, NodeKind::Json(vec![("a".into(), "1".into()), ("b.c".into(), "true".into())]));
        assert_eq!(p.classes, vec!["data".to_string()]);
    }
}
