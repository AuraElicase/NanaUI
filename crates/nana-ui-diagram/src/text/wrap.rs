//! Greedy line filling on Unicode break opportunities with measured widths.

use std::collections::BTreeSet;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;

use super::{FontSpec, Label, Measurer};

/// A run placed on a line.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidRun {
    pub text: Arc<str>,
    /// Offset from the line's start.
    pub x: f32,
    pub width: f32,
    pub font: FontSpec,
    pub code: bool,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaidLine {
    pub runs: Vec<LaidRun>,
    pub width: f32,
}

/// A label broken into measured lines.
#[derive(Debug, Clone, PartialEq)]
pub struct LaidText {
    pub lines: Vec<LaidLine>,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    pub font: FontSpec,
}

impl LaidText {
    pub fn size(&self) -> [f32; 2] {
        [self.width, self.height]
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.runs.is_empty())
    }
}

struct Glyph {
    run: usize,
    start: usize,
    end: usize,
    advance: f32,
    space: bool,
}

pub(super) fn lay(
    m: &Measurer<'_>,
    label: &Label,
    font: FontSpec,
    max_width: Option<f32>,
) -> LaidText {
    let line_height = m.line_advance(font);
    let max_width = max_width.filter(|w| w.is_finite() && *w > 0.0);
    let mut lines = Vec::new();
    for hard in &label.lines {
        let fonts: Vec<FontSpec> = hard
            .iter()
            .map(|run| FontSpec {
                weight: if run.bold {
                    font.weight.max(700)
                } else {
                    font.weight
                },
                italic: font.italic || run.italic,
                mono: font.mono || run.code,
                ..font
            })
            .collect();
        let mut glyphs = Vec::new();
        let mut concat = String::new();
        let mut offsets = Vec::new();
        for (index, run) in hard.iter().enumerate() {
            let advances = m.advances(&run.text, fonts[index]);
            let base = concat.len();
            offsets.push(base);
            concat.push_str(&run.text);
            for ((start, g), advance) in run
                .text
                .grapheme_indices(true)
                .zip(advances.iter().copied())
            {
                glyphs.push(Glyph {
                    run: index,
                    start: base + start,
                    end: base + start + g.len(),
                    advance,
                    space: g.chars().all(char::is_whitespace),
                });
            }
        }
        let breaks: BTreeSet<usize> = unicode_linebreak::linebreaks(&concat)
            .map(|(at, _)| at)
            .collect();
        let mut start = 0;
        let mut width = 0.0;
        let mut last_break: Option<usize> = None;
        for i in 0..glyphs.len() {
            if i > start && breaks.contains(&glyphs[i].start) {
                last_break = Some(i);
            }
            width += glyphs[i].advance;
            let Some(max) = max_width else { continue };
            if width > max && i > start && !glyphs[i].space {
                let cut = match last_break {
                    Some(b) if b > start => b,
                    _ => i,
                };
                lines.push(build_line(
                    hard,
                    &fonts,
                    &concat,
                    &offsets,
                    &glyphs[start..cut],
                ));
                start = cut;
                width = glyphs[start..=i].iter().map(|g| g.advance).sum();
                last_break = None;
            }
        }
        lines.push(build_line(
            hard,
            &fonts,
            &concat,
            &offsets,
            &glyphs[start..],
        ));
    }
    if lines.is_empty() {
        lines.push(LaidLine::default());
    }
    let width = lines.iter().map(|l| l.width).fold(0.0, f32::max);
    LaidText {
        height: line_height * lines.len() as f32,
        lines,
        width,
        line_height,
        font,
    }
}

fn build_line(
    hard: &[super::Run],
    fonts: &[FontSpec],
    concat: &str,
    offsets: &[usize],
    glyphs: &[Glyph],
) -> LaidLine {
    let first = glyphs.iter().position(|g| !g.space);
    let last = glyphs.iter().rposition(|g| !g.space);
    let (Some(first), Some(last)) = (first, last) else {
        return LaidLine::default();
    };
    let glyphs = &glyphs[first..=last];
    let mut runs: Vec<LaidRun> = Vec::new();
    let mut x = 0.0;
    let mut i = 0;
    while i < glyphs.len() {
        let run = glyphs[i].run;
        let mut j = i;
        let mut width = 0.0;
        while j < glyphs.len() && glyphs[j].run == run {
            width += glyphs[j].advance;
            j += 1;
        }
        let text = &concat[glyphs[i].start..glyphs[j - 1].end];
        debug_assert!(glyphs[i].start >= offsets[run]);
        runs.push(LaidRun {
            text: Arc::from(text),
            x,
            width,
            font: fonts[run],
            code: hard[run].code,
        });
        x += width;
        i = j;
    }
    LaidLine { runs, width: x }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::FixedMeasure;

    fn measurer() -> (FixedMeasure,) {
        (FixedMeasure,)
    }

    #[test]
    fn wraps_at_word_breaks_and_trims_spaces() {
        let (fixed,) = measurer();
        let m = Measurer::new(&fixed, 1.4);
        let font = FontSpec::new(10.0);
        let laid = m.lay(&Label::plain("alpha beta gamma"), font, Some(50.0));
        let texts: Vec<String> = laid
            .lines
            .iter()
            .map(|l| l.runs.iter().map(|r| r.text.to_string()).collect())
            .collect();
        assert_eq!(texts, ["alpha", "beta", "gamma"]);
        assert!((laid.height - 3.0 * 14.0).abs() < 1e-3);
    }

    #[test]
    fn cjk_breaks_between_characters_and_keeps_runs_styled() {
        let fixed = FixedMeasure;
        let m = Measurer::new(&fixed, 1.4);
        let laid = m.lay(
            &Label::markdown("**输入**输出结果"),
            FontSpec::new(10.0),
            Some(35.0),
        );
        assert!(laid.lines.len() >= 2);
        assert_eq!(laid.lines[0].runs[0].font.weight, 700);
        assert!(laid.width <= 35.0 + 1e-3);
    }

    #[test]
    fn unbreakable_words_break_at_graphemes() {
        let fixed = FixedMeasure;
        let m = Measurer::new(&fixed, 1.4);
        let laid = m.lay(
            &Label::plain("abcdefghijkl"),
            FontSpec::new(10.0),
            Some(30.0),
        );
        assert!(laid.lines.len() >= 3);
    }
}
