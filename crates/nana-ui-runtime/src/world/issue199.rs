//! Real retained components through context and placement transitions.
#![cfg(test)]

use super::reflow_oracle::{bundled_face_shaper, product_frame, styled};
use crate::{
    AppContext, Button, CustomRenderNode, DocumentId, FormattingContextKind, LayoutViewport,
    MutationQueue, ParticipationKind, Stack, TextInput,
};
use nana_ui_core::{DisplaySpec, FlexDirection, GridTrack, LayoutStyle, LengthSpec, PositionSpec};
use std::sync::Arc;

fn container(display: DisplaySpec, direction: FlexDirection) -> LayoutStyle {
    LayoutStyle {
        display: Some(display),
        direction: Some(direction),
        width: Some(LengthSpec::Px(600.0)),
        grid_columns: (display == DisplaySpec::Grid).then(|| vec![GridTrack::Px(40.0); 10]),
        ..Default::default()
    }
}
fn item(position: PositionSpec) -> LayoutStyle {
    LayoutStyle {
        display: Some(DisplaySpec::InlineBlock),
        width: Some(LengthSpec::Px(40.0)),
        height: Some(LengthSpec::Px(24.0)),
        position,
        ..Default::default()
    }
}

#[test]
fn issue199_a_thousand_components_keep_state_focus_and_resource_identity() {
    let document = DocumentId::new(1).unwrap();
    let viewport = LayoutViewport::new(800.0, 600.0);
    let mut cx = AppContext::new();
    let mut shaper = bundled_face_shaper();
    let root = cx
        .create_component(
            document,
            Stack::from_layout(container(DisplaySpec::Flex, FlexDirection::Row)),
        )
        .unwrap();
    let editor = cx
        .create_detached_component(
            document,
            TextInput::new("retained draft").layout(Arc::new(item(PositionSpec::Static))),
        )
        .unwrap();
    let mut children = vec![editor.stable_id()];
    let mut resources = Vec::new();
    let mut queue = MutationQueue::new();
    for n in 1..1_000 {
        let node = if n % 100 == 0 {
            let node = cx
                .create_detached_component(document, Stack::from_layout(item(PositionSpec::Static)))
                .unwrap()
                .stable_id();
            let descriptor = CustomRenderNode::new("fixture-renderer", format!("resource-{n}"), 7);
            resources.push((node, descriptor.clone()));
            queue.set_custom_render(node, Some(descriptor));
            node
        } else {
            cx.create_detached_component(
                document,
                Button::new("Save").layout(Arc::new(item(PositionSpec::Static))),
            )
            .unwrap()
            .stable_id()
        };
        children.push(node);
    }
    for &child in &children {
        queue.insert(root.stable_id(), child, None);
    }
    cx.commit_mutations(queue).unwrap();
    product_frame(&mut cx, document, viewport, &mut shaper);
    cx.focus_node(document, editor.stable_id()).unwrap();
    product_frame(&mut cx, document, viewport, &mut shaper);
    let nodes = cx.world().len();
    let before = cx.read(editor, |v| v.state.clone()).unwrap();
    let mut cross_hits = 0;
    let mut cross_misses = 0;
    for (display, direction, context) in [
        (
            DisplaySpec::Grid,
            FlexDirection::Row,
            FormattingContextKind::Grid,
        ),
        (
            DisplaySpec::Layers,
            FlexDirection::Row,
            FormattingContextKind::Layers,
        ),
        (
            DisplaySpec::Block,
            FlexDirection::Row,
            FormattingContextKind::Inline,
        ),
        (
            DisplaySpec::Flex,
            FlexDirection::Row,
            FormattingContextKind::Flex,
        ),
        (
            DisplaySpec::Flex,
            FlexDirection::Column,
            FormattingContextKind::Flex,
        ),
    ] {
        let mut q = MutationQueue::new();
        q.set_style(root.stable_id(), styled(container(display, direction)));
        cx.commit_mutations(q).unwrap();
        let work = product_frame(&mut cx, document, viewport, &mut shaper);
        assert_eq!((work.entities_spawned, work.entities_despawned), (0, 0));
        assert_eq!(cx.world().len(), nodes);
        assert_eq!(cx.world().child_ids(root.stable_id()), children);
        assert_eq!(
            cx.world()
                .layout_node(root.stable_id())
                .unwrap()
                .established,
            Some(context)
        );
        assert_eq!(cx.read(editor, |v| v.state.clone()).unwrap(), before);
        assert_eq!(cx.world().focused(document), Some(editor.stable_id()));
        for (id, resource) in &resources {
            let retained = cx.world().custom_render(*id).unwrap();
            assert_eq!(retained, resource);
            assert!(Arc::ptr_eq(&retained.resource, &resource.resource));
        }
        cross_hits += work.cross_context_measure_hits;
        cross_misses += work.cross_context_measure_misses;
    }
    for (position, part) in [
        (PositionSpec::Absolute, ParticipationKind::Absolute),
        (PositionSpec::Fixed, ParticipationKind::Fixed),
        (PositionSpec::Static, ParticipationKind::FlexItem),
    ] {
        let mut q = MutationQueue::new();
        for &child in &children {
            q.set_style(child, styled(item(position)));
        }
        cx.commit_mutations(q).unwrap();
        let work = product_frame(&mut cx, document, viewport, &mut shaper);
        assert_eq!((work.entities_spawned, work.entities_despawned), (0, 0));
        assert_eq!(cx.world().len(), nodes);
        for &child in &children {
            assert_eq!(
                cx.world().layout_node(child).unwrap().participation,
                Some(part)
            );
        }
        assert_eq!(cx.read(editor, |v| v.state.clone()).unwrap(), before);
        assert_eq!(cx.world().focused(document), Some(editor.stable_id()));
        for (id, resource) in &resources {
            assert!(Arc::ptr_eq(
                &cx.world().custom_render(*id).unwrap().resource,
                &resource.resource
            ));
        }
    }
    product_frame(&mut cx, document, viewport, &mut shaper);
    assert!(cx.take_system_work().is_empty());
    let idle = cx.world().frame_counters;
    assert_eq!(
        (
            idle.layout_measure_nodes,
            idle.layout_placement_nodes,
            idle.layout_context_transitions,
            idle.entities_spawned,
            idle.entities_despawned
        ),
        (0, 0, 0, 0, 0)
    );
    assert_eq!(
        cross_misses, 0,
        "compatible constraints must reuse intrinsic facts"
    );
    println!(
        "1k context transitions: cross-context intrinsic hits={cross_hits}, misses={cross_misses}"
    );
}

/// The same container through Flex → Grid → Layers → Inline → Flex many
/// times. What the retained cache holds settles after the first round: a
/// context it left keeps no grid tracks or line plans, and the cache matches
/// one that only ever laid the container out as Flex.
#[test]
fn issue199_repeated_context_switches_keep_the_retained_cache_flat() {
    use crate::layout_engine::RetainedLayoutFootprint;
    let document = DocumentId::new(1).unwrap();
    let viewport = LayoutViewport::new(800.0, 600.0);
    let mount = |cx: &mut AppContext| {
        let root = cx
            .create_component(
                document,
                Stack::from_layout(container(DisplaySpec::Flex, FlexDirection::Row)),
            )
            .unwrap();
        let mut queue = MutationQueue::new();
        for _ in 0..100 {
            let child = cx
                .create_detached_component(
                    document,
                    Button::new("Save").layout(Arc::new(item(PositionSpec::Static))),
                )
                .unwrap();
            queue.insert(root.stable_id(), child.stable_id(), None);
        }
        cx.commit_mutations(queue).unwrap();
        root.stable_id()
    };
    let mut cx = AppContext::new();
    let mut shaper = bundled_face_shaper();
    let root = mount(&mut cx);
    product_frame(&mut cx, document, viewport, &mut shaper);
    let nodes = cx.world().len();
    // Intrinsic facts are the cross-context cache: kept on purpose, held to
    // their budget below instead of compared.
    let flat = |footprint: RetainedLayoutFootprint| RetainedLayoutFootprint {
        intrinsics: 0,
        intrinsic_metrics: 0,
        last_frontier: 0,
        ..footprint
    };
    let round = [
        DisplaySpec::Grid,
        DisplaySpec::Layers,
        DisplaySpec::Block,
        DisplaySpec::Flex,
    ];
    let mut settled = None;
    let mut in_grid = None;
    for _ in 0..20 {
        for display in round {
            let mut q = MutationQueue::new();
            q.set_style(root, styled(container(display, FlexDirection::Row)));
            cx.commit_mutations(q).unwrap();
            let work = product_frame(&mut cx, document, viewport, &mut shaper);
            assert_eq!(work.layout_context_transitions, 1, "{display:?}");
            if display == DisplaySpec::Grid {
                in_grid.get_or_insert(flat(cx.retained_layout_footprint(document)));
            }
        }
        let footprint = cx.retained_layout_footprint(document);
        assert_eq!(footprint.full_snapshots, 0);
        assert!(footprint.intrinsics <= 2 * nodes, "{footprint:?}");
        assert!(footprint.intrinsic_metrics <= 4 * nodes, "{footprint:?}");
        match settled {
            None => settled = Some(flat(footprint)),
            Some(settled) => assert_eq!(flat(footprint), settled),
        }
    }
    let mut cold = AppContext::new();
    mount(&mut cold);
    product_frame(&mut cold, document, viewport, &mut shaper);
    assert_eq!(
        settled.unwrap(),
        flat(cold.retained_layout_footprint(document)),
        "back in Flex, nothing from Grid, Layers or Inline is left"
    );
    // The comparison sees context state: while in Grid the cache held more.
    let (grid, flex) = (in_grid.unwrap(), settled.unwrap());
    assert!(
        grid.plan_entries > flex.plan_entries,
        "{grid:?} vs {flex:?}"
    );
}

/// A modal frame establishes the Overlay context through the same
/// dispatch and participation contract as Flex and Grid: its slots are
/// `OverlayItem`s,
/// placed by the slot contract, and an incremental change to a slot matches a
/// cold layout of the same dialog.
#[test]
fn issue199_a_modal_frame_places_its_slots_as_overlay_items() {
    use super::reflow_oracle::assert_matches_cold;
    use crate::{Dialog, ModalSlots, OverlayHost};
    let document = DocumentId::new(1).unwrap();
    let viewport = LayoutViewport::new(800.0, 600.0);
    let body_layout = |height: f32| LayoutStyle {
        width: Some(LengthSpec::Px(240.0)),
        height: Some(LengthSpec::Px(height)),
        ..Default::default()
    };
    let build = |cx: &mut AppContext| {
        let host = cx.create_component(document, OverlayHost::new()).unwrap();
        let dialog = cx
            .create_component(document, Dialog::new("Settings"))
            .unwrap();
        let body = cx
            .create_detached_component(document, Stack::from_layout(body_layout(80.0)))
            .unwrap();
        let action = cx
            .create_detached_component(document, Button::new("Save"))
            .unwrap();
        cx.append_child(host, dialog).unwrap();
        cx.set_modal_slots(
            dialog,
            ModalSlots {
                body: Some(body.stable_id()),
                actions: vec![action.stable_id()],
                ..Default::default()
            },
        )
        .unwrap();
        cx.activate_overlay(host, dialog).unwrap();
        (dialog.stable_id(), body.stable_id(), action.stable_id())
    };
    let mut shaper = bundled_face_shaper();
    let mut cx = AppContext::new();
    let (dialog, body, action) = build(&mut cx);
    product_frame(&mut cx, document, viewport, &mut shaper);
    let world = cx.world();
    assert_eq!(
        world.layout_node(dialog).unwrap().established,
        Some(FormattingContextKind::Overlay)
    );
    for slot in [body, action] {
        assert_eq!(
            world.layout_node(slot).unwrap().participation,
            Some(ParticipationKind::OverlayItem)
        );
    }
    assert!((world.layout_box(body).unwrap().height - 80.0).abs() < 0.01);

    // A taller body grows the surface around it.
    let mut q = MutationQueue::new();
    q.set_style(body, styled(body_layout(120.0)));
    cx.commit_mutations(q).unwrap();
    let work = product_frame(&mut cx, document, viewport, &mut shaper);
    // Relaying the same dialog out is not a context change.
    assert_eq!(work.layout_context_transitions, 0);
    // The slot contract measures through the surface's float chrome.
    assert!((cx.world().layout_box(body).unwrap().height - 120.0).abs() < 0.01);

    let mut cold = AppContext::new();
    let (_, cold_body, _) = build(&mut cold);
    let mut q = MutationQueue::new();
    q.set_style(cold_body, styled(body_layout(120.0)));
    cold.commit_mutations(q).unwrap();
    product_frame(&mut cold, document, viewport, &mut shaper);
    assert_matches_cold(&mut cx, &mut cold, document);
}
