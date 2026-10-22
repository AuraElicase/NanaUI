//! Folding one shape into another: a scale cross-fade. Each side scales
//! toward the other's box about its transform origin and fades on one timing,
//! so the eye reads one shape changing size (a dock folding into a bar, a
//! card into a pill). Presentation only: neither layout moves.
//!
//! [`NodeMotion::fold`](crate::NodeMotion::fold) runs one side; the caller
//! starts both sides with the same `now` and duration so they land together.
//! Their curves are the caller's choreography. Two linear fades cross at half
//! opacity each and the pair reads as a dip; the fading side wants a curve
//! that stays solid until it is nearly the other's size, the appearing side
//! one that rises early, so one of the two is always nearly opaque.

use nana_ui_core::PaintTransform;

use crate::{AnimatableProperty, LayoutBox, MotionValue, StableNodeId, UiWorld};

/// A side's presented look: its opacity and its paint transform.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    pub opacity: f32,
    pub transform: PaintTransform,
}

impl Pose {
    /// Shown at its own size.
    pub const SHOWN: Self = Self::shown(PaintTransform::IDENTITY);
    /// Faded out at its own size.
    pub const GONE: Self = Self::faded(PaintTransform::IDENTITY);

    pub const fn new(opacity: f32, transform: PaintTransform) -> Self {
        Self { opacity, transform }
    }

    /// Shown, under `transform`.
    pub const fn shown(transform: PaintTransform) -> Self {
        Self::new(1.0, transform)
    }

    /// Faded out, under `transform`.
    pub const fn faded(transform: PaintTransform) -> Self {
        Self::new(0.0, transform)
    }
}

/// The large box's scale down to the small one and the small one's up to
/// the large; `None` while either box has no size.
pub fn scales(large: LayoutBox, small: LayoutBox) -> Option<(PaintTransform, PaintTransform)> {
    let usable = |bounds: LayoutBox| bounds.width > 0.5 && bounds.height > 0.5;
    (usable(large) && usable(small)).then(|| {
        (
            PaintTransform::scaling(small.width / large.width, small.height / large.height),
            PaintTransform::scaling(large.width / small.width, large.height / small.height),
        )
    })
}

/// Where `node` starts a fold: its presented pose, or faded out under
/// `hidden_at` when it shows nothing, so a side that is not there grows
/// from the other's size rather than from wherever it last stopped.
pub fn origin(world: &UiWorld, node: StableNodeId, hidden_at: PaintTransform) -> Pose {
    let now = world.animation_now();
    let opacity = match world.presentation_applied_value(node, AnimatableProperty::Opacity, now) {
        Some(MotionValue::Scalar(value)) => value,
        _ => 1.0,
    };
    if opacity <= f32::EPSILON {
        return Pose::faded(hidden_at);
    }
    let transform = match world.presentation_applied_value(node, AnimatableProperty::Transform, now)
    {
        Some(MotionValue::Transform(value)) => value,
        _ => PaintTransform::IDENTITY,
    };
    Pose::new(opacity, transform)
}

#[cfg(test)]
mod tests;
