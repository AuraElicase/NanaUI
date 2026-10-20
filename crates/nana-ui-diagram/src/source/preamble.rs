//! Frontmatter, `%%{init}%%` directives, `%%` comments and the
//! `accTitle` / `accDescr` statements every diagram type accepts.

use super::{Config, Preprocessed, entities};
use crate::{DiagramError, Meta};

pub fn preprocess(source: &str) -> Result<Preprocessed, DiagramError> {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let lines: Vec<&str> = source.lines().collect();
    let mut meta = Meta::default();
    let mut config = Config::default();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut i = 0;

    // Frontmatter: a leading `---` block.
    let first = lines.iter().position(|l| !l.trim().is_empty());
    if let Some(start) = first
        && lines[start].trim() == "---"
        && let Some(len) = lines[start + 1..].iter().position(|l| l.trim() == "---")
    {
        let block = &lines[start + 1..start + 1 + len];
        frontmatter(block, &mut meta, &mut config);
        for _ in 0..=start + len + 1 {
            out.push(String::new());
        }
        i = start + len + 2;
    }

    let mut descr: Option<String> = None;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();
        i += 1;
        if let Some(text) = descr.as_mut() {
            if let Some(end) = trimmed.find('}') {
                let piece = trimmed[..end].trim();
                if !piece.is_empty() {
                    if !text.is_empty() {
                        text.push('\n');
                    }
                    text.push_str(piece);
                }
                meta.acc_descr = descr.take();
            } else if !trimmed.is_empty() {
                if !text.is_empty() {
                    text.push('\n');
                }
                text.push_str(trimmed);
            }
            out.push(String::new());
            continue;
        }
        if let Some(directive) = trimmed.strip_prefix("%%{") {
            let body = directive.strip_suffix("}%%").unwrap_or(directive);
            init_directive(body, &mut config);
            out.push(String::new());
            continue;
        }
        if trimmed.starts_with("%%") {
            out.push(String::new());
            continue;
        }
        if let Some(rest) = keyword(trimmed, "accTitle")
            && let Some(value) = rest.strip_prefix(':')
        {
            meta.acc_title = Some(entities::decode(value.trim()));
            out.push(String::new());
            continue;
        }
        if let Some(rest) = keyword(trimmed, "accDescr") {
            let rest = rest.trim_start();
            if let Some(value) = rest.strip_prefix(':') {
                meta.acc_descr = Some(entities::decode(value.trim()));
                out.push(String::new());
                continue;
            }
            if let Some(body) = rest.strip_prefix('{') {
                if let Some(end) = body.find('}') {
                    meta.acc_descr = Some(entities::decode(body[..end].trim()));
                } else {
                    descr = Some(body.trim().to_string());
                }
                out.push(String::new());
                continue;
            }
        }
        // A trailing `%%` comment on a statement line.
        out.push(strip_trailing_comment(line).to_string());
    }

    let body = out.join("\n");
    if body.trim().is_empty() {
        return Err(DiagramError::Empty);
    }
    Ok(Preprocessed {
        body,
        body_line: 1,
        meta,
        config,
    })
}

fn keyword<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(word)?;
    rest.chars()
        .next()
        .is_none_or(|c| c.is_whitespace() || c == ':' || c == '{')
        .then_some(rest)
}

fn strip_trailing_comment(line: &str) -> &str {
    let mut quote = false;
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => quote = !quote,
            b'%' if !quote && bytes.get(i + 1) == Some(&b'%') && i > 0 => {
                let before = &line[..i];
                if before.ends_with(char::is_whitespace) {
                    return before.trim_end();
                }
            }
            _ => {}
        }
    }
    line
}

/// The YAML subset frontmatter uses: `key: value` pairs nested by indent.
fn frontmatter(block: &[&str], meta: &mut Meta, config: &mut Config) {
    let mut stack: Vec<(usize, String)> = Vec::new();
    for raw in block {
        let line = raw.split(" #").next().unwrap_or(raw);
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let key = unquote(key.trim()).to_string();
        let value = unquote(value.trim());
        while stack.last().is_some_and(|(depth, _)| *depth >= indent) {
            stack.pop();
        }
        let path: Vec<&str> = stack
            .iter()
            .map(|(_, k)| k.as_str())
            .chain(std::iter::once(key.as_str()))
            .collect();
        if value.is_empty() {
            stack.push((indent, key));
            continue;
        }
        match path.as_slice() {
            ["title"] => meta.title = Some(entities::decode(value)),
            ["config", rest @ ..] if !rest.is_empty() => {
                config.set(rest.join("."), value.to_string())
            }
            _ => {}
        }
    }
}

/// `init: { ... }` / `initialize: { ... }`, or a bare object.
fn init_directive(body: &str, config: &mut Config) {
    let body = body.trim();
    let object = ["init", "initialize"]
        .iter()
        .find_map(|k| {
            let rest = body.strip_prefix(k)?.trim_start();
            Some(rest.strip_prefix(':').unwrap_or(rest).trim_start())
        })
        .unwrap_or(body);
    let mut parser = Lenient {
        chars: object.chars().collect(),
        pos: 0,
    };
    parser.object(String::new(), config, 0);
}

fn unquote(value: &str) -> &str {
    let value = value.trim();
    for q in ['"', '\''] {
        if let Some(inner) = value.strip_prefix(q).and_then(|v| v.strip_suffix(q)) {
            return inner;
        }
    }
    value
}

/// A forgiving JSON reader: unquoted keys, single quotes and trailing commas.
struct Lenient {
    chars: Vec<char>,
    pos: usize,
}

impl Lenient {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn ws(&mut self) {
        while self.peek().is_some_and(|c| c.is_whitespace() || c == ',') {
            self.pos += 1;
        }
    }

    fn object(&mut self, prefix: String, config: &mut Config, depth: usize) {
        self.ws();
        if self.peek() != Some('{') || depth > 8 {
            return;
        }
        self.pos += 1;
        loop {
            self.ws();
            match self.peek() {
                None => return,
                Some('}') => {
                    self.pos += 1;
                    return;
                }
                _ => {}
            }
            let key = self.scalar();
            self.ws();
            if self.peek() == Some(':') {
                self.pos += 1;
            }
            self.ws();
            let path = if prefix.is_empty() {
                key
            } else {
                format!("{prefix}.{key}")
            };
            match self.peek() {
                Some('{') => self.object(path, config, depth + 1),
                Some('[') => self.skip_array(),
                Some(_) => {
                    let value = self.scalar();
                    config.set(path, value);
                }
                None => return,
            }
        }
    }

    fn skip_array(&mut self) {
        let mut depth = 0;
        let mut quote: Option<char> = None;
        while let Some(c) = self.peek() {
            self.pos += 1;
            match (quote, c) {
                (Some(q), c) if c == q => quote = None,
                (Some(_), _) => {}
                (None, '"' | '\'') => quote = Some(c),
                (None, '[' | '{') => depth += 1,
                (None, ']' | '}') => {
                    depth -= 1;
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    fn scalar(&mut self) -> String {
        let mut out = String::new();
        if let Some(q @ ('"' | '\'')) = self.peek() {
            self.pos += 1;
            while let Some(c) = self.peek() {
                self.pos += 1;
                if c == '\\' {
                    if let Some(n) = self.peek() {
                        out.push(n);
                        self.pos += 1;
                    }
                } else if c == q {
                    break;
                } else {
                    out.push(c);
                }
            }
            return out;
        }
        while let Some(c) = self.peek() {
            if matches!(c, ':' | ',' | '}' | ']' | '{' | '[') {
                break;
            }
            out.push(c);
            self.pos += 1;
        }
        out.trim().to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frontmatter_and_init_flatten_into_config() {
        let pre = preprocess(
            "---\ntitle: Flow\nconfig:\n  flowchart:\n    curve: linear\n---\n%%{init: {'flowchart': {'wrappingWidth': 120}, theme: 'dark'}}%%\nflowchart LR\n  A-->B %% trailing\n",
        )
        .unwrap();
        assert_eq!(pre.meta.title.as_deref(), Some("Flow"));
        assert_eq!(pre.config.get("flowchart.curve"), Some("linear"));
        assert_eq!(pre.config.get_f32("flowchart.wrappingWidth"), Some(120.0));
        let lines: Vec<&str> = pre.body.lines().collect();
        assert_eq!(lines[7], "flowchart LR");
        assert_eq!(lines[8], "  A-->B");
    }

    #[test]
    fn accessibility_statements_are_read_and_blanked() {
        let pre =
            preprocess("graph TD\naccTitle: Big\naccDescr {\n  line one\n  line two\n}\nA-->B")
                .unwrap();
        assert_eq!(pre.meta.acc_title.as_deref(), Some("Big"));
        assert_eq!(pre.meta.acc_descr.as_deref(), Some("line one\nline two"));
        assert_eq!(pre.body.lines().filter(|l| !l.is_empty()).count(), 2);
    }

    #[test]
    fn comment_only_source_is_empty() {
        assert_eq!(
            preprocess("%% nothing\n\n").unwrap_err(),
            DiagramError::Empty
        );
    }
}
