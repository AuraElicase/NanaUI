//! Wardley map diagrams. Not yet native: the runtime draws them through its
//! legacy path until this module lands.

use super::{LayoutCx, ParseCx};
use crate::{DiagramError, DiagramKind, DiagramScene, Summary};

pub(crate) const NATIVE: bool = false;

#[derive(Debug, Clone, Default)]
pub(crate) struct Model;

pub(crate) fn parse(_body: &str, _cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    Err(DiagramError::Unsupported(DiagramKind::Wardley))
}

pub(crate) fn layout(_model: &Model, _cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    Err(DiagramError::Unsupported(DiagramKind::Wardley))
}

pub(crate) fn summary(_model: &Model) -> Summary {
    Summary::default()
}
