//! Timeline editor (#344): an editable time base for animation, audio or data
//! editing. One painted node holds
//!
//! - a ruler in frames or seconds whose ticks thin out or fill in with the
//!   zoom, and a playhead that drags;
//! - a collapsible track tree on the left, scrolling with the key area;
//! - keys: a press selects one (Shift toggles it into the selection), a drag
//!   on empty space selects a box, a drag on a selected key moves the
//!   selection (snapped to frames);
//! - a curve view: each visible track's keys joined by cubic Bézier
//!   segments, with the selected keys' tangent handles, which drag aligned
//!   (the opposite handle turns with it) or broken (Alt, or a broken key).
//!
//! The application owns the content: it fills [`TimelineEditor::tracks`] and
//! applies the edits the events report. Drags report previews
//! (`committed: false`) and one commit on release; the timeline shows the
//! dragged state itself meanwhile. A wheel zooms time about the pointer
//! (Shift scrolls the tracks, Ctrl pans time); a middle drag pans. Keys such
//! as Delete or fitting the view belong to the application's keymap, which
//! reads [`TimelineEditor::selection`] and calls [`AppContext::fit_timeline`].

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use nana_ui_core::{LengthSpec, SemanticColorRole};

use crate::view_components::project_common;
use crate::{
    AccessibilityRole, AccessibilityState, AppContext, BoxPaint, ComponentView, Entity,
    FrameworkError, InteractionState, LayoutBox, MutationQueue, NodeKind, NodePointer,
    NodePointerHooks, NodeStyle, NodeWheel, PaintContext, PaintPath, PaintText, Painter,
    StableNodeId, StrokeStyle, UiWorld,
};

/// The ruler's height, px.
pub const TIMELINE_RULER: f32 = 24.0;
/// A track row's height, px.
pub const TIMELINE_ROW: f32 = 20.0;
/// How near a key or handle a press must land, px.
const HIT: f32 = 6.0;
/// The curve view's margin above and below the values, px.
const CURVE_MARGIN: f32 = 12.0;
/// Least spacing of labelled ticks, px.
const LABEL_SPACING: f64 = 64.0;
/// Indent per track depth, px.
const INDENT: f32 = 12.0;

/// One key: a time, a value and its two tangent handles relative to it
/// (`[dt, dv]`; the in handle points back in time, the out handle forward).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineKey {
    pub time: f64,
    pub value: f64,
    pub in_handle: [f64; 2],
    pub out_handle: [f64; 2],
    /// The handles move independently.
    pub broken: bool,
}

impl TimelineKey {
    /// A key with flat handles a third of a second long.
    pub fn new(time: f64, value: f64) -> Self {
        Self {
            time,
            value,
            in_handle: [-1.0 / 3.0, 0.0],
            out_handle: [1.0 / 3.0, 0.0],
            broken: false,
        }
    }
}

/// A row of the track tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineTrack {
    pub id: Arc<str>,
    pub label: Arc<str>,
    /// Nesting depth; a track belongs to the nearest earlier track with a
    /// smaller depth.
    pub depth: u8,
    /// `Some(open)` for a track with children; `None` for a leaf.
    pub expanded: Option<bool>,
    pub keys: Vec<TimelineKey>,
}

impl TimelineTrack {
    pub fn new(
        id: impl Into<Arc<str>>,
        label: impl Into<Arc<str>>,
        keys: impl IntoIterator<Item = TimelineKey>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            depth: 0,
            expanded: None,
            keys: keys.into_iter().collect(),
        }
    }

    pub fn depth(mut self, depth: u8) -> Self {
        self.depth = depth;
        self
    }

    /// Make this a branch, open or closed.
    pub fn expanded(mut self, open: bool) -> Self {
        self.expanded = Some(open);
        self
    }
}

/// A key by track and position in it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TimelineKeyRef {
    pub track: usize,
    pub key: usize,
}

/// The ruler's unit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TimelineUnit {
    #[default]
    Frames,
    Seconds,
}

/// What the key area shows.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum TimelineMode {
    /// Keys as diamonds on their track rows.
    #[default]
    DopeSheet,
    /// Keys as points on their value curves.
    Curves,
}

/// The visible window: time at the key area's left edge, the zoom, the curve
/// view's value range and how far the tracks scrolled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineView {
    pub start: f64,
    pub pixels_per_second: f64,
    pub value_min: f64,
    pub value_max: f64,
    pub scroll: f32,
}

impl Default for TimelineView {
    fn default() -> Self {
        Self {
            start: 0.0,
            pixels_per_second: 120.0,
            value_min: -1.0,
            value_max: 1.0,
            scroll: 0.0,
        }
    }
}

/// The playhead was dragged to `time`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelinePlayheadChanged {
    pub time: f64,
    pub committed: bool,
}

/// The selected keys changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineSelectionChanged {
    pub keys: Vec<TimelineKeyRef>,
}

/// The selected keys were dragged by `dt` seconds and `dv` value units.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineKeysMoved {
    pub keys: Vec<TimelineKeyRef>,
    pub dt: f64,
    pub dv: f64,
    pub committed: bool,
}

/// A key's tangent handle was dragged to `handle` (relative to the key).
/// When `broken` is false the application turns the opposite handle to
/// match.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineHandleMoved {
    pub key: TimelineKeyRef,
    /// The out (forward) handle; else the in handle.
    pub out: bool,
    pub handle: [f64; 2],
    pub broken: bool,
    pub committed: bool,
}

/// A branch track folded or opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimelineTrackToggled {
    pub track: usize,
    pub expanded: bool,
}

/// The view zoomed, panned or scrolled; keep it to restore the panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TimelineViewChanged {
    pub view: TimelineView,
}

/// A tick on the ruler: its x in the key area and a label on major ticks.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineTick {
    pub x: f32,
    pub label: Option<String>,
}

/// What a point of the timeline is over.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TimelineHit {
    Ruler,
    /// A track's header; `toggle` when on a branch's disclosure.
    Header {
        track: usize,
        toggle: bool,
    },
    Key(TimelineKeyRef),
    /// A selected key's handle (curve view).
    Handle {
        key: TimelineKeyRef,
        out: bool,
    },
    Empty,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Gesture {
    Scrub,
    Pan {
        last: [f32; 2],
    },
    Box {
        from: [f32; 2],
        to: [f32; 2],
        add: bool,
    },
    Move {
        from: [f32; 2],
        dt: f64,
        dv: f64,
    },
    Handle {
        key: TimelineKeyRef,
        out: bool,
        handle: [f64; 2],
        broken: bool,
    },
}

/// The timeline; see the module docs.
#[derive(Debug, Clone, PartialEq)]
pub struct TimelineEditor {
    pub tracks: Vec<TimelineTrack>,
    pub fps: f64,
    pub unit: TimelineUnit,
    pub mode: TimelineMode,
    pub view: TimelineView,
    pub playhead: f64,
    /// Selected keys, sorted.
    pub selection: Vec<TimelineKeyRef>,
    /// Drags and the playhead land on whole frames.
    pub snap: bool,
    /// The track header column's width, px.
    pub header_width: f32,
    /// What the timeline is for, said by assistive technology.
    pub label: Option<Arc<str>>,
    pub style: NodeStyle,
    pub(crate) gesture: Option<Gesture>,
}

impl TimelineEditor {
    pub fn new(tracks: impl IntoIterator<Item = TimelineTrack>) -> Self {
        let mut style = NodeStyle::default();
        {
            let layout = Arc::make_mut(&mut style.layout);
            layout.width = Some(LengthSpec::Fill);
            layout.height = Some(LengthSpec::Fill);
            layout.min_height = Some(LengthSpec::Px(TIMELINE_RULER + TIMELINE_ROW));
        }
        style.background = Some(SemanticColorRole::Background);
        Self {
            tracks: tracks.into_iter().collect(),
            fps: 30.0,
            unit: TimelineUnit::Frames,
            mode: TimelineMode::DopeSheet,
            view: TimelineView::default(),
            playhead: 0.0,
            selection: Vec::new(),
            snap: true,
            header_width: 140.0,
            label: None,
            style,
            gesture: None,
        }
    }

    pub fn fps(mut self, fps: f64) -> Self {
        self.fps = fps.max(1.0);
        self
    }

    pub fn unit(mut self, unit: TimelineUnit) -> Self {
        self.unit = unit;
        self
    }

    pub fn mode(mut self, mode: TimelineMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn view(mut self, view: TimelineView) -> Self {
        self.view = view;
        self
    }

    pub fn playhead(mut self, time: f64) -> Self {
        self.playhead = time;
        self
    }

    pub fn snap(mut self, snap: bool) -> Self {
        self.snap = snap;
        self
    }

    pub fn header_width(mut self, width: f32) -> Self {
        self.header_width = width.max(0.0);
        self
    }

    pub fn label(mut self, label: impl Into<Arc<str>>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Replaces the node style wholesale.
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.style = style;
        self
    }

    /// The x of `time` in the node.
    pub fn time_to_x(&self, time: f64) -> f32 {
        self.header_width + ((time - self.view.start) * self.view.pixels_per_second) as f32
    }

    /// The time at `x` in the node.
    pub fn x_to_time(&self, x: f32) -> f64 {
        self.view.start + f64::from(x - self.header_width) / self.view.pixels_per_second
    }

    /// `time` on the nearest frame when snapping.
    pub fn snapped(&self, time: f64) -> f64 {
        if self.snap {
            (time * self.fps).round() / self.fps
        } else {
            time
        }
    }

    /// The y of `value` in a node `height` tall (curve view).
    pub fn value_to_y(&self, value: f64, height: f32) -> f32 {
        let (top, bottom) = (TIMELINE_RULER + CURVE_MARGIN, height - CURVE_MARGIN);
        let span = (self.view.value_max - self.view.value_min).max(1e-9);
        bottom - ((value - self.view.value_min) / span) as f32 * (bottom - top).max(1.0)
    }

    /// The value at `y` in a node `height` tall (curve view).
    pub fn y_to_value(&self, y: f32, height: f32) -> f64 {
        let (top, bottom) = (TIMELINE_RULER + CURVE_MARGIN, height - CURVE_MARGIN);
        let span = self.view.value_max - self.view.value_min;
        self.view.value_min + f64::from((bottom - y) / (bottom - top).max(1.0)) * span
    }

    /// The tracks shown (a closed branch hides the tracks under it), in
    /// order.
    pub fn visible_tracks(&self) -> Vec<usize> {
        let mut out = Vec::new();
        let mut hidden_below: Option<u8> = None;
        for (i, t) in self.tracks.iter().enumerate() {
            if let Some(d) = hidden_below {
                if t.depth > d {
                    continue;
                }
                hidden_below = None;
            }
            out.push(i);
            if t.expanded == Some(false) {
                hidden_below = Some(t.depth);
            }
        }
        out
    }

    /// The top of row `n` among the visible tracks.
    fn row_top(&self, n: usize) -> f32 {
        TIMELINE_RULER + n as f32 * TIMELINE_ROW - self.view.scroll
    }

    /// The ruler's ticks over a key area `width` px wide: labelled ones at
    /// least [`LABEL_SPACING`] apart, finer ones between.
    pub fn ticks(&self, width: f32) -> Vec<TimelineTick> {
        let per_second = match self.unit {
            TimelineUnit::Frames => self.fps,
            TimelineUnit::Seconds => 1.0,
        };
        let px_per_unit = self.view.pixels_per_second / per_second;
        let mut major = 1.0e-3;
        'nice: for exponent in -3..9 {
            for m in [1.0, 2.0, 5.0] {
                let step = m * 10f64.powi(exponent);
                if self.unit == TimelineUnit::Frames && step < 1.0 {
                    continue;
                }
                if step * px_per_unit >= LABEL_SPACING {
                    major = step;
                    break 'nice;
                }
            }
        }
        // the finest tenth, fifth or half of a major step that stays 6 px apart (whole frames)
        let minor = [10.0, 5.0, 2.0]
            .into_iter()
            .map(|d| major / d)
            .find(|&m| {
                m * px_per_unit >= 6.0
                    && (self.unit == TimelineUnit::Seconds || (m >= 1.0 && m.fract() == 0.0))
            })
            .unwrap_or(major);
        let start_units = self.view.start * per_second;
        let end_units = start_units + f64::from(width) / px_per_unit;
        let mut out = Vec::new();
        let mut n = (start_units / minor).floor() as i64;
        loop {
            let u = n as f64 * minor;
            if u > end_units {
                break;
            }
            let x = ((u - start_units) * px_per_unit) as f32;
            if x >= 0.0 {
                let on_major = ((u / major).round() * major - u).abs() < minor * 1e-3;
                let label = on_major.then(|| match self.unit {
                    TimelineUnit::Frames => format!("{}", u.round() as i64),
                    TimelineUnit::Seconds => {
                        let s = format!("{u:.3}");
                        s.trim_end_matches('0').trim_end_matches('.').to_owned()
                    }
                });
                out.push(TimelineTick { x, label });
            }
            n += 1;
        }
        out
    }

    /// A key as it shows now: moved by a drag in progress.
    fn shown_key(&self, r: TimelineKeyRef) -> Option<TimelineKey> {
        let mut k = *self.tracks.get(r.track)?.keys.get(r.key)?;
        match self.gesture {
            Some(Gesture::Move { dt, dv, .. }) if self.selection.contains(&r) => {
                k.time += dt;
                k.value += dv;
            }
            Some(Gesture::Handle {
                key,
                out,
                handle,
                broken,
            }) if key == r => {
                if out {
                    k.out_handle = handle;
                } else {
                    k.in_handle = handle;
                }
                if !broken {
                    let opposite = aligned(handle, if out { k.in_handle } else { k.out_handle });
                    if out {
                        k.in_handle = opposite;
                    } else {
                        k.out_handle = opposite;
                    }
                }
            }
            _ => {}
        }
        Some(k)
    }

    /// Where key `r` shows in a node `height` tall.
    pub fn key_point(&self, r: TimelineKeyRef, height: f32) -> Option<[f32; 2]> {
        let k = self.shown_key(r)?;
        let x = self.time_to_x(k.time);
        match self.mode {
            TimelineMode::DopeSheet => {
                let n = self.visible_tracks().iter().position(|&t| t == r.track)?;
                Some([x, self.row_top(n) + TIMELINE_ROW / 2.0])
            }
            TimelineMode::Curves => self
                .visible_tracks()
                .contains(&r.track)
                .then_some([x, self.value_to_y(k.value, height)]),
        }
    }

    /// Where a key's handle shows (curve view).
    fn handle_point(&self, r: TimelineKeyRef, out: bool, height: f32) -> Option<[f32; 2]> {
        let k = self.shown_key(r)?;
        let h = if out { k.out_handle } else { k.in_handle };
        Some([
            self.time_to_x(k.time + h[0]),
            self.value_to_y(k.value + h[1], height),
        ])
    }

    /// Every key shown, by reference.
    fn shown_keys(&self) -> Vec<TimelineKeyRef> {
        self.visible_tracks()
            .into_iter()
            .flat_map(|track| {
                (0..self.tracks[track].keys.len()).map(move |key| TimelineKeyRef { track, key })
            })
            .collect()
    }

    /// What the point `p` (node-local) is over, in a node `size` big.
    pub fn hit(&self, p: [f32; 2], size: [f32; 2]) -> TimelineHit {
        if p[1] < TIMELINE_RULER {
            return if p[0] >= self.header_width {
                TimelineHit::Ruler
            } else {
                TimelineHit::Empty
            };
        }
        if p[0] < self.header_width {
            let n = ((p[1] - TIMELINE_RULER + self.view.scroll) / TIMELINE_ROW).floor();
            // the curve view lists its tracks without folding
            if self.mode == TimelineMode::DopeSheet
                && n >= 0.0
                && let Some(&track) = self.visible_tracks().get(n as usize)
            {
                let t = &self.tracks[track];
                let indent = 4.0 + f32::from(t.depth) * INDENT;
                let toggle = t.expanded.is_some() && p[0] < indent + 14.0;
                return TimelineHit::Header { track, toggle };
            }
            return TimelineHit::Empty;
        }
        let near = |q: [f32; 2]| (q[0] - p[0]).hypot(q[1] - p[1]) <= HIT;
        if self.mode == TimelineMode::Curves {
            for &key in &self.selection {
                for out in [false, true] {
                    if self.handle_point(key, out, size[1]).is_some_and(near) {
                        return TimelineHit::Handle { key, out };
                    }
                }
            }
        }
        // the topmost: the last drawn
        self.shown_keys()
            .into_iter()
            .rev()
            .find(|&r| self.key_point(r, size[1]).is_some_and(near))
            .map_or(TimelineHit::Empty, TimelineHit::Key)
    }

    /// The keys shown inside the node-local rectangle between two corners.
    pub fn keys_in(&self, a: [f32; 2], b: [f32; 2], height: f32) -> Vec<TimelineKeyRef> {
        let (x0, x1) = (a[0].min(b[0]), a[0].max(b[0]));
        let (y0, y1) = (a[1].min(b[1]), a[1].max(b[1]));
        self.shown_keys()
            .into_iter()
            .filter(|&r| {
                self.key_point(r, height)
                    .is_some_and(|q| q[0] >= x0 && q[0] <= x1 && q[1] >= y0 && q[1] <= y1)
            })
            .collect()
    }

    /// Frame the selected keys (every key when none is selected) in a node
    /// `size` big: their times across the key area, their values (and
    /// handles, in the curve view) top to bottom.
    pub fn fit(&mut self, size: [f32; 2]) {
        let keys: Vec<TimelineKey> = if self.selection.is_empty() {
            self.tracks
                .iter()
                .flat_map(|t| t.keys.iter().copied())
                .collect()
        } else {
            self.selection
                .iter()
                .filter_map(|r| self.tracks.get(r.track)?.keys.get(r.key).copied())
                .collect()
        };
        if keys.is_empty() {
            return;
        }
        let (mut t0, mut t1) = (f64::MAX, f64::MIN);
        let (mut v0, mut v1) = (f64::MAX, f64::MIN);
        for k in &keys {
            t0 = t0.min(k.time);
            t1 = t1.max(k.time);
            let mut values = vec![k.value];
            if self.mode == TimelineMode::Curves {
                values.extend([k.value + k.in_handle[1], k.value + k.out_handle[1]]);
            }
            for v in values {
                v0 = v0.min(v);
                v1 = v1.max(v);
            }
        }
        if t1 - t0 < 1e-6 {
            (t0, t1) = (t0 - 0.5, t1 + 0.5);
        }
        if v1 - v0 < 1e-6 {
            (v0, v1) = (v0 - 1.0, v1 + 1.0);
        }
        let width = f64::from((size[0] - self.header_width).max(1.0));
        let margin = (0.05 * width).min(32.0);
        self.view.pixels_per_second = (width - 2.0 * margin).max(1.0) / (t1 - t0);
        self.view.start = t0 - margin / self.view.pixels_per_second;
        let pad = 0.1 * (v1 - v0);
        (self.view.value_min, self.view.value_max) = (v0 - pad, v1 + pad);
    }

    /// A summary of everything the painter draws.
    fn digest(&self) -> u64 {
        let mut h = DefaultHasher::new();
        for t in &self.tracks {
            t.id.hash(&mut h);
            t.label.hash(&mut h);
            t.depth.hash(&mut h);
            t.expanded.hash(&mut h);
            for k in &t.keys {
                for v in [
                    k.time,
                    k.value,
                    k.in_handle[0],
                    k.in_handle[1],
                    k.out_handle[0],
                    k.out_handle[1],
                ] {
                    v.to_bits().hash(&mut h);
                }
                k.broken.hash(&mut h);
            }
        }
        for v in [
            self.fps,
            self.view.start,
            self.view.pixels_per_second,
            self.view.value_min,
            self.view.value_max,
            self.playhead,
        ] {
            v.to_bits().hash(&mut h);
        }
        self.view.scroll.to_bits().hash(&mut h);
        self.unit.hash(&mut h);
        self.mode.hash(&mut h);
        self.selection.hash(&mut h);
        self.header_width.to_bits().hash(&mut h);
        format!("{:?}", self.gesture).hash(&mut h);
        h.finish()
    }
}

/// `handle` turned to point opposite `toward`, keeping the length of
/// `keep` (an aligned pair).
fn aligned(toward: [f64; 2], keep: [f64; 2]) -> [f64; 2] {
    let len = toward[0].hypot(toward[1]);
    if len < 1e-12 {
        return keep;
    }
    let keep_len = keep[0].hypot(keep[1]);
    [-toward[0] / len * keep_len, -toward[1] / len * keep_len]
}

impl ComponentView for TimelineEditor {
    const BEHAVIOR: crate::TypeBehavior<Self> = crate::TypeBehavior {
        pointer: Some(&TIMELINE_POINTER),
        ..crate::TypeBehavior::NONE
    };

    fn share_layouts(&mut self, share: &mut dyn FnMut(&mut Arc<nana_ui_core::LayoutStyle>)) {
        share(&mut self.style.layout);
    }

    fn reconcile(&mut self, mut next: Self) {
        // the view and a drag in progress are the user's
        next.view = self.view;
        next.gesture = self.gesture;
        *self = next;
    }

    fn node_kind(&self) -> NodeKind {
        NodeKind::Element {
            tag: "timeline".into(),
        }
    }

    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut style = self.style.clone();
        style.painter = Some(crate::NodePainter::new(TimelinePainter {
            timeline: Arc::new(self.clone()),
            key: self.digest(),
        }));
        project_common(
            id,
            world,
            mutations,
            &style,
            InteractionState {
                pointer_events: true,
                focusable: true,
            },
            AccessibilityState {
                role: AccessibilityRole::Generic,
                label: self.label.clone(),
                value: Some(
                    format!(
                        "{} tracks, {} keys selected",
                        self.tracks.len(),
                        self.selection.len()
                    )
                    .into(),
                ),
                ..AccessibilityState::default()
            },
        );
    }
}

const TIMELINE_POINTER: NodePointerHooks<TimelineEditor> = NodePointerHooks {
    moved: timeline_moved,
    left: |_, _| Ok(()),
    pressed: timeline_pressed,
    released: timeline_released,
    wheel: timeline_wheel,
};

fn node_size(cx: &AppContext, timeline: Entity<TimelineEditor>) -> Option<[f32; 2]> {
    let b = cx.world().component_layout_box(timeline.stable_id())?;
    Some([b.width, b.height])
}

fn timeline_pressed(
    cx: &mut AppContext,
    timeline: Entity<TimelineEditor>,
    pointer: NodePointer,
) -> Result<bool, FrameworkError> {
    let Some(size) = node_size(cx, timeline) else {
        return Ok(false);
    };
    let p = [pointer.x, pointer.y];
    if pointer.button == 1 {
        cx.update_component(timeline, |t, _| t.gesture = Some(Gesture::Pan { last: p }))?;
        return Ok(true);
    }
    if pointer.button != 0 {
        return Ok(false);
    }
    let alt = pointer.modifiers.alt;
    cx.update_component(timeline, |t, view| match t.hit(p, size) {
        TimelineHit::Ruler => {
            t.playhead = t.snapped(t.x_to_time(p[0]));
            t.gesture = Some(Gesture::Scrub);
            view.emit(TimelinePlayheadChanged {
                time: t.playhead,
                committed: false,
            });
        }
        TimelineHit::Header { track, toggle } => {
            if toggle && let Some(open) = t.tracks[track].expanded {
                t.tracks[track].expanded = Some(!open);
                view.emit(TimelineTrackToggled {
                    track,
                    expanded: !open,
                });
            }
        }
        TimelineHit::Handle { key, out } => {
            if let Some(k) = t.tracks[key.track].keys.get(key.key) {
                t.gesture = Some(Gesture::Handle {
                    key,
                    out,
                    handle: if out { k.out_handle } else { k.in_handle },
                    broken: k.broken || alt,
                });
            }
        }
        TimelineHit::Key(r) => {
            let before = t.selection.clone();
            if pointer.shift {
                match t.selection.binary_search(&r) {
                    Ok(i) => {
                        t.selection.remove(i);
                    }
                    Err(i) => t.selection.insert(i, r),
                }
            } else if !t.selection.contains(&r) {
                t.selection = vec![r];
            }
            if t.selection != before {
                view.emit(TimelineSelectionChanged {
                    keys: t.selection.clone(),
                });
            }
            if t.selection.contains(&r) {
                t.gesture = Some(Gesture::Move {
                    from: p,
                    dt: 0.0,
                    dv: 0.0,
                });
            }
        }
        TimelineHit::Empty => {
            t.gesture = Some(Gesture::Box {
                from: p,
                to: p,
                add: pointer.shift,
            });
        }
    })?;
    Ok(true)
}

fn timeline_moved(
    cx: &mut AppContext,
    timeline: Entity<TimelineEditor>,
    pointer: NodePointer,
) -> Result<bool, FrameworkError> {
    if !pointer.captured {
        return Ok(false);
    }
    let Some(size) = node_size(cx, timeline) else {
        return Ok(false);
    };
    let p = [pointer.x, pointer.y];
    cx.update_component(timeline, |t, view| match t.gesture {
        Some(Gesture::Scrub) => {
            let time = t.snapped(t.x_to_time(p[0]));
            if time != t.playhead {
                t.playhead = time;
                view.emit(TimelinePlayheadChanged {
                    time,
                    committed: false,
                });
            }
        }
        Some(Gesture::Pan { last }) => {
            t.view.start -= f64::from(p[0] - last[0]) / t.view.pixels_per_second;
            match t.mode {
                TimelineMode::DopeSheet => {
                    t.view.scroll = (t.view.scroll - (p[1] - last[1])).max(0.0);
                }
                TimelineMode::Curves => {
                    let dv = t.y_to_value(last[1], size[1]) - t.y_to_value(p[1], size[1]);
                    t.view.value_min += dv;
                    t.view.value_max += dv;
                }
            }
            t.gesture = Some(Gesture::Pan { last: p });
            view.emit(TimelineViewChanged { view: t.view });
        }
        Some(Gesture::Box { from, add, .. }) => {
            t.gesture = Some(Gesture::Box { from, to: p, add });
        }
        Some(Gesture::Move { from, .. }) => {
            let mut dt = f64::from(p[0] - from[0]) / t.view.pixels_per_second;
            // the first selected key lands on a frame
            if let Some(k) = t
                .selection
                .first()
                .and_then(|r| t.tracks.get(r.track)?.keys.get(r.key))
            {
                dt = t.snapped(k.time + dt) - k.time;
            }
            let dv = match t.mode {
                TimelineMode::DopeSheet => 0.0,
                TimelineMode::Curves => {
                    t.y_to_value(p[1], size[1]) - t.y_to_value(from[1], size[1])
                }
            };
            let changed = t.gesture != Some(Gesture::Move { from, dt, dv });
            t.gesture = Some(Gesture::Move { from, dt, dv });
            if changed {
                view.emit(TimelineKeysMoved {
                    keys: t.selection.clone(),
                    dt,
                    dv,
                    committed: false,
                });
            }
        }
        Some(Gesture::Handle {
            key, out, broken, ..
        }) => {
            let Some(k) = t.tracks.get(key.track).and_then(|tr| tr.keys.get(key.key)) else {
                return;
            };
            let mut handle = [
                t.x_to_time(p[0]) - k.time,
                t.y_to_value(p[1], size[1]) - k.value,
            ];
            // a handle stays on its side of the key
            handle[0] = if out {
                handle[0].max(0.0)
            } else {
                handle[0].min(0.0)
            };
            let next = Some(Gesture::Handle {
                key,
                out,
                handle,
                broken,
            });
            if t.gesture != next {
                t.gesture = next;
                view.emit(TimelineHandleMoved {
                    key,
                    out,
                    handle,
                    broken,
                    committed: false,
                });
            }
        }
        None => {}
    })?;
    Ok(true)
}

fn timeline_released(
    cx: &mut AppContext,
    timeline: Entity<TimelineEditor>,
    pointer: NodePointer,
) -> Result<bool, FrameworkError> {
    let Some(size) = node_size(cx, timeline) else {
        return Ok(false);
    };
    let p = [pointer.x, pointer.y];
    cx.update_component(timeline, |t, view| match t.gesture.take() {
        Some(Gesture::Scrub) => view.emit(TimelinePlayheadChanged {
            time: t.playhead,
            committed: true,
        }),
        Some(Gesture::Box { from, add, .. }) => {
            let inside = t.keys_in(from, p, size[1]);
            let mut next = if add { t.selection.clone() } else { Vec::new() };
            for r in inside {
                if let Err(i) = next.binary_search(&r) {
                    next.insert(i, r);
                }
            }
            if next != t.selection {
                t.selection = next;
                view.emit(TimelineSelectionChanged {
                    keys: t.selection.clone(),
                });
            }
        }
        Some(Gesture::Move { dt, dv, .. }) => {
            if dt != 0.0 || dv != 0.0 {
                view.emit(TimelineKeysMoved {
                    keys: t.selection.clone(),
                    dt,
                    dv,
                    committed: true,
                });
            }
        }
        Some(Gesture::Handle {
            key,
            out,
            handle,
            broken,
        }) => view.emit(TimelineHandleMoved {
            key,
            out,
            handle,
            broken,
            committed: true,
        }),
        Some(Gesture::Pan { .. }) | None => {}
    })?;
    Ok(true)
}

fn timeline_wheel(
    cx: &mut AppContext,
    timeline: Entity<TimelineEditor>,
    wheel: NodeWheel,
) -> Result<bool, FrameworkError> {
    let shift = wheel.modifiers.shift;
    cx.update_component(timeline, |t, view| {
        let dy = if wheel.delta_y != 0.0 {
            wheel.delta_y
        } else {
            wheel.delta_x
        };
        if shift {
            // scroll the tracks
            t.view.scroll = (t.view.scroll + dy).max(0.0);
        } else if wheel.ctrl {
            // pan time
            t.view.start += f64::from(dy) / t.view.pixels_per_second;
        } else {
            // zoom about the time under the pointer
            let at = t.x_to_time(wheel.x);
            let lines = if dy.abs() < 5.0 { 40.0 } else { 1.0 };
            let factor = 1.0015f64.powf(-f64::from(dy) * lines);
            t.view.pixels_per_second = (t.view.pixels_per_second * factor).clamp(1.0, 100_000.0);
            t.view.start = at - f64::from(wheel.x - t.header_width) / t.view.pixels_per_second;
        }
        view.emit(TimelineViewChanged { view: t.view });
    })?;
    Ok(true)
}

impl AppContext {
    /// Frame `timeline`'s selected keys (every key when none is selected) in
    /// its laid-out size; see [`TimelineEditor::fit`].
    pub fn fit_timeline(&mut self, timeline: Entity<TimelineEditor>) -> Result<(), FrameworkError> {
        let Some(size) = node_size(self, timeline) else {
            return Ok(());
        };
        self.update_component(timeline, |t, view| {
            t.fit(size);
            view.emit(TimelineViewChanged { view: t.view });
        })
    }
}

/// Paints a [`TimelineEditor`] from a snapshot of it.
struct TimelinePainter {
    timeline: Arc<TimelineEditor>,
    key: u64,
}

impl Painter for TimelinePainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let t = &self.timeline;
        let [width, height] = cx.size();
        let header = t.header_width.min(width);
        let rect = |x: f32, y: f32, w: f32, h: f32| LayoutBox {
            x,
            y,
            width: w.max(0.0),
            height: h.max(0.0),
        };
        let fill = |cx: &mut PaintContext<'_>, r: LayoutBox, role: SemanticColorRole| {
            cx.rounded_rect(r, 0.0, BoxPaint::fill(role));
        };
        let label = |content: &str, size: f32, role: SemanticColorRole| {
            PaintText::new(content.to_owned()).size(size).color(role)
        };
        fill(
            cx,
            rect(0.0, 0.0, width, height),
            SemanticColorRole::Background,
        );
        let rows = t.visible_tracks();

        // track rows: alternating bands, then the key area
        let mut clip = PaintPath::new();
        clip.rect(rect(0.0, TIMELINE_RULER, width, height - TIMELINE_RULER));
        cx.push_clip(&clip);
        if t.mode == TimelineMode::DopeSheet {
            for (n, &track) in rows.iter().enumerate() {
                let y = t.row_top(n);
                if y > height || y + TIMELINE_ROW < TIMELINE_RULER {
                    continue;
                }
                if n % 2 == 1 {
                    fill(
                        cx,
                        rect(header, y, width - header, TIMELINE_ROW),
                        SemanticColorRole::Subtle,
                    );
                }
                let _ = track;
            }
        } else {
            // the zero line and the value range's ends
            for v in [t.view.value_min, 0.0, t.view.value_max] {
                let y = t.value_to_y(v, height);
                if (TIMELINE_RULER..height).contains(&y) {
                    fill(
                        cx,
                        rect(header, y, width - header, 1.0),
                        SemanticColorRole::BorderSoft,
                    );
                }
            }
        }
        // ticks run down the key area
        for tick in t.ticks(width - header) {
            let role = if tick.label.is_some() {
                SemanticColorRole::Border
            } else {
                SemanticColorRole::BorderSoft
            };
            fill(cx, rect(header + tick.x, TIMELINE_RULER, 1.0, height), role);
        }
        // curves
        if t.mode == TimelineMode::Curves {
            for &track in &rows {
                let keys: Vec<TimelineKey> = (0..t.tracks[track].keys.len())
                    .filter_map(|key| t.shown_key(TimelineKeyRef { track, key }))
                    .collect();
                let Some(first) = keys.first() else { continue };
                let point = |k: &TimelineKey| [t.time_to_x(k.time), t.value_to_y(k.value, height)];
                let mut path = PaintPath::new();
                let p0 = point(first);
                path.move_to(header, p0[1]);
                path.line_to(p0[0], p0[1]);
                for pair in keys.windows(2) {
                    let (a, b) = (&pair[0], &pair[1]);
                    let c1 = [
                        t.time_to_x(a.time + a.out_handle[0]),
                        t.value_to_y(a.value + a.out_handle[1], height),
                    ];
                    let c2 = [
                        t.time_to_x(b.time + b.in_handle[0]),
                        t.value_to_y(b.value + b.in_handle[1], height),
                    ];
                    let end = point(b);
                    path.cubic_to(c1[0], c1[1], c2[0], c2[1], end[0], end[1]);
                }
                let last = point(keys.last().unwrap_or(first));
                path.line_to(width, last[1]);
                cx.stroke_path(&path, StrokeStyle::new(1.5), SemanticColorRole::Accent);
            }
            // the selected keys' handles
            for &r in &t.selection {
                let (Some(k), Some(i), Some(o)) = (
                    t.key_point(r, height),
                    t.handle_point(r, false, height),
                    t.handle_point(r, true, height),
                ) else {
                    continue;
                };
                for h in [i, o] {
                    let mut line = PaintPath::new();
                    line.move_to(k[0], k[1]);
                    line.line_to(h[0], h[1]);
                    cx.stroke_path(&line, StrokeStyle::new(1.0), SemanticColorRole::Muted);
                    cx.rounded_rect(
                        rect(h[0] - 3.0, h[1] - 3.0, 6.0, 6.0),
                        3.0,
                        BoxPaint::fill(SemanticColorRole::Text),
                    );
                }
            }
        }
        // keys: diamonds, the selected in the highlight colour
        for r in t.shown_keys() {
            let Some(p) = t.key_point(r, height) else {
                continue;
            };
            if p[0] < header - HIT || p[0] > width + HIT {
                continue;
            }
            let role = if t.selection.contains(&r) {
                SemanticColorRole::Highlight
            } else {
                SemanticColorRole::Text
            };
            let s = 5.0;
            let mut diamond = PaintPath::new();
            diamond.move_to(p[0], p[1] - s);
            diamond.line_to(p[0] + s, p[1]);
            diamond.line_to(p[0], p[1] + s);
            diamond.line_to(p[0] - s, p[1]);
            diamond.close();
            cx.fill_path(&diamond, role);
            cx.stroke_path(
                &diamond,
                StrokeStyle::new(1.0),
                SemanticColorRole::Background,
            );
        }
        // a box selection being dragged
        if let Some(Gesture::Box { from, to, .. }) = t.gesture {
            let r = rect(
                from[0].min(to[0]),
                from[1].min(to[1]),
                (to[0] - from[0]).abs(),
                (to[1] - from[1]).abs(),
            );
            cx.rounded_rect(
                r,
                0.0,
                BoxPaint::fill(SemanticColorRole::AccentSoft)
                    .border(SemanticColorRole::Accent, 1.0),
            );
        }
        cx.pop_clip();

        // the track headers
        fill(
            cx,
            rect(0.0, TIMELINE_RULER, header, height - TIMELINE_RULER),
            SemanticColorRole::Surface,
        );
        if t.mode == TimelineMode::DopeSheet {
            for (n, &track) in rows.iter().enumerate() {
                let y = t.row_top(n);
                if y > height || y + TIMELINE_ROW < TIMELINE_RULER {
                    continue;
                }
                let tr = &t.tracks[track];
                let indent = 4.0 + f32::from(tr.depth) * INDENT;
                if let Some(open) = tr.expanded {
                    let mark = if open { "▾" } else { "▸" };
                    cx.text(
                        rect(indent, y, 14.0, TIMELINE_ROW),
                        label(mark, 11.0, SemanticColorRole::Muted),
                    );
                }
                cx.text(
                    rect(indent + 14.0, y, header - indent - 18.0, TIMELINE_ROW),
                    label(&tr.label, 12.0, SemanticColorRole::Text),
                );
            }
        } else {
            for (n, &track) in rows.iter().enumerate() {
                let y = TIMELINE_RULER + 4.0 + n as f32 * TIMELINE_ROW;
                cx.text(
                    rect(8.0, y, header - 12.0, TIMELINE_ROW),
                    label(&t.tracks[track].label, 12.0, SemanticColorRole::Text),
                );
            }
        }
        fill(
            cx,
            rect(header - 1.0, TIMELINE_RULER, 1.0, height),
            SemanticColorRole::Border,
        );

        // the ruler
        fill(
            cx,
            rect(0.0, 0.0, width, TIMELINE_RULER),
            SemanticColorRole::Subtle,
        );
        for tick in t.ticks(width - header) {
            let x = header + tick.x;
            let h = if tick.label.is_some() { 8.0 } else { 4.0 };
            fill(
                cx,
                rect(x, TIMELINE_RULER - h, 1.0, h),
                SemanticColorRole::Muted,
            );
            if let Some(text) = tick.label {
                cx.text(
                    rect(x + 3.0, 2.0, 60.0, TIMELINE_RULER - 10.0),
                    label(&text, 11.0, SemanticColorRole::Muted),
                );
            }
        }
        fill(
            cx,
            rect(0.0, TIMELINE_RULER - 1.0, width, 1.0),
            SemanticColorRole::Border,
        );

        // the playhead over everything
        let x = t.time_to_x(t.playhead);
        if x >= header && x <= width {
            fill(
                cx,
                rect(x - 0.5, 0.0, 1.5, height),
                SemanticColorRole::Accent,
            );
            cx.rounded_rect(
                rect(x - 5.0, 2.0, 10.0, TIMELINE_RULER - 6.0),
                2.0,
                BoxPaint::fill(SemanticColorRole::Accent),
            );
        }
    }

    fn paint_key(&self) -> u64 {
        self.key
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timeline() -> TimelineEditor {
        TimelineEditor::new([
            TimelineTrack::new("face", "Face", []).expanded(true),
            TimelineTrack::new(
                "x",
                "Angle X",
                [TimelineKey::new(0.0, 0.0), TimelineKey::new(1.0, 30.0)],
            )
            .depth(1),
            TimelineTrack::new("y", "Angle Y", [TimelineKey::new(0.5, -10.0)]).depth(1),
            TimelineTrack::new("blink", "Blink", [TimelineKey::new(2.0, 1.0)]),
        ])
        .fps(30.0)
        .header_width(100.0)
    }

    #[test]
    fn time_maps_to_x_and_back_and_snaps() {
        let mut t = timeline();
        t.view.start = 1.0;
        t.view.pixels_per_second = 60.0;
        assert_eq!(t.time_to_x(1.5), 130.0);
        assert!((t.x_to_time(130.0) - 1.5).abs() < 1e-9);
        assert!((t.snapped(1.0 / 30.0 + 0.004) - 1.0 / 30.0).abs() < 1e-12);
        t.snap = false;
        assert_eq!(t.snapped(0.123), 0.123);
        let y = t.value_to_y(0.25, 200.0);
        assert!((t.y_to_value(y, 200.0) - 0.25).abs() < 1e-6);
    }

    #[test]
    fn ticks_thin_out_as_the_view_zooms_out() {
        let mut t = timeline();
        t.view.pixels_per_second = 300.0; // 10 px a frame
        let ticks = t.ticks(600.0);
        let labelled: Vec<_> = ticks.iter().filter_map(|k| k.label.clone()).collect();
        // every 10 frames (100 px ≥ 64), a tick per frame between
        assert_eq!(&labelled[..3], ["0", "10", "20"]);
        assert_eq!(ticks[1].x, 10.0);
        t.view.pixels_per_second = 6.0; // 0.2 px a frame
        let labelled: Vec<_> = t.ticks(600.0).into_iter().filter_map(|k| k.label).collect();
        assert_eq!(&labelled[..2], ["0", "500"]);
        t.unit = TimelineUnit::Seconds;
        t.view.pixels_per_second = 200.0;
        let labelled: Vec<_> = t.ticks(400.0).into_iter().filter_map(|k| k.label).collect();
        assert_eq!(&labelled[..3], ["0", "0.5", "1"]);
    }

    #[test]
    fn closed_branches_hide_their_tracks() {
        let mut t = timeline();
        assert_eq!(t.visible_tracks(), [0, 1, 2, 3]);
        t.tracks[0].expanded = Some(false);
        assert_eq!(t.visible_tracks(), [0, 3]);
    }

    #[test]
    fn hits_find_the_ruler_headers_keys_and_handles() {
        let mut t = timeline();
        let size = [500.0, 200.0];
        assert_eq!(t.hit([200.0, 10.0], size), TimelineHit::Ruler);
        // the branch's disclosure toggles; its label does not
        let row0 = TIMELINE_RULER + TIMELINE_ROW / 2.0;
        assert_eq!(
            t.hit([8.0, row0], size),
            TimelineHit::Header {
                track: 0,
                toggle: true
            }
        );
        assert_eq!(
            t.hit([60.0, row0], size),
            TimelineHit::Header {
                track: 0,
                toggle: false
            }
        );
        // Angle X's second key at 1 s, row 1
        let key = TimelineKeyRef { track: 1, key: 1 };
        let p = t.key_point(key, size[1]).unwrap();
        assert_eq!(p, [t.time_to_x(1.0), TIMELINE_RULER + TIMELINE_ROW * 1.5]);
        assert_eq!(t.hit([p[0] + 3.0, p[1]], size), TimelineHit::Key(key));
        assert_eq!(t.hit([p[0] + 30.0, p[1]], size), TimelineHit::Empty);
        // the curve view: a selected key's handles are hit first
        t.mode = TimelineMode::Curves;
        t.view.value_min = -20.0;
        t.view.value_max = 40.0;
        t.selection = vec![key];
        let h = t.handle_point(key, true, size[1]).unwrap();
        assert_eq!(t.hit(h, size), TimelineHit::Handle { key, out: true });
        // a box over the first second takes both Angle X keys and Angle Y's
        let inside = t.keys_in([100.0, 0.0], [t.time_to_x(1.1), 200.0], size[1]);
        assert_eq!(inside.len(), 3);
    }

    #[test]
    fn a_drag_shows_its_preview_and_handles_stay_aligned() {
        let mut t = timeline();
        let key = TimelineKeyRef { track: 1, key: 0 };
        t.selection = vec![key];
        t.gesture = Some(Gesture::Move {
            from: [0.0; 2],
            dt: 0.5,
            dv: 2.0,
        });
        let k = t.shown_key(key).unwrap();
        assert_eq!((k.time, k.value), (0.5, 2.0));
        // the unselected key does not move
        assert_eq!(
            t.shown_key(TimelineKeyRef { track: 1, key: 1 })
                .unwrap()
                .time,
            1.0
        );
        t.gesture = Some(Gesture::Handle {
            key,
            out: true,
            handle: [0.0, 1.0],
            broken: false,
        });
        let k = t.shown_key(key).unwrap();
        // the in handle turns opposite, keeping its length (1/3)
        assert!((k.in_handle[0]).abs() < 1e-9 && (k.in_handle[1] + 1.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn fit_frames_the_selection() {
        let mut t = timeline();
        t.selection = vec![
            TimelineKeyRef { track: 1, key: 0 },
            TimelineKeyRef { track: 1, key: 1 },
        ];
        t.fit([500.0, 200.0]);
        // 0..1 s across the 400 px key area, inside a margin
        assert!(t.time_to_x(0.0) >= 100.0 && t.time_to_x(0.0) < 140.0);
        assert!(t.time_to_x(1.0) <= 500.0 && t.time_to_x(1.0) > 460.0);
        assert!(t.view.value_min < 0.0 && t.view.value_max > 30.0);
    }
}
