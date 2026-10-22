//! XY charts: `xychart-beta` / `xychart`, vertical or `horizontal`, with a
//! category or numeric x axis, a y range and any number of bar and line
//! series. Scales come from the chart crate through `layout::axis`.

mod layout;

use super::kit::{self, first_word, number};
use super::{LayoutCx, ParseCx};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, LimitKind, Summary};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Plot {
    Bar,
    Line,
}

#[derive(Debug, Clone)]
pub(crate) struct Series {
    pub plot: Plot,
    pub name: Option<Label>,
    pub values: Vec<f64>,
}

#[derive(Debug, Clone, Default)]
pub(crate) enum XAxis {
    /// No `x-axis` data: the points are numbered from 1.
    #[default]
    Auto,
    Categories(Vec<Label>),
    Range(f64, f64),
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub horizontal: bool,
    pub x_title: Option<Label>,
    pub x: XAxis,
    pub y_title: Option<Label>,
    pub y_range: Option<(f64, f64)>,
    pub series: Vec<Series>,
    /// `xyChart.showDataLabel`: values over the bars.
    pub data_labels: bool,
}

impl Model {
    /// Points along the x axis.
    pub fn points(&self) -> usize {
        match &self.x {
            XAxis::Categories(c) => c.len(),
            _ => self
                .series
                .iter()
                .map(|s| s.values.len())
                .max()
                .unwrap_or(0),
        }
    }
}

/// A quoted string or one bare word, and what follows. `None` when the
/// text starts with a list or a range instead.
fn title(text: &str) -> (Option<String>, &str) {
    let text = text.trim_start();
    if let Some(q) = text.strip_prefix('"')
        && let Some(end) = q.find('"')
    {
        return (Some(q[..end].to_string()), &q[end + 1..]);
    }
    if text.is_empty() || text.starts_with('[') || range(text).is_some() {
        return (None, text);
    }
    let end = text
        .find(|c: char| c.is_whitespace() || c == '[')
        .unwrap_or(text.len());
    (Some(text[..end].to_string()), &text[end..])
}

/// `min --> max`.
fn range(text: &str) -> Option<(f64, f64)> {
    let (a, b) = text.split_once("-->")?;
    Some((number(a)?, number(b)?))
}

/// The items of `[a, "b, c", d]`.
fn list(text: &str) -> Option<Vec<String>> {
    let inner = text.trim().strip_prefix('[')?.strip_suffix(']')?;
    if inner.trim().is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in inner.chars() {
        match c {
            '"' => quoted = !quoted,
            ',' if !quoted => out.push(std::mem::take(&mut current).trim().to_string()),
            _ => current.push(c),
        }
    }
    out.push(current.trim().to_string());
    Some(out)
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model {
        horizontal: cx
            .config
            .get("xyChart.chartOrientation")
            .is_some_and(|o| o.trim() == "horizontal"),
        data_labels: cx.config.get_bool("xyChart.showDataLabel").unwrap_or(false),
        ..Model::default()
    };
    let (header, rest) = kit::header(body)?;
    match rest.trim_end_matches(';').trim_end() {
        "" | "vertical" => {}
        "horizontal" => model.horizontal = true,
        other => cx.warn(header, format!("unknown orientation `{other}`")),
    }
    let mut count = 0usize;
    for (index, line) in kit::statements(body, header) {
        let line = line.trim_end_matches(';').trim_end();
        if line.is_empty() {
            continue;
        }
        let err = |message: String| DiagramError::syntax(index as u32 + 1, 1, message);
        cx.limits.check_label(line)?;
        let (word, rest) = first_word(line);
        match word {
            "title" => {
                let (t, tail) = title(rest);
                let text = if tail.trim().is_empty() {
                    t.unwrap_or_default()
                } else {
                    rest.to_string()
                };
                cx.meta.title = Some(text);
            }
            "x-axis" => {
                let (t, tail) = title(rest);
                model.x_title = t.map(|t| Label::plain(&t));
                let tail = tail.trim();
                if tail.is_empty() {
                    continue;
                }
                if let Some(items) = list(tail) {
                    count += items.len();
                    if count > cx.limits.elements {
                        return Err(DiagramError::TooLarge(LimitKind::Elements));
                    }
                    model.x = XAxis::Categories(items.iter().map(|s| Label::plain(s)).collect());
                } else if let Some((a, b)) = range(tail) {
                    model.x = XAxis::Range(a, b);
                } else {
                    return Err(err(format!(
                        "expected `[categories]` or `min --> max` after x-axis, found `{tail}`"
                    )));
                }
            }
            "y-axis" => {
                let (t, tail) = title(rest);
                model.y_title = t.map(|t| Label::plain(&t));
                let tail = tail.trim();
                if tail.is_empty() {
                    continue;
                }
                match range(tail) {
                    Some(r) => model.y_range = Some(r),
                    None => {
                        return Err(err(format!(
                            "expected `min --> max` after y-axis, found `{tail}`"
                        )));
                    }
                }
            }
            "bar" | "line" => {
                let (name, tail) = title(rest);
                let items =
                    list(tail).ok_or_else(|| err(format!("expected `[values]` after `{word}`")))?;
                count += items.len();
                if count > cx.limits.elements {
                    return Err(DiagramError::TooLarge(LimitKind::Elements));
                }
                let values = items
                    .iter()
                    .map(|v| number(v).filter(|v| v.abs() < 1e15))
                    .collect::<Option<Vec<f64>>>()
                    .ok_or_else(|| err(format!("`{word}` values must be numbers")))?;
                model.series.push(Series {
                    plot: if word == "bar" { Plot::Bar } else { Plot::Line },
                    name: name.map(|n| Label::plain(&n)),
                    values,
                });
            }
            other => cx.warn(index, format!("unknown statement `{other}`")),
        }
    }
    if let XAxis::Categories(c) = &model.x
        && model.series.iter().any(|s| s.values.len() > c.len())
    {
        cx.warn(0, "values past the last category are not drawn");
    }
    Ok(model)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.series.iter().map(|s| s.values.len()).sum::<usize>() as u32,
        edges: 0,
        groups: model.series.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_pieces() {
        assert_eq!(
            title("\"Revenue (in $)\" 4000 --> 11000").0.as_deref(),
            Some("Revenue (in $)")
        );
        assert_eq!(title("4000 --> 11000").0, None);
        assert_eq!(title("Months [a, b]").0.as_deref(), Some("Months"));
        assert_eq!(range(" -2.5 --> 10"), Some((-2.5, 10.0)));
        assert_eq!(
            list("[jan, \"feb, mar\", apr]").unwrap(),
            vec!["jan", "feb, mar", "apr"]
        );
    }
}
