//! Completed-revision ancestry for the retained output composition path.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT_SURFACE: AtomicU64 = AtomicU64::new(1);

/// Stable producer identity, independent of resource/device generations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OutputSurfaceId(u64);

impl OutputSurfaceId {
    pub(crate) fn next() -> Self {
        let id = NEXT_SURFACE.fetch_add(1, Ordering::Relaxed);
        assert_ne!(id, 0, "output surface identity exhausted");
        Self(id)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// The producer and transitive producer identities actually sampled by one
/// submitted output revision. This is metadata, not another Scene tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutputProvenance {
    source: OutputSurfaceId,
    ancestry: Arc<[OutputSurfaceId]>,
}

impl OutputProvenance {
    pub const fn source(&self) -> OutputSurfaceId {
        self.source
    }

    pub fn ancestry(&self) -> &[OutputSurfaceId] {
        &self.ancestry
    }

    pub(crate) fn composed(
        source: OutputSurfaceId,
        sampled: &[Self],
    ) -> Result<Self, OutputSurfaceId> {
        if sampled
            .iter()
            .any(|dependency| dependency.ancestry.contains(&source))
        {
            return Err(source);
        }
        let mut ancestry = vec![source];
        for dependency in sampled {
            ancestry.extend_from_slice(&dependency.ancestry);
        }
        ancestry.sort_unstable();
        ancestry.dedup();
        Ok(Self {
            source,
            ancestry: ancestry.into(),
        })
    }
}
