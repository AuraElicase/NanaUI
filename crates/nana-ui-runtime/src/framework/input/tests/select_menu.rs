//! An open Select menu whose rows do not fit: the wheel and the scrollbar
//! move them, the keyboard brings the highlight into view, and a press lands
//! on the row drawn under it.

use std::sync::{Arc, Mutex};

use nana_ui_core::LengthSpec;

use super::*;
use crate::{
    Dropdown, DropdownOption, Entity, LayoutBox, NodeStyle, ScrollAxes, ScrollOffset, ScrollView,
    SearchDropdown, SearchDropdownOption, Select, SelectChanged, SelectMenuGeometry, SelectOption,
    Stack,
};

fn document() -> DocumentId {
    DocumentId::new(1).unwrap()
}

fn pointer_at(phase: PointerPhase, x: f32, y: f32) -> Gesture {
    pointer_fixture! {
        phase,
        pointer_id: 1,
        pointer_type: PointerType::Mouse,
        x,
        y,
        screen_x: x,
        screen_y: y,
        button: 0,
        buttons: u16::from(phase == PointerPhase::Down),
        pressure: 0.0,
        tangential_pressure: 0.0,
        tilt_x: 0,
        tilt_y: 0,
        twist: 0,
        is_primary: true,
        activation_click: false,
        modifiers: InputModifiers::default(),
    }
}

fn wheel_at(x: f32, y: f32, delta_y: f32) -> Gesture {
    wheel_fixture! {
        x,
        y,
        delta_x: 0.0,
        delta_y,
        line_delta: false,
        modifiers: InputModifiers::default(),
    }
}

fn arrow_down() -> Gesture {
    key_fixture! {
        pressed: true,
        key: "ArrowDown".into(),
        text: None,
        code: "ArrowDown".into(),
        repeat: false,
        modifiers: InputModifiers::default(),
    }
}

fn options(count: usize) -> Vec<SelectOption> {
    (0..count)
        .map(|n| SelectOption::new(format!("v{n}"), format!("Option {n}")))
        .collect()
}

fn centre(bounds: LayoutBox) -> (f32, f32) {
    (
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    )
}

/// A 400×300 page that scrolls, holding `above` px of space, `field`, and a
/// tall block under it; laid out and hit-tested.
fn page_with<C: crate::ComponentView>(
    context: &mut AppContext,
    above: f32,
    field: C,
) -> (Entity<ScrollView>, Entity<C>) {
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(400.0));
        layout.height = Some(LengthSpec::Px(300.0));
    }
    let page = context
        .create_component(
            document(),
            ScrollView::new(ScrollAxes::Vertical).style(style),
        )
        .unwrap();
    let block = |height: f32| {
        Stack::column(0.0).with_layout(move |layout| {
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Px(height));
        })
    };
    let spacer = context
        .create_detached_component(document(), block(above))
        .unwrap();
    context.append_child(page, spacer).unwrap();
    let field = context
        .create_detached_component(document(), field)
        .unwrap();
    context.append_child(page, field).unwrap();
    let tail = context
        .create_detached_component(document(), block(900.0))
        .unwrap();
    context.append_child(page, tail).unwrap();
    settle(context);
    (page, field)
}

fn settle(context: &mut AppContext) {
    context
        .layout_document(document(), crate::LayoutViewport::new(400.0, 300.0))
        .unwrap();
    context.rebuild_hit_test(document());
}

fn menu_of(context: &AppContext, id: StableNodeId) -> SelectMenuGeometry {
    context
        .open_select_menu(id)
        .expect("the field's menu is open")
}

/// A layout point of `field`'s menu where the window shows it.
fn on_screen(context: &AppContext, field: StableNodeId, (x, y): (f32, f32)) -> (f32, f32) {
    context
        .world()
        .layout_pointer_position(field, x, y)
        .expect("a laid-out field")
}

fn page_offset(context: &AppContext, page: Entity<ScrollView>) -> f32 {
    context
        .world()
        .scroll_offset(page.stable_id())
        .unwrap_or_default()
        .y
}

#[test]
fn the_wheel_scrolls_an_overflowing_menu_and_keeps_the_page_still() {
    let mut context = AppContext::new();
    let (page, select) = page_with(
        &mut context,
        0.0,
        Select::new(Some("v0")).options(options(40)).opened(true),
    );
    let id = select.stable_id();
    let menu = menu_of(&context, id);
    assert_eq!(menu.scroll, 0.0);
    assert!(menu.max_scroll > 0.0);
    let (x, y) = centre(menu.viewport);
    let mut adapter = TestInput::default();
    let mut wheel = |context: &mut AppContext, delta: f32| {
        adapter
            .dispatch(context, document(), &wheel_at(x, y, delta))
            .unwrap()
            .prevent_default
    };
    assert!(wheel(&mut context, -120.0));
    assert_eq!(menu_of(&context, id).scroll, 120.0);
    // Past either end the rows stop there and the wheel still stays with
    // the menu: the page under it does not scroll it away.
    assert!(wheel(&mut context, 1_000.0));
    assert_eq!(menu_of(&context, id).scroll, 0.0);
    assert!(wheel(&mut context, -10_000.0));
    assert_eq!(menu_of(&context, id).scroll, menu.max_scroll);
    assert!(wheel(&mut context, -120.0));
    assert_eq!(menu_of(&context, id).scroll, menu.max_scroll);
    assert_eq!(page_offset(&context, page), 0.0);
}

#[test]
fn the_wheel_over_a_menu_that_holds_its_rows_scrolls_the_page() {
    let mut context = AppContext::new();
    let (page, select) = page_with(
        &mut context,
        0.0,
        Select::new(Some("v0")).options(options(3)).opened(true),
    );
    let menu = menu_of(&context, select.stable_id());
    assert_eq!(menu.max_scroll, 0.0);
    let (x, y) = centre(menu.viewport);
    assert!(
        TestInput::default()
            .dispatch(&mut context, document(), &wheel_at(x, y, -120.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(page_offset(&context, page), 120.0);
}

#[test]
fn dragging_the_thumb_scrolls_the_rows_without_choosing_one() {
    let mut context = AppContext::new();
    let (_, select) = page_with(
        &mut context,
        0.0,
        Select::new(Some("v0")).options(options(40)).opened(true),
    );
    let id = select.stable_id();
    let changes = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&changes);
    context
        .on(select, move |_, event: &SelectChanged, _| {
            sink.lock().unwrap().push(event.value.clone())
        })
        .unwrap();
    let menu = menu_of(&context, id);
    let bar = menu.scrollbar.expect("an overflowing menu has a bar");
    let resting = bar.thumb.width;
    let (x, y) = centre(bar.thumb);
    let mut adapter = TestInput::default();
    // Over the lane it heats up and the thumb grows.
    adapter
        .dispatch(
            &mut context,
            document(),
            &pointer_at(PointerPhase::Move, x, y),
        )
        .unwrap();
    assert!(
        context
            .read(select, |select| select.menu_scroll.hot)
            .unwrap()
    );
    assert!(menu_of(&context, id).scrollbar.unwrap().thumb.width > resting);
    // A press on the thumb holds it where it was taken.
    assert!(
        adapter
            .dispatch(
                &mut context,
                document(),
                &pointer_at(PointerPhase::Down, x, y)
            )
            .unwrap()
            .prevent_default
    );
    let track = bar.track_geometry(nana_ui_core::ScrollbarAxis::Vertical);
    adapter
        .dispatch(
            &mut context,
            document(),
            &pointer_at(PointerPhase::Move, x, y + 40.0),
        )
        .unwrap();
    let expected = track.offset_for_thumb_origin(track.thumb_origin + 40.0);
    assert!((menu_of(&context, id).scroll - expected).abs() < 0.01);
    // The release ends the drag where it is; nothing is chosen and the menu
    // stays open.
    adapter
        .dispatch(
            &mut context,
            document(),
            &pointer_at(PointerPhase::Up, x, y + 40.0),
        )
        .unwrap();
    let scroll = context.read(select, |select| select.menu_scroll).unwrap();
    assert!(scroll.drag.is_none());
    assert!(context.read(select, |select| select.opened).unwrap());
    assert!((menu_of(&context, id).scroll - expected).abs() < 0.01);
    assert!(changes.lock().unwrap().is_empty());
}

#[test]
fn a_press_on_bare_track_jumps_there_and_a_cancel_goes_back() {
    let mut context = AppContext::new();
    let (_, select) = page_with(
        &mut context,
        0.0,
        Select::new(Some("v0")).options(options(40)).opened(true),
    );
    let id = select.stable_id();
    let bar = menu_of(&context, id).scrollbar.unwrap();
    let x = bar.lane.x + bar.lane.width / 2.0;
    let y = bar.track.y + bar.track.height - 2.0;
    let mut adapter = TestInput::default();
    adapter
        .dispatch(
            &mut context,
            document(),
            &pointer_at(PointerPhase::Down, x, y),
        )
        .unwrap();
    let track = bar.track_geometry(nana_ui_core::ScrollbarAxis::Vertical);
    let jumped = menu_of(&context, id).scroll;
    assert_eq!(jumped, track.offset_for_position(y));
    assert!(jumped > 0.0);
    adapter
        .dispatch(
            &mut context,
            document(),
            &pointer_at(PointerPhase::Cancel, x, y),
        )
        .unwrap();
    assert_eq!(menu_of(&context, id).scroll, 0.0);
    let scroll = context.read(select, |select| select.menu_scroll).unwrap();
    assert!(scroll.drag.is_none() && !scroll.hot);
}

#[test]
fn arrow_keys_bring_the_highlighted_row_into_view() {
    let mut context = AppContext::new();
    let (_, select) = page_with(
        &mut context,
        0.0,
        Select::new(Some("v0")).options(options(40)).opened(true),
    );
    let id = select.stable_id();
    assert!(context.focus_node(document(), id).unwrap());
    let mut adapter = TestInput::default();
    for _ in 0..25 {
        adapter
            .dispatch(&mut context, document(), &arrow_down())
            .unwrap();
    }
    assert_eq!(
        context.read(select, |select| select.highlighted).unwrap(),
        Some(25)
    );
    let menu = menu_of(&context, id);
    let row = menu.options[25].bounds;
    assert!(row.y >= menu.viewport.y);
    assert!(row.y + row.height <= menu.viewport.y + menu.viewport.height);
}

/// A press lands on the row drawn under the pointer, wherever the page
/// above the field is scrolled to.
#[test]
fn a_press_chooses_the_row_drawn_under_it_in_a_scrolled_page() {
    let mut context = AppContext::new();
    let (page, select) = page_with(
        &mut context,
        420.0,
        Select::new(None::<Arc<str>>).options(options(40)),
    );
    let id = select.stable_id();
    assert!(
        context
            .scroll_to(page, ScrollOffset { x: 0.0, y: 400.0 })
            .unwrap()
    );
    context.rebuild_hit_test(document());
    let changes = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&changes);
    context
        .on(select, move |_, event: &SelectChanged, _| {
            sink.lock().unwrap().push(event.value.clone())
        })
        .unwrap();
    let field = context.world().layout_box(id).unwrap();
    let (x, y) = on_screen(&context, id, centre(field));
    assert!(y < 300.0, "the field shows in the window");
    let mut adapter = TestInput::default();
    let click = |adapter: &mut TestInput, context: &mut AppContext, (x, y): (f32, f32)| {
        for phase in [PointerPhase::Down, PointerPhase::Up] {
            adapter
                .dispatch(context, document(), &pointer_at(phase, x, y))
                .unwrap();
        }
    };
    click(&mut adapter, &mut context, (x, y));
    assert!(context.read(select, |select| select.opened).unwrap());
    context.rebuild_hit_test(document());
    let menu = menu_of(&context, id);
    // It opens where the window shows it: under the field the page scrolled
    // up, capped to the window's bottom, not to where the field would sit
    // unscrolled.
    let surface = menu.surface;
    let top = on_screen(&context, id, (surface.x, surface.y)).1;
    let bottom = on_screen(&context, id, (surface.x, surface.y + surface.height)).1;
    assert!(top >= 0.0 && bottom <= 300.0, "menu at {top}..{bottom}");
    assert!(menu.max_scroll > 0.0);
    let row = menu
        .options
        .iter()
        .position(|option| {
            let (_, y) = on_screen(&context, id, centre(option.bounds));
            (0.0..300.0).contains(&y)
        })
        .expect("a row shows in the window");
    let point = on_screen(&context, id, centre(menu.options[row].bounds));
    click(&mut adapter, &mut context, point);
    assert_eq!(
        changes.lock().unwrap().as_slice(),
        [Arc::<str>::from(format!("v{row}"))]
    );
}

/// Dropdown and SearchDropdown open the same menu, and it scrolls the same.
#[test]
fn dropdowns_scroll_their_menus_too() {
    let mut context = AppContext::new();
    let dropdown = Dropdown::single(Some("v0"))
        .options((0..40).map(|n| DropdownOption::new(format!("v{n}"), format!("Option {n}"))))
        .opened(true);
    let (_, dropdown) = page_with(&mut context, 0.0, dropdown);
    let menu = menu_of(&context, dropdown.stable_id());
    let (x, y) = centre(menu.viewport);
    assert!(
        TestInput::default()
            .dispatch(&mut context, document(), &wheel_at(x, y, -120.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(menu_of(&context, dropdown.stable_id()).scroll, 120.0);

    let mut context = AppContext::new();
    let search = SearchDropdown::new(Some("v0"))
        .options((0..40).map(|n| SearchDropdownOption::new(format!("v{n}"), format!("Option {n}"))))
        .opened(true);
    let (_, search) = page_with(&mut context, 0.0, search);
    let id = search.stable_id();
    let menu = menu_of(&context, id);
    let (x, y) = centre(menu.viewport);
    assert!(
        TestInput::default()
            .dispatch(&mut context, document(), &wheel_at(x, y, -120.0))
            .unwrap()
            .prevent_default
    );
    assert_eq!(menu_of(&context, id).scroll, 120.0);
    // A new query lists other rows and shows the first of them.
    context
        .update_component(search, |search, _| {
            search.set_query("Option 3");
        })
        .unwrap();
    let menu = menu_of(&context, id);
    assert_eq!(menu.scroll, 0.0);
    assert_eq!(menu.options.len(), 11);
}

/// Inside a card that clips what it holds, the menu opens into the card:
/// rows never go where the card would cut them away.
#[test]
fn a_menu_opens_inside_the_card_that_clips_it() {
    let mut context = AppContext::new();
    let root = context
        .create_component(
            document(),
            Stack::column(0.0).with_layout(|layout| {
                layout.width = Some(LengthSpec::Px(400.0));
            }),
        )
        .unwrap();
    let header = context
        .create_detached_component(
            document(),
            Stack::column(0.0).with_layout(|layout| {
                layout.width = Some(LengthSpec::Fill);
                layout.height = Some(LengthSpec::Px(100.0));
            }),
        )
        .unwrap();
    context.append_child(root, header).unwrap();
    let mut style = NodeStyle::default();
    {
        let layout = Arc::make_mut(&mut style.layout);
        layout.width = Some(LengthSpec::Px(400.0));
        layout.height = Some(LengthSpec::Px(150.0));
    }
    let card = context
        .create_detached_component(
            document(),
            ScrollView::new(ScrollAxes::Vertical).style(style),
        )
        .unwrap();
    context.append_child(root, card).unwrap();
    let select = context
        .create_detached_component(document(), Select::new(Some("v0")).options(options(40)))
        .unwrap();
    context.append_child(card, select).unwrap();
    settle(&mut context);
    assert!(context.toggle_select(select).unwrap());
    let card = context.world().layout_box(card.stable_id()).unwrap();
    let menu = menu_of(&context, select.stable_id());
    // The field sits at the card's top: down into the card, capped at its
    // bottom rather than the window's.
    assert_eq!(menu.surface.y, 132.0);
    assert_eq!(menu.surface.y + menu.surface.height, card.y + card.height);
    assert!(menu.max_scroll > 0.0);
}

/// A menu opened across the next group of the page is above that group:
/// the pointer reaches its rows even where a later sibling with the same
/// z-index lies under them, as the scene paints the menu last.
#[test]
fn a_menu_takes_the_pointer_over_a_later_sibling_group() {
    let mut context = AppContext::new();
    let root = context
        .create_component(
            document(),
            Stack::column(0.0).with_layout(|layout| {
                layout.width = Some(LengthSpec::Px(400.0));
            }),
        )
        .unwrap();
    let group = |height: f32| {
        Stack::column(0.0).hittable().with_layout(move |layout| {
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Px(height));
            layout.position = nana_ui_core::PositionSpec::Relative;
            layout.z_index = Some(1);
        })
    };
    let first = context
        .create_detached_component(document(), group(40.0))
        .unwrap();
    context.append_child(root, first).unwrap();
    let select = context
        .create_detached_component(document(), Select::new(Some("v0")).options(options(5)))
        .unwrap();
    context.append_child(first, select).unwrap();
    let second = context
        .create_detached_component(document(), group(200.0))
        .unwrap();
    context.append_child(root, second).unwrap();
    settle(&mut context);
    let below = (200.0, 80.0);
    assert_eq!(
        context.world().hit_test(document(), below.0, below.1),
        Some(second.stable_id())
    );
    assert!(context.toggle_select(select).unwrap());
    context.rebuild_hit_test(document());
    let menu = menu_of(&context, select.stable_id());
    assert!(
        menu.viewport.contains(below.0, below.1),
        "{:?}",
        menu.viewport
    );
    assert_eq!(
        context.world().hit_test(document(), below.0, below.1),
        Some(select.stable_id())
    );
    assert_eq!(
        context
            .world()
            .hit_test_candidates(document(), below.0, below.1)
            .first(),
        Some(&select.stable_id())
    );
    // Closed again, the group under it has the pointer back.
    assert!(context.toggle_select(select).unwrap());
    context.rebuild_hit_test(document());
    assert_eq!(
        context.world().hit_test(document(), below.0, below.1),
        Some(second.stable_id())
    );
}
