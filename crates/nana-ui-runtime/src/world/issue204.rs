//! Issue #204: an overlay stack is its own formatting context, not a flex row.
//!
//! `Stack::layers()` (`display: layers`) gives every in-flow child the same
//! area, as SwiftUI `ZStack`, Compose `Box` and Flutter `Stack` do: each child
//! aligns in it, the container takes the largest child's size, later children
//! sit above earlier ones, and an out-of-flow child keeps its own placement.

#![cfg(test)]

use std::sync::Arc;

use nana_ui_core::{AlignSpec, DirSpec, JustifySpec, LayoutStyle, LengthSpec};
use nana_ui_core::{PositionSpec, WorkCounters};

use super::reflow_oracle::{
    Builder, assert_matches_cold, bundled_face_shaper, fixed, product_frame, styled,
};
use super::{DocumentId, StableNodeId};
use crate::{
    AppContext, Button, FormattingContextKind, LayoutViewport, MutationQueue, NanaTextEngineShaper,
    ParticipationKind, Stack,
};

fn viewport() -> LayoutViewport {
    LayoutViewport::new(800.0, 600.0)
}

fn layers(stack: Stack) -> LayoutStyle {
    (*stack.style_ref().layout).clone()
}

fn sized(width: f32, height: f32) -> LayoutStyle {
    LayoutStyle {
        position: PositionSpec::Static,
        ..fixed(width, height)
    }
}

struct Page {
    context: AppContext,
    document: DocumentId,
    shaper: NanaTextEngineShaper,
    stack: StableNodeId,
    children: Vec<StableNodeId>,
}

impl Page {
    /// `stack` holding one child per entry of `children`, under a page.
    fn new(stack: LayoutStyle, children: &[LayoutStyle]) -> Self {
        let document = DocumentId::new(1).unwrap();
        let (mut b, page) = Builder::page(document, 1, 800.0);
        let id = b.element(page, stack);
        let children = children
            .iter()
            .map(|child| b.element(id, child.clone()))
            .collect();
        let mut context = AppContext::new();
        context.commit_mutations(b.queue).unwrap();
        let mut page = Self {
            context,
            document,
            shaper: bundled_face_shaper(),
            stack: id,
            children,
        };
        page.frame();
        page
    }

    fn frame(&mut self) -> WorkCounters {
        product_frame(
            &mut self.context,
            self.document,
            viewport(),
            &mut self.shaper,
        )
    }

    fn rect(&self, id: StableNodeId) -> (f32, f32, f32, f32) {
        let b = self.context.world().layout_box(id).unwrap();
        (b.x, b.y, b.width, b.height)
    }

    /// `id`'s offset from the stack's content origin.
    fn offset(&self, id: StableNodeId) -> (f32, f32) {
        let (sx, sy, ..) = self.rect(self.stack);
        let (x, y, ..) = self.rect(id);
        (x - sx, y - sy)
    }
}

#[test]
fn issue204_layers_share_one_area_and_size_to_the_largest() {
    let absolute = LayoutStyle {
        position: PositionSpec::Absolute,
        offset_left: Some(LengthSpec::Px(300.0)),
        offset_top: Some(LengthSpec::Px(0.0)),
        ..sized(500.0, 500.0)
    };
    let mut page = Page::new(
        layers(Stack::layers()),
        &[sized(100.0, 40.0), sized(60.0, 80.0), absolute],
    );
    let world = page.context.world();
    let node = world.layout_node(page.stack).unwrap();
    assert_eq!(node.established, Some(FormattingContextKind::Layers));
    let parts: Vec<_> = page
        .children
        .iter()
        .map(|id| world.layout_node(*id).unwrap().participation)
        .collect();
    assert_eq!(
        parts,
        [
            Some(ParticipationKind::LayerItem),
            Some(ParticipationKind::LayerItem),
            Some(ParticipationKind::Absolute),
        ]
    );
    // As large as the largest layer; the positioned child does not count.
    let (.., width, height) = page.rect(page.stack);
    assert_eq!((width, height), (100.0, 80.0));
    // Both layers start at the top left and keep their own sizes.
    assert_eq!(page.offset(page.children[0]), (0.0, 0.0));
    assert_eq!(page.offset(page.children[1]), (0.0, 0.0));
    assert_eq!(page.rect(page.children[0]).2, 100.0);
    assert_eq!(page.rect(page.children[1]).3, 80.0);
    assert_eq!(page.offset(page.children[2]), (300.0, 0.0));

    let mut cold = Page::new(
        layers(Stack::layers()),
        &[
            sized(100.0, 40.0),
            sized(60.0, 80.0),
            LayoutStyle {
                position: PositionSpec::Absolute,
                offset_left: Some(LengthSpec::Px(300.0)),
                offset_top: Some(LengthSpec::Px(0.0)),
                ..sized(500.0, 500.0)
            },
        ],
    );
    // A layer growing moves nothing else and still matches a cold layout.
    let mut queue = MutationQueue::new();
    queue.set_style(page.children[1], styled(sized(160.0, 80.0)));
    page.context.commit_mutations(queue).unwrap();
    page.frame();
    assert_eq!(page.rect(page.stack).2, 160.0);
    assert_eq!(page.offset(page.children[0]), (0.0, 0.0));
    let mut queue = MutationQueue::new();
    queue.set_style(cold.children[1], styled(sized(160.0, 80.0)));
    cold.context.commit_mutations(queue).unwrap();
    cold.frame();
    assert_matches_cold(&mut page.context, &mut cold.context, page.document);
}

#[test]
fn issue204_layers_align_each_child_in_the_shared_area() {
    let anchors = [AlignSpec::Start, AlignSpec::Center, AlignSpec::End];
    let justify = [JustifySpec::Start, JustifySpec::Center, JustifySpec::End];
    for (row, align) in anchors.iter().enumerate() {
        for (column, justify) in justify.iter().enumerate() {
            // The anchoring an application builds from the facade: a layer
            // covering a positioned 300x200 box, one 40x20 part at an anchor.
            let mut stack = layers(Stack::overlay_layer().align(*align).justify(*justify));
            stack.overflow_x = nana_ui_core::OverflowSpec::Visible;
            stack.overflow_y = nana_ui_core::OverflowSpec::Visible;
            let document = DocumentId::new(1).unwrap();
            let (mut b, page) = Builder::page(document, 1, 800.0);
            let frame = b.element(
                page,
                LayoutStyle {
                    position: PositionSpec::Relative,
                    ..sized(300.0, 200.0)
                },
            );
            let layer = b.element(frame, stack);
            let part = b.element(layer, sized(40.0, 20.0));
            let mut context = AppContext::new();
            context.commit_mutations(b.queue).unwrap();
            product_frame(
                &mut context,
                document,
                viewport(),
                &mut bundled_face_shaper(),
            );
            let world = context.world();
            let frame = world.layout_box(frame).unwrap();
            assert_eq!(world.layout_box(layer).unwrap(), frame);
            let part = world.layout_box(part).unwrap();
            let expected = (
                column as f32 * (300.0 - 40.0) / 2.0,
                row as f32 * (200.0 - 20.0) / 2.0,
            );
            assert_eq!(
                (part.x - frame.x, part.y - frame.y),
                expected,
                "{align:?} {justify:?}"
            );
        }
    }

    // A child's own `align-self` / `justify-self` wins, `stretch` fills the
    // area, and `start` follows the writing direction.
    let mut stack = layers(Stack::layers().width(LengthSpec::Px(300.0)));
    stack.height = Some(LengthSpec::Px(200.0));
    stack.dir = Some(DirSpec::Rtl);
    let corner = LayoutStyle {
        align_self: Some(AlignSpec::End),
        justify_self: Some(AlignSpec::End),
        ..sized(40.0, 20.0)
    };
    let fill = LayoutStyle {
        align_self: Some(AlignSpec::Stretch),
        justify_self: Some(AlignSpec::Stretch),
        ..LayoutStyle::default()
    };
    let page = Page::new(stack, &[sized(40.0, 20.0), corner, fill]);
    assert_eq!(page.offset(page.children[0]), (260.0, 0.0));
    assert_eq!(page.offset(page.children[1]), (0.0, 180.0));
    assert_eq!(page.rect(page.children[2]).2, 300.0);
    assert_eq!(page.rect(page.children[2]).3, 200.0);
}

#[test]
fn issue204_the_later_layer_takes_the_pointer() {
    let document = DocumentId::new(1).unwrap();
    let mut cx = AppContext::new();
    let stack = cx.create_component(document, Stack::layers()).unwrap();
    let below = cx
        .create_detached_component(
            document,
            Button::new("Below").layout(Arc::new(sized(120.0, 40.0))),
        )
        .unwrap();
    let above = cx
        .create_detached_component(
            document,
            Button::new("Above").layout(Arc::new(sized(60.0, 40.0))),
        )
        .unwrap();
    let mut queue = MutationQueue::new();
    queue.insert(stack.stable_id(), below.stable_id(), None);
    queue.insert(stack.stable_id(), above.stable_id(), None);
    cx.commit_mutations(queue).unwrap();
    product_frame(&mut cx, document, viewport(), &mut bundled_face_shaper());
    cx.rebuild_hit_test(document);
    let world = cx.world();
    let origin = world.layout_box(stack.stable_id()).unwrap();
    // Where the two overlap the later one is on top; past it, the earlier.
    assert_eq!(
        world.hit_test(document, origin.x + 30.0, origin.y + 20.0),
        Some(above.stable_id())
    );
    assert_eq!(
        world.hit_test(document, origin.x + 90.0, origin.y + 20.0),
        Some(below.stable_id())
    );
}

/// A facade is only spelling. The preset constructor, the view function and
/// `Stack::from_layout` with the preset's intent mount the same component
/// over the same nodes, with the same intent. Their first frame, a local edit
/// and an idle frame cost the same layout work. (CSS lowers at build time to
/// `Stack::from_layout` plus style data, so it takes the third path; its
/// structural parity is in `tests/view_macro.rs`.)
#[test]
fn issue204_facades_share_identity_and_layout_work() {
    use crate::view::{IntoView, button, row, widget};
    use nana_ui_core::{AlignSpec, DisplaySpec, FlexDirection};

    struct Run {
        nodes: usize,
        kinds: Vec<crate::NodeKind>,
        intent: Arc<LayoutStyle>,
        work: [(usize, usize, usize, usize); 3],
    }
    let document = DocumentId::new(1).unwrap();
    let preset_intent = LayoutStyle {
        display: Some(DisplaySpec::Flex),
        direction: Some(FlexDirection::Row),
        gap: Some(LengthSpec::Px(0.0)),
        align_items: AlignSpec::Center,
        width: Some(LengthSpec::Shrink),
        ..LayoutStyle::default()
    };
    let cost = |work: WorkCounters| {
        (
            work.entities_spawned,
            work.layout_measure_nodes,
            work.layout_placement_nodes,
            work.layout_context_transitions,
        )
    };
    let runs: Vec<Run> = (0..3)
        .map(|facade| {
            let mut cx = AppContext::new();
            let mut shaper = bundled_face_shaper();
            let intent = preset_intent.clone();
            let _view = cx
                .mount_view_root(document, move || {
                    let buttons = (0..50).map(|_| button("Save")).collect::<Vec<_>>();
                    match facade {
                        0 => widget(Stack::row(0.0)).children(buttons).into_any(),
                        1 => row().children(buttons).into_any(),
                        _ => widget(Stack::from_layout(intent))
                            .children(buttons)
                            .into_any(),
                    }
                })
                .unwrap();
            let first = product_frame(&mut cx, document, viewport(), &mut shaper);
            let order = cx.world().document_order(document);
            let root = order[0];
            let child = cx.world().child_ids(root)[7];
            let mut queue = MutationQueue::new();
            queue.set_style(child, styled(sized(90.0, 30.0)));
            cx.commit_mutations(queue).unwrap();
            let edit = product_frame(&mut cx, document, viewport(), &mut shaper);
            // The next frame is idle: nothing is left to do, and its own
            // counters (not the last non-empty snapshot) are all zero.
            product_frame(&mut cx, document, viewport(), &mut shaper);
            assert!(cx.take_system_work().is_empty(), "facade {facade}");
            let idle = cx.world().frame_counters;
            let world = cx.world();
            Run {
                nodes: world.len(),
                kinds: order
                    .iter()
                    .map(|id| world.node(*id).unwrap().kind.clone())
                    .collect(),
                intent: world.layout_node(root).unwrap().intent,
                work: [cost(first), cost(edit), cost(idle)],
            }
        })
        .collect();
    let preset = &runs[0];
    assert!(preset.work[0].1 > 0 && preset.work[1].1 > 0);
    assert_eq!(preset.work[2], (0, 0, 0, 0), "an idle frame does nothing");
    for (facade, run) in runs.iter().enumerate().skip(1) {
        assert_eq!(run.nodes, preset.nodes, "facade {facade}");
        assert_eq!(run.kinds, preset.kinds, "facade {facade}");
        assert!(
            preset.intent.changed_fields(&run.intent).is_empty(),
            "facade {facade}: {:?}",
            preset.intent.changed_fields(&run.intent)
        );
        assert_eq!(run.work, preset.work, "facade {facade}");
    }
}
