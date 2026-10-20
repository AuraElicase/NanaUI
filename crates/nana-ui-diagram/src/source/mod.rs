//! Source handling shared by every family: preamble, comments, accessibility
//! statements, configuration, style declarations, colours and entities.

pub mod color;
pub mod directives;
pub mod entities;
mod preamble;

use std::collections::BTreeMap;

use crate::{DiagramError, Meta};

pub use preamble::preprocess;

/// What survives preprocessing: the body with every handled line blanked
/// (so line numbers still match the source), plus what those lines said.
#[derive(Debug, Clone)]
pub struct Preprocessed {
    pub body: String,
    /// 1-based source line of the body's first line.
    pub body_line: u32,
    pub meta: Meta,
    pub config: Config,
}

/// Diagram configuration from frontmatter `config:` and `%%{init}%%`,
/// flattened to dotted keys (`flowchart.curve`). Theme settings are read but
/// never used: colours always come from the NanaUI theme.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    values: BTreeMap<String, String>,
}

impl Config {
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values.get(key).map(String::as_str)
    }

    pub fn get_f32(&self, key: &str) -> Option<f32> {
        self.get(key)?
            .trim_end_matches("px")
            .trim()
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite())
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        match self.get(key)?.trim() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        }
    }

    pub(crate) fn set(&mut self, key: String, value: String) {
        self.values.insert(key, value);
    }
}

/// Split a statement list on `;` outside quotes and brackets, keeping each
/// piece's byte offset. Families with `;`-separated statements use this.
pub fn split_statements(line: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut quote = false;
    let mut depth = 0i32;
    for (i, c) in line.char_indices() {
        match c {
            '"' => quote = !quote,
            '[' | '(' | '{' if !quote => depth += 1,
            ']' | ')' | '}' if !quote => depth -= 1,
            ';' if !quote && depth <= 0 => {
                out.push((start, &line[start..i]));
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push((start, &line[start..]));
    out
}

pub(crate) fn empty_error() -> DiagramError {
    DiagramError::Empty
}
