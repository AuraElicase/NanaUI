//! Hard bounds that keep the worst case of a single diagram short.
//!
//! Every bound is checked while parsing or by the layout's own work budget,
//! so a hostile or runaway source ends in [`DiagramError::TooLarge`] instead
//! of a long frame.

use crate::{DiagramError, DiagramScene, Item, LimitKind};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    pub source_bytes: usize,
    pub lines: usize,
    pub label_chars: usize,
    pub nodes: usize,
    pub edges: usize,
    pub cluster_depth: usize,
    pub clusters: usize,
    /// Per-family element bound: participants, tasks, commits, points, links,
    /// leaves. Families that need a different bound scale this one.
    pub elements: usize,
    pub items: usize,
    pub text_items: usize,
    /// Largest side of the finished drawing, in logical pixels.
    pub canvas: f32,
    /// Ordering work budget: `edges × ranks × sweeps`.
    pub order_work: u64,
}

impl Limits {
    pub const DEFAULT: Self = Self {
        source_bytes: 128_000,
        lines: 10_000,
        label_chars: 2_000,
        nodes: 1_500,
        edges: 3_000,
        cluster_depth: 16,
        clusters: 300,
        elements: 5_000,
        items: 65_536,
        text_items: 8_000,
        canvas: 16_384.0,
        order_work: 4_000_000,
    };

    pub(crate) fn check_source(&self, source: &str) -> Result<(), DiagramError> {
        if source.len() > self.source_bytes {
            return Err(DiagramError::TooLarge(LimitKind::SourceBytes));
        }
        if source.lines().count() > self.lines {
            return Err(DiagramError::TooLarge(LimitKind::Lines));
        }
        Ok(())
    }

    pub(crate) fn check_label(&self, text: &str) -> Result<(), DiagramError> {
        if text.chars().count() > self.label_chars {
            return Err(DiagramError::TooLarge(LimitKind::LabelChars));
        }
        Ok(())
    }

    pub(crate) fn check_count(
        &self,
        count: usize,
        bound: usize,
        kind: LimitKind,
    ) -> Result<(), DiagramError> {
        if count > bound {
            return Err(DiagramError::TooLarge(kind));
        }
        Ok(())
    }

    pub(crate) fn check_scene(&self, scene: &DiagramScene) -> Result<(), DiagramError> {
        if scene.items.len() > self.items {
            return Err(DiagramError::TooLarge(LimitKind::Items));
        }
        let texts = scene
            .items
            .iter()
            .filter(|item| matches!(item, Item::Text { .. }))
            .count();
        if texts > self.text_items {
            return Err(DiagramError::TooLarge(LimitKind::TextItems));
        }
        let [w, h] = scene.size;
        if !(w.is_finite() && h.is_finite()) || w > self.canvas || h > self.canvas {
            return Err(DiagramError::TooLarge(LimitKind::Canvas));
        }
        Ok(())
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self::DEFAULT
    }
}
