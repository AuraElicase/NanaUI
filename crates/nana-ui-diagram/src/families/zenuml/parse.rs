//! The ZenUML grammar: participants and annotators, `group`, sync calls
//! with `{}` nesting, async `A->B: text`, `new`, `return` / `@return`,
//! `if` / `else`, the loops, `opt`, `par`, `critical`, `try` / `catch` /
//! `finally` and `//` comments, lowered onto the sequence event list.

use crate::families::ParseCx;
use crate::families::sequence::{
    Builder, Event, FragmentKind, Group, LineKind, Message, Model, ParticipantKind,
};
use crate::geom::Marker;
use crate::kind;
use crate::text::Label;
use crate::{DiagramError, LimitKind};

/// Brace nesting deeper than this is hostile, not a diagram.
const MAX_BRACES: usize = 64;
const STARTER_ID: &str = "\u{0}starter";

enum Tok {
    Stmt(String, usize),
    Comment(String),
    Open(usize),
    Close(usize),
}

enum Node {
    Stmt {
        text: String,
        line: usize,
        block: Option<Vec<Node>>,
    },
    Comment(String),
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut lines = body.lines().enumerate().skip_while(|(_, l)| l.trim().is_empty());
    let Some((first_index, first)) = lines.next() else {
        return Err(DiagramError::Empty);
    };
    let Some((_, after)) = kind::header(first.trim()) else {
        return Err(DiagramError::syntax(first_index as u32 + 1, 1, "expected `zenuml`"));
    };
    let mut toks = Vec::new();
    tokenize(after, first_index, &mut toks);
    let mut last = first_index;
    for (index, line) in lines {
        if !line.trim().is_empty() {
            last = index;
        }
        tokenize(line, index, &mut toks);
    }
    let mut at = 0;
    let mut depth = 0;
    let nodes = tree(&toks, &mut at, &mut depth, 0, cx)?;
    let mut lower = Lower {
        cx,
        b: Builder::new(false),
        starter: None,
        implicit_starter: None,
        comment: Vec::new(),
        reply_next: false,
        stack: Vec::new(),
        group: None,
        fragments: 0,
        line: first_index,
    };
    lower.prescan(&nodes);
    lower.block(&nodes)?;
    let Lower {
        cx,
        b,
        implicit_starter,
        ..
    } = lower;
    let mut model = b.finish(cx, last);
    if let Some(s) = implicit_starter {
        move_to_front(&mut model, s);
    }
    Ok(model)
}

/// One line into statements, braces and comments.
fn tokenize(line: &str, index: usize, out: &mut Vec<Tok>) {
    let mut cur = String::new();
    let mut depth = 0usize;
    let mut quote = false;
    let mut label = false;
    let flush = |cur: &mut String, out: &mut Vec<Tok>| {
        let text = cur.trim();
        if !text.is_empty() {
            out.push(Tok::Stmt(text.to_string(), index));
        }
        cur.clear();
    };
    for (i, c) in line.char_indices() {
        if label || quote {
            cur.push(c);
            if quote && c == '"' {
                quote = false;
            }
            continue;
        }
        match c {
            '"' => {
                quote = true;
                cur.push(c);
            }
            '/' if depth == 0 && line[i + 1..].starts_with('/') => {
                if cur.trim().is_empty() {
                    out.push(Tok::Comment(line[i + 2..].trim().to_string()));
                }
                cur.clear();
                return;
            }
            '(' => {
                depth += 1;
                cur.push(c);
            }
            ')' => {
                depth = depth.saturating_sub(1);
                cur.push(c);
            }
            '{' if depth == 0 => {
                flush(&mut cur, out);
                out.push(Tok::Open(index));
            }
            '}' if depth == 0 => {
                flush(&mut cur, out);
                out.push(Tok::Close(index));
            }
            ';' if depth == 0 => flush(&mut cur, out),
            ':' if depth == 0 && cur.contains("->") => {
                label = true;
                cur.push(c);
            }
            _ => cur.push(c),
        }
    }
    flush(&mut cur, out);
}

/// Statements followed by `{` own the block up to its `}`.
fn tree(
    toks: &[Tok],
    at: &mut usize,
    depth: &mut usize,
    level: usize,
    cx: &mut ParseCx<'_>,
) -> Result<Vec<Node>, DiagramError> {
    let mut out = Vec::new();
    while *at < toks.len() {
        match &toks[*at] {
            Tok::Comment(text) => {
                out.push(Node::Comment(text.clone()));
                *at += 1;
            }
            Tok::Close(line) => {
                *at += 1;
                if level > 0 {
                    return Ok(out);
                }
                cx.warn(*line, "`}` without an open block");
            }
            Tok::Open(line) => {
                *at += 1;
                let block = nested(toks, at, depth, level, cx)?;
                out.push(Node::Stmt {
                    text: String::new(),
                    line: *line,
                    block: Some(block),
                });
            }
            Tok::Stmt(text, line) => {
                *at += 1;
                let block = if matches!(toks.get(*at), Some(Tok::Open(_))) {
                    *at += 1;
                    Some(nested(toks, at, depth, level, cx)?)
                } else {
                    None
                };
                out.push(Node::Stmt {
                    text: text.clone(),
                    line: *line,
                    block,
                });
            }
        }
    }
    Ok(out)
}

fn nested(
    toks: &[Tok],
    at: &mut usize,
    depth: &mut usize,
    level: usize,
    cx: &mut ParseCx<'_>,
) -> Result<Vec<Node>, DiagramError> {
    if level + 1 > MAX_BRACES {
        return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
    }
    *depth = (*depth).max(level + 1);
    tree(toks, at, depth, level + 1, cx)
}

/// A call frame: who is speaking and whom it answers.
struct Frame {
    who: usize,
    caller: usize,
    active: bool,
    /// Fragment depth of the call's own block: a `return` inside an `if`
    /// is one way out, not the end of the activation.
    fragments: usize,
}

struct Lower<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    b: Builder,
    starter: Option<usize>,
    implicit_starter: Option<usize>,
    comment: Vec<String>,
    reply_next: bool,
    stack: Vec<Frame>,
    group: Option<usize>,
    fragments: usize,
    line: usize,
}

/// Split at the first `pat` outside quotes and parentheses.
fn split_top<'s>(s: &'s str, pat: &str) -> Option<(&'s str, &'s str)> {
    let mut depth = 0i32;
    let mut quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quote = !quote,
            '(' if !quote => depth += 1,
            ')' if !quote => depth -= 1,
            _ => {}
        }
        if !quote && depth <= 0 && s[i..].starts_with(pat) {
            return Some((&s[..i], &s[i + pat.len()..]));
        }
    }
    None
}

/// The leading keyword of a statement and what follows it.
fn keyword(text: &str) -> (&str, &str) {
    let end = text
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(text.len());
    (&text[..end], text[end..].trim())
}

/// `(cond)` → `cond`.
fn condition(rest: &str) -> Option<&str> {
    let rest = rest.trim();
    let inner = rest
        .strip_prefix('(')
        .map(|r| r.strip_suffix(')').unwrap_or(r))
        .unwrap_or(rest)
        .trim();
    (!inner.is_empty()).then_some(inner)
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .unwrap_or(s)
}

/// A participant reference at the start of `s`: a quoted name or the text
/// up to the first `.`. Returns the name and what follows the dot.
fn target(s: &str) -> Option<(&str, &str)> {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix('"') {
        let end = rest.find('"')?;
        let after = rest[end + 1..].trim_start().strip_prefix('.')?;
        return Some((&rest[..end], after));
    }
    let paren = s.find('(').unwrap_or(s.len());
    let dot = s[..paren].find('.')?;
    Some((s[..dot].trim(), &s[dot + 1..]))
}

fn is_name(s: &str) -> bool {
    let s = s.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return true;
    }
    !s.is_empty() && s.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$')
}

impl Lower<'_, '_> {
    fn warn(&mut self, message: impl Into<String>) {
        self.cx.warn(self.line, message);
    }

    fn push(&mut self, event: Event) -> Result<(), DiagramError> {
        self.b.push(self.cx, event)
    }

    fn take_comment(&mut self) -> Result<Option<Label>, DiagramError> {
        if self.comment.is_empty() {
            return Ok(None);
        }
        let text = self.comment.join("\n");
        self.comment.clear();
        self.cx.limits.check_label(&text)?;
        Ok(Some(Label::markdown(&text)))
    }

    /// `@Starter(Name)` anywhere at the top level names the caller of
    /// top-level messages.
    fn prescan(&mut self, nodes: &[Node]) {
        for node in nodes {
            if let Node::Stmt { text, .. } = node
                && let Some(rest) = text.strip_prefix("@Starter")
                && let Some(name) = condition(rest)
            {
                if let Ok(p) = self.b.participant(self.cx, unquote(name)) {
                    self.starter = Some(p);
                }
                return;
            }
        }
    }

    /// The participant speaking now.
    fn speaker(&mut self) -> Result<usize, DiagramError> {
        if let Some(f) = self.stack.last() {
            return Ok(f.who);
        }
        if let Some(s) = self.starter.or(self.implicit_starter) {
            return Ok(s);
        }
        let s = self.b.participant(self.cx, STARTER_ID)?;
        let p = &mut self.b.model.participants[s];
        p.kind = ParticipantKind::Starter;
        p.label = Label::default();
        self.implicit_starter = Some(s);
        Ok(s)
    }

    fn block(&mut self, nodes: &[Node]) -> Result<(), DiagramError> {
        let mut i = 0;
        while i < nodes.len() {
            match &nodes[i] {
                Node::Comment(text) => {
                    if !text.is_empty() {
                        self.comment.push(text.clone());
                    }
                    i += 1;
                }
                Node::Stmt { text, line, block } => {
                    self.line = *line;
                    i += 1;
                    let (kw, rest) = keyword(text);
                    match kw {
                        "if" => {
                            let mut branches = vec![(None, block.as_deref())];
                            while let Some(Node::Stmt {
                                text: next,
                                block: next_block,
                                ..
                            }) = nodes.get(i)
                                && keyword(next).0 == "else"
                            {
                                let tail = keyword(next).1;
                                let label = match keyword(tail) {
                                    ("if", cond) => Some(format!("else if {}", condition(cond).unwrap_or(""))),
                                    _ => Some("else".to_string()),
                                };
                                branches.push((label, next_block.as_deref()));
                                i += 1;
                            }
                            self.fragment(FragmentKind::Alt, condition(rest), branches)?;
                        }
                        "try" => {
                            let mut branches = vec![(None, block.as_deref())];
                            while let Some(Node::Stmt {
                                text: next,
                                block: next_block,
                                ..
                            }) = nodes.get(i)
                                && matches!(keyword(next).0, "catch" | "finally")
                            {
                                let (k, tail) = keyword(next);
                                let label = match condition(tail) {
                                    Some(c) => format!("{k}({c})"),
                                    None => k.to_string(),
                                };
                                branches.push((Some(label), next_block.as_deref()));
                                i += 1;
                            }
                            self.fragment(FragmentKind::Try, None, branches)?;
                        }
                        "while" | "for" | "forEach" | "foreach" | "loop" if rest.is_empty() || rest.starts_with('(') => {
                            self.fragment(FragmentKind::Loop, condition(rest), vec![(None, block.as_deref())])?;
                        }
                        "opt" | "par" | "critical" | "section" | "ref" if rest.is_empty() || rest.starts_with('(') => {
                            let kind = match kw {
                                "opt" => FragmentKind::Opt,
                                "par" => FragmentKind::Par,
                                "critical" => FragmentKind::Critical,
                                "section" => FragmentKind::Section,
                                _ => FragmentKind::Ref,
                            };
                            self.fragment(kind, condition(rest), vec![(None, block.as_deref())])?;
                        }
                        "else" | "catch" | "finally" => {
                            self.warn(format!("`{kw}` without its opening block"));
                            if let Some(b) = block {
                                self.block(b)?;
                            }
                        }
                        "group" if block.is_some() => self.group(rest, block.as_deref().unwrap_or_default())?,
                        "title" if block.is_none() && !rest.is_empty() && !rest.starts_with(['.', '-', '=']) => {
                            self.cx.limits.check_label(rest)?;
                            self.cx.meta.title = Some(rest.to_string());
                        }
                        _ => self.statement(text, block.as_deref())?,
                    }
                }
            }
        }
        Ok(())
    }

    fn fragment(
        &mut self,
        kind: FragmentKind,
        cond: Option<&str>,
        branches: Vec<(Option<String>, Option<&[Node]>)>,
    ) -> Result<(), DiagramError> {
        let label = match cond {
            Some(c) => Some(self.cx.label(c)?),
            None => None,
        };
        let comment = self.take_comment()?;
        self.push(Event::Open {
            kind,
            label,
            color: None,
            comment,
        })?;
        self.fragments += 1;
        for (n, (section, nodes)) in branches.into_iter().enumerate() {
            if n > 0 {
                let label = match section {
                    Some(s) => Some(self.cx.label(&s)?),
                    None => None,
                };
                self.push(Event::Section { label })?;
            }
            if let Some(nodes) = nodes {
                self.block(nodes)?;
            }
        }
        self.comment.clear();
        self.fragments -= 1;
        self.push(Event::Close)
    }

    fn group(&mut self, rest: &str, nodes: &[Node]) -> Result<(), DiagramError> {
        let title = unquote(rest);
        let title = if title.is_empty() { None } else { Some(self.cx.label(title)?) };
        let limits = self.cx.limits;
        limits.check_count(self.b.model.groups.len() + 1, limits.clusters, LimitKind::Clusters)?;
        let outer = self.group.replace(self.b.model.groups.len());
        self.b.model.groups.push(Group { title, color: None });
        self.block(nodes)?;
        self.group = outer;
        Ok(())
    }

    fn statement(&mut self, text: &str, block: Option<&[Node]>) -> Result<(), DiagramError> {
        let text = text.trim();
        if text.is_empty() {
            if let Some(b) = block {
                self.block(b)?;
            }
            return Ok(());
        }
        if text.starts_with("@Starter") {
            return Ok(());
        }
        if let Some(rest) = text.strip_prefix("@return").or_else(|| text.strip_prefix("@reply")) {
            self.reply_next = true;
            let rest = rest.trim();
            return if rest.is_empty() { Ok(()) } else { self.statement(rest, block) };
        }
        if let Some(rest) = text.strip_prefix('@') {
            let (annotation, name) = keyword(rest);
            let kind = match annotation.to_ascii_lowercase().as_str() {
                "actor" => ParticipantKind::Actor,
                "boundary" => ParticipantKind::Boundary,
                "control" => ParticipantKind::Control,
                "entity" => ParticipantKind::Entity,
                "database" | "db" => ParticipantKind::Database,
                "collection" | "collections" => ParticipantKind::Collections,
                "queue" | "sqs" => ParticipantKind::Queue,
                _ => ParticipantKind::Participant,
            };
            return self.declare(name, kind);
        }
        if let Some(rest) = text.strip_prefix("return")
            && (rest.is_empty() || rest.starts_with(char::is_whitespace))
        {
            return self.reply(rest.trim());
        }
        if let Some((from, rest)) = split_top(text, "->") {
            return self.arrow(from.trim(), rest.trim(), block);
        }
        // `[Type] var = expr`
        let (assign, expr) = match split_top(text, "=") {
            Some((lhs, rhs)) if !rhs.starts_with('=') && !lhs.trim().is_empty() => {
                let var = lhs.split_whitespace().last().unwrap_or("").to_string();
                (Some(var), rhs.trim())
            }
            _ => (None, text),
        };
        if let Some(rest) = expr.strip_prefix("new ").or_else(|| (expr == "new").then_some("")) {
            return self.create(rest.trim(), assign, block);
        }
        if let Some((who, call)) = target(expr) {
            if who.is_empty() || call.trim().is_empty() {
                self.warn("a call needs a participant and a method");
                return Ok(());
            }
            let from = self.speaker()?;
            let to = self.b.participant(self.cx, who)?;
            return self.sync(from, to, call.trim(), assign, block);
        }
        if expr.ends_with(')') && expr.contains('(') {
            let from = self.speaker()?;
            return self.sync(from, from, expr, assign, block);
        }
        if assign.is_none() {
            return self.declare(expr, ParticipantKind::Participant);
        }
        let shown: String = text.chars().take(40).collect();
        self.warn(format!("unrecognised statement `{shown}`"));
        Ok(())
    }

    /// `Name`, `"Long name"`, `<<stereotype>> Name`, `Name as Label`.
    fn declare(&mut self, text: &str, kind: ParticipantKind) -> Result<(), DiagramError> {
        let mut text = text.trim();
        if text.starts_with("<<")
            && let Some(end) = text.find(">>")
        {
            text = text[end + 2..].trim();
        }
        let (name, alias) = match split_top(text, " as ") {
            Some((n, a)) => (n.trim(), Some(unquote(a))),
            None => {
                // A trailing width hint (`A 200`) is not part of the name.
                let name = match text.rsplit_once(char::is_whitespace) {
                    Some((n, w)) if w.parse::<f32>().is_ok() => n.trim(),
                    _ => text,
                };
                (name, None)
            }
        };
        if !is_name(name) {
            let shown: String = text.chars().take(40).collect();
            self.warn(format!("unrecognised statement `{shown}`"));
            return Ok(());
        }
        let label = match alias {
            Some(a) if !a.is_empty() => Some(self.cx.label(a)?),
            _ => None,
        };
        self.comment.clear();
        let name = unquote(name);
        self.b.declare(self.cx, name, label, kind, self.group)?;
        Ok(())
    }

    fn arrow(&mut self, from: &str, rest: &str, block: Option<&[Node]>) -> Result<(), DiagramError> {
        let from = if from.is_empty() {
            self.speaker()?
        } else {
            self.b.participant(self.cx, unquote(from))?
        };
        if let Some((to, label)) = split_top(rest, ":") {
            let to = to.trim();
            if to.is_empty() {
                self.warn("a message needs a receiver");
                return Ok(());
            }
            let to = self.b.participant(self.cx, unquote(to))?;
            return self.message(from, to, label.trim(), block);
        }
        if let Some((who, call)) = target(rest) {
            if who.is_empty() || call.trim().is_empty() {
                self.warn("a call needs a participant and a method");
                return Ok(());
            }
            let to = self.b.participant(self.cx, unquote(who))?;
            return self.sync(from, to, call.trim(), None, block);
        }
        if rest.is_empty() {
            self.warn("a message needs a receiver");
            return Ok(());
        }
        let to = self.b.participant(self.cx, unquote(rest))?;
        self.message(from, to, "", block)
    }

    /// An async message, or a reply after `@return`.
    fn message(&mut self, from: usize, to: usize, text: &str, block: Option<&[Node]>) -> Result<(), DiagramError> {
        let reply = std::mem::take(&mut self.reply_next);
        let label = self.cx.label(text)?;
        let comment = self.take_comment()?;
        self.push(Event::Message(Message {
            from,
            to,
            line: if reply { LineKind::Dashed } else { LineKind::Solid },
            start: None,
            end: Some(Marker::OpenArrow),
            label,
            wrap: false,
            activate: false,
            deactivate: false,
            comment,
        }))?;
        if let Some(b) = block {
            self.block(b)?;
        }
        Ok(())
    }

    fn sync(
        &mut self,
        from: usize,
        to: usize,
        call: &str,
        assign: Option<String>,
        block: Option<&[Node]>,
    ) -> Result<(), DiagramError> {
        let label = self.cx.label(call)?;
        self.call(from, to, label, assign, block)
    }

    fn create(&mut self, rest: &str, assign: Option<String>, block: Option<&[Node]>) -> Result<(), DiagramError> {
        let (name, args) = match rest.find('(') {
            Some(i) => (rest[..i].trim(), condition(&rest[i..])),
            None => (rest.trim(), None),
        };
        if name.is_empty() {
            self.warn("`new` needs a participant");
            return Ok(());
        }
        let from = self.speaker()?;
        let name = unquote(name);
        let fresh = self.b.find(name).is_none();
        let to = self.b.participant(self.cx, name)?;
        if fresh && self.group.is_some() {
            self.b.model.participants[to].group = self.group;
        }
        if fresh && to != from {
            self.push(Event::Create(to))?;
        }
        let text = match args {
            Some(a) => format!("«create»({a})"),
            None => "«create»".to_string(),
        };
        let label = self.cx.label(&text)?;
        self.call(from, to, label, assign, block)
    }

    /// A sync call: the receiver stays active through its block and answers
    /// with the assigned name.
    fn call(
        &mut self,
        from: usize,
        to: usize,
        label: Label,
        assign: Option<String>,
        block: Option<&[Node]>,
    ) -> Result<(), DiagramError> {
        let activate = block.is_some() || assign.is_some();
        let comment = self.take_comment()?;
        self.reply_next = false;
        self.push(Event::Message(Message {
            from,
            to,
            line: LineKind::Solid,
            start: None,
            end: Some(Marker::Arrow),
            label,
            wrap: false,
            activate,
            deactivate: false,
            comment,
        }))?;
        if !activate {
            return Ok(());
        }
        self.stack.push(Frame {
            who: to,
            caller: from,
            active: true,
            fragments: self.fragments,
        });
        if let Some(b) = block {
            self.block(b)?;
        }
        let Some(frame) = self.stack.pop() else {
            return Ok(());
        };
        if !frame.active {
            return Ok(());
        }
        match assign {
            Some(var) => {
                let label = self.cx.label(&var)?;
                self.push(Event::Message(Message {
                    from: to,
                    to: from,
                    line: LineKind::Dashed,
                    start: None,
                    end: Some(Marker::OpenArrow),
                    label,
                    wrap: false,
                    activate: false,
                    deactivate: true,
                    comment: None,
                }))
            }
            None => self.push(Event::Deactivate(to)),
        }
    }

    fn reply(&mut self, value: &str) -> Result<(), DiagramError> {
        let Some(frame) = self.stack.last() else {
            self.warn("`return` outside a call");
            return Ok(());
        };
        let ends = frame.active && frame.fragments == self.fragments;
        let (who, caller) = (frame.who, frame.caller);
        if ends && let Some(f) = self.stack.last_mut() {
            f.active = false;
        }
        let label = self.cx.label(value)?;
        let comment = self.take_comment()?;
        self.push(Event::Message(Message {
            from: who,
            to: caller,
            line: LineKind::Dashed,
            start: None,
            end: Some(Marker::OpenArrow),
            label,
            wrap: false,
            activate: false,
            deactivate: ends,
            comment,
        }))
    }
}

/// Put participant `s` in the first column, renumbering every reference.
fn move_to_front(model: &mut Model, s: usize) {
    if s == 0 || s >= model.participants.len() {
        return;
    }
    let map = |p: usize| match p {
        p if p == s => 0,
        p if p < s => p + 1,
        p => p,
    };
    let starter = model.participants.remove(s);
    model.participants.insert(0, starter);
    for event in &mut model.events {
        match event {
            Event::Message(m) => {
                m.from = map(m.from);
                m.to = map(m.to);
            }
            Event::Note { place, .. } => {
                use crate::families::sequence::NotePlace;
                *place = match *place {
                    NotePlace::Left(p) => NotePlace::Left(map(p)),
                    NotePlace::Right(p) => NotePlace::Right(map(p)),
                    NotePlace::Over(a, b) => NotePlace::Over(map(a), map(b)),
                };
            }
            Event::Activate(p) | Event::Deactivate(p) | Event::Create(p) | Event::Destroy(p) => *p = map(*p),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::families::sequence::{Event, FragmentKind, LineKind, Model, ParticipantKind};
    use crate::{Limits, parse as parse_source};

    fn model(source: &str) -> Model {
        match parse_source(source, &Limits::DEFAULT).unwrap().model {
            crate::families::Model::ZenUml(m) => m,
            _ => panic!("not zenuml"),
        }
    }

    #[test]
    fn sync_calls_nest_and_return_to_their_caller() {
        let m = model("zenuml\n@Actor A\nA->B.call(x) {\n  C.inner()\n  return done\n}");
        assert_eq!(m.participants[0].kind, ParticipantKind::Actor);
        let msgs: Vec<_> = m
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message(m) => Some((m.from, m.to, m.line, m.label.text())),
                _ => None,
            })
            .collect();
        assert_eq!(
            msgs,
            [
                (0, 1, LineKind::Solid, "call(x)".to_string()),
                (1, 2, LineKind::Solid, "inner()".to_string()),
                (1, 0, LineKind::Dashed, "done".to_string()),
            ]
        );
    }

    #[test]
    fn top_level_calls_come_from_a_starter_in_the_first_column() {
        let m = model("zenuml\nBob\nx = Bob.ask()\nnew Carl(1)");
        assert_eq!(m.participants[0].kind, ParticipantKind::Starter);
        assert_eq!(m.participants[1].id, "Bob");
        assert!(m.events.iter().any(|e| matches!(e, Event::Create(2))));
    }

    #[test]
    fn control_blocks_become_fragments_with_sections() {
        let m = model(
            "zenuml\nif(a) {\n A->B: one\n} else if(b) {\n A->B: two\n} else {\n A->B: three\n}\ntry {\n A->B: t\n} catch(e) {\n A->B: c\n} finally {\n A->B: f\n}\nwhile(x) { A->B: w }",
        );
        let kinds: Vec<_> = m
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Open { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(kinds, [FragmentKind::Alt, FragmentKind::Try, FragmentKind::Loop]);
        let sections = m.events.iter().filter(|e| matches!(e, Event::Section { .. })).count();
        assert_eq!(sections, 4);
    }

    #[test]
    fn comments_attach_to_the_next_message() {
        let m = model("zenuml\n// **why** we call\nA.go()");
        let Some(Event::Message(msg)) = m.events.first() else { panic!() };
        assert_eq!(msg.comment.as_ref().unwrap().text(), "why we call");
    }

    #[test]
    fn braces_too_deep_end_cleanly() {
        let source = format!("zenuml\n{}", "A.m() {\n".repeat(200));
        assert!(parse_source(&source, &Limits::DEFAULT).is_err());
    }
}
