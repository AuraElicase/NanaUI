//! `info`: the engine's name and version.

use super::{LayoutCx, ParseCx};
use crate::ir::{Align, Rect, SceneBuilder};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary, style};

#[derive(Debug, Clone, Default)]
pub(crate) struct Model;

pub(crate) fn parse(_body: &str, _cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    Ok(Model)
}

pub(crate) fn layout(_model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let text = format!("NanaUI diagrams v{}", env!("CARGO_PKG_VERSION"));
    let laid = cx.text.lay(&Label::plain(&text), cx.font(), None);
    let mut out = SceneBuilder::new();
    out.text(&laid, [0.0, 0.0], laid.width, Align::Start, style::TEXT);
    Ok(out.finish(
        Rect::new(0.0, 0.0, laid.width, laid.height),
        cx.metrics.margin,
    ))
}

pub(crate) fn summary(_model: &Model) -> Summary {
    Summary::default()
}
