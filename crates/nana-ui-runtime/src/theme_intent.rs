//! Typed defaults shared by compound controls. Numeric public setters stay literal.
use nana_ui_core::{LayoutStyle, SpacingStep, TextWeight, ThemeExpr, ThemeToken, TypeRole};

pub(crate) fn space(step: SpacingStep) -> ThemeExpr {
    ThemeToken::Space(step).into()
}

pub(crate) fn font(layout: &mut LayoutStyle, role: TypeRole) {
    layout.theme.font_size = Some(ThemeToken::Type(role).into());
}

pub(crate) fn weight(layout: &mut LayoutStyle, role: TextWeight) {
    layout.theme.font_weight = Some(ThemeToken::Weight(role).into());
}
