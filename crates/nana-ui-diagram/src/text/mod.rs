//! Text: the caller's measurement, the label model and line wrapping.

mod label;
mod wrap;

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use unicode_segmentation::UnicodeSegmentation;

pub use label::{Label, Run};
pub use wrap::{LaidLine, LaidRun, LaidText};

/// The face a run of text is set in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontSpec {
    pub size: f32,
    pub weight: u16,
    pub italic: bool,
    /// The monospace family, for code-like labels.
    pub mono: bool,
}

impl FontSpec {
    pub const fn new(size: f32) -> Self {
        Self {
            size,
            weight: 400,
            italic: false,
            mono: false,
        }
    }

    pub const fn weight(mut self, weight: u16) -> Self {
        self.weight = weight;
        self
    }

    pub const fn italic(mut self, italic: bool) -> Self {
        self.italic = italic;
        self
    }

    fn key(self) -> (u32, u16, bool, bool) {
        (self.size.to_bits(), self.weight, self.italic, self.mono)
    }
}

/// Measures text the way it will be drawn.
pub trait TextMeasure {
    /// Identity of the fonts and shaping behind [`Self::advances`]. Layout
    /// caches include it, so a font change re-lays every diagram.
    fn key(&self) -> u64;
    /// One advance per extended grapheme cluster of `text`, shaped as a
    /// single run.
    fn advances(&self, text: &str, font: FontSpec) -> Vec<f32>;
}

/// Deterministic advances that need no font: tests, goldens and hosts
/// without a text engine. East Asian wide characters take a full em, every
/// other grapheme 0.58 em, bold 6% more.
#[derive(Debug, Clone, Copy, Default)]
pub struct FixedMeasure;

impl TextMeasure for FixedMeasure {
    fn key(&self) -> u64 {
        0
    }

    fn advances(&self, text: &str, font: FontSpec) -> Vec<f32> {
        let bold = if font.weight >= 600 { 1.06 } else { 1.0 };
        text.graphemes(true)
            .map(|g| {
                let c = g.chars().next().unwrap_or(' ');
                let em = if c.is_control() {
                    0.0
                } else if is_wide(c) {
                    1.0
                } else if font.mono {
                    0.6
                } else {
                    0.58
                };
                em * font.size * bold
            })
            .collect()
    }
}

pub(crate) fn is_wide(c: char) -> bool {
    let c = u32::from(c);
    (0x1100..=0x115f).contains(&c)
        || (0x2e80..=0xa4cf).contains(&c)
        || (0xac00..=0xd7a3).contains(&c)
        || (0xf900..=0xfaff).contains(&c)
        || (0xfe30..=0xfe4f).contains(&c)
        || (0xff00..=0xff60).contains(&c)
        || (0xffe0..=0xffe6).contains(&c)
        || (0x1f300..=0x1faff).contains(&c)
        || (0x20000..=0x3fffd).contains(&c)
}

type MemoKey = (Arc<str>, (u32, u16, bool, bool));

/// A [`TextMeasure`] with a per-layout memo: diagrams measure the same
/// short strings many times while wrapping and fitting.
pub struct Measurer<'a> {
    inner: &'a dyn TextMeasure,
    memo: RefCell<HashMap<MemoKey, Arc<[f32]>>>,
    /// Line advance as a multiple of the font size.
    pub line_height: f32,
}

impl<'a> Measurer<'a> {
    pub fn new(inner: &'a dyn TextMeasure, line_height: f32) -> Self {
        Self {
            inner,
            memo: RefCell::new(HashMap::new()),
            line_height,
        }
    }

    pub fn advances(&self, text: &str, font: FontSpec) -> Arc<[f32]> {
        let key: MemoKey = (Arc::from(text), font.key());
        if let Some(hit) = self.memo.borrow().get(&key) {
            return hit.clone();
        }
        let mut values = self.inner.advances(text, font);
        for v in &mut values {
            if !v.is_finite() || *v < 0.0 {
                *v = 0.0;
            }
        }
        let values: Arc<[f32]> = values.into();
        self.memo.borrow_mut().insert(key, values.clone());
        values
    }

    /// Width of `text` on one line.
    pub fn width(&self, text: &str, font: FontSpec) -> f32 {
        self.advances(text, font).iter().sum()
    }

    pub fn line_advance(&self, font: FontSpec) -> f32 {
        font.size * self.line_height
    }

    /// Lay a label out, wrapping lines longer than `max_width`.
    pub fn lay(&self, label: &Label, font: FontSpec, max_width: Option<f32>) -> LaidText {
        wrap::lay(self, label, font, max_width)
    }

    /// One line of plain text, never wrapped.
    pub fn lay_plain(&self, text: &str, font: FontSpec) -> LaidText {
        wrap::lay(self, &Label::plain(text), font, None)
    }
}
