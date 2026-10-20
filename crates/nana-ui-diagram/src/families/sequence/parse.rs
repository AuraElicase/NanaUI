//! The `sequenceDiagram` grammar: participants and their types, boxes,
//! create / destroy, every arrow, activations, notes, the block statements,
//! `autonumber`, `title` and actor links.

use super::{Builder, Event, FragmentKind, Group, LineKind, Message, NotePlace, ParticipantKind};
use crate::families::ParseCx;
use crate::geom::Marker;
use crate::kind;
use crate::source::color;
use crate::DiagramError;

/// Arrow spellings, longest first at any one position.
const ARROWS: &[(&str, LineKind, Option<Marker>, Option<Marker>)] = &[
    ("<<-->>", LineKind::Dashed, Some(Marker::Arrow), Some(Marker::Arrow)),
    ("<<->>", LineKind::Solid, Some(Marker::Arrow), Some(Marker::Arrow)),
    ("-->>", LineKind::Dashed, None, Some(Marker::Arrow)),
    ("->>", LineKind::Solid, None, Some(Marker::Arrow)),
    ("--|\\", LineKind::Dashed, None, Some(Marker::Arrow)),
    ("--|/", LineKind::Dashed, None, Some(Marker::Arrow)),
    ("--\\\\", LineKind::Dashed, None, Some(Marker::OpenArrow)),
    ("--//", LineKind::Dashed, None, Some(Marker::OpenArrow)),
    ("-|\\", LineKind::Solid, None, Some(Marker::Arrow)),
    ("-|/", LineKind::Solid, None, Some(Marker::Arrow)),
    ("-\\\\", LineKind::Solid, None, Some(Marker::OpenArrow)),
    ("-//", LineKind::Solid, None, Some(Marker::OpenArrow)),
    ("/|--", LineKind::Dashed, Some(Marker::Arrow), None),
    ("\\|--", LineKind::Dashed, Some(Marker::Arrow), None),
    ("//--", LineKind::Dashed, Some(Marker::OpenArrow), None),
    ("\\\\--", LineKind::Dashed, Some(Marker::OpenArrow), None),
    ("/|-", LineKind::Solid, Some(Marker::Arrow), None),
    ("\\|-", LineKind::Solid, Some(Marker::Arrow), None),
    ("//-", LineKind::Solid, Some(Marker::OpenArrow), None),
    ("\\\\-", LineKind::Solid, Some(Marker::OpenArrow), None),
    ("--x", LineKind::Dashed, None, Some(Marker::Cross)),
    ("-x", LineKind::Solid, None, Some(Marker::Cross)),
    ("--)", LineKind::Dashed, None, Some(Marker::OpenArrow)),
    ("-)", LineKind::Solid, None, Some(Marker::OpenArrow)),
    ("-->", LineKind::Dashed, None, None),
    ("->", LineKind::Solid, None, None),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Open {
    Fragment(FragmentKind),
    Box,
}

struct Parser<'a, 'c> {
    cx: &'a mut ParseCx<'c>,
    b: Builder,
    stack: Vec<Open>,
    group: Option<usize>,
    line: usize,
}

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<super::Model, DiagramError> {
    let mirror = cx.config.get_bool("sequence.mirrorActors").unwrap_or(true);
    let numbered = cx.config.get_bool("sequence.showSequenceNumbers") == Some(true);
    let mut p = Parser {
        cx,
        b: Builder::new(mirror),
        stack: Vec::new(),
        group: None,
        line: 0,
    };
    if numbered {
        p.b.push(p.cx, Event::Autonumber(Some((1.0, 1.0))))?;
    }
    let mut header_seen = false;
    let mut last = 0;
    for (index, line) in body.lines().enumerate() {
        p.line = index;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        last = index;
        let mut rest = trimmed;
        if !header_seen {
            let Some((_, after)) = kind::header(trimmed) else {
                return Err(DiagramError::syntax(index as u32 + 1, 1, "expected `sequenceDiagram`"));
            };
            header_seen = true;
            rest = after.trim_start_matches([':', ';']).trim();
        }
        for stmt in split_statements(rest) {
            let stmt = stmt.trim();
            if !stmt.is_empty() {
                p.statement(stmt)?;
            }
        }
    }
    if !header_seen {
        return Err(DiagramError::Empty);
    }
    let Parser { cx, b, .. } = p;
    Ok(b.finish(cx, last))
}

/// Split on `;`, except the one that ends an entity code (`#59;`).
fn split_statements(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = line.as_bytes();
    for (i, c) in line.char_indices() {
        if c != ';' {
            continue;
        }
        let mut j = i;
        while j > start && bytes[j - 1].is_ascii_alphanumeric() {
            j -= 1;
        }
        if j < i && j > start && matches!(bytes[j - 1], b'#' | b'&') {
            continue;
        }
        out.push(&line[start..i]);
        start = i + 1;
    }
    out.push(&line[start..]);
    out
}

fn first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// A message or note text: an optional `wrap:` / `nowrap:` prefix, then the
/// label.
fn text_and_wrap(raw: &str) -> (&str, bool) {
    let raw = raw.trim();
    if let Some(rest) = raw.strip_prefix("wrap:") {
        return (rest.trim(), true);
    }
    if let Some(rest) = raw.strip_prefix("nowrap:") {
        return (rest.trim(), false);
    }
    (raw, false)
}

impl Parser<'_, '_> {
    fn warn(&mut self, message: impl Into<String>) {
        self.cx.warn(self.line, message);
    }

    fn syntax(&self, message: &str) -> DiagramError {
        DiagramError::syntax(self.line as u32 + 1, 1, message)
    }

    fn push(&mut self, event: Event) -> Result<(), DiagramError> {
        self.b.push(self.cx, event)
    }

    fn statement(&mut self, stmt: &str) -> Result<(), DiagramError> {
        let (word, rest) = first_word(stmt);
        let lower = word.to_ascii_lowercase();
        match lower.as_str() {
            "participant" => return self.declare(ParticipantKind::Participant, rest, false),
            "actor" => return self.declare(ParticipantKind::Actor, rest, false),
            "create" => {
                let (what, tail) = first_word(rest);
                return match what.to_ascii_lowercase().as_str() {
                    "participant" => self.declare(ParticipantKind::Participant, tail, true),
                    "actor" => self.declare(ParticipantKind::Actor, tail, true),
                    _ => {
                        self.warn("`create` needs `participant` or `actor`");
                        Ok(())
                    }
                };
            }
            "destroy" => {
                if rest.is_empty() {
                    self.warn("`destroy` needs a participant");
                    return Ok(());
                }
                let p = self.b.participant(self.cx, rest)?;
                return self.push(Event::Destroy(p));
            }
            "box" => return self.open_box(rest),
            "end" if rest.is_empty() => return self.close(),
            "loop" => return self.open(FragmentKind::Loop, rest),
            "alt" => return self.open(FragmentKind::Alt, rest),
            "opt" => return self.open(FragmentKind::Opt, rest),
            "par" | "par_over" => return self.open(FragmentKind::Par, rest),
            "critical" => return self.open(FragmentKind::Critical, rest),
            "break" => return self.open(FragmentKind::Break, rest),
            "rect" => return self.open_rect(rest),
            "else" | "and" | "option" => return self.section(rest),
            "activate" | "deactivate" => {
                if rest.is_empty() {
                    self.warn(format!("`{word}` needs a participant"));
                    return Ok(());
                }
                let p = self.b.participant(self.cx, rest)?;
                let event = if lower == "activate" { Event::Activate(p) } else { Event::Deactivate(p) };
                return self.push(event);
            }
            "autonumber" => return self.autonumber(rest),
            "link" | "links" => {
                self.link(lower == "links", rest);
                return Ok(());
            }
            "properties" | "details" => return Ok(()),
            _ => {}
        }
        if lower == "note" {
            return self.note(rest);
        }
        if let Some(title) = stmt.strip_prefix("title")
            && (title.is_empty() || title.starts_with([' ', '\t', ':']))
        {
            let title = title.trim_start_matches(':').trim();
            if !title.is_empty() {
                self.cx.limits.check_label(title)?;
                self.cx.meta.title = Some(crate::source::entities::decode(title));
            }
            return Ok(());
        }
        if self.message(stmt)? {
            return Ok(());
        }
        let shown: String = stmt.chars().take(40).collect();
        self.warn(format!("unrecognised statement `{shown}`"));
        Ok(())
    }

    fn declare(&mut self, kind: ParticipantKind, rest: &str, create: bool) -> Result<(), DiagramError> {
        let rest = rest.trim();
        let (mut id, mut tail) = (rest, "");
        let mut kind = kind;
        let mut alias: Option<String> = None;
        if let Some(at) = rest.find("@{") {
            id = &rest[..at];
            let json_start = at + 1;
            let close = rest[json_start..].find('}').map(|e| json_start + e + 1).unwrap_or(rest.len());
            let json = &rest[json_start..close];
            if let Some(t) = json_str(json, "type") {
                match ParticipantKind::from_type(&t) {
                    Some(k) => kind = k,
                    None => self.warn(format!("unknown participant type `{t}`")),
                }
            }
            alias = json_str(json, "alias");
            tail = &rest[close..];
        } else if let Some(at) = find_as(rest) {
            id = &rest[..at];
            tail = &rest[at..];
        }
        let tail = tail.trim();
        if let Some(external) = tail.strip_prefix("as").filter(|t| t.is_empty() || t.starts_with(char::is_whitespace)) {
            let external = external.trim();
            if !external.is_empty() {
                alias = Some(external.to_string());
            }
        }
        let id = id.trim();
        if id.is_empty() {
            self.warn("a participant needs a name");
            return Ok(());
        }
        let label = match alias {
            Some(a) => Some(self.cx.label(&a)?),
            None => None,
        };
        let p = self.b.declare(self.cx, id, label, kind, self.group)?;
        if create {
            self.push(Event::Create(p))?;
        }
        Ok(())
    }

    fn open_box(&mut self, rest: &str) -> Result<(), DiagramError> {
        if self.group.is_some() || !self.stack.is_empty() {
            self.warn("`box` inside another block is ignored");
            self.stack.push(Open::Box);
            return Ok(());
        }
        let (color, title) = leading_color(rest);
        let title = title.trim();
        let title = if title.is_empty() { None } else { Some(self.cx.label(title)?) };
        let limits = self.cx.limits;
        limits.check_count(self.b.model.groups.len() + 1, limits.clusters, crate::LimitKind::Clusters)?;
        self.group = Some(self.b.model.groups.len());
        self.b.model.groups.push(Group { title, color });
        self.stack.push(Open::Box);
        Ok(())
    }

    fn open(&mut self, kind: FragmentKind, rest: &str) -> Result<(), DiagramError> {
        let label = if rest.trim().is_empty() { None } else { Some(self.cx.label(rest)?) };
        self.push(Event::Open {
            kind,
            label,
            color: None,
            comment: None,
        })?;
        self.stack.push(Open::Fragment(kind));
        Ok(())
    }

    fn open_rect(&mut self, rest: &str) -> Result<(), DiagramError> {
        let (color, tail) = leading_color(rest);
        if color.is_none() && !rest.trim().is_empty() {
            self.warn("unreadable `rect` colour");
        }
        let _ = tail;
        self.push(Event::Open {
            kind: FragmentKind::Rect,
            label: None,
            color,
            comment: None,
        })?;
        self.stack.push(Open::Fragment(FragmentKind::Rect));
        Ok(())
    }

    fn section(&mut self, rest: &str) -> Result<(), DiagramError> {
        match self.stack.last() {
            Some(Open::Fragment(kind)) if *kind != FragmentKind::Rect => {}
            _ => return Err(self.syntax("`else`, `and` or `option` outside a block")),
        }
        let label = if rest.trim().is_empty() { None } else { Some(self.cx.label(rest)?) };
        self.push(Event::Section { label })
    }

    fn close(&mut self) -> Result<(), DiagramError> {
        match self.stack.pop() {
            Some(Open::Box) => {
                if self.stack.iter().all(|o| *o != Open::Box) {
                    self.group = None;
                }
                Ok(())
            }
            Some(Open::Fragment(_)) => self.push(Event::Close),
            None => Err(self.syntax("`end` without an open block")),
        }
    }

    fn autonumber(&mut self, rest: &str) -> Result<(), DiagramError> {
        let rest = rest.trim();
        if rest == "off" {
            return self.push(Event::Autonumber(None));
        }
        let mut nums = rest.split_whitespace().map(|w| w.parse::<f64>().ok().filter(|v| v.is_finite() && v.abs() < 1e9));
        let start = nums.next().flatten().unwrap_or(1.0);
        let step = nums.next().flatten().unwrap_or(1.0);
        self.push(Event::Autonumber(Some((start, step))))
    }

    fn link(&mut self, json: bool, rest: &str) {
        let Some((who, what)) = rest.split_once(':') else {
            self.warn("`link` needs `participant: label @ url`");
            return;
        };
        let Some(p) = self.b.find(who.trim()) else {
            return;
        };
        let pair = if json {
            json_pairs(what).into_iter().next()
        } else {
            what.split_once('@').map(|(l, u)| (l.trim().to_string(), u.trim().to_string()))
        };
        if let Some((label, url)) = pair.filter(|(_, u)| !u.is_empty())
            && self.b.model.participants[p].link.is_none()
        {
            self.b.model.participants[p].link = Some((url, label));
        }
    }

    fn note(&mut self, rest: &str) -> Result<(), DiagramError> {
        let Some((place, text)) = rest.split_once(':') else {
            self.warn("a note needs `: text`");
            return Ok(());
        };
        let place = place.trim();
        let lower = place.to_ascii_lowercase();
        let (which, names) = if let Some(r) = strip_words(&lower, place, &["right", "of"]) {
            (0, r)
        } else if let Some(r) = strip_words(&lower, place, &["left", "of"]) {
            (1, r)
        } else if let Some(r) = strip_words(&lower, place, &["over"]) {
            (2, r)
        } else {
            self.warn("a note goes `left of`, `right of` or `over` a participant");
            return Ok(());
        };
        let mut ids = names.split(',').map(str::trim).filter(|n| !n.is_empty());
        let Some(first) = ids.next() else {
            self.warn("a note needs a participant");
            return Ok(());
        };
        let a = self.b.participant(self.cx, first)?;
        let place = match which {
            0 => NotePlace::Right(a),
            1 => NotePlace::Left(a),
            _ => match ids.next() {
                Some(second) => NotePlace::Over(a, self.b.participant(self.cx, second)?),
                None => NotePlace::Over(a, a),
            },
        };
        let (text, _) = text_and_wrap(text);
        let label = self.cx.label(text)?;
        self.push(Event::Note { place, label })
    }

    /// `A->>+B: text`. `Ok(false)` when the statement has no arrow.
    fn message(&mut self, stmt: &str) -> Result<bool, DiagramError> {
        let (head, text) = match stmt.find(':') {
            Some(i) => (&stmt[..i], &stmt[i + 1..]),
            None => (stmt, ""),
        };
        let Some((at, len, line, start, end)) = find_arrow(head) else {
            return Ok(false);
        };
        let from = head[..at].trim();
        let from = from.strip_suffix("()").unwrap_or(from).trim();
        let mut to = head[at + len..].trim();
        let mut activate = false;
        let mut deactivate = false;
        to = to.strip_prefix("()").unwrap_or(to).trim();
        if let Some(t) = to.strip_prefix('+') {
            activate = true;
            to = t;
        } else if let Some(t) = to.strip_prefix('-') {
            deactivate = true;
            to = t;
        }
        let to = to.trim();
        let to = to.strip_prefix("()").unwrap_or(to).trim();
        if from.is_empty() || to.is_empty() {
            self.warn("a message needs a sender and a receiver");
            return Ok(true);
        }
        let from = self.b.participant(self.cx, from)?;
        let to = self.b.participant(self.cx, to)?;
        let (text, wrap) = text_and_wrap(text);
        let label = self.cx.label(text)?;
        self.push(Event::Message(Message {
            from,
            to,
            line,
            start,
            end,
            label,
            wrap,
            activate,
            deactivate,
            comment: None,
        }))?;
        Ok(true)
    }
}

type ArrowHit = (usize, usize, LineKind, Option<Marker>, Option<Marker>);

/// The first arrow in `head`: byte offset, length and what it draws.
fn find_arrow(head: &str) -> Option<ArrowHit> {
    for (i, _) in head.char_indices() {
        let tail = &head[i..];
        for &(spelling, line, start, end) in ARROWS {
            if tail.starts_with(spelling) {
                return Some((i, spelling.len(), line, start, end));
            }
        }
    }
    None
}

/// Where ` as ` starts in a declaration, if anywhere.
fn find_as(rest: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut from = 0;
    while let Some(i) = rest[from..].find("as").map(|i| i + from) {
        let before = i > 0 && bytes[i - 1].is_ascii_whitespace();
        let after = bytes.get(i + 2).is_none_or(|b| b.is_ascii_whitespace());
        if before && after {
            return Some(i);
        }
        from = i + 2;
    }
    None
}

/// `place` with the leading `words` removed (matched case-insensitively
/// against `lower`), keeping the original spelling of what follows.
fn strip_words<'a>(lower: &str, place: &'a str, words: &[&str]) -> Option<&'a str> {
    let mut l = lower;
    let mut offset = 0;
    for w in words {
        let trimmed = l.trim_start();
        offset += l.len() - trimmed.len();
        let rest = trimmed.strip_prefix(w)?;
        if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
            return None;
        }
        offset += w.len();
        l = rest;
    }
    Some(place[offset..].trim())
}

/// The string value of `"key": "value"` in a small JSON object.
pub(crate) fn json_str(json: &str, key: &str) -> Option<String> {
    let quoted = format!("\"{key}\"");
    let at = json.find(&quoted)? + quoted.len();
    let rest = json[at..].trim_start().strip_prefix(':')?.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Every `"name": "value"` pair of a flat JSON object, in order.
fn json_pairs(json: &str) -> Vec<(String, String)> {
    let mut strings = Vec::new();
    let mut rest = json;
    while let Some(start) = rest.find('"') {
        let tail = &rest[start + 1..];
        let Some(end) = tail.find('"') else { break };
        strings.push(tail[..end].to_string());
        rest = &tail[end + 1..];
    }
    strings
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].clone(), c[1].clone()))
        .collect()
}

/// A colour at the start of `rest` (a CSS function or a colour word) and
/// what follows it. `transparent` reads as a fully transparent colour.
fn leading_color(rest: &str) -> (Option<[f32; 4]>, &str) {
    let rest = rest.trim();
    let lower = rest.to_ascii_lowercase();
    if ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|f| lower.starts_with(f))
        && let Some(end) = rest.find(')')
    {
        let func = &rest[..=end];
        return (parse_color(func), &rest[end + 1..]);
    }
    let (word, tail) = first_word(rest);
    match parse_color(word) {
        Some(c) => (Some(c), tail),
        None => (None, rest),
    }
}

fn parse_color(value: &str) -> Option<[f32; 4]> {
    let lower = value.trim().to_ascii_lowercase();
    if let Some(args) = lower
        .strip_prefix("hsla(")
        .or_else(|| lower.strip_prefix("hsl("))
        .and_then(|a| a.strip_suffix(')'))
    {
        return hsl(args);
    }
    color::parse(value)
}

fn hsl(args: &str) -> Option<[f32; 4]> {
    let parts: Vec<&str> = args
        .split([',', ' ', '/'])
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let num = |p: &str| p.trim_end_matches(['%']).trim_end_matches("deg").parse::<f32>().ok().filter(|v| v.is_finite());
    let h = num(parts[0])?.rem_euclid(360.0) / 360.0;
    let s = (num(parts[1])? / 100.0).clamp(0.0, 1.0);
    let l = (num(parts[2])? / 100.0).clamp(0.0, 1.0);
    let a = match parts.get(3) {
        Some(p) if p.ends_with('%') => num(p)? / 100.0,
        Some(p) => num(p)?,
        None => 1.0,
    }
    .clamp(0.0, 1.0);
    let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
    let p = 2.0 * l - q;
    let channel = |t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    Some([channel(h + 1.0 / 3.0), channel(h), channel(h - 1.0 / 3.0), a])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::families::sequence::{Event, Model};
    use crate::{Limits, parse as parse_source};

    fn model(source: &str) -> Model {
        let parsed = parse_source(source, &Limits::DEFAULT).unwrap();
        match parsed.model {
            crate::families::Model::Sequence(m) => m,
            _ => panic!("not a sequence"),
        }
    }

    #[test]
    fn arrows_read_their_line_and_heads() {
        let m = model("sequenceDiagram\nA->>B: one\nB-->>A: two\nA-xB: x\nA--)B: async\nA<<->>B: both\nA->B: plain");
        let msgs: Vec<_> = m
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Message(m) => Some((m.line, m.start, m.end)),
                _ => None,
            })
            .collect();
        assert_eq!(
            msgs,
            [
                (LineKind::Solid, None, Some(Marker::Arrow)),
                (LineKind::Dashed, None, Some(Marker::Arrow)),
                (LineKind::Solid, None, Some(Marker::Cross)),
                (LineKind::Dashed, None, Some(Marker::OpenArrow)),
                (LineKind::Solid, Some(Marker::Arrow), Some(Marker::Arrow)),
                (LineKind::Solid, None, None),
            ]
        );
        assert_eq!(m.participants.len(), 2);
    }

    #[test]
    fn declarations_read_types_aliases_and_boxes() {
        let m = model(
            "sequenceDiagram\nbox Aqua Team\nparticipant API@{ \"type\": \"boundary\", \"alias\": \"In\" } as Out\nactor U\nend\nparticipant D@{\"type\":\"database\"}\nparticipant A as Alice Smith",
        );
        let p = &m.participants;
        assert_eq!(p[0].kind, ParticipantKind::Boundary);
        assert_eq!(p[0].label.text(), "Out");
        assert_eq!(p[0].group, Some(0));
        assert_eq!(p[1].kind, ParticipantKind::Actor);
        assert_eq!(p[2].kind, ParticipantKind::Database);
        assert_eq!(p[2].group, None);
        assert_eq!(p[3].id, "A");
        assert_eq!(p[3].label.text(), "Alice Smith");
        assert_eq!(m.groups[0].title.as_ref().unwrap().text(), "Team");
        assert!(m.groups[0].color.is_some());
    }

    #[test]
    fn activation_shorthand_and_entity_semicolons() {
        let m = model("sequenceDiagram\nA->>+B: I #9829; you#59; really\nB-->>-A: ok");
        let Event::Message(first) = &m.events[0] else { panic!() };
        assert!(first.activate);
        assert_eq!(first.label.text(), "I \u{2665} you; really");
        let Event::Message(second) = &m.events[1] else { panic!() };
        assert!(second.deactivate);
    }

    #[test]
    fn stray_end_is_a_syntax_error_and_unclosed_blocks_close() {
        assert!(matches!(
            parse_source("sequenceDiagram\nend", &Limits::DEFAULT),
            Err(DiagramError::Syntax { .. })
        ));
        let m = model("sequenceDiagram\nloop forever\nA->>B: hi");
        assert!(matches!(m.events.last(), Some(Event::Close)));
    }

    #[test]
    fn hsl_colours_convert() {
        let c = hsl("0, 100%, 50%").unwrap();
        assert!((c[0] - 1.0).abs() < 1e-3 && c[1].abs() < 1e-3);
    }
}
