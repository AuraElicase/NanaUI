//! The focused editor's caret blink: a paint overlay on the animation clock.
//!
//! The caret's position, the IME anchor and scrolling never see the blink;
//! only extraction leaves the caret out in the hidden half. The clock is
//! asked for a wake only while a caret blinks, so a window with nothing to
//! type into sleeps.

use std::time::Duration;

use crate::{AnimationFrame, AppContext, StableNodeId};

/// How long the caret stays shown, then hidden.
pub(crate) const CARET_BLINK_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Default)]
pub(super) struct CaretBlink {
    /// When the current half ends. `None` until the clock next advances,
    /// which starts a shown half: a reset does not know the time.
    next_toggle: Option<Duration>,
    hidden: bool,
    /// A reset showed a hidden caret outside a clock advance; the next one
    /// reports it so the host draws a frame.
    revealed: bool,
}

impl AppContext {
    /// The carets that blink: each source's editor, while its window has
    /// focus. A source that never heard about window focus (a headless one)
    /// counts as focused.
    fn blinking_carets(&self) -> Vec<StableNodeId> {
        let mut carets = Vec::new();
        for state in self.input.sources.values() {
            if state.disconnected || (state.focus_ever && !state.focused) {
                continue;
            }
            if let Some(node) = state.effects.caret_node()
                && self.world.is_mounted(node)
                && !carets.contains(&node)
            {
                carets.push(node);
            }
        }
        carets
    }

    /// Show the caret and start its blink over at the next clock advance.
    pub(super) fn reset_caret_blink(&mut self) {
        let blink = &mut self.input.caret_blink;
        blink.next_toggle = None;
        if std::mem::take(&mut blink.hidden) {
            blink.revealed = true;
            self.world.set_hidden_carets(&[]);
        }
    }

    /// When the blink next changes, on the animation clock; `None` while no
    /// caret blinks.
    pub(crate) fn caret_blink_deadline(&self) -> Option<Duration> {
        if self.blinking_carets().is_empty() {
            return None;
        }
        // Zero: due at once, to start the half (and draw a revealed caret).
        Some(self.input.caret_blink.next_toggle.unwrap_or(Duration::ZERO))
    }

    /// Advance the blink to `now`, adding the editors whose caret changed
    /// to `frame` so the host draws them.
    pub(crate) fn advance_caret_blink(&mut self, now: Duration, frame: &mut AnimationFrame) {
        let carets = self.blinking_carets();
        if carets.is_empty() {
            self.input.caret_blink = CaretBlink::default();
            self.world.set_hidden_carets(&[]);
            return;
        }
        let blink = &mut self.input.caret_blink;
        let mut changed = std::mem::take(&mut blink.revealed);
        match blink.next_toggle {
            None => blink.next_toggle = Some(now + CARET_BLINK_INTERVAL),
            Some(at) if now >= at => {
                blink.hidden = !blink.hidden;
                // A late wake (a stalled loop) starts the next half now
                // rather than toggling again to catch up.
                blink.next_toggle = Some(now + CARET_BLINK_INTERVAL);
                changed = true;
            }
            Some(_) => {}
        }
        let hidden = blink.hidden;
        self.world
            .set_hidden_carets(if hidden { &carets } else { &[] });
        if changed {
            for caret in carets {
                if !frame.component_updates.contains(&caret) {
                    frame.component_updates.push(caret);
                }
            }
        }
    }
}
