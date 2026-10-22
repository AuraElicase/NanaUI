//! Author styling: the CSS-ish declarations behind `style`, `classDef` and
//! `linkStyle`. Only the properties a native drawing can honour are kept.

use super::color;

/// Declarations an author attached to a node, edge or class.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct AuthorStyle {
    pub fill: Option<[f32; 4]>,
    pub stroke: Option<[f32; 4]>,
    pub stroke_width: Option<f32>,
    pub dashed: Option<bool>,
    pub color: Option<[f32; 4]>,
    pub font_weight: Option<u16>,
    pub italic: Option<bool>,
}

impl AuthorStyle {
    /// Read `fill:#f9f,stroke:#333,stroke-width:4px,color:#fff`.
    pub fn parse(declarations: &str) -> Self {
        let mut style = Self::default();
        style.apply(declarations);
        style
    }

    pub fn apply(&mut self, declarations: &str) {
        for decl in split_declarations(declarations) {
            let Some((name, value)) = decl.split_once(':') else {
                continue;
            };
            let value = value.trim().trim_end_matches("!important").trim();
            match name.trim().to_ascii_lowercase().as_str() {
                "fill" | "background" | "background-color" => {
                    self.fill = color::parse(value).or(self.fill)
                }
                "stroke" | "border-color" => self.stroke = color::parse(value).or(self.stroke),
                "stroke-width" | "border-width" => {
                    self.stroke_width = value
                        .trim_end_matches("px")
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|w| w.is_finite() && *w >= 0.0)
                        .map(|w| w.min(12.0))
                        .or(self.stroke_width)
                }
                "stroke-dasharray" => {
                    self.dashed = Some(!matches!(value, "" | "0" | "none"));
                }
                "color" => self.color = color::parse(value).or(self.color),
                "font-weight" => {
                    self.font_weight = match value {
                        "bold" | "bolder" => Some(700),
                        "normal" | "lighter" => Some(400),
                        v => v.parse::<u16>().ok().map(|w| w.clamp(100, 900)),
                    }
                    .or(self.font_weight)
                }
                "font-style" => self.italic = Some(value == "italic" || value == "oblique"),
                _ => {}
            }
        }
    }

    /// `other` wins wherever it says something.
    pub fn merged(mut self, other: &Self) -> Self {
        self.fill = other.fill.or(self.fill);
        self.stroke = other.stroke.or(self.stroke);
        self.stroke_width = other.stroke_width.or(self.stroke_width);
        self.dashed = other.dashed.or(self.dashed);
        self.color = other.color.or(self.color);
        self.font_weight = other.font_weight.or(self.font_weight);
        self.italic = other.italic.or(self.italic);
        self
    }

    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Split on commas and semicolons that are not inside parentheses, so
/// `rgb(1,2,3)` stays whole.
fn split_declarations(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut start = 0;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' | ';' if depth <= 0 => {
                out.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&text[start..]);
    out.into_iter()
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_drawable_properties() {
        let style = AuthorStyle::parse(
            "fill:#f9f,stroke:rgb(51, 51, 51),stroke-width:4px,color:#fff,stroke-dasharray: 5 5",
        );
        assert!(style.fill.is_some());
        assert_eq!(style.stroke.map(|c| (c[0] * 255.0).round()), Some(51.0));
        assert_eq!(style.stroke_width, Some(4.0));
        assert_eq!(style.dashed, Some(true));
        assert_eq!(style.color, Some([1.0; 4]));
    }
}
