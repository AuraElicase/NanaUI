//! Which of the application's windows a host [`WindowId`] is.
//!
//! The host allocates window ids; the application knows its windows by kind
//! (the main window, a tool card, an onboarding card). [`WindowRegistry`]
//! keeps the mapping, so every [`RuntimeProgram`](crate::RuntimeProgram) hook
//! that receives an id asks one place and matches on the kind, instead of each
//! part of the application remembering its own id and recognising its windows
//! by tag strings.
//!
//! The registry stamps each requested window with a tag of its own making, so
//! the application never writes or parses tags. Keys are the application's
//! own `Copy` type, typically an enum.

use std::sync::Arc;

use nana_ui_platform::{WindowEvent, WindowId};

use crate::RuntimeProgramContext;

/// Prefix of the tags a registry stamps; the rest is a per-registry serial.
const TAG_PREFIX: &str = "nana.window.";

/// The application's windows by kind `K`.
///
/// 1. [`Self::request`] before creating a window, and put the returned tag on
///    its [`WindowDescriptor`](crate::WindowDescriptor).
/// 2. [`Self::claim`] in [`RuntimeProgram::initialize_window`](crate::RuntimeProgram::initialize_window)
///    binds the host's id to the key.
/// 3. [`Self::key`] in every other hook says which window an id is.
/// 4. [`Self::route_event`] in [`RuntimeProgram::window_event`](crate::RuntimeProgram::window_event)
///    unbinds a window once it is closed, and [`Self::release`] in
///    [`RuntimeProgram::discard_window`](crate::RuntimeProgram::discard_window)
///    one whose creation was rolled back.
///
/// A window stays bound until it is closed, even after the application let
/// go of it, so its last events still reach the code that knows what it was.
/// Several windows can share a key while one closes and its replacement
/// opens; [`Self::window`] answers with the latest.
#[derive(Debug, Clone)]
pub struct WindowRegistry<K> {
    bound: Vec<(WindowId, K)>,
    pending: Vec<(Arc<str>, K)>,
    serial: u64,
}

impl<K: Copy + Eq> WindowRegistry<K> {
    /// A registry whose primary window is `primary`. The host creates that
    /// window itself, so it is bound from the start.
    pub fn new(primary: K) -> Self {
        Self {
            bound: vec![(WindowId::PRIMARY, primary)],
            pending: Vec::new(),
            serial: 0,
        }
    }

    /// The tag for a window about to be requested as `key`. Put it on the
    /// descriptor ([`WindowDescriptor::tag`](crate::WindowDescriptor::tag)).
    /// A new request for the same key replaces one that was never claimed.
    pub fn request(&mut self, key: K) -> Arc<str> {
        self.serial += 1;
        let tag: Arc<str> = format!("{TAG_PREFIX}{}", self.serial).into();
        self.pending.retain(|(_, pending)| *pending != key);
        self.pending.push((Arc::clone(&tag), key));
        tag
    }

    /// Drop the request for `key` that the host has not built yet; a window
    /// created for it is then claimed by nobody.
    pub fn withdraw(&mut self, key: K) {
        self.pending.retain(|(_, pending)| *pending != key);
    }

    /// The host is building window `id`: bind it to the key it was requested
    /// as. `None` for a window this registry did not request, or whose
    /// request was withdrawn.
    pub fn claim<M: Send + 'static>(
        &mut self,
        id: WindowId,
        context: &RuntimeProgramContext<M>,
    ) -> Option<K> {
        self.claim_tag(id, context.window_tag())
    }

    fn claim_tag(&mut self, id: WindowId, tag: Option<&str>) -> Option<K> {
        if let Some(key) = self.key(id) {
            return Some(key);
        }
        let tag = tag?;
        let index = self
            .pending
            .iter()
            .position(|(pending, _)| **pending == *tag)?;
        let (_, key) = self.pending.remove(index);
        self.bound.push((id, key));
        Some(key)
    }

    /// Bind `id` to `key` directly, for a host without a window service
    /// (offscreen rendering, tests).
    pub fn bind(&mut self, id: WindowId, key: K) {
        self.bound.retain(|(bound, _)| *bound != id);
        self.bound.push((id, key));
    }

    /// Unbind `id`; its key, if it was bound.
    pub fn release(&mut self, id: WindowId) -> Option<K> {
        let index = self.bound.iter().position(|(bound, _)| *bound == id)?;
        Some(self.bound.remove(index).1)
    }

    /// Which window `id` is.
    pub fn key(&self, id: WindowId) -> Option<K> {
        self.bound
            .iter()
            .find(|(bound, _)| *bound == id)
            .map(|(_, key)| *key)
    }

    /// The window bound to `key`, the latest one if several are.
    pub fn window(&self, key: K) -> Option<WindowId> {
        self.bound
            .iter()
            .rev()
            .find(|(_, bound)| *bound == key)
            .map(|(id, _)| *id)
    }

    /// Every bound window, oldest first.
    pub fn iter(&self) -> impl Iterator<Item = (WindowId, K)> + '_ {
        self.bound.iter().copied()
    }

    /// Which window `event` (delivered for window `id`) belongs to. A
    /// `Closed` event unbinds it after answering.
    pub fn route_event(&mut self, id: WindowId, event: &WindowEvent) -> Option<K> {
        if matches!(event, WindowEvent::Closed { .. }) {
            self.release(id)
        } else {
            self.key(id)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Kind {
        Main,
        Tool(u8),
    }

    fn claim(registry: &mut WindowRegistry<Kind>, id: WindowId, tag: &Arc<str>) -> Option<Kind> {
        registry.claim_tag(id, Some(tag))
    }

    #[test]
    fn requested_windows_are_claimed_by_their_tag() {
        let mut registry = WindowRegistry::new(Kind::Main);
        assert_eq!(registry.key(WindowId::PRIMARY), Some(Kind::Main));
        let first = registry.request(Kind::Tool(1));
        let second = registry.request(Kind::Tool(2));
        assert_ne!(first, second);
        assert_eq!(
            claim(&mut registry, WindowId(5), &second),
            Some(Kind::Tool(2))
        );
        assert_eq!(
            claim(&mut registry, WindowId(4), &first),
            Some(Kind::Tool(1))
        );
        assert_eq!(registry.key(WindowId(4)), Some(Kind::Tool(1)));
        assert_eq!(registry.window(Kind::Tool(2)), Some(WindowId(5)));
        // A tag is claimed once.
        assert_eq!(claim(&mut registry, WindowId(6), &first), None);
    }

    #[test]
    fn withdrawn_and_replaced_requests_are_claimed_by_nobody() {
        let mut registry = WindowRegistry::new(Kind::Main);
        let stale = registry.request(Kind::Tool(1));
        let fresh = registry.request(Kind::Tool(1));
        assert_eq!(claim(&mut registry, WindowId(3), &stale), None);
        registry.withdraw(Kind::Tool(1));
        assert_eq!(claim(&mut registry, WindowId(3), &fresh), None);
        assert_eq!(registry.window(Kind::Tool(1)), None);
    }

    #[test]
    fn a_window_stays_bound_until_it_closes() {
        let mut registry = WindowRegistry::new(Kind::Main);
        let old = registry.request(Kind::Tool(1));
        claim(&mut registry, WindowId(3), &old);
        // Its replacement opens before the old one has closed.
        let new = registry.request(Kind::Tool(1));
        claim(&mut registry, WindowId(4), &new);
        assert_eq!(registry.window(Kind::Tool(1)), Some(WindowId(4)));
        let ready = WindowEvent::CloseRequested { id: WindowId(3) };
        assert_eq!(
            registry.route_event(WindowId(3), &ready),
            Some(Kind::Tool(1))
        );
        let closed = WindowEvent::Closed { id: WindowId(3) };
        assert_eq!(
            registry.route_event(WindowId(3), &closed),
            Some(Kind::Tool(1))
        );
        assert_eq!(registry.key(WindowId(3)), None);
        assert_eq!(registry.window(Kind::Tool(1)), Some(WindowId(4)));
        assert_eq!(registry.release(WindowId(4)), Some(Kind::Tool(1)));
        assert_eq!(registry.window(Kind::Tool(1)), None);
    }

    #[test]
    fn offscreen_hosts_bind_directly() {
        let mut registry = WindowRegistry::new(Kind::Main);
        registry.bind(WindowId(9), Kind::Tool(3));
        registry.bind(WindowId(9), Kind::Tool(4));
        assert_eq!(registry.key(WindowId(9)), Some(Kind::Tool(4)));
        assert_eq!(
            registry.iter().collect::<Vec<_>>(),
            vec![
                (WindowId::PRIMARY, Kind::Main),
                (WindowId(9), Kind::Tool(4))
            ]
        );
    }
}
