//! [`ScrollbarRecipe`]: what a scroll container's bar paints with, how it
//! comes and goes, and whether bars hide on their own.
//!
//! The Scrollbar family's recipe. A bar's sizes are density and live in
//! [`ThemeMetrics::scrollbar`](super::ThemeMetrics::scrollbar); this is the
//! rest of how it looks and behaves: the palette roles of the thumb at rest,
//! while its lane is hot and while it is dragged, the track behind it, the
//! fades and the growth between those states, and the application's policy
//! for when bars show — which a single scroll view may still override.

use crate::motion::Easing;
use crate::scrollbar::{ScrollbarSkin, ScrollbarVisibility};
use crate::style_model::{SemanticColor, SemanticColorMix, SemanticColorRole, StyleModelRef};

/// One scrollbar transition: how long, along which curve. CSS
/// `transition: <property> <duration> <timing-function>`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarTransition {
    pub millis: u16,
    pub easing: Easing,
}

impl ScrollbarTransition {
    pub const fn new(millis: u16, easing: Easing) -> Self {
        Self { millis, easing }
    }

    pub const fn duration(self) -> std::time::Duration {
        std::time::Duration::from_millis(self.millis as u64)
    }
}

/// How a bar comes, goes and grows. With reduced motion on none of it
/// plays: the bar shows, hides and changes thickness at once.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarMotion {
    pub fade_in: ScrollbarTransition,
    pub fade_out: ScrollbarTransition,
    /// The thumb growing to its active thickness while its lane is hot or it
    /// is dragged, and shrinking back.
    pub expand: ScrollbarTransition,
    /// How long an auto-hiding bar stays after its last reason to show — the
    /// pointer leaving, a scroll, a drag ending — before it fades out. Zero
    /// fades it out at once.
    pub hide_delay_ms: u16,
}

impl ScrollbarMotion {
    /// A short fade in, a slightly longer fade out, a quick growth, and a
    /// bar that lingers long enough to be reached after a scroll.
    pub const DEFAULT: Self = Self {
        fade_in: ScrollbarTransition::new(120, Easing::EaseOutCubic),
        fade_out: ScrollbarTransition::new(140, Easing::EaseOutCubic),
        expand: ScrollbarTransition::new(120, Easing::EaseOutCubic),
        hide_delay_ms: 600,
    };

    pub const fn hide_delay(self) -> std::time::Duration {
        std::time::Duration::from_millis(self.hide_delay_ms as u64)
    }
}

impl Default for ScrollbarMotion {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The Scrollbar family: what a bar paints with and when it shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarRecipe {
    /// The application's policy for every scroll view that does not choose
    /// its own: bars that hide on their own, resident bars, or none.
    pub visibility: ScrollbarVisibility,
    /// The thumb at rest.
    pub thumb: SemanticColorMix,
    /// The thumb while the pointer is on its lane.
    pub thumb_hot: SemanticColorMix,
    /// The thumb while it is dragged.
    pub thumb_active: SemanticColorMix,
    /// The track while the lane is hot or the thumb is dragged; `None` draws
    /// none.
    pub track_hot: Option<SemanticColorMix>,
    /// The track of a resident ([`ScrollbarVisibility::Always`]) bar at rest;
    /// `None` draws none.
    pub track_resident: Option<SemanticColorMix>,
    pub motion: ScrollbarMotion,
}

impl ScrollbarRecipe {
    /// What both built-in themes use: bars that hide on their own; a
    /// `BorderStrong` thumb that turns `Muted` while hot or dragged; a
    /// `Subtle` track while hot and behind a resident bar.
    pub const DEFAULT: Self = Self {
        visibility: ScrollbarVisibility::AutoHide,
        thumb: SemanticColorMix::role(SemanticColorRole::BorderStrong),
        thumb_hot: SemanticColorMix::role(SemanticColorRole::Muted),
        thumb_active: SemanticColorMix::role(SemanticColorRole::Muted),
        track_hot: Some(SemanticColorMix::role(SemanticColorRole::Subtle)),
        track_resident: Some(SemanticColorMix::role(SemanticColorRole::Subtle)),
        motion: ScrollbarMotion::DEFAULT,
    };

    /// The colours one axis of a bar paints with.
    ///
    /// A skin colour (CSS `::-webkit-scrollbar*`) wins over the recipe. The
    /// thumb blends from its resting role to its hot one as the lane heats
    /// up and takes the active role while dragged; the track blends from the
    /// resident track (or nothing) to the hot one. Both fade with the bar's
    /// presence.
    pub fn paint(
        &self,
        model: StyleModelRef,
        skin: Option<ScrollbarSkin>,
        state: ScrollbarPaintState,
    ) -> ScrollbarPaint {
        let presence = unit(state.presence);
        let expand = if state.dragging {
            1.0
        } else {
            unit(state.expand)
        };
        let skin_color = |rgba: [f32; 4]| SemanticColor::rgba(rgba[0], rgba[1], rgba[2], rgba[3]);
        let thumb = match skin.and_then(|skin| skin.thumb_color) {
            Some(rgba) => skin_color(rgba),
            None if state.dragging => self.thumb_active.resolve(model),
            None => blend(
                self.thumb.resolve(model),
                self.thumb_hot.resolve(model),
                expand,
            ),
        };
        let track = match skin.and_then(|skin| skin.track_color) {
            Some(rgba) => Some(skin_color(rgba)),
            None => {
                let resting = self
                    .track_resident
                    .filter(|_| state.resident)
                    .map(|mix| mix.resolve(model));
                let hot = self.track_hot.map(|mix| mix.resolve(model));
                match (resting, hot) {
                    (None, None) => None,
                    (resting, hot) => {
                        let clear = SemanticColor::rgba(0.0, 0.0, 0.0, 0.0);
                        let from = resting.unwrap_or(clear);
                        Some(blend(from, hot.unwrap_or(from), expand))
                    }
                }
            }
        };
        let fade = |color: SemanticColor| {
            SemanticColor {
                a: color.a * presence,
                ..color
            }
            .as_rgba_array()
        };
        ScrollbarPaint {
            thumb: fade(thumb),
            track: track.map(fade).filter(|rgba| rgba[3] > 0.0),
        }
    }
}

impl Default for ScrollbarRecipe {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// The state one axis of a bar is painted in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarPaintState {
    /// How far the bar has faded in, `0..=1`.
    pub presence: f32,
    /// How far the thumb has grown toward its active thickness, `0..=1`.
    pub expand: f32,
    /// The thumb is being dragged.
    pub dragging: bool,
    /// The bar is resident, so its resting track shows.
    pub resident: bool,
}

/// The colours one axis of a bar paints with, as straight-alpha RGBA.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarPaint {
    pub thumb: [f32; 4],
    /// `None` paints no track.
    pub track: Option<[f32; 4]>,
}

fn unit(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// `from` to `to` by `t`, interpolated with premultiplied alpha so a fully
/// transparent end does not drag the colour toward black.
fn blend(from: SemanticColor, to: SemanticColor, t: f32) -> SemanticColor {
    let t = unit(t);
    let a = from.a + (to.a - from.a) * t;
    if a <= 0.0 {
        return SemanticColor::rgba(0.0, 0.0, 0.0, 0.0);
    }
    let channel = |x: f32, y: f32| (x * from.a * (1.0 - t) + y * to.a * t) / a;
    SemanticColor::rgba(
        channel(from.r, to.r),
        channel(from.g, to.g),
        channel(from.b, to.b),
        a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> StyleModelRef {
        StyleModelRef::default()
    }

    fn state(presence: f32, expand: f32) -> ScrollbarPaintState {
        ScrollbarPaintState {
            presence,
            expand,
            dragging: false,
            resident: false,
        }
    }

    fn role(role: SemanticColorRole) -> [f32; 4] {
        model().color(role).as_rgba_array()
    }

    /// At rest the default recipe paints the bar today's bars always had: a
    /// `BorderStrong` thumb, no track unless the bar is resident.
    #[test]
    fn the_default_recipe_paints_the_resting_bar_as_before() {
        let recipe = ScrollbarRecipe::DEFAULT;
        let rest = recipe.paint(model(), None, state(1.0, 0.0));
        assert_eq!(rest.thumb, role(SemanticColorRole::BorderStrong));
        assert_eq!(rest.track, None);
        let resident = recipe.paint(
            model(),
            None,
            ScrollbarPaintState {
                resident: true,
                ..state(1.0, 0.0)
            },
        );
        assert_eq!(resident.track, Some(role(SemanticColorRole::Subtle)));
        let dragged = recipe.paint(
            model(),
            None,
            ScrollbarPaintState {
                dragging: true,
                ..state(1.0, 0.0)
            },
        );
        assert_eq!(dragged.thumb, role(SemanticColorRole::Muted));
        assert_eq!(dragged.track, Some(role(SemanticColorRole::Subtle)));
    }

    /// A hot lane brings the hot thumb and the hot track in as it grows.
    #[test]
    fn heating_the_lane_brings_in_the_hot_thumb_and_track() {
        let recipe = ScrollbarRecipe::DEFAULT;
        let hot = recipe.paint(model(), None, state(1.0, 1.0));
        assert_eq!(hot.thumb, role(SemanticColorRole::Muted));
        assert_eq!(hot.track, Some(role(SemanticColorRole::Subtle)));
        let half = recipe.paint(model(), None, state(1.0, 0.5));
        let track = half.track.expect("a half-hot track");
        assert!((track[3] - role(SemanticColorRole::Subtle)[3] * 0.5).abs() < 1e-5);
        // Premultiplied: fading the track in keeps its hue.
        assert!((track[0] - role(SemanticColorRole::Subtle)[0]).abs() < 1e-5);
    }

    /// Presence fades both parts; an invisible bar paints no track at all.
    #[test]
    fn presence_fades_the_whole_bar() {
        let recipe = ScrollbarRecipe::DEFAULT;
        let half = recipe.paint(model(), None, state(0.5, 1.0));
        assert!((half.thumb[3] - role(SemanticColorRole::Muted)[3] * 0.5).abs() < 1e-5);
        let gone = recipe.paint(model(), None, state(0.0, 1.0));
        assert_eq!(gone.thumb[3], 0.0);
        assert_eq!(gone.track, None);
    }

    /// A CSS colour wins over the recipe, still fading with the bar.
    #[test]
    fn a_skin_colour_wins_over_the_recipe() {
        let skin = ScrollbarSkin {
            thumb_color: Some([1.0, 0.0, 0.0, 1.0]),
            track_color: Some([0.0, 0.0, 1.0, 1.0]),
            ..ScrollbarSkin::default()
        };
        let painted = ScrollbarRecipe::DEFAULT.paint(model(), Some(skin), state(0.5, 1.0));
        assert_eq!(painted.thumb, [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(painted.track, Some([0.0, 0.0, 1.0, 0.5]));
    }

    /// A recipe with no tracks paints none, hot or resident.
    #[test]
    fn a_recipe_without_tracks_paints_none() {
        let recipe = ScrollbarRecipe {
            track_hot: None,
            track_resident: None,
            ..ScrollbarRecipe::DEFAULT
        };
        let painted = recipe.paint(
            model(),
            None,
            ScrollbarPaintState {
                resident: true,
                ..state(1.0, 1.0)
            },
        );
        assert_eq!(painted.track, None);
    }
}
