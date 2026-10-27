//! Scroll is a behavior of the same retained formatting context.
use super::reflow_oracle::{bundled_face_shaper, product_frame};
use crate::{
    AppContext, Button, DocumentId, GpuTextureView, InlineContent, InlineFlow, LayoutViewport,
    MutationQueue, NodeStyle, ParticipationKind, ScrollAxes, ScrollOffset, ScrollView,
};
use nana_ui_core::{
    DisplaySpec, FlexDirection, GridTrack, LayoutStyle, LengthSpec, ScrollbarVisibility,
};
use std::sync::Arc;

#[test]
fn issue201_scroll_is_orthogonal_to_flow_flex_grid_and_inline_contexts() {
    for display in [DisplaySpec::Block, DisplaySpec::Flex, DisplaySpec::Grid] {
        let document = DocumentId::new(1).unwrap();
        let mut cx = AppContext::new();
        let mut shaper = bundled_face_shaper();
        let viewport = LayoutViewport::new(400.0, 300.0);
        let scroll = cx
            .create_component(
                document,
                ScrollView::new(ScrollAxes::Both)
                    .scrollbars(ScrollbarVisibility::Hidden)
                    .with_layout(|style| {
                        style.display = Some(display);
                        style.direction = Some(FlexDirection::Row);
                        style.width = Some(LengthSpec::Px(160.0));
                        style.height = Some(LengthSpec::Px(60.0));
                        style.grid_columns = Some(vec![GridTrack::Px(80.0); 2]);
                    }),
            )
            .unwrap();
        for _ in 0..20 {
            let layout = LayoutStyle {
                display: Some(DisplaySpec::InlineBlock),
                width: Some(LengthSpec::Px(80.0)),
                height: Some(LengthSpec::Px(24.0)),
                flex_shrink: Some(0.0),
                ..Default::default()
            };
            let child = cx
                .create_detached_component(document, Button::new("Item").layout(Arc::new(layout)))
                .unwrap();
            cx.append_child(scroll, child).unwrap();
        }
        product_frame(&mut cx, document, viewport, &mut shaper);
        let world = cx.world();
        let generation = world.layout_generation();
        let context = world.layout_node(scroll.stable_id()).unwrap().established;
        let boxes: Vec<_> = world
            .document_order(document)
            .into_iter()
            .map(|id| (id, world.layout_box(id)))
            .collect();
        let mut q = MutationQueue::new();
        q.set_scroll_offset(scroll.stable_id(), ScrollOffset { x: 20.0, y: 20.0 });
        cx.commit_mutations(q).unwrap();
        let work = product_frame(&mut cx, document, viewport, &mut shaper);
        assert_eq!(
            (
                work.layout_measure_nodes,
                work.layout_placement_nodes,
                work.text_shaped,
                work.entities_spawned,
                work.entities_despawned
            ),
            (0, 0, 0, 0, 0),
            "{display:?}"
        );
        assert_eq!(cx.world().layout_generation(), generation);
        assert_eq!(
            cx.world()
                .layout_node(scroll.stable_id())
                .unwrap()
                .established,
            context
        );
        for (id, bounds) in boxes {
            assert_eq!(cx.world().layout_box(id), bounds);
        }
        let offset = cx.world().scroll_offset(scroll.stable_id()).unwrap();
        assert!(
            offset.x > 0.0 || offset.y > 0.0,
            "fixture must really scroll in {display:?}"
        );
    }
}

/// A native mixed paragraph and a host-texture custom renderer under one
/// scroller: scrolling lays nothing out, keeps the renderer's descriptor, and
/// moves hit testing and accessibility together by the offset.
#[test]
fn issue201_a_paragraph_and_a_custom_renderer_scroll_without_layout() {
    let document = DocumentId::new(1).unwrap();
    let mut cx = AppContext::new();
    let mut shaper = bundled_face_shaper();
    let viewport = LayoutViewport::new(400.0, 300.0);
    let scroll = cx
        .create_component(
            document,
            ScrollView::new(ScrollAxes::Vertical)
                .scrollbars(ScrollbarVisibility::Hidden)
                .with_layout(|style| {
                    style.display = Some(DisplaySpec::Block);
                    style.width = Some(LengthSpec::Px(300.0));
                    style.height = Some(LengthSpec::Px(100.0));
                }),
        )
        .unwrap();
    let fixed = |width: f32, height: f32| {
        Arc::new(LayoutStyle {
            width: Some(LengthSpec::Px(width)),
            height: Some(LengthSpec::Px(height)),
            ..Default::default()
        })
    };
    let atom = cx
        .create_detached_component(document, Button::new("Go").layout(fixed(40.0, 20.0)))
        .unwrap();
    let paragraph = cx
        .create_detached_component(
            document,
            InlineFlow::new(
                InlineContent::builder()
                    .text("Status ")
                    .child(atom.stable_id())
                    .text(" and a line of text long enough to wrap in the scroller")
                    .build(),
            )
            .width(LengthSpec::Px(280.0)),
        )
        .unwrap();
    let texture = cx
        .create_detached_component(
            document,
            GpuTextureView::new("preview")
                .label("Preview")
                .with_pointer_events(true)
                .style(NodeStyle {
                    layout: fixed(200.0, 160.0),
                    ..Default::default()
                }),
        )
        .unwrap();
    cx.append_child(scroll, paragraph).unwrap();
    cx.append_child(scroll, texture).unwrap();
    product_frame(&mut cx, document, viewport, &mut shaper);
    cx.rebuild_hit_test(document);

    let world = cx.world();
    assert_eq!(
        world.layout_node(atom.stable_id()).unwrap().participation,
        Some(ParticipationKind::AtomicInline)
    );
    let render = world.custom_render(texture.stable_id()).cloned().unwrap();
    let generation = world.layout_generation();
    let boxes: Vec<_> = world
        .document_order(document)
        .into_iter()
        .map(|id| (id, world.layout_box(id)))
        .collect();
    let texture_box = world.layout_box(texture.stable_id()).unwrap();
    // The content overflows the 100px viewport; scroll most of the way.
    assert!(
        texture_box.y + texture_box.height > 180.0,
        "{texture_box:?}"
    );
    let offset = 80.0;

    let mut q = MutationQueue::new();
    q.set_scroll_offset(scroll.stable_id(), ScrollOffset { x: 0.0, y: offset });
    cx.commit_mutations(q).unwrap();
    let work = product_frame(&mut cx, document, viewport, &mut shaper);
    assert_eq!(
        (
            work.layout_measure_nodes,
            work.layout_placement_nodes,
            work.text_shaped,
            work.entities_spawned,
            work.entities_despawned
        ),
        (0, 0, 0, 0, 0)
    );
    // The hit index takes the scroll as an in-place patch, not a rebuild.
    for (scroller, delta) in cx.take_scroll_hit_updates() {
        cx.update_hit_test_scroll(document, scroller, delta);
    }
    let world = cx.world();
    assert_eq!(world.layout_generation(), generation);
    for (id, bounds) in boxes {
        assert_eq!(world.layout_box(id), bounds);
    }
    assert_eq!(world.custom_render(texture.stable_id()), Some(&render));
    assert_eq!(world.scroll_offset(scroll.stable_id()).unwrap().y, offset);

    // Hit testing and accessibility both see the texture where it scrolled to.
    let moved_y = texture_box.y - offset;
    // A point inside the viewport, on the part of the texture still shown.
    let center = (texture_box.x + 20.0, moved_y + 60.0);
    assert!((0.0..100.0).contains(&center.1));
    assert_eq!(
        world.hit_test(document, center.0, center.1),
        Some(texture.stable_id())
    );
    let accessible = world
        .project_accessibility(document)
        .into_iter()
        .find(|node| node.id == texture.stable_id())
        .unwrap()
        .bounds;
    assert!((accessible.y - moved_y).abs() < 0.01, "{accessible:?}");
    // The paragraph's atom scrolled up out of the viewport with it: its old
    // place no longer reaches it.
    let atom_box = world.layout_box(atom.stable_id()).unwrap();
    assert!(atom_box.y + atom_box.height < offset, "{atom_box:?}");
    assert_ne!(
        world.hit_test(
            document,
            atom_box.x + atom_box.width / 2.0,
            atom_box.y + atom_box.height / 2.0
        ),
        Some(atom.stable_id())
    );
}
