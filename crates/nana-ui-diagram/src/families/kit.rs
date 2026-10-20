//! Small pieces every family can use: header and statement splitting,
//! words, quotes, identifiers and brackets, numbers, warning excerpts and
//! the series legend.

use crate::DiagramError;
use crate::families::LayoutCx;
use crate::ir::{Align, Corner, Geom, Ink, Rect, SceneBuilder, Tone};
use crate::kind;
use crate::style;
use crate::text::LaidText;

/// The header's line index and the text after its keyword, or `Empty`.
pub(crate) fn header(body: &str) -> Result<(usize, &str), DiagramError> {
    for (index, line) in body.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        return match kind::header(trimmed) {
            Some((_, rest)) => Ok((index, rest.trim_start_matches(':').trim())),
            None => Err(DiagramError::syntax(
                index as u32 + 1,
                1,
                "expected a diagram header",
            )),
        };
    }
    Err(DiagramError::Empty)
}

/// Lines after the header: `(index, trimmed text)`, blank lines skipped.
pub(crate) fn statements(body: &str, header: usize) -> impl Iterator<Item = (usize, &str)> {
    body.lines()
        .enumerate()
        .skip(header + 1)
        .map(|(i, l)| (i, l.trim()))
        .filter(|(_, l)| !l.is_empty())
}

/// `word rest` with `word` matched as a whole keyword.
pub(crate) fn keyword<'a>(line: &'a str, word: &str) -> Option<&'a str> {
    let rest = line.strip_prefix(word)?;
    if rest.is_empty() {
        return Some("");
    }
    rest.starts_with(char::is_whitespace).then(|| rest.trim())
}

/// The first whitespace-separated word and the rest, trimmed.
pub(crate) fn first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    match s.find(char::is_whitespace) {
        Some(i) => (&s[..i], s[i..].trim_start()),
        None => (s, ""),
    }
}

/// The first word, or a whole `"quoted words"` run without its quotes,
/// and the rest.
pub(crate) fn first_name(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    if let Some(body) = s.strip_prefix('"')
        && let Some(end) = body.find('"')
    {
        return (&body[..end], body[end + 1..].trim_start());
    }
    first_word(s)
}

/// The end of a quoted label at the start of `s`: past the closing `"`,
/// or past `` `" `` for a markdown string.
pub(crate) fn quoted_end(s: &str) -> Option<usize> {
    if let Some(body) = s.strip_prefix("\"`") {
        body.find("`\"").map(|e| e + 4)
    } else {
        s.strip_prefix('"')?.find('"').map(|e| e + 2)
    }
}

/// Byte length of the identifier at the start of `s`: letters, digits,
/// `_` and any of `extra`.
pub(crate) fn ident_len(s: &str, extra: &str) -> usize {
    s.char_indices()
        .find(|&(_, c)| !(c.is_alphanumeric() || c == '_' || extra.contains(c)))
        .map_or(s.len(), |(i, _)| i)
}

/// The byte index of the `close` that balances the `open` at index 0,
/// skipping quoted text.
pub(crate) fn balanced(s: &str, open: char, close: char) -> Option<usize> {
    let mut depth = 0i32;
    let mut quote = false;
    for (i, c) in s.char_indices() {
        match c {
            '"' => quote = !quote,
            c if c == open && !quote => depth += 1,
            c if c == close && !quote => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Strip one pair of matching quotes.
pub(crate) fn unquote(s: &str) -> &str {
    let s = s.trim();
    for q in ['"', '\''] {
        if s.len() >= 2 && s.starts_with(q) && s.ends_with(q) {
            return &s[1..s.len() - 1];
        }
    }
    s
}

/// Split on `sep` outside quotes, brackets and braces.
pub(crate) fn split_top(s: &str, sep: char) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None => match c {
                '"' => quote = Some(c),
                '[' | '{' | '(' => depth += 1,
                ']' | '}' | ')' => depth -= 1,
                c if c == sep && depth <= 0 => {
                    out.push(&s[start..i]);
                    start = i + c.len_utf8();
                }
                _ => {}
            },
        }
    }
    out.push(&s[start..]);
    out
}

pub(crate) fn number(s: &str) -> Option<f64> {
    s.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// `[a, b]` → `(a, b)`.
pub(crate) fn pair(s: &str) -> Option<(f64, f64)> {
    let inner = s.trim().strip_prefix('[')?.strip_suffix(']')?;
    let (a, b) = inner.split_once(',')?;
    Some((number(a)?, number(b)?))
}

/// A number the way an author would write it: no trailing zeros, at most
/// two decimals.
pub(crate) fn fmt_num(v: f64) -> String {
    if !v.is_finite() {
        return String::new();
    }
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" {
        "0".to_string()
    } else {
        s.to_string()
    }
}

/// The start of `s`, short enough to quote in a warning.
pub(crate) fn truncate(s: &str) -> String {
    s.chars().take(40).collect()
}

/// One legend row: the series swatch, its name and an optional value.
pub(crate) struct LegendRow {
    pub index: u16,
    pub name: LaidText,
    pub value: Option<LaidText>,
}

impl LegendRow {
    fn width(&self, swatch: f32, gap: f32) -> f32 {
        swatch + gap + self.name.width + self.value.as_ref().map_or(0.0, |v| gap * 0.6 + v.width)
    }
}

/// Legend rows stacked from `origin`, vertically centred on `center_y`.
/// Returns the legend's bounds.
pub(crate) fn legend(
    out: &mut SceneBuilder,
    cx: &LayoutCx<'_>,
    rows: &[LegendRow],
    left: f32,
    center_y: f32,
) -> Rect {
    if rows.is_empty() {
        return Rect::new(left, center_y, 0.0, 0.0);
    }
    let swatch = cx.metrics.small_size;
    let gap = cx.metrics.small_size * 0.6;
    let row_gap = cx.metrics.small_size * 0.45;
    let heights: Vec<f32> = rows.iter().map(|r| r.name.height.max(swatch)).collect();
    let total: f32 = heights.iter().sum::<f32>() + row_gap * (rows.len() - 1) as f32;
    let mut y = center_y - total / 2.0;
    let mut width: f32 = 0.0;
    for (row, h) in rows.iter().zip(&heights) {
        let line = row.name.line_height;
        let sw = Rect::new(left, y + (line.min(*h) - swatch) / 2.0, swatch, swatch);
        out.fill(
            Geom::Rect {
                rect: sw,
                corner: Corner::Tier(style::CHIP_CORNER),
            },
            Ink::Series {
                index: row.index,
                tone: Tone::Solid,
            },
        );
        let tx = left + swatch + gap;
        out.text(
            &row.name,
            [tx, y],
            row.name.width,
            Align::Start,
            style::TEXT,
        );
        if let Some(value) = &row.value {
            let vx = tx + row.name.width + gap * 0.6;
            out.text(value, [vx, y], value.width, Align::Start, style::MUTED);
        }
        width = width.max(row.width(swatch, gap));
        y += h + row_gap;
    }
    Rect::new(left, center_y - total / 2.0, width, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_read_and_write_numbers_and_quotes() {
        assert_eq!(fmt_num(65.30), "65.3");
        assert_eq!(fmt_num(386.0), "386");
        assert_eq!(fmt_num(0.004), "0");
        assert_eq!(unquote("\"a b\""), "a b");
        assert_eq!(
            split_top("a[\"x, y\"], b{1, 2}", ','),
            vec!["a[\"x, y\"]", " b{1, 2}"]
        );
        assert_eq!(pair("[0.3, 0.6]"), Some((0.3, 0.6)));
        assert_eq!(keyword("title  Hi", "title"), Some("Hi"));
        assert_eq!(keyword("titles", "title"), None);
    }

    /// Every family of this module group at its element bounds: each either
    /// lays out or reports `TooLarge`, and none takes long.
    #[test]
    fn large_inputs_finish() {
        use crate::{DiagramMetrics, FixedMeasure, render};
        let limits = crate::Limits::DEFAULT;
        let mut sources = Vec::new();
        let mut pie = String::from("pie showData\n");
        let mut quad = String::from("quadrantChart\n");
        let mut radar = String::from("radar-beta\naxis ");
        let mut sankey = String::from("sankey-beta\n");
        let mut venn = String::from("venn-beta\nset A\nset B\nset C\nset D\n");
        let mut packet = String::from("packet\n");
        let mut wardley = String::from("wardley-beta\n");
        let mut cynefin = String::from("cynefin-beta\ncomplex\n");
        for i in 0..1200 {
            pie.push_str(&format!("\"s{i}\" : {}\n", i % 7 + 1));
            quad.push_str(&format!(
                "p{i}: [{}, {}]\n",
                (i % 97) as f32 / 97.0,
                (i % 89) as f32 / 89.0
            ));
            radar.push_str(&format!("a{i}, "));
            sankey.push_str(&format!(
                "n{},n{},{}\n",
                i % 300,
                i % 300 + 1 + i % 5,
                i % 9 + 1
            ));
            venn.push_str(&format!("union A,B[\"u{i}\"]\n  text t{i}\n"));
            packet.push_str("+3: \"field with a long name\"\n");
            wardley.push_str(&format!(
                "component c{i} [{}, {}]\n",
                (i % 83) as f32 / 83.0,
                (i % 79) as f32 / 79.0
            ));
            if i > 0 {
                wardley.push_str(&format!("c{} -> c{i}\n", i - 1));
            }
            cynefin.push_str(&format!("\"item {i}\"\n"));
        }
        radar.push_str("z\ncurve c{1,2,3}\n");
        sources.extend([pie, quad, radar, sankey, venn, packet, wardley, cynefin]);
        for source in sources {
            let started = std::time::Instant::now();
            match render(&source, &FixedMeasure, &DiagramMetrics::DEFAULT, &limits) {
                Ok(_) | Err(DiagramError::TooLarge(_)) => {}
                Err(e) => panic!("{e}: {}", &source[..40]),
            }
            assert!(started.elapsed().as_secs() < 20, "{}", &source[..40]);
        }
    }
}
