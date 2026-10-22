//! Labels: hard lines of styled runs.

use crate::source::entities;

/// A styled piece of a label line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

impl Run {
    pub fn plain(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            bold: false,
            italic: false,
            code: false,
        }
    }
}

/// Text as authored: explicit line breaks are kept, wrapping happens later.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Label {
    pub lines: Vec<Vec<Run>>,
    /// A markdown string: wraps at the diagram's wrap width by default.
    pub markdown: bool,
}

impl Label {
    /// Plain text: entity codes, `<br>` breaks and the simple inline tags
    /// (`<b>`, `<strong>`, `<i>`, `<em>`, `<code>`). Other tags are dropped.
    pub fn plain(text: &str) -> Self {
        let text = entities::decode(text);
        let mut lines = Vec::new();
        let mut current = Vec::new();
        let mut state = (false, false, false);
        let mut buf = String::new();
        let mut rest = text.as_str();
        let flush = |buf: &mut String, current: &mut Vec<Run>, state: (bool, bool, bool)| {
            if !buf.is_empty() {
                current.push(Run {
                    text: std::mem::take(buf),
                    bold: state.0,
                    italic: state.1,
                    code: state.2,
                });
            }
        };
        while let Some(c) = rest.chars().next() {
            if c == '\n' {
                flush(&mut buf, &mut current, state);
                lines.push(std::mem::take(&mut current));
                rest = &rest[1..];
                continue;
            }
            if c == '<'
                && let Some(end) = rest.find('>')
            {
                let tag = rest[1..end].trim().to_ascii_lowercase();
                let name = tag.trim_start_matches('/').trim_end_matches('/').trim();
                let name = name.split_whitespace().next().unwrap_or("");
                let known = matches!(
                    name,
                    "br" | "b"
                        | "strong"
                        | "i"
                        | "em"
                        | "code"
                        | "u"
                        | "span"
                        | "p"
                        | "div"
                        | "small"
                        | "sub"
                        | "sup"
                        | "s"
                        | "del"
                );
                if known {
                    flush(&mut buf, &mut current, state);
                    let closing = tag.starts_with('/');
                    match name {
                        "br" => lines.push(std::mem::take(&mut current)),
                        "b" | "strong" => state.0 = !closing,
                        "i" | "em" => state.1 = !closing,
                        "code" => state.2 = !closing,
                        "p" | "div" if closing => lines.push(std::mem::take(&mut current)),
                        _ => {}
                    }
                    rest = &rest[end + 1..];
                    continue;
                }
            }
            buf.push(c);
            rest = &rest[c.len_utf8()..];
        }
        flush(&mut buf, &mut current, state);
        lines.push(current);
        trim_lines(&mut lines);
        Self {
            lines,
            markdown: false,
        }
    }

    /// A Mermaid markdown string (`` "`...`" ``): `**bold**`, `*italic*`,
    /// `_italic_`, `` `code` `` and newlines as breaks.
    pub fn markdown(text: &str) -> Self {
        let text = entities::decode(text);
        let mut lines = Vec::new();
        for raw in text.split('\n') {
            lines.push(markdown_line(raw.trim()));
        }
        trim_lines(&mut lines);
        Self {
            lines,
            markdown: true,
        }
    }

    /// Read an authored label: a `` "`...`" `` markdown string, a quoted
    /// string or bare text.
    pub fn parse(raw: &str) -> Self {
        let raw = raw.trim();
        if let Some(inner) = raw.strip_prefix("\"`").and_then(|r| r.strip_suffix("`\"")) {
            return Self::markdown(inner);
        }
        if let Some(inner) = raw.strip_prefix('"').and_then(|r| r.strip_suffix('"')) {
            return Self::plain(inner);
        }
        Self::plain(raw)
    }

    pub fn is_empty(&self) -> bool {
        self.lines
            .iter()
            .all(|line| line.iter().all(|run| run.text.trim().is_empty()))
    }

    /// The label as plain text, lines joined by spaces.
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.iter().map(|r| r.text.as_str()).collect::<String>())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

fn trim_lines(lines: &mut Vec<Vec<Run>>) {
    for line in lines.iter_mut() {
        if let Some(first) = line.first_mut() {
            first.text = first.text.trim_start().to_string();
        }
        if let Some(last) = line.last_mut() {
            last.text = last.text.trim_end().to_string();
        }
        line.retain(|r| !r.text.is_empty());
    }
    while lines.len() > 1 && lines.last().is_some_and(Vec::is_empty) {
        lines.pop();
    }
    while lines.len() > 1 && lines.first().is_some_and(Vec::is_empty) {
        lines.remove(0);
    }
}

fn markdown_line(text: &str) -> Vec<Run> {
    let mut runs = Vec::new();
    let mut buf = String::new();
    let (mut bold, mut italic) = (false, false);
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    let push = |buf: &mut String, runs: &mut Vec<Run>, bold: bool, italic: bool, code: bool| {
        if !buf.is_empty() {
            runs.push(Run {
                text: std::mem::take(buf),
                bold,
                italic,
                code,
            });
        }
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '`'
            && let Some(len) = chars[i + 1..].iter().position(|&d| d == '`')
        {
            push(&mut buf, &mut runs, bold, italic, false);
            buf.extend(&chars[i + 1..i + 1 + len]);
            push(&mut buf, &mut runs, bold, italic, true);
            i += len + 2;
            continue;
        }
        if (c == '*' || c == '_') && chars.get(i + 1) == Some(&c) {
            push(&mut buf, &mut runs, bold, italic, false);
            bold = !bold;
            i += 2;
            continue;
        }
        if c == '*' || (c == '_' && (i == 0 || !chars[i - 1].is_alphanumeric() || italic)) {
            push(&mut buf, &mut runs, bold, italic, false);
            italic = !italic;
            i += 1;
            continue;
        }
        buf.push(c);
        i += 1;
    }
    push(&mut buf, &mut runs, bold, italic, false);
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_labels_break_on_br_and_read_inline_tags() {
        let label = Label::plain("one<br/>two <b>bold</b> #quot;q#quot;");
        assert_eq!(label.lines.len(), 2);
        assert_eq!(label.lines[0][0].text, "one");
        assert_eq!(label.lines[1][1].text, "bold");
        assert!(label.lines[1][1].bold);
        assert_eq!(label.text(), "one two bold \"q\"");
    }

    #[test]
    fn markdown_strings_style_runs_and_keep_newlines() {
        let label = Label::parse("\"`**Bold** and *it*\nnext`\"");
        assert!(label.markdown);
        assert_eq!(label.lines.len(), 2);
        assert!(label.lines[0][0].bold);
        assert!(label.lines[0][2].italic);
        assert_eq!(label.lines[1][0].text, "next");
    }

    #[test]
    fn snake_case_words_keep_their_underscores() {
        let label = Label::markdown("snake_case_name");
        assert_eq!(label.text(), "snake_case_name");
    }
}
