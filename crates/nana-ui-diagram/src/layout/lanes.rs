//! Lanes: nodes kept inside parallel bands that share one set of ranks.
//!
//! Swimlane and event-modeling diagrams place every node in its own lane
//! and advance along a common flow axis. Ranks come from the edges
//! (longest path after breaking cycles) unless the caller fixes them, so
//! one rank is one column across every lane. Nodes that share a lane and a
//! rank stack across the flow.
//!
//! Edges are orthogonal and never cross a node: a run across the lanes
//! always sits in the gap between two ranks on its own track, and a run
//! along the flow either follows a node's own line when nothing stands in
//! the way or drops into the padding band of the target lane. Gaps widen
//! to fit their tracks and the labels riding on them.
//!
//! The work happens in canonical coordinates, flow along `x` and lanes
//! stacked along `y`, and is mapped to the requested direction at the end.
//! Lane headers stay at the left (or top) whatever the flow direction.

use std::collections::HashMap;

use super::graph::Direction;
use super::grid::{simplify, spread};
use crate::ir::Rect;
use crate::{DiagramError, LimitKind, Limits};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneSpec {
    /// The title's on-screen size; zero for an untitled lane.
    pub title: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneNode {
    pub lane: usize,
    /// On-screen size.
    pub size: [f32; 2],
    /// A fixed rank (a time frame); `None` ranks from the edges.
    pub rank: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneEdge {
    pub from: usize,
    pub to: usize,
    /// The label's on-screen size.
    pub label: Option<[f32; 2]>,
}

#[derive(Debug, Clone, Default)]
pub struct LaneGraph {
    pub direction: Direction,
    pub lanes: Vec<LaneSpec>,
    pub nodes: Vec<LaneNode>,
    pub edges: Vec<LaneEdge>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneSpacing {
    /// Least gap between two ranks.
    pub rank_sep: f32,
    /// Gap between nodes stacked in one cell.
    pub node_sep: f32,
    /// Padding between a lane's border and its nodes.
    pub lane_pad: f32,
    /// Distance between parallel edge runs.
    pub track_sep: f32,
    /// Padding around a lane title.
    pub header_pad: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LaneRoute {
    pub points: Vec<[f32; 2]>,
    pub label: Option<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct LaneLayout {
    /// Each lane's whole band, header included.
    pub lanes: Vec<Rect>,
    /// Each lane's header cell, where its title is centred.
    pub headers: Vec<Rect>,
    pub nodes: Vec<Rect>,
    pub edges: Vec<LaneRoute>,
    pub bounds: Rect,
    /// The rank of every node.
    pub ranks: Vec<usize>,
}

/// Which way a canonical run leaves or enters a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Port {
    /// Flow end (right in canonical terms).
    Out,
    /// Flow start.
    In,
    /// Across start (up).
    Up,
    /// Across end (down).
    Down,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Plan {
    /// No route: a dropped self-loop or a broken reference.
    None,
    SelfLoop,
    /// Straight across the lanes inside one rank.
    Across,
    /// Leave at `Out`, cross in the gap after the rank, enter at `Out`.
    SameRankDetour {
        gap: usize,
    },
    /// Run along the source's line to `gap`, cross there, then along the
    /// target's line.
    Direct {
        gap: usize,
    },
    /// Cross at `first` into lane `band`'s padding (`top` or bottom), run
    /// along it to `second`, cross to the target.
    Band {
        first: usize,
        second: usize,
        lane: usize,
        top: bool,
    },
}

fn ranks_from_edges(n: usize, edges: &[(usize, usize)]) -> Vec<usize> {
    // Break cycles: an edge back to a node still on the DFS stack is
    // reversed. Iterative so deep chains cannot overflow.
    let mut out: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, &(a, b)) in edges.iter().enumerate() {
        if a < n && b < n && a != b {
            out[a].push(i);
        }
    }
    let mut state = vec![0u8; n];
    let mut reversed = vec![false; edges.len()];
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        state[root] = 1;
        while let Some(top) = stack.last_mut() {
            let (v, k) = *top;
            if k < out[v].len() {
                top.1 += 1;
                let e = out[v][k];
                let w = edges[e].1;
                match state[w] {
                    0 => {
                        state[w] = 1;
                        stack.push((w, 0));
                    }
                    1 => reversed[e] = true,
                    _ => {}
                }
            } else {
                state[v] = 2;
                stack.pop();
            }
        }
    }
    let mut succ: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for (i, &(a, b)) in edges.iter().enumerate() {
        if a >= n || b >= n || a == b {
            continue;
        }
        let (a, b) = if reversed[i] { (b, a) } else { (a, b) };
        succ[a].push(b);
        indeg[b] += 1;
    }
    let mut rank = vec![0usize; n];
    let mut queue: std::collections::VecDeque<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    while let Some(v) = queue.pop_front() {
        for &w in &succ[v] {
            rank[w] = rank[w].max(rank[v] + 1);
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push_back(w);
            }
        }
    }
    rank
}

/// Lay a lane graph out.
pub fn layout(
    graph: &LaneGraph,
    spacing: &LaneSpacing,
    limits: &Limits,
) -> Result<LaneLayout, DiagramError> {
    let n = graph.nodes.len();
    let lanes = graph.lanes.len().max(1);
    let horizontal = graph.direction.horizontal();
    // Canonical sizes: [along, across].
    let canon = |s: [f32; 2]| if horizontal { s } else { [s[1], s[0]] };

    // Ranks: fixed ones win, the rest come from the edges.
    let mut ranks = if graph.nodes.iter().all(|nd| nd.rank.is_some()) {
        vec![0; n]
    } else {
        let pairs: Vec<(usize, usize)> = graph.edges.iter().map(|e| (e.from, e.to)).collect();
        ranks_from_edges(n, &pairs)
    };
    for (r, nd) in ranks.iter_mut().zip(&graph.nodes) {
        if let Some(fixed) = nd.rank {
            *r = fixed;
        }
    }
    // Compact unused ranks away.
    let mut distinct = ranks.clone();
    distinct.sort_unstable();
    distinct.dedup();
    for r in &mut ranks {
        *r = distinct.partition_point(|&d| d < *r);
    }
    let nranks = distinct.len().max(1);
    limits.check_count(
        nranks.saturating_mul(lanes),
        limits.elements.saturating_mul(4),
        LimitKind::Elements,
    )?;
    let lane_of = |v: usize| graph.nodes[v].lane.min(lanes - 1);

    // Cells and stacking order.
    let mut cells: HashMap<(usize, usize), Vec<usize>> = HashMap::new();
    for (v, &r) in ranks.iter().enumerate() {
        cells.entry((lane_of(v), r)).or_default().push(v);
    }
    let mut stack_index = vec![0usize; n];
    for list in cells.values() {
        for (k, &v) in list.iter().enumerate() {
            stack_index[v] = k;
        }
    }

    // Lane-local across geometry: each cell's stack is centred in the
    // lane's inner height.
    let sizes: Vec<[f32; 2]> = graph.nodes.iter().map(|nd| canon(nd.size)).collect();
    let titles: Vec<[f32; 2]> = (0..lanes)
        .map(|l| graph.lanes.get(l).map_or([0.0, 0.0], |s| canon(s.title)))
        .collect();
    let stack_h = |list: &[usize]| {
        list.iter().map(|&v| sizes[v][1]).sum::<f32>()
            + spacing.node_sep * (list.len().saturating_sub(1)) as f32
    };
    let mut inner = vec![0.0f32; lanes];
    for (&(l, _), list) in &cells {
        inner[l] = inner[l].max(stack_h(list));
    }
    for l in 0..lanes {
        let title = titles[l][1];
        if title > 0.0 {
            inner[l] = inner[l].max(title + 2.0 * spacing.header_pad - 2.0 * spacing.lane_pad);
        }
        inner[l] = inner[l].max(spacing.track_sep);
    }
    // Centre of every node, measured from the top of its lane's inner area.
    let mut local = vec![0.0f32; n];
    for (&(l, _), list) in &cells {
        let mut y = (inner[l] - stack_h(list)) / 2.0;
        for &v in list {
            local[v] = y + sizes[v][1] / 2.0;
            y += sizes[v][1] + spacing.node_sep;
        }
    }

    // Rank widths along the flow.
    let mut rank_w = vec![0.0f32; nranks];
    for v in 0..n {
        rank_w[ranks[v]] = rank_w[ranks[v]].max(sizes[v][0]);
    }

    // Is the line `y` (lane-local) blocked in lane `l` strictly between
    // ranks `a` and `b`?
    let blocked_line = |l: usize, y0: f32, y1: f32, a: usize, b: usize| -> bool {
        let (lo, hi) = (a.min(b), a.max(b));
        (lo + 1..hi).any(|r| {
            cells.get(&(l, r)).is_some_and(|list| {
                list.iter().any(|&w| {
                    local[w] - sizes[w][1] / 2.0 - 2.0 < y1
                        && local[w] + sizes[w][1] / 2.0 + 2.0 > y0
                })
            })
        })
    };
    let key = |v: usize| (lane_of(v), stack_index[v]);
    // Across position over the whole diagram, by lane then lane-local centre.
    let at = |v: usize| lane_of(v) as f32 * 1e6 + local[v];

    // Plans.
    let mut plans = Vec::with_capacity(graph.edges.len());
    for e in &graph.edges {
        let (u, v) = (e.from, e.to);
        if u >= n || v >= n {
            plans.push(Plan::None);
            continue;
        }
        if u == v {
            plans.push(Plan::SelfLoop);
            continue;
        }
        let (ru, rv) = (ranks[u], ranks[v]);
        if ru == rv {
            let (ka, kb) = (key(u).min(key(v)), key(u).max(key(v)));
            let between = (ka.0..=kb.0)
                .filter_map(|l| cells.get(&(l, ru)))
                .flatten()
                .any(|&w| key(w) > ka && key(w) < kb);
            plans.push(if between {
                Plan::SameRankDetour { gap: ru }
            } else {
                Plan::Across
            });
            continue;
        }
        let forward = rv > ru;
        let gu = if forward { ru } else { ru - 1 };
        let gv = if forward { rv - 1 } else { rv };
        let band_u = sizes[u][1] * 0.3;
        let band_v = sizes[v][1] * 0.3;
        let a_free = !blocked_line(lane_of(u), local[u] - band_u, local[u] + band_u, ru, rv);
        let b_free = !blocked_line(lane_of(v), local[v] - band_v, local[v] + band_v, ru, rv);
        let plan = if a_free {
            Plan::Direct { gap: gv }
        } else if b_free {
            // Through the source gap, then along the target's line.
            Plan::Direct { gap: gu }
        } else {
            let (lu, lv) = (lane_of(u), lane_of(v));
            Plan::Band {
                first: gu,
                second: gv,
                lane: lv,
                top: lu < lv || (lu == lv && local[u] < local[v]),
            }
        };
        plans.push(plan);
    }

    // Ports: which side each end uses, then spread ends that share one.
    let mut ports: Vec<[Option<Port>; 2]> = vec![[None, None]; graph.edges.len()];
    for (i, e) in graph.edges.iter().enumerate() {
        let (u, v) = (e.from, e.to);
        ports[i] = match plans[i] {
            Plan::None | Plan::SelfLoop => [None, None],
            Plan::Across => {
                if key(u) < key(v) {
                    [Some(Port::Down), Some(Port::Up)]
                } else {
                    [Some(Port::Up), Some(Port::Down)]
                }
            }
            Plan::SameRankDetour { .. } => [Some(Port::Out), Some(Port::Out)],
            Plan::Direct { .. } | Plan::Band { .. } => {
                if ranks[v] > ranks[u] {
                    [Some(Port::Out), Some(Port::In)]
                } else {
                    [Some(Port::In), Some(Port::Out)]
                }
            }
        };
    }
    // Ends on one port: (edge, which end, sort key, tie-break).
    let mut shared: HashMap<(usize, Port), Vec<_>> = HashMap::new();
    for (i, e) in graph.edges.iter().enumerate() {
        for (end, node, other) in [(0usize, e.from, e.to), (1, e.to, e.from)] {
            if let Some(p) = ports[i][end] {
                // Order by where the other end lies across (for in/out
                // sides) or along (for up/down sides).
                // Ends whose other node lies on the same line nest by
                // distance: the nearest one takes the port on the side the
                // others come from, so no run crosses another's bend.
                let dist = ranks[other].abs_diff(ranks[node]) as f32;
                let (k, tie) = match p {
                    Port::Out | Port::In => {
                        let k = at(other);
                        (k, if k >= at(node) { -dist } else { dist })
                    }
                    Port::Up | Port::Down => (ranks[other] as f32, 0.0),
                };
                shared.entry((node, p)).or_default().push((i, end, k, tie));
            }
        }
    }
    let mut offset: Vec<[f32; 2]> = vec![[0.5, 0.5]; graph.edges.len()];
    for list in shared.values_mut() {
        list.sort_by(|a, b| {
            a.2.total_cmp(&b.2)
                .then(a.3.total_cmp(&b.3))
                .then(a.0.cmp(&b.0))
                .then(a.1.cmp(&b.1))
        });
        for (k, &(i, end, _, _)) in list.iter().enumerate() {
            offset[i][end] = spread(k, list.len());
        }
    }

    // Tracks: every run across a gap, and every run along a band. A gap
    // run is (edge, label along-size, across position where it meets the
    // gap's left side, across position where it leaves on the right),
    // positions approximated by lane and lane-local centre.
    let mut gap_tracks: Vec<Vec<(usize, f32, f32, f32)>> = vec![Vec::new(); nranks + 1];
    let mut band_tracks: HashMap<(usize, bool), Vec<usize>> = HashMap::new();
    let label_along = |i: usize| graph.edges[i].label.map_or(0.0, |l| canon(l)[0]);
    let band_at = |l: usize, top: bool| l as f32 * 1e6 + if top { -1.0 } else { inner[l] + 1.0 };
    for (i, plan) in plans.iter().enumerate() {
        let (u, v) = (graph.edges[i].from, graph.edges[i].to);
        match *plan {
            Plan::Direct { gap } => {
                let (l, r) = if ranks[v] > ranks[u] {
                    (at(u), at(v))
                } else {
                    (at(v), at(u))
                };
                gap_tracks[gap.min(nranks)].push((i, label_along(i), l, r));
            }
            Plan::SameRankDetour { gap } => {
                gap_tracks[gap.min(nranks)].push((i, label_along(i), at(u), at(v)));
            }
            Plan::Band {
                first,
                second,
                lane,
                top,
            } => {
                let b = band_at(lane, top);
                let forward = ranks[v] > ranks[u];
                let (l1, r1) = if forward { (at(u), b) } else { (b, at(u)) };
                let (l2, r2) = if forward { (b, at(v)) } else { (at(v), b) };
                gap_tracks[first.min(nranks)].push((i, label_along(i), l1, r1));
                gap_tracks[second.min(nranks)].push((i, 0.0, l2, r2));
                band_tracks.entry((lane, top)).or_default().push(i);
            }
            _ => {}
        }
    }
    // Order tracks so runs nest instead of crossing: a run heading down
    // takes a track further right the higher it starts, a run heading up
    // the lower it starts. Down runs sit left of up runs.
    for list in &mut gap_tracks {
        list.sort_by(|a, b| {
            // Equal starts share a port side, spread in target order.
            let key = |t: &(usize, f32, f32, f32)| {
                let down = t.3 >= t.2;
                if down {
                    (0u8, -t.2, -t.3)
                } else {
                    (1u8, t.2, t.3)
                }
            };
            let (ka, kb) = (key(a), key(b));
            ka.0.cmp(&kb.0)
                .then(ka.1.total_cmp(&kb.1))
                .then(ka.2.total_cmp(&kb.2))
                .then(a.0.cmp(&b.0))
        });
    }
    let track_w = |l: f32| l.max(0.0) + spacing.track_sep;
    let need: Vec<f32> = gap_tracks
        .iter()
        .map(|list| list.iter().map(|t| track_w(t.1)).sum::<f32>() + spacing.track_sep)
        .collect();
    let gap_w: Vec<f32> = need.iter().map(|n| n.max(spacing.rank_sep)).collect();

    // Along positions: header strip, then ranks and gaps.
    let header_w = titles.iter().map(|t| t[0]).fold(0.0f32, f32::max);
    let header_w = if header_w > 0.0 {
        header_w + 2.0 * spacing.header_pad
    } else {
        0.0
    };
    let mut rank_x = vec![0.0f32; nranks];
    let mut x = header_w + spacing.lane_pad;
    for r in 0..nranks {
        rank_x[r] = x;
        x += rank_w[r];
        if r + 1 < nranks {
            x += gap_w[r];
        }
    }
    // A trailing gap only when a detour uses it.
    let trailing = if gap_tracks[nranks - 1].is_empty() {
        0.0
    } else {
        gap_w[nranks - 1]
    };
    let total_along = x + trailing.max(spacing.lane_pad);
    let mut track_x: HashMap<(usize, usize), f32> = HashMap::new();
    for (g, list) in gap_tracks.iter().enumerate().take(nranks) {
        let mut at = rank_x[g] + rank_w[g] + (gap_w[g] - need[g]) / 2.0 + spacing.track_sep;
        for &(i, l, _, _) in list {
            let w = track_w(l);
            let center = at + (w - spacing.track_sep) / 2.0;
            track_x.entry((g, i)).or_insert(center);
            at += w;
        }
    }

    // Across positions: lanes stacked with their bands.
    let band_size = |l: usize, top: bool| {
        let k = band_tracks.get(&(l, top)).map_or(0, Vec::len);
        spacing.lane_pad.max((k as f32 + 1.0) * spacing.track_sep)
    };
    let mut lane_y = vec![0.0f32; lanes];
    let mut lane_h = vec![0.0f32; lanes];
    let mut y = 0.0;
    for l in 0..lanes {
        lane_y[l] = y;
        lane_h[l] = band_size(l, true) + inner[l] + band_size(l, false);
        y += lane_h[l];
    }
    let total_across = y;
    let inner_top = |l: usize| lane_y[l] + band_size(l, true);
    let mut band_y: HashMap<(usize, bool, usize), f32> = HashMap::new();
    // Each list is already in edge order.
    for (&(l, top), list) in &band_tracks {
        for (k, &i) in list.iter().enumerate() {
            let yy = if top {
                inner_top(l) - (k as f32 + 1.0) * spacing.track_sep
            } else {
                inner_top(l) + inner[l] + (k as f32 + 1.0) * spacing.track_sep
            };
            band_y.insert((l, top, i), yy);
        }
    }

    // Canonical node rectangles.
    let rects: Vec<Rect> = (0..n)
        .map(|v| {
            let r = ranks[v];
            let cx = rank_x[r] + rank_w[r] / 2.0;
            let cy = inner_top(lane_of(v)) + local[v];
            Rect::centered([cx, cy], sizes[v])
        })
        .collect();

    // Routes.
    let port_point = |v: usize, p: Port, t: f32| -> [f32; 2] {
        let r = rects[v];
        match p {
            Port::Out => [r.right(), r.y + r.h * t],
            Port::In => [r.x, r.y + r.h * t],
            Port::Up => [r.x + r.w * t, r.y],
            Port::Down => [r.x + r.w * t, r.bottom()],
        }
    };
    let mut routes = Vec::with_capacity(graph.edges.len());
    for (i, e) in graph.edges.iter().enumerate() {
        let (u, v) = (e.from, e.to);
        let ends = |end: usize| {
            ports[i][end].map(|p| port_point(if end == 0 { u } else { v }, p, offset[i][end]))
        };
        let (mut points, mut label): (Vec<[f32; 2]>, Option<[f32; 2]>) = (Vec::new(), None);
        match plans[i] {
            Plan::None => {}
            Plan::SelfLoop => {
                let r = rects[u];
                let s = spacing.lane_pad * 0.6;
                points = vec![
                    [r.right(), r.y + r.h * 0.3],
                    [r.right() + s, r.y + r.h * 0.3],
                    [r.right() + s, r.bottom() + s],
                    [r.x + r.w * 0.7, r.bottom() + s],
                    [r.x + r.w * 0.7, r.bottom()],
                ];
                label = Some([r.right() + s, r.bottom() + s]);
            }
            Plan::Across => {
                if let (Some(a), Some(mut b)) = (ends(0), ends(1)) {
                    b[0] = a[0];
                    label = Some([a[0], (a[1] + b[1]) / 2.0]);
                    points = vec![a, b];
                }
            }
            Plan::SameRankDetour { gap } | Plan::Direct { gap } => {
                if let (Some(a), Some(b)) = (ends(0), ends(1)) {
                    let tx = track_x
                        .get(&(gap, i))
                        .copied()
                        .unwrap_or((a[0] + b[0]) / 2.0);
                    points = vec![a, [tx, a[1]], [tx, b[1]], b];
                    label = Some([tx, (a[1] + b[1]) / 2.0]);
                }
            }
            Plan::Band {
                first,
                second,
                lane,
                top,
            } => {
                if let (Some(a), Some(b)) = (ends(0), ends(1)) {
                    let t1 = track_x.get(&(first, i)).copied().unwrap_or(a[0]);
                    let t2 = track_x.get(&(second, i)).copied().unwrap_or(b[0]);
                    let by = band_y.get(&(lane, top, i)).copied().unwrap_or(a[1]);
                    points = vec![a, [t1, a[1]], [t1, by], [t2, by], [t2, b[1]], b];
                    label = Some([t1, (a[1] + by) / 2.0]);
                }
            }
        }
        routes.push(LaneRoute {
            points: simplify(&points),
            label,
        });
    }

    // Lane bands and headers.
    let lane_rects: Vec<Rect> = (0..lanes)
        .map(|l| Rect::new(0.0, lane_y[l], total_along, lane_h[l]))
        .collect();
    let header_rects: Vec<Rect> = (0..lanes)
        .map(|l| Rect::new(0.0, lane_y[l], header_w, lane_h[l]))
        .collect();

    // Map to screen: content mirrored for RL/BT, headers kept at the start.
    let dir = graph.direction;
    let mirror = matches!(dir, Direction::RL | Direction::BT);
    let map_point = |p: [f32; 2]| -> [f32; 2] {
        let along = if mirror && p[0] >= header_w {
            header_w + total_along - p[0]
        } else {
            p[0]
        };
        if horizontal {
            [along, p[1]]
        } else {
            [p[1], along]
        }
    };
    let map_rect = |r: Rect| -> Rect {
        let a = map_point([r.x, r.y]);
        let b = map_point([r.right(), r.bottom()]);
        Rect::new(
            a[0].min(b[0]),
            a[1].min(b[1]),
            (a[0] - b[0]).abs(),
            (a[1] - b[1]).abs(),
        )
    };
    let swap = |r: Rect| {
        if horizontal {
            r
        } else {
            Rect::new(r.y, r.x, r.h, r.w)
        }
    };
    let size = if horizontal {
        [total_along, total_across]
    } else {
        [total_across, total_along]
    };
    Ok(LaneLayout {
        lanes: lane_rects.into_iter().map(swap).collect(),
        headers: header_rects.into_iter().map(swap).collect(),
        nodes: rects.into_iter().map(map_rect).collect(),
        edges: routes
            .into_iter()
            .map(|r| LaneRoute {
                points: r.points.into_iter().map(map_point).collect(),
                label: r.label.map(map_point),
            })
            .collect(),
        bounds: Rect::new(0.0, 0.0, size[0], size[1]),
        ranks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spacing() -> LaneSpacing {
        LaneSpacing {
            rank_sep: 40.0,
            node_sep: 10.0,
            lane_pad: 12.0,
            track_sep: 8.0,
            header_pad: 8.0,
        }
    }

    fn node(lane: usize) -> LaneNode {
        LaneNode {
            lane,
            size: [60.0, 30.0],
            rank: None,
        }
    }

    fn edge(from: usize, to: usize) -> LaneEdge {
        LaneEdge {
            from,
            to,
            label: None,
        }
    }

    #[test]
    fn ranks_are_shared_and_nodes_stay_in_their_lanes() {
        let g = LaneGraph {
            direction: Direction::LR,
            lanes: vec![
                LaneSpec {
                    title: [40.0, 16.0],
                },
                LaneSpec {
                    title: [40.0, 16.0],
                },
            ],
            nodes: vec![node(0), node(0), node(1), node(1)],
            edges: vec![edge(0, 1), edge(1, 2), edge(2, 3), edge(1, 3)],
        };
        let out = layout(&g, &spacing(), &Limits::DEFAULT).unwrap();
        assert_eq!(out.ranks, vec![0, 1, 2, 3]);
        for (v, nd) in g.nodes.iter().enumerate() {
            let lane = out.lanes[nd.lane];
            let r = out.nodes[v];
            assert!(r.y >= lane.y && r.bottom() <= lane.bottom(), "{v}");
            assert!(r.x >= out.headers[nd.lane].right());
        }
        for (i, a) in out.nodes.iter().enumerate() {
            for b in &out.nodes[i + 1..] {
                assert!(!a.intersects(b));
            }
        }
        // Every route is orthogonal.
        for route in &out.edges {
            for w in route.points.windows(2) {
                assert!((w[0][0] - w[1][0]).abs() < 0.01 || (w[0][1] - w[1][1]).abs() < 0.01);
            }
        }
    }

    #[test]
    fn routes_do_not_cross_nodes() {
        // 0 -> 2 in lane 0 skips over 1, which sits on the same line.
        let g = LaneGraph {
            direction: Direction::LR,
            lanes: vec![LaneSpec { title: [0.0, 0.0] }],
            nodes: vec![node(0), node(0), node(0)],
            edges: vec![edge(0, 1), edge(1, 2), edge(0, 2), edge(2, 0)],
        };
        let out = layout(&g, &spacing(), &Limits::DEFAULT).unwrap();
        for (i, route) in out.edges.iter().enumerate() {
            for w in route.points.windows(2) {
                let seg = Rect::new(
                    w[0][0].min(w[1][0]),
                    w[0][1].min(w[1][1]),
                    (w[0][0] - w[1][0]).abs(),
                    (w[0][1] - w[1][1]).abs(),
                );
                for r in &out.nodes {
                    assert!(
                        !seg.intersects(&r.inset(1.0, 1.0)),
                        "edge {i} crosses {r:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn vertical_flow_puts_lanes_side_by_side() {
        let g = LaneGraph {
            direction: Direction::TB,
            lanes: vec![
                LaneSpec {
                    title: [40.0, 16.0],
                },
                LaneSpec {
                    title: [40.0, 16.0],
                },
            ],
            nodes: vec![node(0), node(1)],
            edges: vec![edge(0, 1)],
        };
        let out = layout(&g, &spacing(), &Limits::DEFAULT).unwrap();
        assert!(out.lanes[1].x >= out.lanes[0].right() - 0.01);
        assert!(out.nodes[1].y > out.nodes[0].bottom());
    }
}
