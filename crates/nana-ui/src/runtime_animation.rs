//! Hosted-clock adapter for the backend-neutral Runtime AnimationSystem.

use std::time::{Duration, Instant};

use nana_ui_runtime::{AnimationFrame, AppContext};

/// Maps Runtime's explicit monotonic durations to the host's `Instant` epoch.
/// It owns no timer or redraw policy; hosted programs combine the returned
/// deadline with their other wake sources and consume samples on wake.
#[derive(Debug, Clone, Copy)]
pub struct RuntimeAnimationClock {
    epoch: Instant,
}

impl RuntimeAnimationClock {
    pub fn new(epoch: Instant) -> Self {
        Self { epoch }
    }

    pub fn now() -> Self {
        Self::new(Instant::now())
    }

    pub const fn epoch(self) -> Instant {
        self.epoch
    }

    pub fn runtime_time(self, now: Instant) -> Duration {
        now.saturating_duration_since(self.epoch)
    }

    pub fn next_wakeup(self, context: &AppContext) -> Option<Instant> {
        self.epoch.checked_add(context.next_animation_deadline()?)
    }

    pub fn wake(self, context: &mut AppContext, now: Instant) -> AnimationFrame {
        context.advance_animations(self.runtime_time(now))
    }
}

#[cfg(test)]
mod tests {
    use nana_ui_runtime::{AnimationId, AnimationSpec, Easing, NodeKind};

    use super::*;

    #[derive(Debug)]
    struct View;

    #[test]
    fn line_wheel_wakes_the_host_until_the_effective_offset_finishes() {
        use nana_ui_platform::{InputModifiers, InputPayload, PointerId, WheelInput, WheelUnit};
        use nana_ui_runtime::{
            DocumentId, HeadlessInput, LayoutBox, MutationQueue, ScrollAxes, ScrollMetrics,
            ScrollView,
        };
        let epoch = Instant::now();
        let clock = RuntimeAnimationClock::new(epoch);
        let mut cx = AppContext::new();
        let doc = DocumentId::new(1).unwrap();
        let scroll = cx
            .create_component(
                doc,
                ScrollView::new(ScrollAxes::Vertical)
                    .scrollbars(nana_ui_core::ScrollbarVisibility::Hidden),
            )
            .unwrap();
        let mut q = MutationQueue::new();
        q.write_layout(
            scroll.stable_id(),
            LayoutBox {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        cx.commit_mutations(q).unwrap();
        cx.set_scroll_metrics(
            scroll,
            ScrollMetrics {
                viewport_width: 100.0,
                viewport_height: 100.0,
                content_width: 100.0,
                content_height: 1000.0,
                origin_x: 0.0,
                origin_y: 0.0,
            },
        )
        .unwrap();
        cx.rebuild_hit_test(doc);
        let mut input = HeadlessInput::bind(&mut cx, doc);
        input
            .route(
                &mut cx,
                InputPayload::Wheel(WheelInput {
                    pointer_id: PointerId(1),
                    x: 10.0,
                    y: 10.0,
                    delta_x: 0.0,
                    delta_y: -1.0,
                    unit: WheelUnit::Lines,
                    modifiers: InputModifiers::default(),
                }),
            )
            .unwrap();
        let wakeup = clock
            .next_wakeup(&cx)
            .expect("a line wheel requests a host frame");
        assert!(wakeup > epoch);
        let frame = clock.wake(&mut cx, epoch + Duration::from_millis(90));
        assert!(frame.has_updates());
        let middle = cx.world().scroll_offset(scroll.stable_id()).unwrap().y;
        assert!(middle > 0.0 && middle < 60.0);
        clock.wake(&mut cx, epoch + Duration::from_millis(180));
        assert_eq!(
            cx.world().scroll_offset(scroll.stable_id()).unwrap().y,
            60.0
        );
        assert_eq!(clock.next_wakeup(&cx), None);
    }

    #[test]
    fn maps_runtime_deadline_without_owning_redraw_policy() {
        let epoch = Instant::now();
        let clock = RuntimeAnimationClock::new(epoch);
        let mut context = AppContext::new();
        let document = nana_ui_runtime::DocumentId::new(1).unwrap();
        let view = context
            .create_view(document, NodeKind::Document, View)
            .unwrap();
        context
            .update(view, |_view, cx| {
                let target = cx.entity().stable_id();
                cx.mutations().start_animation(AnimationSpec::new(
                    AnimationId::new(1).unwrap(),
                    target,
                    Duration::from_millis(10),
                    Duration::from_millis(20),
                    Duration::from_millis(5),
                    Easing::Linear,
                ));
            })
            .unwrap();

        assert_eq!(
            clock.next_wakeup(&context),
            epoch.checked_add(Duration::from_millis(10))
        );
        assert!(clock.wake(&mut context, epoch).samples.is_empty());
        let frame = clock.wake(&mut context, epoch + Duration::from_millis(10));
        assert_eq!(frame.samples.len(), 1);
        assert_eq!(frame.next_deadline, Some(Duration::from_millis(15)));
        assert_eq!(
            clock.next_wakeup(&context),
            epoch.checked_add(Duration::from_millis(15))
        );
    }

    #[test]
    fn a_stopped_run_wakes_the_host_to_report_its_end() {
        let epoch = Instant::now();
        let clock = RuntimeAnimationClock::new(epoch);
        let mut context = AppContext::new();
        let document = nana_ui_runtime::DocumentId::new(1).unwrap();
        let view = context
            .create_view(document, NodeKind::Document, View)
            .unwrap();
        let id = AnimationId::new(1).unwrap();
        context
            .update(view, |_view, cx| {
                let target = cx.entity().stable_id();
                cx.mutations().start_animation(AnimationSpec::new(
                    id,
                    target,
                    Duration::ZERO,
                    Duration::from_secs(5),
                    Duration::from_millis(16),
                    Easing::Linear,
                ));
            })
            .unwrap();
        clock.wake(&mut context, epoch + Duration::from_millis(20));
        context
            .update(view, |_view, cx| cx.mutations().stop_animation(id))
            .unwrap();

        // Nothing is left to sample, but the end has not been reported yet.
        let due = clock
            .next_wakeup(&context)
            .expect("a wake to report the end");
        assert!(due <= epoch + Duration::from_millis(20));
        let frame = clock.wake(&mut context, epoch + Duration::from_millis(21));
        assert_eq!(frame.events.len(), 1);
        assert_eq!(
            frame.events[0].kind,
            nana_ui_runtime::AnimationEventKind::Cancelled
        );
        assert_eq!(clock.next_wakeup(&context), None);
    }
}
