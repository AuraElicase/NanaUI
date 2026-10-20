//! ZenUML diagrams: `zenuml`. The grammar lowers onto the sequence model,
//! so ZenUML draws with the sequence layout and roles.

mod parse;

use super::sequence;
use super::{LayoutCx, ParseCx};
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

pub(crate) type Model = sequence::Model;

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    sequence::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    sequence::summary(model)
}
