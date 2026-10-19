//! Theme intent survives compiled CSS cascade, responsive rules and live installs.
#![cfg(feature = "view-macro")]
use nana_ui_core::{ThemeAppearance, ThemeDefinition};
use nana_ui_runtime::view::{signal, theme_signal};
use nana_ui_runtime::{AppContext, DocumentId, LayoutViewport, view};
use std::cell::Cell;

fn frames(cx: &mut AppContext, document: DocumentId) {
    cx.begin_frame_profile();
    for _ in 0..6 {
        let work = cx.take_system_work();
        cx.resolve_styles(&work.style).unwrap();
        cx.layout_document(document, LayoutViewport::new(800.0, 600.0))
            .unwrap();
    }
    cx.finish_frame_profile();
}

#[test]
fn compiled_theme_values_follow_live_installs_and_literal_overrides() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let flag = Cell::new(None);
    let observer = Cell::new(None);
    let mounted = cx.mount_view_root(document, || {
        let literal = signal(false);
        flag.set(Some(literal));
        observer.set(Some(theme_signal()));
        view! {
            <style>
                .panel { width: 400px; padding: theme(space-md); gap: theme(space-xs); }
                .copy { font-size: theme(type-body); font-weight: theme(weight-semibold); line-height: theme(line-tall); height: calc(2 * theme(line-tall) + theme(space-xs)); }
                .copy.literal { font-size: 13px; height: 40px; }
            </style>
            <Column class="panel">
                <Text class="copy" class:literal={literal}>"Theme values"</Text>
            </Column>
        }
    }).unwrap();
    let root = mounted.roots()[0];
    let text = cx.world().node(root).unwrap().children[0];
    frames(&mut cx, document);
    assert_eq!(cx.world().layout_box(text).unwrap().height, 40.0);
    let mut changed = ThemeDefinition::NANA_DARK;
    changed.typography.body = 19.0;
    changed.typography.line_tall = 24.0;
    changed.typography.semibold = 650;
    changed.tokens.spacing.md = 20.0;
    changed.tokens.spacing.xs = 6.0;
    cx.set_theme_definition(&changed).unwrap();
    frames(&mut cx, document);
    let style = cx.world().computed_style(text).unwrap();
    assert_eq!(style.font_size, 19.0);
    assert_eq!(style.font_weight, Some(650));
    assert_eq!(cx.world().layout_box(text).unwrap().height, 54.0);
    assert_eq!(cx.world().layout_box(text).unwrap().x, 20.0);
    assert_eq!(observer.get().unwrap().get().typography().body, 19.0);
    let mut palette_only = changed.clone();
    palette_only.tokens.palette = nana_ui_core::SemanticPalette::light();
    cx.take_system_work();
    cx.set_theme_definition(&palette_only).unwrap();
    let work = cx.take_system_work();
    assert!(
        work.layout_frontier_seeds.is_empty(),
        "palette changes do not lay out token-bound nodes"
    );
    assert!(
        work.style.is_empty(),
        "palette changes do not reshape token-bound text"
    );
    flag.get().unwrap().set(true);
    frames(&mut cx, document);
    assert_eq!(cx.world().computed_style(text).unwrap().font_size, 13.0);
    assert_eq!(cx.world().layout_box(text).unwrap().height, 40.0);
    flag.get().unwrap().set(false);
    frames(&mut cx, document);
    assert_eq!(cx.world().computed_style(text).unwrap().font_size, 19.0);
}

#[test]
fn responsive_token_and_literal_variants_keep_their_cascade_on_theme_switch() {
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let flag = Cell::new(None);
    let mounted = cx.mount_view_root(document, || {
        let narrow = signal(false);
        flag.set(Some(narrow));
        view! {
            <style>
                .host { container-type: inline-size; width: 400px; }
                .host.narrow { width: 200px; }
                .item { height: theme(space-page); padding-left: theme(space-md); }
                @container (max-width: 300px) { .item { height: 30px; padding-left: theme(space-xl); } }
            </style>
            <Column class="host" class:narrow={narrow}>
                <Column class="item" />
            </Column>
        }
    }).unwrap();
    let item = cx.world().node(mounted.roots()[0]).unwrap().children[0];
    frames(&mut cx, document);
    assert_eq!(cx.world().layout_box(item).unwrap().height, 24.0);
    flag.get().unwrap().set(true);
    frames(&mut cx, document);
    assert_eq!(cx.world().layout_box(item).unwrap().height, 30.0);
    let mut theme = ThemeDefinition::NANA_DARK;
    theme.tokens.spacing.page = 36.0;
    theme.tokens.spacing.xl = 18.0;
    cx.set_theme_definition(&theme).unwrap();
    frames(&mut cx, document);
    assert_eq!(cx.world().layout_box(item).unwrap().height, 30.0);
    flag.get().unwrap().set(false);
    frames(&mut cx, document);
    assert_eq!(cx.world().layout_box(item).unwrap().height, 36.0);
    cx.set_preset_theme(ThemeAppearance::Light).unwrap();
    frames(&mut cx, document);
    assert_eq!(cx.world().layout_box(item).unwrap().height, 24.0);
}

#[test]
fn a_themed_grid_regroups_and_keeps_row_gaps_after_a_theme_install() {
    use nana_ui_core::SpacingStep;
    use nana_ui_runtime::view::{IntoView, each_virtual, virtual_list_ref, widget};
    use nana_ui_runtime::{LengthSpec, Stack};
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let rows = virtual_list_ref::<u32>();
    let mounted = cx
        .mount_view_root(document, || {
            each_virtual(
                signal((0..12).collect::<Vec<u32>>()),
                |item| *item,
                40.0,
                |_| widget(Stack::column(0.0).height(LengthSpec::Px(40.0))).into_any(),
            )
            .grid_theme(120.0, SpacingStep::Xxxl)
            .list_ref(rows.clone())
            .width(400.0)
            .height(200.0)
        })
        .unwrap();
    frames(&mut cx, document);
    let first = rows.row(&0).unwrap();
    let third = rows.row(&2).unwrap();
    assert_eq!(
        cx.world().layout_box(first).unwrap().y,
        cx.world().layout_box(third).unwrap().y
    );
    let mut theme = ThemeDefinition::NANA_DARK;
    theme.tokens.spacing.xxxl = 48.0;
    cx.set_theme_definition(&theme).unwrap();
    frames(&mut cx, document);
    let first = cx.world().layout_box(rows.row(&0).unwrap()).unwrap();
    let third = cx.world().layout_box(rows.row(&2).unwrap()).unwrap();
    assert_eq!(
        third.y - first.y,
        88.0,
        "two columns, 40px row and themed 48px gap"
    );
    assert!(!mounted.roots().is_empty());
}

#[test]
fn a_plain_measured_list_refreshes_published_extents_when_theme_geometry_changes() {
    use nana_ui_core::{SpacingStep, ThemeToken};
    use nana_ui_runtime::Stack;
    use nana_ui_runtime::view::{each_virtual, virtual_list_ref, widget};
    let mut cx = AppContext::new();
    let document = DocumentId::new(1).unwrap();
    let rows = virtual_list_ref::<u32>();
    let _mounted = cx
        .mount_view_root(document, || {
            each_virtual(
                signal((0..20).collect::<Vec<u32>>()),
                |item| *item,
                24.0,
                |_| {
                    widget(Stack::column(0.0).with_layout(|layout| {
                        layout.theme.height = Some(ThemeToken::Space(SpacingStep::Page).into());
                    }))
                },
            )
            .measured()
            .list_ref(rows.clone())
            .height(200.0)
        })
        .unwrap();
    frames(&mut cx, document);
    assert_eq!(rows.item(&1).unwrap().offset, 24.0);
    let mut theme = ThemeDefinition::NANA_DARK;
    theme.tokens.spacing.page = 48.0;
    cx.set_theme_definition(&theme).unwrap();
    frames(&mut cx, document);
    assert_eq!(rows.item(&1).unwrap().offset, 48.0);
    assert_eq!(rows.item(&0).unwrap().extent, 48.0);
}
