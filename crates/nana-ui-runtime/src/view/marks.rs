//! Small marks an application's chrome repeats: keycaps and chords, a tag,
//! a callout, a heading line with its tools, the fade at a scroll area's
//! foot, and a settings group (a titled card of rows with inset dividers).
//! Each names theme roles and steps only, so a theme moves them.
//!
//! ```ignore
//! settings_group(Some("Input"), vec![
//!     group_row(settings_row("Send").control(chord("Ctrl+Enter"))),
//!     group_row(settings_row("Mirror").control(mirror_switch)).visible(advanced),
//! ])
//! ```

use std::sync::Arc;

use nana_ui_core::{
    AlignSpec, Icon, JustifySpec, LengthSpec, RadiusTier, SemanticColorMix, SemanticColorRole,
    SpacingStep, SquareSize, StatusTone, TextWeight, TypeRole,
};

use super::node::{AnyView, IntoView, widget};
use super::prop::{IntoProp, PropSource};
use super::style::{ContainerStyle, container_styles};
use crate::{
    BoxPaint, IconGlyph, PaintContext, PaintPath, PaintShadow, Painter, Stack, Text,
    ThemeAppearance,
};

/// The colour a status tone marks with: its dot, its glyph, its tinted fill.
pub const fn tone_role(tone: StatusTone) -> SemanticColorRole {
    match tone {
        StatusTone::Neutral => SemanticColorRole::Muted,
        StatusTone::Info => SemanticColorRole::Accent,
        StatusTone::Success => SemanticColorRole::Success,
        StatusTone::Warning => SemanticColorRole::Warning,
        StatusTone::Danger => SemanticColorRole::Danger,
    }
}

/// A raised key: the surface, a border and a bottom edge in the strong
/// border colour, so the cap reads as raised in either appearance. The edge
/// is the theme's [`nana_ui_core::KeycapMetrics::edge`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct KeycapPainter;

impl Painter for KeycapPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let rect = cx.bounds();
        let r = cx.radius(RadiusTier::Xs);
        cx.rounded_rect(
            rect,
            r,
            BoxPaint::fill(SemanticColorRole::Surface).border(SemanticColorRole::Border, 1.0),
        );
        let mut path = PaintPath::new();
        path.rounded_rect(rect, [r; 4]);
        let edge = cx.color(SemanticColorRole::BorderStrong);
        cx.shadow(
            &path,
            PaintShadow::Custom {
                color: [edge[0], edge[1], edge[2], edge[3] * 0.9].into(),
                offset: [0.0, -cx.metrics().keycap.edge],
                blur: 0.0,
                spread: 0.0,
                inset: true,
            },
        );
    }

    fn paint_key(&self) -> u64 {
        0x6b65_7963_6170
    }
}

/// A surface lifted off the page: the surface at `radius` with a soft
/// hairline, and on a light appearance a short soft shadow (one barely reads
/// on dark, where the hairline alone separates it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RaisedSurfacePainter(pub RadiusTier);

impl Painter for RaisedSurfacePainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let rect = cx.bounds();
        let r = cx.radius(self.0);
        if cx.theme_appearance() == ThemeAppearance::Light {
            let mut path = PaintPath::new();
            path.rounded_rect(rect, [r; 4]);
            let ink = cx.color(SemanticColorRole::Text);
            cx.shadow(
                &path,
                PaintShadow::Custom {
                    color: [ink[0], ink[1], ink[2], 0.06].into(),
                    offset: [0.0, 1.0],
                    blur: 3.0,
                    spread: 0.0,
                    inset: false,
                },
            );
        }
        cx.rounded_rect(
            rect,
            r,
            BoxPaint::fill(SemanticColorRole::Surface).border(SemanticColorRole::BorderSoft, 1.0),
        );
    }

    fn paint_key(&self) -> u64 {
        0x7261_6973_6564 ^ self.0 as u64
    }
}

/// A keycap's face, for a cap whose label is bound in place. Its height, and
/// the narrowest a one-letter cap gets, is the theme's
/// [`nana_ui_core::KeycapMetrics::height`].
pub fn keycap_face() -> Stack {
    Stack::row(0.0)
        .square(SquareSize::Keycap)
        .padding_step(SpacingStep::Md, SpacingStep::Xxs)
        .align(AlignSpec::Center)
        .justify(JustifySpec::Center)
        .radius(RadiusTier::Xs)
        .painter(KeycapPainter)
        .with_layout(|l| l.flex_shrink = Some(0.0))
}

/// A keycap's label.
pub fn keycap_label(key: impl Into<String>) -> Text {
    let mut text = Text::new(key.into())
        .role(TypeRole::Meta)
        .weight(TextWeight::Semibold)
        .color(SemanticColorRole::Text);
    Arc::make_mut(&mut text.style.layout).white_space_nowrap = true;
    text
}

/// One key as a keycap.
pub fn kbd(key: impl Into<String>) -> AnyView {
    widget(keycap_face())
        .children(widget(keycap_label(key).decorative()))
        .into_any()
}

/// A key drawn as a glyph (an arrow) on a keycap.
pub fn kbd_glyph(icon: Icon) -> AnyView {
    // The label's own size, so a glyph key matches a lettered one.
    let mut glyph = IconGlyph::new(icon)
        .size(nana_ui_core::type_scale::META)
        .decorative();
    glyph.style.foreground = Some(SemanticColorRole::Text);
    widget(keycap_face()).children(widget(glyph)).into_any()
}

/// The muted joiner between the keys of a chord.
pub fn chord_joiner(joiner: impl Into<String>) -> Text {
    Text::new(joiner.into())
        .role(TypeRole::Meta)
        .weight(TextWeight::Medium)
        .color(SemanticColorRole::Muted)
}

/// A chord ("Ctrl+B") as keycaps joined by "+". `key` draws each key, so an
/// application can give arrows a glyph.
pub fn chord_with(keys: &str, key: impl Fn(&str) -> AnyView) -> AnyView {
    let mut caps: Vec<AnyView> = Vec::new();
    for (i, k) in keys.split('+').enumerate() {
        if i > 0 {
            caps.push(widget(chord_joiner("+").decorative()).into_any());
        }
        caps.push(key(k));
    }
    widget(
        Stack::row(0.0)
            .gap_step(SpacingStep::Xs)
            .align(AlignSpec::Center),
    )
    .children(caps)
    .into_any()
}

/// A chord ("Ctrl+B") as keycaps joined by "+".
pub fn chord(keys: &str) -> AnyView {
    chord_with(keys, |k| kbd(k))
}

/// A quiet, non-interactive tag: a subtle fill with a soft hairline at the
/// control radius, its text small and muted. A label, not a control (for
/// one that can be chosen or dismissed, use [`crate::Chip`]).
pub fn tag_frame() -> Stack {
    Stack::row(0.0)
        .gap_step(SpacingStep::Xs)
        .padding_step(SpacingStep::Md, SpacingStep::Xxs)
        .align(AlignSpec::Center)
        .surface(SemanticColorRole::Subtle)
        .outline(SemanticColorRole::BorderSoft, 1.0)
        .radius(RadiusTier::Sm)
        .with_layout(|l| l.flex_shrink = Some(0.0))
}

/// [`tag_frame`] holding `text`.
pub fn tag(text: impl Into<String>) -> AnyView {
    widget(tag_frame())
        .children(widget(
            Text::new(text.into())
                .role(TypeRole::Meta)
                .weight(TextWeight::Medium)
                .color(SemanticColorRole::Muted),
        ))
        .into_any()
}

/// A note marked by `tone`: `icon` in the tone's colour, then `text`,
/// wrapping within the row. In a box tinted by the tone unless
/// [`Callout::quiet`]; hidden while `text` is empty. `.class`, `.css` and
/// `.visible` style its row as they style an element, e.g.
/// `.css(css! { align-items: center; })` to centre a one-line note on a
/// taller trailing button (it aligns to the top for notes that wrap).
pub fn callout(icon: Icon, tone: StatusTone, text: impl IntoProp<String>) -> Callout {
    Callout {
        icon,
        tone,
        text: text.into_source(),
        quiet: false,
        trailing: None,
        container: ContainerStyle::default(),
    }
}

/// A [`callout`] being set up; a view.
pub struct Callout {
    icon: Icon,
    tone: StatusTone,
    text: PropSource<String>,
    quiet: bool,
    trailing: Option<AnyView>,
    container: ContainerStyle,
}

container_styles!([] Callout);

impl Callout {
    fn container_style_mut(&mut self) -> &mut ContainerStyle {
        &mut self.container
    }

    /// No tint and no inset of its own, its text small and muted, centred on
    /// its glyph: a one-line note inside a block that already owns the inset.
    pub fn quiet(mut self) -> Self {
        self.quiet = true;
        self
    }

    /// `view` at the end of the line: a link to where the note is put right,
    /// or its buttons. It keeps its width; the text wraps first.
    pub fn trailing(mut self, view: impl IntoView) -> Self {
        self.trailing = Some(view.into_any());
        self
    }
}

impl IntoView for Callout {
    fn build(self, vb: &mut super::node::ViewBuilder<'_, '_, '_>) {
        let Callout {
            icon,
            tone,
            text,
            quiet,
            trailing,
            mut container,
        } = self;
        let role = tone_role(tone);
        let text = super::reactive::computed(move || text.get());
        let frame = if quiet {
            Stack::row(0.0)
                .width(LengthSpec::Fill)
                .gap_step(SpacingStep::Sm)
                // The block around it owns the inset across.
                .padding_xy(0.0, nana_ui_core::space::XS)
                .align(AlignSpec::Center)
        } else {
            Stack::row(0.0)
                .width(LengthSpec::Fill)
                .gap_step(SpacingStep::Md)
                .padding_step(SpacingStep::Xl, SpacingStep::Md)
                .align(AlignSpec::Start)
                .radius(RadiusTier::Sm)
                .surface_mix(SemanticColorMix::alpha(role, 0.10))
        };
        let mut glyph = IconGlyph::new(icon).role(role).decorative();
        if !quiet {
            glyph = glyph.size(nana_ui_core::type_scale::LINE);
        }
        let copy = if quiet {
            Text::new("")
                .role(TypeRole::Meta)
                .color(SemanticColorRole::Muted)
        } else {
            Text::new("").role(TypeRole::Body)
        };
        // A `.visible` given to the callout joins its own rule rather than
        // racing it for the same field.
        let shown = container.take_visible();
        let frame = widget(frame)
            .visible(move || !text.with(String::is_empty) && shown.iter().all(PropSource::get));
        container
            .apply(frame)
            .children((
                widget(glyph),
                widget(
                    Stack::column(0.0)
                        .grow(1.0)
                        .shrink(1.0)
                        .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
                )
                .children(widget(copy).value(text)),
                trailing.map(|trailing| {
                    widget(Stack::row(0.0).align(AlignSpec::Center).shrink(0.0)).children(trailing)
                }),
            ))
            .build(vb)
    }
}

/// A heading line: `title` takes the width that is left and gives it up
/// first, ellipsing if it must; `tools` keep theirs at the end. `.class`,
/// `.css` and `.visible` style the line as they style an element.
pub fn heading_row(title: impl IntoView, tools: impl IntoView) -> HeadingRow {
    HeadingRow {
        title: title.into_any(),
        tools: tools.into_any(),
        container: ContainerStyle::default(),
    }
}

/// A [`heading_row`] being set up; a view.
pub struct HeadingRow {
    title: AnyView,
    tools: AnyView,
    container: ContainerStyle,
}

impl HeadingRow {
    fn container_style_mut(&mut self) -> &mut ContainerStyle {
        &mut self.container
    }
}

impl IntoView for HeadingRow {
    fn build(self, vb: &mut super::node::ViewBuilder<'_, '_, '_>) {
        let HeadingRow {
            title,
            tools,
            container,
        } = self;
        container
            .apply(widget(
                Stack::row(0.0)
                    .width(LengthSpec::Fill)
                    .gap_step(SpacingStep::Sm)
                    .align(AlignSpec::Center),
            ))
            .children((
                widget(
                    Stack::row(0.0)
                        .width(LengthSpec::Fill)
                        .gap_step(SpacingStep::Sm)
                        .shrink(1.0)
                        .align(AlignSpec::Center)
                        .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
                )
                .children(title),
                tools,
            ))
            .build(vb)
    }
}

/// A heading's tools ([`crate::IconButton::tool`], compact menus): a
/// toolbar without chrome or vertical inset, so the heading line keeps the
/// height of its text or its tools, whichever is taller; it never shrinks
/// its targets. `.class`, `.css` and `.visible` style the toolbar.
pub fn heading_tools(tools: impl IntoView) -> HeadingTools {
    HeadingTools {
        tools: tools.into_any(),
        container: ContainerStyle::default(),
    }
}

/// A [`heading_tools`] being set up; a view.
pub struct HeadingTools {
    tools: AnyView,
    container: ContainerStyle,
}

impl HeadingTools {
    fn container_style_mut(&mut self) -> &mut ContainerStyle {
        &mut self.container
    }
}

impl IntoView for HeadingTools {
    fn build(self, vb: &mut super::node::ViewBuilder<'_, '_, '_>) {
        let HeadingTools { tools, container } = self;
        container
            .apply(widget(
                crate::Toolbar::new().chrome(false).style(
                    Stack::row(0.0)
                        .gap_step(SpacingStep::Xxs)
                        .shrink(0.0)
                        .align(AlignSpec::Center)
                        .node_style(),
                ),
            ))
            .children(tools)
            .build(vb)
    }
}

container_styles!([] HeadingRow, [] HeadingTools, [] SettingsGroupView);

/// The foot of a scroll area fading into `surface`, `height` tall: placed
/// over the area's bottom edge, it shows a list runs on past it instead of
/// cutting through a row. It takes no input, so the wheel and the pointer
/// reach the list under it; end the list with `height` of air so its last
/// row can scroll clear of the fade.
pub fn scroll_foot_fade(height: f32, surface: SemanticColorRole) -> AnyView {
    use nana_ui_core::{CssGradient, GradientStop, LinearGradient, MaskImage};
    let stop = |position: f32, alpha: f32| GradientStop {
        paint_color: None,
        position,
        color: [1.0, 1.0, 1.0, alpha],
    };
    widget(Stack::row(0.0).surface(surface).with_layout(|l| {
        l.position = nana_ui_core::PositionSpec::Absolute;
        l.offset_left = Some(LengthSpec::Px(0.0));
        l.offset_right = Some(LengthSpec::Px(0.0));
        l.offset_bottom = Some(LengthSpec::Px(0.0));
        l.height = Some(LengthSpec::Px(height));
        l.pointer_events = Some(nana_ui_core::PointerEventsSpec::None);
        l.z_index = Some(1);
        // CSS angles: 180deg runs top to bottom.
        l.paint.mask = Some(MaskImage::Gradient(CssGradient::Linear(LinearGradient {
            angle_deg: 180.0,
            stops: vec![stop(0.0, 0.0), stop(1.0, 1.0)],
        })));
    }))
    .into_any()
}

/// One row of a [`settings_group`].
pub struct GroupRow {
    view: AnyView,
    visible: Option<PropSource<bool>>,
}

/// `view` as a row of a [`settings_group`].
pub fn group_row(view: impl IntoView) -> GroupRow {
    GroupRow {
        view: view.into_any(),
        visible: None,
    }
}

impl GroupRow {
    /// Show the row only while `visible` holds; a hidden row takes its
    /// divider with it.
    pub fn visible(mut self, visible: impl IntoProp<bool>) -> Self {
        self.visible = Some(visible.into_source());
        self
    }
}

/// The inset hairline between two rows of a group. The group shows it only
/// between two shown rows.
fn group_divider() -> AnyView {
    // Inset like the rows' own copy: the settings row's horizontal inset.
    widget(crate::Divider::horizontal().inset(nana_ui_core::space::XXXL)).into_any()
}

/// A settings group: `title` (when there is one) above a raised card whose
/// rows are separated by inset hairlines. A hidden row takes its hairline
/// with it, whichever row it is. `.class`, `.css` and `.visible` style the
/// group (the column holding the title and the card).
pub fn settings_group(title: Option<String>, rows: Vec<GroupRow>) -> SettingsGroupView {
    SettingsGroupView {
        title,
        rows,
        container: ContainerStyle::default(),
    }
}

/// A [`settings_group`] being set up; a view.
pub struct SettingsGroupView {
    title: Option<String>,
    rows: Vec<GroupRow>,
    container: ContainerStyle,
}

impl SettingsGroupView {
    fn container_style_mut(&mut self) -> &mut ContainerStyle {
        &mut self.container
    }
}

impl IntoView for SettingsGroupView {
    fn build(self, vb: &mut super::node::ViewBuilder<'_, '_, '_>) {
        let SettingsGroupView {
            title,
            rows,
            container,
        } = self;
        let mut members: Vec<AnyView> = Vec::new();
        for (i, GroupRow { view, visible }) in rows.into_iter().enumerate() {
            if i > 0 {
                members.push(group_divider());
            }
            let row = widget(Stack::column(0.0)).children(view);
            members.push(match visible {
                Some(visible) => row.visible(visible).into_any(),
                None => row.into_any(),
            });
        }
        let mut group = Stack::column(0.0);
        if title.is_some() {
            group = group.gap_step(SpacingStep::Md);
        }
        container
            .apply(widget(group))
            .children((
                title.map(|title| {
                    widget(Stack::row(0.0).with_layout(|l| {
                        l.padding_left = Some(LengthSpec::Px(nana_ui_core::space::XS));
                    }))
                    .children(widget(
                        Text::new(title)
                            .role(TypeRole::Meta)
                            .weight(TextWeight::Semibold)
                            .color(SemanticColorRole::Muted),
                    ))
                }),
                widget(
                    Stack::column(0.0)
                        .radius(RadiusTier::Lg)
                        .painter(RaisedSurfacePainter(RadiusTier::Lg)),
                )
                .children(widget(crate::SettingsGroup::new()).children(members)),
            ))
            .build(vb)
    }
}
