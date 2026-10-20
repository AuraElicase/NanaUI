//! Git graph drawing: one lane per branch in `order`, commits stepping
//! along the flow, rounded elbows for branch-offs, merges and
//! cherry-picks.

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use super::{CommitKind, Flow, Model};
use crate::families::LayoutCx;
use crate::geom::PathData;
use crate::geom::path::rounded_polyline;
use crate::ir::{Dash, Geom, Ink, Rect, SceneBuilder, Stroke, StrokeWidth, Tone};
use crate::text::LaidText;
use crate::{DiagramError, DiagramScene, style};

const TAG_FILL: Ink = Ink::Mix(SemanticColorMix::alpha(R::Accent, 0.14));
const TAG_STROKE: Ink = Ink::Mix(SemanticColorMix::alpha(R::Accent, 0.5));

/// Whether a lane holds a commit strictly between two steps.
fn busy(steps: &[usize], a: usize, b: usize) -> bool {
    let (lo, hi) = (a.min(b), a.max(b));
    let from = steps.partition_point(|&s| s <= lo);
    steps.get(from).is_some_and(|&s| s < hi)
}

fn series(index: usize, tone: Tone) -> Ink {
    Ink::Series {
        index: index.min(u16::MAX as usize) as u16,
        tone,
    }
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let k = m.font_size / 14.0;
    let small = cx.small_font();
    let font = cx.font();
    let mut out = SceneBuilder::new();
    if model.commits.is_empty() {
        let content = cx.title_above(&mut out, Rect::default());
        return Ok(out.finish(content, m.margin));
    }
    let horizontal = model.flow == Flow::LeftRight;
    let r = 7.0 * k;
    let gap = 6.0 * k;
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];

    // Lanes: branches sorted by order, ties by creation.
    let mut lanes: Vec<usize> = (0..model.branches.len()).collect();
    lanes.sort_by(|&a, &b| model.branches[a].order.total_cmp(&model.branches[b].order).then(a.cmp(&b)));
    let mut lane_of = vec![0usize; model.branches.len()];
    for (lane, &b) in lanes.iter().enumerate() {
        lane_of[b] = lane;
    }
    // Every branch keeps its colour by creation, whatever its lane.
    let color = |branch: usize| branch;

    // Steps along the flow.
    let mut step_of = vec![0usize; model.commits.len()];
    for (i, c) in model.commits.iter().enumerate() {
        step_of[i] = if model.parallel {
            c.parents.iter().map(|&p| step_of[p] + 1).max().unwrap_or(0)
        } else {
            i
        };
    }
    let last_step = step_of.iter().copied().max().unwrap_or(0);

    // Text: commit labels and tag chips.
    let label_wrap = 100.0 * k;
    let labels: Vec<Option<LaidText>> = model
        .commits
        .iter()
        .map(|c| model.show_labels.then(|| cx.text.lay(&crate::text::Label::plain(&c.id), small, Some(label_wrap))))
        .collect();
    let tags: Vec<Vec<LaidText>> = model
        .commits
        .iter()
        .map(|c| c.tags.iter().map(|t| cx.text.lay_plain(t, small)).collect())
        .collect();
    let chip = |l: &LaidText| [l.width + 2.0 * chip_pad[0], l.height + 2.0 * chip_pad[1]];
    let label_w = labels.iter().flatten().map(|l| chip(l)[0]).fold(0.0_f32, f32::max);
    let label_h = labels.iter().flatten().map(|l| chip(l)[1]).fold(0.0_f32, f32::max);
    // Tags stack across the flow (above in LR) or along it (left in TB/BT).
    let tags_across = tags
        .iter()
        .map(|t| t.iter().map(|l| chip(l)[1] + gap / 2.0).sum::<f32>())
        .fold(0.0_f32, f32::max);
    let tags_along = tags
        .iter()
        .map(|t| t.iter().map(|l| chip(l)[0] + gap / 2.0).sum::<f32>())
        .fold(0.0_f32, f32::max);
    let tag_w = tags.iter().flatten().map(|l| chip(l)[0]).fold(0.0_f32, f32::max);

    let names: Vec<LaidText> = model.branches.iter().map(|b| cx.text.lay_plain(&b.name, font)).collect();
    let name_chip = |l: &LaidText| [l.width + 2.0 * m.node_pad_x * 0.6, l.height + 2.0 * m.node_pad_y * 0.4];
    let name_w = names.iter().map(|l| name_chip(l)[0]).fold(0.0_f32, f32::max);
    let name_h = names.iter().map(|l| name_chip(l)[1]).fold(0.0_f32, f32::max);

    // Spacing along the flow and between lanes.
    let (step, lane_gap) = if horizontal {
        let step = (label_w + 2.0 * gap).max(tag_w + gap).max(4.0 * r + gap);
        let lane = (2.0 * r + 3.0 * gap + label_h + tags_across).max(name_h + 2.0 * gap).max(48.0 * k);
        (step, lane)
    } else {
        let tag_h = tags.iter().flatten().map(|l| chip(l)[1]).fold(0.0_f32, f32::max);
        let step = (label_h.max(tag_h) + 2.0 * gap).max(4.0 * r + gap);
        let lane = (2.0 * r + 3.0 * gap + label_w + tags_along).max(name_w + 2.0 * gap).max(48.0 * k);
        (step, lane)
    };
    // In TB/BT the tags sit on the left of their lane: shift lanes right so
    // the first lane's tags stay clear of the edge.
    let lane_origin = if horizontal { 0.0 } else { tags_along };
    let along = |s: usize| match model.flow {
        Flow::BottomTop => (last_step - s) as f32 * step,
        _ => s as f32 * step,
    };
    let point = |s: usize, lane: usize| -> [f32; 2] {
        let a = along(s);
        let c = lane_origin + lane as f32 * lane_gap;
        if horizontal { [a, c] } else { [c, a] }
    };
    let at = |i: usize| point(step_of[i], lane_of[model.commits[i].branch]);

    // Lanes and branch names before the first step.
    let lead = match (model.show_branches, horizontal) {
        (false, _) => 2.0 * r,
        // Clear of the first commit's label and tags, which spill backwards.
        (true, true) => name_w + 2.0 * gap + label_w.max(tag_w) / 2.0,
        (true, false) => name_h + 2.0 * gap + label_h.max(tags.iter().flatten().map(|l| chip(l)[1]).fold(0.0_f32, f32::max)) / 2.0,
    };
    let span_end = last_step as f32 * step;
    if model.show_branches {
        for (b, laid) in names.iter().enumerate() {
            let lane = lane_of[b];
            let size = name_chip(laid);
            let (from, to, center) = match model.flow {
                Flow::LeftRight => {
                    let y = lane_origin + lane as f32 * lane_gap;
                    ([-gap, y], [span_end + 2.0 * r, y], [-lead + size[0] / 2.0, y])
                }
                Flow::TopBottom => {
                    let x = lane_origin + lane as f32 * lane_gap;
                    ([x, -gap], [x, span_end + 2.0 * r], [x, -lead + size[1] / 2.0])
                }
                Flow::BottomTop => {
                    let x = lane_origin + lane as f32 * lane_gap;
                    ([x, -2.0 * r], [x, span_end + gap], [x, span_end + lead - size[1] / 2.0])
                }
            };
            out.stroke(Geom::path(PathData::polyline(&[from, to])), Stroke::hairline(style::GRID).dash(Dash::Dashed));
            let rect = Rect::centered(center, size);
            out.shape(
                Geom::rounded(rect, RadiusTier::Sm),
                Some(series(color(b), Tone::Soft)),
                Some(Stroke::hairline(series(color(b), Tone::Solid))),
            );
            out.text_centered(laid, rect.center(), style::TEXT);
        }
    }

    // Steps that hold a commit, per lane.
    let mut lane_steps: Vec<Vec<usize>> = vec![Vec::new(); lanes.len()];
    for (i, c) in model.commits.iter().enumerate() {
        lane_steps[lane_of[c.branch]].push(step_of[i]);
    }
    for steps in &mut lane_steps {
        steps.sort_unstable();
    }
    // The steps each lane's own line covers: from where it branched off to
    // its last commit.
    let mut lane_extent: Vec<Option<(usize, usize)>> = vec![None; lanes.len()];
    for (i, c) in model.commits.iter().enumerate() {
        let lane = lane_of[c.branch];
        let lo = c.parents.first().map_or(step_of[i], |&p| step_of[p].min(step_of[i]));
        let e = lane_extent[lane].get_or_insert((lo, step_of[i]));
        e.0 = e.0.min(lo);
        e.1 = e.1.max(step_of[i]);
    }
    // A run along `lane` from `a` to `b` is clear when no commit sits on it
    // and, for a line in another lane's colour, the lane's own line is not
    // there to be covered.
    let clear = |lane: usize, a: usize, b: usize, own_colour: bool| {
        let (lo, hi) = (a.min(b), a.max(b));
        let covered = !own_colour && lane_extent[lane].is_some_and(|(x, y)| x < hi && y > lo);
        !busy(&lane_steps[lane], a, b) && !covered
    };

    // Links between commits, under the dots.
    let bend = (step.min(lane_gap) * 0.45).min(16.0 * k);
    let line = |ink: Ink| Stroke::new(ink, StrokeWidth::Thick);
    for (i, c) in model.commits.iter().enumerate() {
        for (n, &p) in c.parents.iter().enumerate() {
            let (a, b) = (at(p), at(i));
            let parent_branch = model.commits[p].branch;
            let same_lane = lane_of[parent_branch] == lane_of[c.branch];
            let (from_lane, to_lane) = (lane_of[parent_branch], lane_of[c.branch]);
            // Across the flow at the parent, then along the commit's lane;
            // or along the parent's lane, then across at the commit.
            let (cross_first, along_first) = if horizontal {
                (vec![a, [a[0], b[1]], b], vec![a, [b[0], a[1]], b])
            } else {
                (vec![a, [b[0], a[1]], b], vec![a, [a[0], b[1]], b])
            };
            let points: Vec<[f32; 2]> = if same_lane {
                vec![a, b]
            } else {
                let (s0, s1) = (step_of[p], step_of[i]);
                // Branch-offs prefer the new lane, merges and picks the source
                // lane; either gives way when another commit sits on its run.
                let prefer_cross = n == 0;
                // Branch-offs take the commit's colour, merges the source's.
                let cross_clear = clear(to_lane, s0, s1, n == 0);
                let along_clear = clear(from_lane, s0, s1, n != 0);
                match (prefer_cross, cross_clear, along_clear) {
                    (true, true, _) | (false, true, false) => cross_first,
                    (false, _, true) | (true, false, true) => along_first,
                    _ => {
                        // Both lanes are taken: run just short of the
                        // commit's lane.
                        let toward = if to_lane > from_lane { 1.0 } else { -1.0 };
                        if horizontal {
                            let mid = b[1] - toward * lane_gap / 2.0;
                            vec![a, [a[0], mid], [b[0], mid], b]
                        } else {
                            let mid = b[0] - toward * lane_gap / 2.0;
                            vec![a, [mid, a[1]], [mid, b[1]], b]
                        }
                    }
                }
            };
            let ink = if n == 0 { series(color(c.branch), Tone::Solid) } else { series(color(parent_branch), Tone::Solid) };
            let mut stroke = line(ink);
            if n > 0 && c.kind == CommitKind::CherryPick {
                stroke = stroke.dash(Dash::Dashed);
            }
            out.stroke(Geom::path(rounded_polyline(&points, bend)), stroke);
        }
    }

    // Commits, labels and tags.
    let ring = Stroke::new(style::NODE_FILL, StrokeWidth::Px(2.0 * k));
    for (i, c) in model.commits.iter().enumerate() {
        let p = at(i);
        let solid = series(color(c.branch), Tone::Solid);
        let dot = Rect::centered(p, [2.0 * r, 2.0 * r]);
        match c.kind {
            CommitKind::Normal => out.shape(Geom::Ellipse(dot), Some(solid), Some(ring)),
            CommitKind::Merge => {
                out.shape(Geom::Ellipse(dot), Some(solid), Some(ring));
                out.fill(Geom::Ellipse(dot.inset(r * 0.5, r * 0.5)), style::NODE_FILL);
            }
            CommitKind::Reverse => {
                out.shape(Geom::Ellipse(dot), Some(solid), Some(ring));
                let d = r * 0.45;
                let mut cross = PathData::new();
                cross
                    .move_to([p[0] - d, p[1] - d])
                    .line_to([p[0] + d, p[1] + d])
                    .move_to([p[0] + d, p[1] - d])
                    .line_to([p[0] - d, p[1] + d]);
                out.stroke(Geom::path(cross), Stroke::new(series(color(c.branch), Tone::On), StrokeWidth::Thick));
            }
            CommitKind::Highlight => {
                let square = Rect::centered(p, [2.0 * r * 0.95, 2.0 * r * 0.95]);
                out.shape(Geom::rounded(square, RadiusTier::Xs), Some(solid), Some(ring));
                out.fill(Geom::rounded(square.inset(r * 0.45, r * 0.45), RadiusTier::Xs), series(color(c.branch), Tone::On));
            }
            CommitKind::CherryPick => {
                out.shape(Geom::Ellipse(dot.inset(1.0 * k, 1.0 * k)), Some(style::NODE_FILL), Some(Stroke::new(solid, StrokeWidth::Thick)));
                out.fill(Geom::Ellipse(dot.inset(r * 0.6, r * 0.6)), solid);
            }
        }
        if let Some(laid) = &labels[i] {
            let size = chip(laid);
            let center = if horizontal {
                [p[0], p[1] + r + gap / 2.0 + size[1] / 2.0]
            } else {
                [p[0] + r + gap / 2.0 + size[0] / 2.0, p[1]]
            };
            out.chip(laid, center, chip_pad);
        }
        let mut offset = r + gap;
        for laid in &tags[i] {
            let size = chip(laid);
            let center = if horizontal {
                [p[0], p[1] - offset - size[1] / 2.0]
            } else {
                [p[0] - offset - size[0] / 2.0, p[1]]
            };
            let rect = Rect::centered(center, size);
            out.shape(Geom::rounded(rect, style::CHIP_CORNER), Some(TAG_FILL), Some(Stroke::hairline(TAG_STROKE)));
            out.text_centered(laid, center, style::TEXT);
            offset += if horizontal { size[1] } else { size[0] } + gap / 2.0;
        }
    }

    let content = out.bounds();
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}
