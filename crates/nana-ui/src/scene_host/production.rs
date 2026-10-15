//! Submission-settled production shared by visible and hidden window paths.
use nana_ui_platform::WindowId;
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub(super) struct ProducerFrame {
    window: WindowId,
    generation: u64,
    revision: Option<u64>,
}

#[derive(Default)]
pub(super) struct WindowProducerCache {
    submitted: HashMap<WindowId, (u64, u64)>,
}

impl WindowProducerCache {
    /// Generation comes from the host's actual GpuContext, never the surface
    /// resize counter. A token records intent and does not advance the cache.
    pub fn prepare(
        &mut self,
        window: WindowId,
        generation: u64,
        signature: (bool, Option<u64>),
    ) -> Option<ProducerFrame> {
        let (used, revision) = signature;
        if !used {
            self.remove(window);
            return None;
        }
        if revision
            .is_some_and(|revision| self.submitted.get(&window) == Some(&(generation, revision)))
        {
            return None;
        }
        Some(ProducerFrame {
            window,
            generation,
            revision,
        })
    }

    pub fn submitted(&mut self, frame: ProducerFrame) {
        if let Some(revision) = frame.revision {
            self.submitted
                .insert(frame.window, (frame.generation, revision));
        } else {
            self.remove(frame.window);
        }
    }

    pub fn remove(&mut self, window: WindowId) {
        self.submitted.remove(&window);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_window_production_is_cached_only_after_successful_submission() {
        let mut cache = WindowProducerCache::default();
        let window = WindowId(1);
        let first = cache.prepare(window, 1, (true, Some(11))).unwrap();
        // A failed/discarded encode never calls submitted; a retry stays dirty.
        assert!(cache.prepare(window, 1, (true, Some(11))).is_some());
        cache.submitted(first);
        for _ in 0..240 {
            assert!(cache.prepare(window, 1, (true, Some(11))).is_none());
        }
        assert!(cache.prepare(window, 1, (true, Some(12))).is_some());
        assert!(cache.prepare(window, 2, (true, Some(11))).is_some());
        assert!(cache.prepare(WindowId(2), 1, (true, Some(11))).is_some());
    }

    #[test]
    fn unused_removed_closed_and_volatile_producers_do_not_reuse_stale_cache_entries() {
        let mut cache = WindowProducerCache::default();
        let window = WindowId(1);
        let initial = cache.prepare(window, 1, (true, Some(11))).unwrap();
        cache.submitted(initial);
        assert!(cache.prepare(window, 1, (false, Some(0))).is_none());
        assert!(cache.submitted.is_empty());
        let restored = cache.prepare(window, 1, (true, Some(11))).unwrap();
        cache.submitted(restored);
        cache.remove(window);
        assert!(cache.submitted.is_empty());
        assert!(cache.prepare(window, 1, (true, Some(11))).is_some());
        for _ in 0..8 {
            let dynamic = cache.prepare(window, 1, (true, None)).unwrap();
            cache.submitted(dynamic);
        }
        assert!(cache.submitted.is_empty());
    }
}
