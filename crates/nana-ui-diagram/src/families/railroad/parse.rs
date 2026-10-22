//! Grammar notations: EBNF (`=`/`::=`, `|`, `[ ]`, `{ }`, `* + ?`, `-`),
//! ABNF (`=`/`=/`, `/`, `n*m` repetition, `%x` values, `<prose>`), PEG
//! (`<-`, `/`, `! &`, `* + ?`, `[a-z]`, `.`) and the constructor calls of
//! railroad-diagram libraries, which every notation accepts.
//!
//! A rule that does not parse is skipped with a warning; parsing resumes
//! at the next `;` or the next `name =`.

use super::{Expr, Model, Notation, Rule};
use crate::families::{ParseCx, kit};
use crate::{DiagramError, LimitKind};

/// How deeply groups and calls may nest.
const MAX_DEPTH: usize = 48;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    Ident(String),
    Str(String),
    Num(u32),
    Special(String),
    Punct(&'static str),
}

#[derive(Debug, Clone)]
struct Token {
    tok: Tok,
    line: usize,
    /// No whitespace before it.
    glued: bool,
}

const PUNCT: &[&str] = &[
    "::=", ":=", "=/", "<-", "=", "|", "/", "(", ")", "[", "]", "{", "}", "*", "+", "?", "!", "&",
    ",", ";", ".", "-",
];

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, _) = kit::header(body)?;
    let mut lines: Vec<&str> = body.lines().collect();
    let head = lines[header].trim();
    let notation = if head.starts_with("railroad-abnf") {
        Notation::Abnf
    } else if head.starts_with("railroad-peg") {
        Notation::Peg
    } else {
        Notation::Ebnf
    };
    lines[header] = "";
    for line in lines.iter_mut() {
        if let Some(rest) = kit::keyword(line.trim(), "title")
            && !rest.starts_with(['=', ':', '<'])
        {
            let title = rest.trim_matches(['"', '\'']);
            cx.limits.check_label(title)?;
            cx.meta.title = Some(title.to_string()).filter(|t| !t.is_empty());
            *line = "";
        }
    }
    let toks = tokenize(&lines, notation, cx);
    let mut p = Parser {
        toks,
        pos: 0,
        notation,
        depth: 0,
        elements: 0,
        cx,
    };
    let rules = p.rules()?;
    Ok(Model { rules })
}

fn tokenize(lines: &[&str], notation: Notation, cx: &mut ParseCx<'_>) -> Vec<Token> {
    let mut out = Vec::new();
    let mut in_comment = false;
    for (line_no, line) in lines.iter().enumerate() {
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut glued = false;
        let mut warned = false;
        while i < chars.len() {
            let c = chars[i];
            let next = chars.get(i + 1).copied();
            if in_comment {
                if c == '*' && next == Some(')') {
                    in_comment = false;
                    i += 2;
                } else {
                    i += 1;
                }
                glued = false;
                continue;
            }
            if c.is_whitespace() {
                glued = false;
                i += 1;
                continue;
            }
            let start_glued = glued;
            glued = true;
            let mut push = |tok: Tok| {
                out.push(Token {
                    tok,
                    line: line_no,
                    glued: start_glued,
                })
            };
            // Comments.
            if c == '(' && next == Some('*') {
                in_comment = true;
                i += 2;
                continue;
            }
            if (c == ';' && notation == Notation::Abnf)
                || (c == '#' && notation == Notation::Peg)
                || (c == '/' && next == Some('/') && notation != Notation::Abnf)
            {
                break;
            }
            // Strings.
            if c == '"' || c == '\'' {
                let mut text = String::new();
                let mut j = i + 1;
                while j < chars.len() && chars[j] != c {
                    if chars[j] == '\\' && j + 1 < chars.len() {
                        j += 1;
                    }
                    text.push(chars[j]);
                    j += 1;
                }
                push(Tok::Str(text));
                i = j + 1;
                continue;
            }
            // ABNF numeric and case-sensitive values.
            if c == '%' && notation == Notation::Abnf {
                if matches!(next, Some('s' | 'i' | 'S' | 'I')) && chars.get(i + 2) == Some(&'"') {
                    i += 2;
                    continue;
                }
                let mut j = i + 1;
                while j < chars.len()
                    && (chars[j].is_ascii_alphanumeric() || matches!(chars[j], '.' | '-'))
                {
                    j += 1;
                }
                push(Tok::Special(chars[i..j].iter().collect()));
                i = j;
                continue;
            }
            // `<name>`: prose in ABNF, a rule name elsewhere.
            if c == '<'
                && next != Some('-')
                && let Some(len) = chars[i + 1..].iter().position(|&d| d == '>')
            {
                let text: String = chars[i + 1..i + 1 + len].iter().collect();
                push(if notation == Notation::Abnf {
                    Tok::Special(text)
                } else {
                    Tok::Ident(text.trim().to_string())
                });
                i += len + 2;
                continue;
            }
            // PEG character classes.
            if c == '[' && notation == Notation::Peg {
                let mut j = i + 1;
                while j < chars.len() && chars[j] != ']' {
                    if chars[j] == '\\' {
                        j += 1;
                    }
                    j += 1;
                }
                let end = j.min(chars.len());
                push(Tok::Special(
                    chars[i..(end + 1).min(chars.len())].iter().collect(),
                ));
                i = end + 1;
                continue;
            }
            if c.is_ascii_digit() {
                let mut j = i;
                while j < chars.len() && chars[j].is_ascii_digit() {
                    j += 1;
                }
                let text: String = chars[i..j].iter().collect();
                push(Tok::Num(text.parse().unwrap_or(u32::MAX)));
                i = j;
                continue;
            }
            if c.is_alphabetic() || c == '_' {
                let mut j = i;
                while j < chars.len()
                    && (chars[j].is_alphanumeric()
                        || chars[j] == '_'
                        || (chars[j] == '-'
                            && chars.get(j + 1).is_some_and(|d| d.is_alphanumeric())))
                {
                    j += 1;
                }
                push(Tok::Ident(chars[i..j].iter().collect()));
                i = j;
                continue;
            }
            let rest: String = chars[i..chars.len().min(i + 3)].iter().collect();
            if let Some(p) = PUNCT.iter().find(|p| rest.starts_with(**p)) {
                push(Tok::Punct(p));
                i += p.chars().count();
                continue;
            }
            if !warned {
                cx.warn(line_no, format!("unexpected `{c}`"));
                warned = true;
            }
            i += 1;
        }
    }
    out
}

enum Fail {
    Syntax(usize, String),
    Fatal(DiagramError),
}

impl From<DiagramError> for Fail {
    fn from(e: DiagramError) -> Self {
        Self::Fatal(e)
    }
}

struct Parser<'a, 'c> {
    toks: Vec<Token>,
    pos: usize,
    notation: Notation,
    depth: usize,
    elements: usize,
    cx: &'a mut ParseCx<'c>,
}

fn is_define(tok: Option<&Token>) -> bool {
    matches!(
        tok.map(|t| &t.tok),
        Some(Tok::Punct("=" | "::=" | ":=" | "<-" | "=/"))
    )
}

fn zero_or_more(item: Expr, separator: Option<Expr>) -> Expr {
    Expr::Optional(Box::new(one_or_more(item, separator, None)))
}

fn one_or_more(item: Expr, separator: Option<Expr>, note: Option<String>) -> Expr {
    Expr::OneOrMore {
        item: Box::new(item),
        separator: separator.map(Box::new),
        note,
    }
}

impl Parser<'_, '_> {
    fn peek(&self) -> Option<&Tok> {
        self.toks.get(self.pos).map(|t| &t.tok)
    }

    fn line(&self) -> usize {
        self.toks
            .get(self.pos)
            .or_else(|| self.toks.last())
            .map_or(0, |t| t.line)
    }

    fn at_punct(&self, p: &str) -> bool {
        matches!(self.peek(), Some(Tok::Punct(q)) if *q == p)
    }

    fn at_rule_start(&self) -> bool {
        matches!(self.peek(), Some(Tok::Ident(_))) && is_define(self.toks.get(self.pos + 1))
    }

    fn fail<T>(&self, message: impl Into<String>) -> Result<T, Fail> {
        Err(Fail::Syntax(self.line(), message.into()))
    }

    fn count(&mut self, expr: Expr) -> Result<Expr, Fail> {
        self.elements += 1;
        self.cx
            .limits
            .check_count(self.elements, self.cx.limits.elements, LimitKind::Elements)?;
        Ok(expr)
    }

    fn rules(&mut self) -> Result<Vec<Rule>, DiagramError> {
        let mut rules: Vec<Rule> = Vec::new();
        while self.pos < self.toks.len() {
            if self.at_punct(";") {
                self.pos += 1;
                continue;
            }
            if !self.at_rule_start() {
                let line = self.line();
                self.cx.warn(line, "expected `name = ...`");
                self.resync();
                continue;
            }
            let Some(Tok::Ident(name)) = self.peek().cloned() else {
                unreachable!("checked by at_rule_start")
            };
            let incremental = matches!(self.toks[self.pos + 1].tok, Tok::Punct("=/"));
            self.pos += 2;
            match self.alt(false) {
                Ok(expr) => {
                    if !(self.pos >= self.toks.len()
                        || self.at_punct(";")
                        || (self.at_punct(".") && self.notation != Notation::Peg)
                        || self.at_rule_start())
                    {
                        let line = self.line();
                        self.cx
                            .warn(line, format!("unexpected text in rule `{name}`"));
                        self.resync();
                    } else if self.at_punct(";") || self.at_punct(".") {
                        self.pos += 1;
                    }
                    self.cx.limits.check_label(&name)?;
                    match rules
                        .iter_mut()
                        .find(|r| r.name == name)
                        .filter(|_| incremental)
                    {
                        Some(rule) => {
                            let old = std::mem::replace(&mut rule.expr, Expr::Skip);
                            let mut branches = match old {
                                Expr::Choice(b) => b,
                                other => vec![other],
                            };
                            match expr {
                                Expr::Choice(b) => branches.extend(b),
                                other => branches.push(other),
                            }
                            rule.expr = Expr::Choice(branches);
                        }
                        None => rules.push(Rule { name, expr }),
                    }
                }
                Err(Fail::Syntax(line, message)) => {
                    self.cx
                        .warn(line, format!("rule `{name}` skipped: {message}"));
                    self.resync();
                }
                Err(Fail::Fatal(e)) => return Err(e),
            }
        }
        Ok(rules)
    }

    /// Skip to just past the next `;` or to the next rule start.
    fn resync(&mut self) {
        self.pos += 1;
        while self.pos < self.toks.len() {
            if self.at_punct(";") {
                self.pos += 1;
                return;
            }
            if self.at_rule_start() {
                return;
            }
            self.pos += 1;
        }
    }

    fn alt(&mut self, in_call: bool) -> Result<Expr, Fail> {
        let mut branches = vec![self.seq(in_call)?];
        while self.at_punct("|") || self.at_punct("/") {
            self.pos += 1;
            branches.push(self.seq(in_call)?);
        }
        if branches.len() == 1 {
            return Ok(branches.pop().unwrap_or(Expr::Skip));
        }
        self.count(Expr::Choice(branches))
    }

    fn seq(&mut self, in_call: bool) -> Result<Expr, Fail> {
        let mut items = Vec::new();
        loop {
            match self.peek() {
                None => break,
                Some(Tok::Punct(")" | "]" | "}" | "|" | "/" | ";")) => break,
                Some(Tok::Punct(".")) if self.notation != Notation::Peg => break,
                Some(Tok::Punct(",")) => {
                    if in_call {
                        break;
                    }
                    self.pos += 1;
                }
                _ if self.at_rule_start() => break,
                _ => items.push(self.term()?),
            }
        }
        match items.len() {
            0 => Ok(Expr::Skip),
            1 => Ok(items.pop().unwrap_or(Expr::Skip)),
            _ => self.count(Expr::Sequence(items)),
        }
    }

    fn term(&mut self) -> Result<Expr, Fail> {
        if self.notation == Notation::Abnf {
            return self.abnf_term();
        }
        let predicate = match self.peek() {
            Some(Tok::Punct("!")) => Some("not"),
            Some(Tok::Punct("&")) => Some("and"),
            _ => None,
        };
        if predicate.is_some() {
            self.pos += 1;
        }
        let mut expr = self.primary()?;
        let mut marks = 0;
        loop {
            if marks == 4 {
                return self.fail("too many repetition marks");
            }
            marks += 1;
            expr = match self.peek() {
                Some(Tok::Punct("*")) => zero_or_more(expr, None),
                Some(Tok::Punct("+")) => one_or_more(expr, None, None),
                Some(Tok::Punct("?")) => Expr::Optional(Box::new(expr)),
                _ => break,
            };
            self.pos += 1;
            expr = self.count(expr)?;
        }
        if self.notation == Notation::Ebnf && self.at_punct("-") {
            self.pos += 1;
            let except = self.primary()?;
            expr = self.count(Expr::Sequence(vec![
                expr,
                Expr::Comment("except".into()),
                except,
            ]))?;
        }
        match predicate {
            Some(label) => self.count(Expr::Group(Box::new(expr), Some(label.into()))),
            None => Ok(expr),
        }
    }

    /// `[min] "*" [max] element` or `n element`.
    fn abnf_term(&mut self) -> Result<Expr, Fail> {
        let mut min = None;
        let mut star = false;
        let mut max = None;
        if let Some(Tok::Num(n)) = self.peek() {
            let n = *n;
            let next = self.toks.get(self.pos + 1);
            let repeat = next.is_some_and(|t| {
                t.glued
                    && (t.tok == Tok::Punct("*")
                        || !matches!(t.tok, Tok::Punct(_))
                        || t.tok == Tok::Punct("(")
                        || t.tok == Tok::Punct("["))
            });
            if repeat {
                min = Some(n);
                self.pos += 1;
            }
        }
        if self.at_punct("*") {
            star = true;
            self.pos += 1;
            if let Some(Tok::Num(n)) = self.peek()
                && self.toks[self.pos].glued
            {
                max = Some(*n);
                self.pos += 1;
            }
        }
        let item = self.primary()?;
        let (lo, hi) = match (min, star, max) {
            (None, false, _) => return Ok(item),
            (Some(n), false, _) => (n, Some(n)),
            (lo, true, hi) => (lo.unwrap_or(0), hi),
        };
        let expr = match (lo, hi) {
            (_, Some(0)) => Expr::Skip,
            (1, Some(1)) => return Ok(item),
            (0, Some(1)) => Expr::Optional(Box::new(item)),
            (0, None) => zero_or_more(item, None),
            (1, None) => one_or_more(item, None, None),
            (0, Some(m)) => Expr::Optional(Box::new(one_or_more(
                item,
                None,
                Some(format!("at most {m}")),
            ))),
            (n, None) => one_or_more(item, None, Some(format!("at least {n}"))),
            (n, Some(m)) if n == m => one_or_more(item, None, Some(format!("{n} times"))),
            (n, Some(m)) => one_or_more(item, None, Some(format!("{n} to {m} times"))),
        };
        self.count(expr)
    }

    fn primary(&mut self) -> Result<Expr, Fail> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(Fail::Fatal(DiagramError::TooLarge(LimitKind::ClusterDepth)));
        }
        let result = self.primary_inner();
        self.depth -= 1;
        result
    }

    fn primary_inner(&mut self) -> Result<Expr, Fail> {
        let Some(token) = self.toks.get(self.pos).cloned() else {
            return self.fail("the rule ends early");
        };
        self.pos += 1;
        let expr = match token.tok {
            Tok::Str(s) => {
                self.cx.limits.check_label(&s)?;
                Expr::Terminal(s)
            }
            Tok::Special(s) => {
                self.cx.limits.check_label(&s)?;
                Expr::Special(s)
            }
            Tok::Num(n) => Expr::Terminal(n.to_string()),
            Tok::Ident(name) => {
                self.cx.limits.check_label(&name)?;
                let call = self
                    .toks
                    .get(self.pos)
                    .is_some_and(|t| t.glued && t.tok == Tok::Punct("("));
                if call && let Some(f) = Function::named(&name) {
                    self.pos += 1;
                    return self.call(f, &name);
                }
                Expr::NonTerminal(name)
            }
            Tok::Punct("(") => {
                let inner = self.alt(false)?;
                self.expect(")")?;
                return Ok(inner);
            }
            Tok::Punct("[") => {
                let inner = self.alt(false)?;
                self.expect("]")?;
                Expr::Optional(Box::new(inner))
            }
            Tok::Punct("{") => {
                let inner = self.alt(false)?;
                self.expect("}")?;
                zero_or_more(inner, None)
            }
            Tok::Punct(".") if self.notation == Notation::Peg => {
                Expr::Special("any character".into())
            }
            Tok::Punct(p) => {
                self.pos -= 1;
                return self.fail(format!("unexpected `{p}`"));
            }
        };
        self.count(expr)
    }

    fn expect(&mut self, p: &str) -> Result<(), Fail> {
        if self.at_punct(p) {
            self.pos += 1;
            Ok(())
        } else {
            self.fail(format!("expected `{p}`"))
        }
    }

    fn call(&mut self, f: Function, name: &str) -> Result<Expr, Fail> {
        let mut args = Vec::new();
        while !self.at_punct(")") {
            if self.pos >= self.toks.len() {
                return self.fail(format!("`{name}(` is not closed"));
            }
            // Numeric arguments (a choice's default index) only steer
            // drawing libraries; they draw nothing.
            let numeric = matches!(self.peek(), Some(Tok::Num(_)))
                && matches!(
                    self.toks.get(self.pos + 1).map(|t| &t.tok),
                    Some(Tok::Punct("," | ")"))
                );
            if numeric {
                self.pos += 1;
            } else {
                args.push(self.alt(true)?);
            }
            if self.at_punct(",") {
                self.pos += 1;
            } else if !self.at_punct(")") {
                return self.fail(format!("expected `,` or `)` in `{name}(...)`"));
            }
        }
        self.pos += 1;
        let text = |e: Option<&Expr>| -> String {
            match e {
                Some(
                    Expr::Terminal(s) | Expr::NonTerminal(s) | Expr::Special(s) | Expr::Comment(s),
                ) => s.clone(),
                _ => String::new(),
            }
        };
        let mut args = args.into_iter();
        let expr = match f {
            Function::Sequence => Expr::Sequence(args.collect()),
            Function::OptionalSequence => {
                Expr::Sequence(args.map(|a| Expr::Optional(Box::new(a))).collect())
            }
            Function::AlternatingSequence => {
                let a = args.next().unwrap_or(Expr::Skip);
                one_or_more(a, args.next(), None)
            }
            Function::Choice => Expr::Choice(args.collect()),
            Function::MultipleChoice => {
                let mut all: Vec<Expr> = args.collect();
                if matches!(all.first(), Some(Expr::Terminal(t)) if t == "any" || t == "all") {
                    all.remove(0);
                }
                Expr::Choice(all)
            }
            Function::Optional => Expr::Optional(Box::new(args.next().unwrap_or(Expr::Skip))),
            Function::ZeroOrMore => {
                let item = args.next().unwrap_or(Expr::Skip);
                zero_or_more(item, args.next())
            }
            Function::OneOrMore => {
                let item = args.next().unwrap_or(Expr::Skip);
                one_or_more(item, args.next(), None)
            }
            Function::Terminal => Expr::Terminal(text(args.next().as_ref())),
            Function::NonTerminal => Expr::NonTerminal(text(args.next().as_ref())),
            Function::Comment => Expr::Comment(text(args.next().as_ref())),
            Function::Skip => Expr::Skip,
            Function::Group => {
                let item = args.next().unwrap_or(Expr::Skip);
                let label = text(args.next().as_ref());
                Expr::Group(Box::new(item), Some(label).filter(|l| !l.is_empty()))
            }
        };
        let expr = match expr {
            Expr::Sequence(mut items) | Expr::Choice(mut items) if items.len() <= 1 => {
                items.pop().unwrap_or(Expr::Skip)
            }
            other => other,
        };
        self.count(expr)
    }
}

#[derive(Debug, Clone, Copy)]
enum Function {
    Sequence,
    OptionalSequence,
    AlternatingSequence,
    Choice,
    MultipleChoice,
    Optional,
    ZeroOrMore,
    OneOrMore,
    Terminal,
    NonTerminal,
    Comment,
    Skip,
    Group,
}

impl Function {
    fn named(name: &str) -> Option<Self> {
        Some(match name.to_ascii_lowercase().as_str() {
            "sequence" | "stack" | "diagram" | "complexdiagram" => Self::Sequence,
            "optionalsequence" => Self::OptionalSequence,
            "alternatingsequence" => Self::AlternatingSequence,
            "choice" | "horizontalchoice" => Self::Choice,
            "multiplechoice" => Self::MultipleChoice,
            "optional" => Self::Optional,
            "zeroormore" => Self::ZeroOrMore,
            "oneormore" => Self::OneOrMore,
            "terminal" => Self::Terminal,
            "nonterminal" => Self::NonTerminal,
            "comment" => Self::Comment,
            "skip" | "start" | "end" => Self::Skip,
            "group" => Self::Group,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules(src: &str) -> Vec<Rule> {
        let (model, warnings, _) = crate::families::parse_for_test(src, parse);
        assert!(warnings.is_empty(), "{warnings:?}");
        model.unwrap().rules
    }

    #[test]
    fn ebnf_groups_repeat_and_choose() {
        let r = rules(
            "railroad-ebnf-beta\nexpr = term ( ( \"+\" | \"-\" ) term )* ;\nlist = \"[\" [ item { \",\" item } ] \"]\" .",
        );
        assert_eq!(r.len(), 2);
        let Expr::Sequence(items) = &r[0].expr else {
            panic!("{:?}", r[0].expr)
        };
        assert_eq!(items[0], Expr::NonTerminal("term".into()));
        assert!(
            matches!(&items[1], Expr::Optional(inner) if matches!(**inner, Expr::OneOrMore { .. }))
        );
    }

    #[test]
    fn abnf_reads_repetition_and_comments() {
        let r = rules(
            "railroad-abnf-beta\nlabel = 1*( ALPHA / DIGIT / \"-\" ) ; a comment\ndomain = label *( \".\" label )\nx = 2*4DIGIT %x41-5A <prose>",
        );
        assert_eq!(r.len(), 3);
        assert!(matches!(&r[0].expr, Expr::OneOrMore { note: None, .. }));
        let Expr::Sequence(items) = &r[2].expr else {
            panic!()
        };
        assert!(matches!(&items[0], Expr::OneOrMore { note: Some(n), .. } if n == "2 to 4 times"));
        assert_eq!(items[1], Expr::Special("%x41-5A".into()));
    }

    #[test]
    fn peg_reads_predicates_classes_and_unterminated_rules() {
        let r = rules(
            "railroad-peg-beta\nId <- !Keyword [a-z_] [a-z0-9_]* .\nKeyword <- \"if\" / \"else\"",
        );
        assert_eq!(r.len(), 2);
        let Expr::Sequence(items) = &r[0].expr else {
            panic!()
        };
        assert!(matches!(&items[0], Expr::Group(_, Some(l)) if l == "not"));
        assert_eq!(items[1], Expr::Special("[a-z_]".into()));
        assert_eq!(items[3], Expr::Special("any character".into()));
    }

    #[test]
    fn constructor_calls_build_the_same_tree() {
        let r = rules(
            "railroad-beta\ntitle Expression\ne = sequence(nonterminal(\"term\"), zeroOrMore(sequence(choice(0, terminal(\"+\"), terminal(\"-\")), nonterminal(\"term\")))) ;",
        );
        let Expr::Sequence(items) = &r[0].expr else {
            panic!()
        };
        assert_eq!(items[0], Expr::NonTerminal("term".into()));
        assert!(matches!(&items[1], Expr::Optional(_)));
    }
}
