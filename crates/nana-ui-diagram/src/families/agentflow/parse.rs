//! The agent flow grammar: flowchart-style nodes with `@{ shape: ... }`
//! and the domain aliases (task, tool, input, decision, refdoc, action),
//! `connector` nodes, `flow ... end` containers (nested), `global ... end`,
//! `@{ view: collapsed }`, the sequence / reference / failure edges with
//! labels and chains, metadata statements, `classDef` / `class` / `:::` and
//! `style`. Conventional metadata keys (`instruction`, `params`, ...) are
//! accepted and not drawn.

use std::collections::HashMap;

use super::{Edge, EdgeKind, EndRef, Flow, Model, Node};
use crate::families::ParseCx;
use crate::families::graphkit::{Styles, attributes, class_list, statements};
use crate::families::kit::{self, balanced, first_word, quoted_end, truncate, unquote};
use crate::geom::Shape;
use crate::layout::Direction;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scope {
    Flow(usize),
    Global,
}

#[derive(Default)]
struct Draft {
    explicit: bool,
    label: Option<Label>,
    shape: Option<Shape>,
    flow: Option<usize>,
    pinned: bool,
    classes: Vec<String>,
}

/// A node reference in a statement.
#[derive(Debug, Default)]
struct NodeRef {
    id: String,
    label: Option<String>,
    shape: Option<Shape>,
    attrs: Vec<(String, String)>,
    classes: Vec<String>,
}

#[derive(Debug)]
struct Link {
    kind: EdgeKind,
    arrow: bool,
    label: Option<String>,
    len: u32,
}

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    order: Vec<String>,
    drafts: HashMap<String, Draft>,
    flows: Vec<Flow>,
    flow_index: HashMap<String, usize>,
    flow_classes: Vec<Vec<String>>,
    stack: Vec<Scope>,
    edges: Vec<(String, String, Edge)>,
    styles: Styles,
    direction: Direction,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut p = Parser {
        cx,
        order: Vec::new(),
        drafts: HashMap::new(),
        flows: Vec::new(),
        flow_index: HashMap::new(),
        flow_classes: Vec::new(),
        stack: Vec::new(),
        edges: Vec::new(),
        styles: Styles::default(),
        direction: Direction::TB,
        line: 0,
    };
    let (header, mut rest) = kit::header(body)?;
    p.line = header;
    // `agentflow-beta LR`: the header may carry the direction.
    let (first, tail) = first_word(rest);
    if let Some(d) = Direction::parse(first) {
        p.direction = d;
        rest = tail;
    }
    if !rest.is_empty() {
        p.statement(rest)?;
    }
    for (line, stmt) in statements(body, header) {
        p.line = line;
        p.statement(&stmt)?;
    }
    if !p.stack.is_empty() {
        p.cx.warn(
            p.line,
            "a `flow` or `global` block is not closed with `end`",
        );
    }
    p.finish()
}

/// A shape name or one of the agent flow aliases.
fn shape_named(name: &str) -> Option<Shape> {
    Some(match name.trim().to_ascii_lowercase().as_str() {
        "task" | "roundedrect" | "rounded-rect" => Shape::Rounded,
        "tool" => Shape::Subroutine,
        "input" => Shape::LeanRight,
        "decision" => Shape::Diamond,
        "refdoc" => Shape::LinedDocument,
        "action" => Shape::Hexagon,
        other => return Shape::from_name(other),
    })
}

const OPENERS: &[(&str, &str, Shape)] = &[
    ("(((", ")))", Shape::DoubleCircle),
    ("((", "))", Shape::Circle),
    ("([", "])", Shape::Stadium),
    ("[[", "]]", Shape::Subroutine),
    ("[(", ")]", Shape::Cylinder),
    ("{{", "}}", Shape::Hexagon),
    ("[", "]", Shape::Rect),
    ("(", ")", Shape::Rounded),
    ("{", "}", Shape::Diamond),
];

struct Cur<'s> {
    s: &'s str,
    i: usize,
}

impl<'s> Cur<'s> {
    fn rest(&self) -> &'s str {
        &self.s[self.i..]
    }

    fn ws(&mut self) {
        let r = self.rest();
        self.i += r.len() - r.trim_start().len();
    }

    fn at_end(&self) -> bool {
        self.rest().trim().is_empty()
    }

    fn eat(&mut self, pat: &str) -> bool {
        if self.rest().starts_with(pat) {
            self.i += pat.len();
            true
        } else {
            false
        }
    }

    fn link_ahead(r: &str) -> bool {
        r.starts_with("--") || r.starts_with("-.") || r.starts_with("==")
    }

    fn node(&mut self) -> Option<NodeRef> {
        self.ws();
        let start = self.i;
        while let Some(c) = self.rest().chars().next() {
            if c.is_whitespace() || "[](){}|&;\"<>@".contains(c) {
                break;
            }
            let r = self.rest();
            if r.starts_with(":::") || Self::link_ahead(r) {
                break;
            }
            self.i += c.len_utf8();
        }
        if self.i == start {
            return None;
        }
        let mut node = NodeRef {
            id: self.s[start..self.i].to_string(),
            ..NodeRef::default()
        };
        for &(open, close, shape) in OPENERS {
            if !self.rest().starts_with(open) {
                continue;
            }
            let inner = &self.rest()[open.len()..];
            let t = inner.trim_start();
            let found = if t.starts_with('"') {
                quoted_end(t).and_then(|e| {
                    let after = t[e..].trim_start();
                    after.starts_with(close).then(|| {
                        let used = inner.len() - after.len() + close.len();
                        (t[..e].to_string(), used)
                    })
                })
            } else {
                inner
                    .find(close)
                    .map(|e| (inner[..e].trim().to_string(), e + close.len()))
            };
            if let Some((label, used)) = found {
                node.label = Some(label);
                node.shape = Some(shape);
                self.i += open.len() + used;
                break;
            }
        }
        if self.rest().starts_with("@{") {
            let body = &self.rest()[1..];
            if let Some(end) = balanced(body, '{', '}') {
                node.attrs = attributes(&body[1..end]);
                self.i += 1 + end + 1;
            }
        }
        if self.eat(":::") {
            let start = self.i;
            while let Some(c) = self.rest().chars().next() {
                if c.is_whitespace() || c == '&' || Self::link_ahead(self.rest()) {
                    break;
                }
                self.i += c.len_utf8();
            }
            node.classes = class_list(&self.s[start..self.i]);
        }
        Some(node)
    }

    fn group(&mut self) -> Option<Vec<NodeRef>> {
        let mut out = vec![self.node()?];
        loop {
            let save = self.i;
            self.ws();
            if self.eat("&") {
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

    fn link(&mut self) -> Option<Link> {
        self.ws();
        // `id@-->`: an edge id, accepted and not used.
        let r = self.rest();
        if let Some(at) = r.find('@')
            && r[..at].chars().all(|c| c.is_alphanumeric() || c == '_')
            && at > 0
            && Self::link_ahead(&r[at + 1..])
        {
            self.i += at + 1;
        }
        let r = self.rest();
        let mut link = if let Some(after) = r.strip_prefix("-.") {
            let dots = after.bytes().take_while(|&b| b == b'.').count();
            let after = &after[dots..];
            if let Some(tail) = after.strip_prefix('-') {
                let arrow = tail.starts_with('>');
                self.i += 2 + dots + 1 + usize::from(arrow);
                Link {
                    kind: EdgeKind::Reference,
                    arrow,
                    label: None,
                    len: dots as u32 + 1,
                }
            } else {
                // `-. text .-` / `-. text .->`
                let close = after.find(".-")?;
                let text = after[..close].trim().to_string();
                let tail = &after[close..];
                let dots2 = tail.bytes().take_while(|&b| b == b'.').count();
                let tail = tail[dots2..].strip_prefix('-')?;
                let arrow = tail.starts_with('>');
                self.i += 2 + dots + close + dots2 + 1 + usize::from(arrow);
                Link {
                    kind: EdgeKind::Reference,
                    arrow,
                    label: Some(text),
                    len: dots2 as u32,
                }
            }
        } else if r.starts_with("--") || r.starts_with("==") {
            let ch = r.as_bytes()[0];
            let n = r.bytes().take_while(|&b| b == ch).count();
            let after = &r[n..];
            if after.starts_with('>') {
                self.i += n + 1;
                Link {
                    kind: EdgeKind::Sequence,
                    arrow: true,
                    label: None,
                    len: n as u32 - 1,
                }
            } else if after.starts_with('x')
                && after[1..].chars().next().is_none_or(|c| c.is_whitespace())
            {
                self.i += n + 1;
                Link {
                    kind: EdgeKind::Failure,
                    arrow: false,
                    label: None,
                    len: n as u32 - 1,
                }
            } else if n == 2 && after.starts_with(char::is_whitespace) && after.contains(&r[..2]) {
                // `-- text -->`
                let close = after.find(&r[..2])?;
                let text = after[..close].trim().to_string();
                let tail = &after[close..];
                let m = tail.bytes().take_while(|&b| b == ch).count();
                let tail = &tail[m..];
                let (kind, arrow, used) = if tail.starts_with('>') {
                    (EdgeKind::Sequence, true, 1)
                } else if tail.starts_with('x') {
                    (EdgeKind::Failure, false, 1)
                } else {
                    (EdgeKind::Link, false, 0)
                };
                self.i += n + close + m + used;
                Link {
                    kind,
                    arrow,
                    label: Some(text),
                    len: (m as u32).saturating_sub(1),
                }
            } else if n >= 3 {
                self.i += n;
                Link {
                    kind: EdgeKind::Link,
                    arrow: false,
                    label: None,
                    len: n as u32 - 2,
                }
            } else {
                return None;
            }
        } else {
            return None;
        };
        // `|label|`
        let save = self.i;
        self.ws();
        if self.eat("|") {
            match self.rest().find('|') {
                Some(end) => {
                    link.label = Some(self.rest()[..end].trim().to_string());
                    self.i += end + 1;
                }
                None => self.i = save,
            }
        } else {
            self.i = save;
        }
        link.len = link.len.clamp(1, 12);
        Some(link)
    }
}

impl Parser<'_, '_> {
    fn scope_flow(&self) -> Option<usize> {
        self.stack.iter().rev().find_map(|s| match s {
            Scope::Flow(f) => Some(*f),
            Scope::Global => None,
        })
    }

    fn in_global(&self) -> bool {
        self.stack.last() == Some(&Scope::Global)
    }

    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let (word, rest) = first_word(stmt);
        match word {
            "end" if rest.is_empty() => {
                if self.stack.pop().is_none() {
                    self.cx.warn(self.line, "`end` without a block");
                }
                return Ok(());
            }
            "global" if rest.is_empty() => {
                self.stack.push(Scope::Global);
                return Ok(());
            }
            "flow" => return self.flow(rest),
            "connector" => return self.connector(rest),
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
        self.chain(stmt)
    }

    fn flow(&mut self, rest: &str) -> Result<(), DiagramError> {
        let mut cur = Cur { s: rest, i: 0 };
        let Some(node) = cur.node() else {
            self.cx.warn(self.line, "a flow needs an id");
            return Ok(());
        };
        if self.stack.len() + 1 > self.cx.limits.cluster_depth {
            return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
        }
        let f = match self.flow_index.get(&node.id) {
            Some(&f) => f,
            None => {
                self.cx.limits.check_count(
                    self.flows.len() + 1,
                    self.cx.limits.clusters,
                    LimitKind::Clusters,
                )?;
                let title = match &node.label {
                    Some(raw) => self.cx.label(raw)?,
                    None => Label::plain(&node.id),
                };
                let f = self.flows.len();
                self.flow_index.insert(node.id.clone(), f);
                self.flows.push(Flow {
                    id: node.id.clone(),
                    title,
                    parent: self.scope_flow(),
                    collapsed: false,
                    style: Default::default(),
                });
                self.flow_classes.push(Vec::new());
                f
            }
        };
        self.flow_meta(f, &node.attrs);
        self.flow_classes[f].extend(node.classes);
        if !cur.at_end() {
            self.cx.warn(self.line, "ignored text after a flow header");
        }
        self.stack.push(Scope::Flow(f));
        Ok(())
    }

    fn flow_meta(&mut self, f: usize, attrs: &[(String, String)]) {
        for (k, v) in attrs {
            if k == "view" {
                self.flows[f].collapsed = unquote(v) == "collapsed";
            }
        }
    }

    fn connector(&mut self, rest: &str) -> Result<(), DiagramError> {
        let mut cur = Cur { s: rest, i: 0 };
        let Some(mut node) = cur.node() else {
            self.cx.warn(self.line, "a connector needs an id");
            return Ok(());
        };
        if node.shape.is_none() || node.shape == Some(Shape::Rect) {
            node.shape = Some(Shape::Stadium);
        }
        self.mention(&node)
    }

    fn mention(&mut self, node: &NodeRef) -> Result<(), DiagramError> {
        let flow = self.scope_flow();
        let global = self.in_global();
        if !self.drafts.contains_key(&node.id) {
            self.cx.limits.check_count(
                self.order.len() + 1,
                self.cx.limits.nodes,
                LimitKind::Nodes,
            )?;
            self.cx.limits.check_label(&node.id)?;
            self.order.push(node.id.clone());
        }
        let label = node
            .label
            .as_deref()
            .map(|raw| self.cx.label(raw))
            .transpose()?;
        let mut attr_label = None;
        let mut attr_shape = None;
        for (k, v) in &node.attrs {
            match k.as_str() {
                "shape" => match shape_named(v) {
                    Some(s) => attr_shape = Some(s),
                    None => self.cx.warn(self.line, format!("unknown shape `{v}`")),
                },
                "label" => attr_label = Some(self.cx.label(v)?),
                _ => {}
            }
        }
        let draft = self.drafts.entry(node.id.clone()).or_default();
        if global {
            draft.pinned = true;
            draft.flow = None;
        } else if draft.flow.is_none() && !draft.pinned {
            draft.flow = flow;
        }
        if let Some(l) = attr_label.or(label) {
            draft.label = Some(l);
            draft.explicit = true;
        }
        if let Some(s) = attr_shape.or(node.shape) {
            draft.shape = Some(s);
            draft.explicit = true;
        }
        draft.classes.extend(node.classes.iter().cloned());
        Ok(())
    }

    fn chain(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let mut cur = Cur { s: stmt, i: 0 };
        let Some(first) = cur.group() else {
            self.cx
                .warn(self.line, format!("cannot read `{}`", truncate(stmt)));
            return Ok(());
        };
        // `id@{ ... }` alone configures a flow, or (ignored) an edge.
        if first.len() == 1 && cur.at_end() && first[0].label.is_none() && first[0].shape.is_none()
        {
            let n = &first[0];
            if let Some(&f) = self.flow_index.get(&n.id) {
                self.flow_meta(f, &n.attrs);
                self.flow_classes[f].extend(n.classes.iter().cloned());
                return Ok(());
            }
            let has_node_keys = n.attrs.iter().any(|(k, _)| k == "shape" || k == "label");
            if !n.attrs.is_empty() && !has_node_keys && !self.drafts.contains_key(&n.id) {
                // Metadata for an edge or a node that does not draw.
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
                self.cx.warn(
                    self.line,
                    format!("cannot read link at `{}`", truncate(cur.rest())),
                );
                break;
            };
            let Some(right) = cur.group() else {
                self.cx.warn(self.line, "a link without a target");
                break;
            };
            for n in &right {
                self.mention(n)?;
            }
            let label = link
                .label
                .as_deref()
                .filter(|l| !l.trim().is_empty())
                .map(|raw| self.cx.label(raw))
                .transpose()?;
            for a in &left {
                for b in &right {
                    self.cx.limits.check_count(
                        self.edges.len() + 1,
                        self.cx.limits.edges,
                        LimitKind::Edges,
                    )?;
                    self.edges.push((
                        a.id.clone(),
                        b.id.clone(),
                        Edge {
                            from: EndRef::Node(0),
                            to: EndRef::Node(0),
                            kind: link.kind,
                            arrow: link.arrow,
                            label: label.clone(),
                            min_len: link.len,
                        },
                    ));
                }
            }
            left = right;
        }
        Ok(())
    }

    fn finish(mut self) -> Result<Model, DiagramError> {
        let mut index = HashMap::new();
        let mut nodes = Vec::new();
        for id in &self.order {
            let d = self.drafts.remove(id).unwrap_or_default();
            // A flow's id names the flow unless a node declared itself.
            if self.flow_index.contains_key(id) && !d.explicit {
                continue;
            }
            index.insert(id.clone(), nodes.len());
            nodes.push(Node {
                id: id.clone(),
                label: d.label.unwrap_or_else(|| Label::plain(id)),
                shape: d.shape.unwrap_or(Shape::Rect),
                flow: d.flow,
                style: self.styles.resolve(id, &d.classes),
            });
        }
        let resolve = |id: &str| -> Option<EndRef> {
            index
                .get(id)
                .map(|&n| EndRef::Node(n))
                .or_else(|| self.flow_index.get(id).map(|&f| EndRef::Flow(f)))
        };
        let mut edges = Vec::with_capacity(self.edges.len());
        for (from, to, mut edge) in std::mem::take(&mut self.edges) {
            let (Some(a), Some(b)) = (resolve(&from), resolve(&to)) else {
                continue;
            };
            edge.from = a;
            edge.to = b;
            edges.push(edge);
        }
        for (f, flow) in self.flows.iter_mut().enumerate() {
            flow.style = self.styles.resolve(&flow.id, &self.flow_classes[f]);
        }
        Ok(Model {
            direction: self.direction,
            nodes,
            flows: self.flows,
            edges,
        })
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

    fn node<'m>(m: &'m Model, id: &str) -> &'m Node {
        m.nodes.iter().find(|n| n.id == id).unwrap()
    }

    #[test]
    fn shapes_flows_and_edges() {
        let m = parse_src(
            "agentflow-beta LR\n  brief[\"Release brief\"]@{ shape: input }\n  flow writer[\"Drafting Agent\"]\n    draft[\"Draft\"]@{ shape: task }\n    lookup[\"search\"]@{ shape: tool }\n    guide[\"Tone\"]@{ shape: refdoc }\n    draft --> lookup\n    draft -.- guide\n  end\n  ok{Ok?}\n  publish[\"Publish\"]@{ shape: action }\n  brief --> writer\n  ok -- yes --> publish\n  ok --x draft\n  a --> b --> c",
        );
        assert_eq!(m.direction, Direction::LR);
        assert_eq!(node(&m, "brief").shape, Shape::LeanRight);
        assert_eq!(node(&m, "draft").shape, Shape::Rounded);
        assert_eq!(node(&m, "lookup").shape, Shape::Subroutine);
        assert_eq!(node(&m, "guide").shape, Shape::LinedDocument);
        assert_eq!(node(&m, "publish").shape, Shape::Hexagon);
        assert_eq!(node(&m, "ok").shape, Shape::Diamond);
        assert_eq!(node(&m, "draft").flow, Some(0));
        assert!(!m.nodes.iter().any(|n| n.id == "writer"));
        let e = &m.edges;
        assert_eq!(e.len(), 7);
        assert_eq!((e[0].kind, e[0].arrow), (EdgeKind::Sequence, true));
        assert_eq!(e[1].kind, EdgeKind::Reference);
        assert_eq!(e[2].to, EndRef::Flow(0));
        assert_eq!(e[3].label.as_ref().map(|l| l.text()), Some("yes".into()));
        assert_eq!(e[4].kind, EdgeKind::Failure);
    }

    #[test]
    fn global_collapsed_connectors_and_metadata() {
        let m = parse_src(
            "agentflow-beta TB\n  connector llm[\"LLM API\"]\n  llm@{ protocol: \"http\", endpoint: \"https://x\" }\n  global\n    corpus[\"Shared\"]@{ shape: refdoc }\n  end\n  flow outer[\"Outer\"]\n    flow inner[\"Inner\"]\n      t[\"t\"]@{\n        shape: tool\n        params: \"city :: String\"\n      }\n      t -.- corpus\n    end\n    inner@{ instruction: \"Do it.\" }\n  end\n  outer@{ view: \"collapsed\" }",
        );
        assert_eq!(node(&m, "llm").shape, Shape::Stadium);
        assert_eq!(node(&m, "corpus").flow, None);
        assert_eq!(node(&m, "t").shape, Shape::Subroutine);
        assert_eq!(node(&m, "t").flow, Some(1));
        assert_eq!(m.flows[1].parent, Some(0));
        assert!(m.flows[0].collapsed);
    }
}
