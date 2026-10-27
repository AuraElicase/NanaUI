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
