//! Grids: measured column and row tracks with spanning cells, flow and
//! constraint placement, and orthogonal connectors between placed boxes.
//!
//! Block, architecture and C4 diagrams put their elements on a grid rather
//! than in ranks. A track is as wide as the widest single-span cell in it;
//! a spanning cell that still does not fit spreads its shortfall evenly
//! over the tracks it covers. A grid given a minimum size grows every
//! track evenly, so a nested grid can fill the cell its parent gave it.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::ir::Rect;

/// One cell of a grid, at its natural size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridItem {
    pub col: usize,
    pub row: usize,
    /// Columns and rows covered; zero counts as one.
    pub cols: usize,
    pub rows: usize,
    pub size: [f32; 2],
}

impl GridItem {
    pub fn new(col: usize, row: usize, size: [f32; 2]) -> Self {
        Self {
            col,
            row,
            cols: 1,
            rows: 1,
            size,
        }
    }

    pub fn span(mut self, cols: usize, rows: usize) -> Self {
        self.cols = cols;
        self.rows = rows;
        self
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridSpec {
    pub items: Vec<GridItem>,
    pub col_gap: f32,
    pub row_gap: f32,
    /// The grid grows its tracks evenly to at least this size.
    pub min_size: [f32; 2],
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct GridLayout {
    /// Left edge and width of each column.
    pub cols: Vec<(f32, f32)>,
    /// Top edge and height of each row.
    pub rows: Vec<(f32, f32)>,
    /// The area each item covers, in item order, from the grid's origin.
    pub cells: Vec<Rect>,
    pub size: [f32; 2],
}

impl GridLayout {
    /// The rectangle covering `cols` columns and `rows` rows from a cell.
    pub fn area(&self, col: usize, row: usize, cols: usize, rows: usize) -> Rect {
        let (x, w) = span_extent(&self.cols, col, cols.max(1));
        let (y, h) = span_extent(&self.rows, row, rows.max(1));
        Rect::new(x, y, w, h)
    }
}

fn span_extent(tracks: &[(f32, f32)], start: usize, count: usize) -> (f32, f32) {
    if tracks.is_empty() {
        return (0.0, 0.0);
    }
    let first = start.min(tracks.len() - 1);
    let last = (start + count - 1).min(tracks.len() - 1);
    let begin = tracks[first].0;
    let end = tracks[last].0 + tracks[last].1;
    (begin, (end - begin).max(0.0))
}

/// Solve one axis: `cells` are `(start, span, size)`.
fn tracks(cells: &[(usize, usize, f32)], gap: f32, min_total: f32) -> Vec<(f32, f32)> {
    let count = cells.iter().map(|c| c.0 + c.1).max().unwrap_or(0);
    let mut widths = vec![0.0f32; count];
    for &(start, span, size) in cells {
        if span <= 1 && start < count {
            widths[start] = widths[start].max(size);
        }
    }
    let mut spanning: Vec<&(usize, usize, f32)> = cells.iter().filter(|c| c.1 > 1).collect();
    spanning.sort_by_key(|c| c.1);
    for &&(start, span, size) in &spanning {
        let end = (start + span).min(count);
        if start >= end {
            continue;
        }
        let n = end - start;
        let have: f32 = widths[start..end].iter().sum::<f32>() + gap * (n - 1) as f32;
        let short = size - have;
        if short > 0.0 {
            for w in &mut widths[start..end] {
                *w += short / n as f32;
            }
        }
    }
    if count > 0 {
        let total: f32 = widths.iter().sum::<f32>() + gap * (count - 1) as f32;
        if min_total > total {
            let extra = (min_total - total) / count as f32;
            for w in &mut widths {
                *w += extra;
            }
        }
    }
    let mut out = Vec::with_capacity(count);
    let mut at = 0.0;
    for w in widths {
        out.push((at, w));
        at += w + gap;
    }
    out
}

/// Size every track and place every item.
pub fn solve(spec: &GridSpec) -> GridLayout {
    let xs: Vec<_> = spec
        .items
        .iter()
        .map(|i| (i.col, i.cols.max(1), i.size[0]))
        .collect();
    let ys: Vec<_> = spec
        .items
        .iter()
        .map(|i| (i.row, i.rows.max(1), i.size[1]))
        .collect();
    let cols = tracks(&xs, spec.col_gap, spec.min_size[0]);
    let rows = tracks(&ys, spec.row_gap, spec.min_size[1]);
    let extent = |t: &[(f32, f32)]| t.last().map_or(0.0, |&(a, w)| a + w);
    let mut layout = GridLayout {
        size: [
            extent(&cols).max(spec.min_size[0]),
            extent(&rows).max(spec.min_size[1]),
        ],
        cols,
        rows,
        cells: Vec::new(),
    };
    layout.cells = spec
        .items
        .iter()
        .map(|i| layout.area(i.col, i.row, i.cols, i.rows))
        .collect();
    layout
}

/// Row-major flow: each span starts where the last ended and wraps to the
/// next row when it no longer fits `columns`. Returns `(col, row)` per span
/// and the spans clamped to the column count.
pub fn flow(spans: &[usize], columns: usize) -> Vec<(usize, usize, usize)> {
    let columns = columns.max(1);
    let mut out = Vec::with_capacity(spans.len());
    let (mut col, mut row) = (0, 0);
    for &span in spans {
        let span = span.clamp(1, columns);
        if col + span > columns {
            col = 0;
            row += 1;
        }
        out.push((col, row, span));
        col += span;
    }
    out
}

/// Grid coordinates from relative placement constraints: each
/// `(a, b, dx, dy)` asks for `b` to sit `dx` columns and `dy` rows from
/// `a` (each −1, 0 or 1). Components are placed breadth-first from their
/// first item; an occupied cell pushes the newcomer further along its
/// direction. Disconnected components sit side by side. The result is
/// normalised to start at `(0, 0)`.
pub fn constrained_cells(
    count: usize,
    constraints: &[(usize, usize, i32, i32)],
) -> Vec<(usize, usize)> {
    let mut adjacency: Vec<Vec<(usize, i32, i32)>> = vec![Vec::new(); count];
    for &(a, b, dx, dy) in constraints {
        if a >= count || b >= count || a == b {
            continue;
        }
        adjacency[a].push((b, dx, dy));
        adjacency[b].push((a, -dx, -dy));
    }
    let mut pos: Vec<Option<(i32, i32)>> = vec![None; count];
    let mut offset_x = 0i32;
    for start in 0..count {
        if pos[start].is_some() {
            continue;
        }
        // Place this component around the origin, then shift it right of
        // everything placed before.
        let mut local = vec![start];
        let mut taken = HashSet::from([(0, 0)]);
        let mut queue = VecDeque::from([start]);
        pos[start] = Some((0, 0));
        while let Some(a) = queue.pop_front() {
            let Some((ax, ay)) = pos[a] else { continue };
            for &(b, dx, dy) in &adjacency[a] {
                if pos[b].is_some() {
                    continue;
                }
                let (sx, sy) = if dx == 0 && dy == 0 {
                    (1, 0)
                } else {
                    (dx.signum(), dy.signum())
                };
                let mut cell = (ax + sx, ay + sy);
                let mut guard = 0;
                while taken.contains(&cell) && guard < count + 4 {
                    cell = (cell.0 + sx, cell.1 + sy);
                    guard += 1;
                }
                if taken.contains(&cell) {
                    // A crowded ray: take the first free cell on its row.
                    let mut x = cell.0;
                    while taken.contains(&(x, cell.1)) {
                        x += 1;
                    }
                    cell = (x, cell.1);
                }
                pos[b] = Some(cell);
                taken.insert(cell);
                local.push(b);
                queue.push_back(b);
            }
        }
        let xs = local.iter().filter_map(|&i| pos[i]).map(|p| p.0);
        let (min_x, max_x) = xs.fold((i32::MAX, i32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        for &i in &local {
            if let Some((x, y)) = pos[i] {
                pos[i] = Some((x - min_x + offset_x, y));
            }
        }
        offset_x += max_x - min_x + 1;
    }
    // Components already start at column 0; rows may still be negative.
    let min_y = pos.iter().flatten().map(|p| p.1).min().unwrap_or(0);
    pos.into_iter()
        .map(|p| {
            let (x, y) = p.unwrap_or((0, 0));
            (x as usize, (y - min_y) as usize)
        })
        .collect()
}

/// The middle of a route's longest segment: where a label reads best and
/// is least likely to sit on a bend.
pub fn label_point(points: &[[f32; 2]]) -> [f32; 2] {
    points
        .windows(2)
        .max_by(|a, b| manhattan(a[0], a[1]).total_cmp(&manhattan(b[0], b[1])))
        .map(|w| [(w[0][0] + w[1][0]) / 2.0, (w[0][1] + w[1][1]) / 2.0])
        .or_else(|| points.first().copied())
        .unwrap_or_default()
}

/// Where a label of `size` sits on a route: on the line where possible,
/// beside it otherwise, always preferring spots clear of `obstacles` and of
/// labels already `taken` (the chosen rectangle is added to them).
pub fn place_label(
    points: &[[f32; 2]],
    size: [f32; 2],
    obstacles: &[Rect],
    taken: &mut Vec<Rect>,
) -> [f32; 2] {
    // Candidates stay within a label's size of the route, so only boxes
    // near it can clash: filter once, then compare candidates to those.
    let (lo, hi) = points.iter().fold(
        ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1])],
                [hi[0].max(p[0]), hi[1].max(p[1])],
            )
        },
    );
    let (x, y) = (lo[0] - size[0] - 4.0, lo[1] - size[1] - 4.0);
    let reach = Rect::new(x, y, hi[0] + size[0] + 4.0 - x, hi[1] + size[1] + 4.0 - y);
    let nearby: Vec<Rect> = obstacles
        .iter()
        .chain(taken.iter())
        .filter(|o| o.intersects(&reach))
        .copied()
        .collect();
    let overlap = |r: &Rect| -> f32 {
        nearby
            .iter()
            .map(|o| {
                let ix = (r.right().min(o.right()) - r.x.max(o.x)).max(0.0);
                let iy = (r.bottom().min(o.bottom()) - r.y.max(o.y)).max(0.0);
                ix * iy
            })
            .sum()
    };
    let total = length(points);
    let mut best: Option<(f32, [f32; 2])> = None;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = manhattan(a, b);
        if len < 1.0 {
            continue;
        }
        let side = if (a[0] - b[0]).abs() < 0.01 {
            [size[0] / 2.0 + 3.0, 0.0]
        } else {
            [0.0, size[1] / 2.0 + 3.0]
        };
        for (k, t) in [0.5f32, 0.35, 0.65, 0.2, 0.8].into_iter().enumerate() {
            let on = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
            for (j, c) in [
                on,
                [on[0] + side[0], on[1] + side[1]],
                [on[0] - side[0], on[1] - side[1]],
            ]
            .into_iter()
            .enumerate()
            {
                let r = Rect::centered(c, size);
                // Clashes dominate; then stay on the line, near the
                // middle of a long segment.
                let cost =
                    overlap(&r) * 100.0 + j as f32 * 40.0 + k as f32 * 6.0 + (total - len) * 0.05;
                if best.is_none_or(|(bc, _)| cost < bc) {
                    best = Some((cost, c));
                }
            }
        }
    }
    let at = best.map_or_else(|| label_point(points), |(_, c)| c);
    taken.push(Rect::centered(at, size));
    at
}

/// Which side of a box a connector leaves or enters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Top,
    Bottom,
    Left,
    Right,
}

impl Side {
    pub fn parse(c: char) -> Option<Self> {
        Some(match c.to_ascii_uppercase() {
            'T' => Self::Top,
            'B' => Self::Bottom,
            'L' => Self::Left,
            'R' => Self::Right,
            _ => return None,
        })
    }

    pub fn horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }

    /// The outward unit vector.
    pub fn normal(self) -> [f32; 2] {
        match self {
            Self::Top => [0.0, -1.0],
            Self::Bottom => [0.0, 1.0],
            Self::Left => [-1.0, 0.0],
            Self::Right => [1.0, 0.0],
        }
    }

    /// The point `t` (0..1) along this side of `r`, top-left to
    /// bottom-right.
    pub fn point(self, r: Rect, t: f32) -> [f32; 2] {
        match self {
            Self::Top => [r.x + r.w * t, r.y],
            Self::Bottom => [r.x + r.w * t, r.bottom()],
            Self::Left => [r.x, r.y + r.h * t],
            Self::Right => [r.right(), r.y + r.h * t],
        }
    }
}

/// Drop repeated points and middle points of straight runs.
pub fn simplify(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(points.len());
    for &p in points {
        if out
            .last()
            .is_some_and(|q| (q[0] - p[0]).abs() < 0.01 && (q[1] - p[1]).abs() < 0.01)
        {
            continue;
        }
        if out.len() >= 2 {
            let a = out[out.len() - 2];
            let b = out[out.len() - 1];
            let straight = ((a[0] - b[0]).abs() < 0.01 && (b[0] - p[0]).abs() < 0.01)
                || ((a[1] - b[1]).abs() < 0.01 && (b[1] - p[1]).abs() < 0.01);
            if straight {
                out.pop();
            }
        }
        out.push(p);
    }
    out
}

/// How long a polyline runs inside the obstacles, a cost for choosing
/// between candidate routes.
pub fn blocked(points: &[[f32; 2]], obstacles: &[Rect]) -> f32 {
    let mut cost = 0.0;
    for w in points.windows(2) {
        let (a, b) = (w[0], w[1]);
        let seg = Rect::new(
            a[0].min(b[0]),
            a[1].min(b[1]),
            (a[0] - b[0]).abs(),
            (a[1] - b[1]).abs(),
        );
        for o in obstacles {
            let o = o.inset(1.0, 1.0);
            let ix = (seg.right().min(o.right()) - seg.x.max(o.x)).max(0.0);
            let iy = (seg.bottom().min(o.bottom()) - seg.y.max(o.y)).max(0.0);
            let touches = seg.x <= o.right()
                && o.x <= seg.right()
                && seg.y <= o.bottom()
                && o.y <= seg.bottom();
            if touches {
                cost += ix.max(iy) + 1.0;
            }
        }
    }
    cost
}

fn manhattan(a: [f32; 2], b: [f32; 2]) -> f32 {
    (a[0] - b[0]).abs() + (a[1] - b[1]).abs()
}

fn length(points: &[[f32; 2]]) -> f32 {
    points.windows(2).map(|w| manhattan(w[0], w[1])).sum()
}

/// One connector between two placed boxes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Connect {
    pub from: usize,
    pub to: usize,
}

/// Orthogonal connectors between boxes: each one leaves the side of its
/// source that faces the target, ends spread along a side shared by
/// several connectors, and a bend goes into whichever free channel keeps
/// the route clear of `obstacles` (the boxes themselves are passed by
/// index and never count against their own connectors). `gap` is the
/// smallest channel width the caller left between boxes. Boxes that
/// overlap each other get no route.
pub fn connect_all(
    boxes: &[Rect],
    links: &[Connect],
    obstacles: &[Rect],
    gap: f32,
) -> Vec<Vec<[f32; 2]>> {
    let sides: Vec<Option<(Side, Side)>> = links
        .iter()
        .map(|l| facing(*boxes.get(l.from)?, *boxes.get(l.to)?))
        .collect();
    // Spread the ends that share a side, ordered by where the other end
    // lies so neighbours do not cross. Entries are (connector, which end,
    // sort key).
    let mut groups: HashMap<(usize, Side), Vec<_>> = HashMap::new();
    for (i, l) in links.iter().enumerate() {
        let Some((sa, sb)) = sides[i] else { continue };
        let ca = boxes[l.from].center();
        let cb = boxes[l.to].center();
        let key_a = if sa.horizontal() { cb[1] } else { cb[0] };
        let key_b = if sb.horizontal() { ca[1] } else { ca[0] };
        groups.entry((l.from, sa)).or_default().push((i, 0, key_a));
        groups.entry((l.to, sb)).or_default().push((i, 1, key_b));
    }
    // Each end sits in exactly one group, so the group order does not matter.
    let mut ends = vec![[0.5f32; 2]; links.len()];
    for list in groups.values_mut() {
        list.sort_by(|a, b| a.2.total_cmp(&b.2).then(a.0.cmp(&b.0)));
        for (k, &(i, end, _)) in list.iter().enumerate() {
            ends[i][end] = spread(k, list.len());
        }
    }
    let mut out = Vec::with_capacity(links.len());
    let mut used = Used::new();
    for (i, l) in links.iter().enumerate() {
        let Some((sa, sb)) = sides[i] else {
            out.push(Vec::new());
            continue;
        };
        let (a, b) = (boxes[l.from], boxes[l.to]);
        // Only boxes near the link can block it or shape a detour: keeping
        // the search local keeps big diagrams linear in their size.
        let span = a.union(&b);
        let reach = gap * 4.0 + span.w.max(span.h) * 0.25;
        let near = span.inset(-reach, -reach);
        let obstacles: Vec<Rect> = obstacles
            .iter()
            .copied()
            .filter(|o| {
                o.intersects(&near)
                    && !same(o, &a)
                    && !same(o, &b)
                    && !o.contains(a.center())
                    && !o.contains(b.center())
            })
            .collect();
        let p0 = sa.point(a, ends[i][0]);
        let p3 = sb.point(b, ends[i][1]);
        let horizontal = sa.horizontal();
        // A shared span lets the connector run straight.
        let (lo, hi) = if horizontal {
            (a.y.max(b.y), a.bottom().min(b.bottom()))
        } else {
            (a.x.max(b.x), a.right().min(b.right()))
        };
        let along = |p: [f32; 2]| if horizontal { p[1] } else { p[0] };
        let mut candidates: Vec<Vec<[f32; 2]>> = Vec::new();
        if hi - lo > 4.0 {
            let mid = if along(p0) >= lo && along(p0) <= hi && along(p3) >= lo && along(p3) <= hi {
                (along(p0) + along(p3)) / 2.0
            } else {
                (lo + hi) / 2.0
            };
            let (mut s, mut e) = (p0, p3);
            if horizontal {
                s[1] = mid;
                e[1] = mid;
            } else {
                s[0] = mid;
                e[0] = mid;
            }
            candidates.push(vec![s, e]);
        }
        // Bends in the channel next to the target, next to the source, and
        // halfway.
        let (from_edge, to_edge) = if horizontal {
            (p0[0], p3[0])
        } else {
            (p0[1], p3[1])
        };
        let dir = if to_edge >= from_edge { 1.0 } else { -1.0 };
        let half = gap / 2.0;
        for bend in [
            to_edge - dir * half,
            from_edge + dir * half,
            (from_edge + to_edge) / 2.0,
        ] {
            let route = if horizontal {
                vec![p0, [bend, p0[1]], [bend, p3[1]], p3]
            } else {
                vec![p0, [p0[0], bend], [p3[0], bend], p3]
            };
            candidates.push(route);
        }
        let shared = |c: &[[f32; 2]]| c.windows(2).map(|w| used.overlap(w[0], w[1])).sum::<f32>();
        let (cost, best) = candidates
            .into_iter()
            .map(|c| (blocked(&c, &obstacles) * 1000.0 + shared(&c) * 4.0, c))
            .min_by(|a, b| (a.0 + length(&a.1)).total_cmp(&(b.0 + length(&b.1))))
            .unwrap_or_default();
        if cost > 0.0 {
            let mut walls = obstacles.clone();
            walls.push(a);
            walls.push(b);
            if let Some(route) = route_around(p0, sa, p3, sb, &walls, gap, &mut used) {
                out.push(route);
                continue;
            }
        }
        let best = simplify(&best);
        used.add(&best);
        out.push(best);
    }
    out
}

/// Where the `k`th of `n` ends sharing a side sits along it (0..1).
pub(crate) fn spread(k: usize, n: usize) -> f32 {
    if n == 1 {
        0.5
    } else {
        0.2 + 0.6 * k as f32 / (n - 1) as f32
    }
}

/// Segments earlier routes took, so later ones prefer a parallel line.
/// Horizontal runs are filed by their y, vertical runs by their x, in
/// buckets as wide as the overlap tolerance.
#[derive(Debug, Clone, Default)]
pub struct Used {
    horizontal: HashMap<i32, Vec<(f32, f32, f32)>>,
    vertical: HashMap<i32, Vec<(f32, f32, f32)>>,
    /// Grid work spent by [`route_around`] so far; past [`ROUTE_BUDGET`]
    /// routes fall back to the simple candidates.
    spent: u64,
}

/// Boxes and connectors a routed grid diagram may hold: obstacle routing
/// and label placement stay near-linear well past this, but a fully
/// interlinked grid does not, so grid families cap their counts here.
pub const MAX_ROUTED: usize = 500;

/// Runs closer than this count as sharing a line.
const OVERLAP_TOLERANCE: f32 = 5.0;

fn bucket(at: f32) -> i32 {
    (at / OVERLAP_TOLERANCE).floor() as i32
}

/// Total search work the obstacle router may spend on one diagram.
const ROUTE_BUDGET: u64 = 4_000_000;

impl Used {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, points: &[[f32; 2]]) {
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (a[1] - b[1]).abs() < 0.01 {
                self.horizontal.entry(bucket(a[1])).or_default().push((
                    a[0].min(b[0]),
                    a[0].max(b[0]),
                    a[1],
                ));
            } else if (a[0] - b[0]).abs() < 0.01 {
                self.vertical.entry(bucket(a[0])).or_default().push((
                    a[1].min(b[1]),
                    a[1].max(b[1]),
                    a[0],
                ));
            }
        }
    }

    /// How far `a`–`b` runs along a segment already taken.
    fn overlap(&self, a: [f32; 2], b: [f32; 2]) -> f32 {
        let (map, at, lo, hi) = if (a[1] - b[1]).abs() < 0.01 {
            (&self.horizontal, a[1], a[0].min(b[0]), a[0].max(b[0]))
        } else if (a[0] - b[0]).abs() < 0.01 {
            (&self.vertical, a[0], a[1].min(b[1]), a[1].max(b[1]))
        } else {
            return 0.0;
        };
        let mut total = 0.0;
        let k = bucket(at);
        for key in k - 1..=k + 1 {
            for &(c0, c1, c) in map.get(&key).into_iter().flatten() {
                if (at - c).abs() < OVERLAP_TOLERANCE {
                    total += (hi.min(c1) - lo.max(c0)).max(0.0);
                }
            }
        }
        total
    }
}

/// An orthogonal route from `p0` (leaving side `sa`) to `p3` (entering
/// through side `sb`) that keeps out of every wall: a shortest path over
/// the sparse grid of wall edges and channel middles, with a penalty per
/// bend and for lines an earlier route already took. `None` when the
/// grid is too large or no path exists.
pub fn route_around(
    p0: [f32; 2],
    sa: Side,
    p3: [f32; 2],
    sb: Side,
    walls: &[Rect],
    gap: f32,
    used: &mut Used,
) -> Option<Vec<[f32; 2]>> {
    if used.spent >= ROUTE_BUDGET {
        return None;
    }
    let margin = (gap * 0.3).clamp(2.0, 6.0);
    let inflated: Vec<Rect> = walls.iter().map(|w| w.inset(-margin, -margin)).collect();
    let na = sa.normal();
    let nb = sb.normal();
    let s = [p0[0] + na[0] * margin, p0[1] + na[1] * margin];
    let t = [p3[0] + nb[0] * margin, p3[1] + nb[1] * margin];
    let mut xs = vec![s[0], t[0]];
    let mut ys = vec![s[1], t[1]];
    for r in &inflated {
        xs.extend([r.x, r.right()]);
        ys.extend([r.y, r.bottom()]);
    }
    let lines = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 0.25);
        let mids: Vec<f32> = v.windows(2).map(|w| (w[0] + w[1]) / 2.0).collect();
        v.extend(mids);
        v.sort_by(f32::total_cmp);
        v.dedup_by(|a, b| (*a - *b).abs() < 0.25);
    };
    lines(&mut xs);
    lines(&mut ys);
    let (nx, ny) = (xs.len(), ys.len());
    let work = (nx * ny) as u64 * (inflated.len() as u64 + 4) * 4;
    if nx * ny > 40_000 || used.spent + work > ROUTE_BUDGET {
        return None;
    }
    used.spent += work;
    let inside = |p: [f32; 2]| {
        inflated.iter().any(|r| {
            p[0] > r.x + 0.01
                && p[0] < r.right() - 0.01
                && p[1] > r.y + 0.01
                && p[1] < r.bottom() - 0.01
        })
    };
    let find = |v: &[f32], x: f32| v.iter().position(|&c| (c - x).abs() < 0.3);
    let (si, sj) = (find(&xs, s[0])?, find(&ys, s[1])?);
    let (ti, tj) = (find(&xs, t[0])?, find(&ys, t[1])?);
    let id = |i: usize, j: usize| j * nx + i;
    // Direction indices: 0 right, 1 left, 2 down, 3 up.
    let dir_of = |side: [f32; 2]| match (side[0] as i32, side[1] as i32) {
        (1, _) => 0usize,
        (-1, _) => 1,
        (_, 1) => 2,
        _ => 3,
    };
    let start_dir = dir_of(na);
    let end_dir = dir_of([-nb[0], -nb[1]]);
    let bend = gap * 1.5;
    let mut dist = vec![f32::INFINITY; nx * ny * 4];
    let mut prev = vec![usize::MAX; nx * ny * 4];
    let mut heap = std::collections::BinaryHeap::new();
    let start = id(si, sj) * 4 + start_dir;
    dist[start] = 0.0;
    heap.push(State {
        cost: 0.0,
        at: start,
    });
    let mut goal = None;
    let mut pops = 0usize;
    while let Some(State { cost, at }) = heap.pop() {
        if cost > dist[at] {
            continue;
        }
        pops += 1;
        if pops > 400_000 {
            return None;
        }
        let node = at / 4;
        let d = at % 4;
        let (i, j) = (node % nx, node / nx);
        if (i, j) == (ti, tj) {
            goal = Some(at);
            break;
        }
        for nd in 0..4 {
            let (ni, nj) = match nd {
                0 if i + 1 < nx => (i + 1, j),
                1 if i > 0 => (i - 1, j),
                2 if j + 1 < ny => (i, j + 1),
                3 if j > 0 => (i, j - 1),
                _ => continue,
            };
            let a = [xs[i], ys[j]];
            let b = [xs[ni], ys[nj]];
            if inside(b) || inside([(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0]) {
                continue;
            }
            let mut step = (a[0] - b[0]).abs() + (a[1] - b[1]).abs();
            if nd != d {
                step += bend;
            }
            step += used.overlap(a, b) * 2.0;
            if (ni, nj) == (ti, tj) && nd != end_dir {
                step += bend;
            }
            let next = id(ni, nj) * 4 + nd;
            if cost + step < dist[next] {
                dist[next] = cost + step;
                prev[next] = at;
                heap.push(State {
                    cost: cost + step,
                    at: next,
                });
            }
        }
    }
    let mut at = goal?;
    let mut nodes = vec![];
    loop {
        let node = at / 4;
        nodes.push([xs[node % nx], ys[node / nx]]);
        if prev[at] == usize::MAX {
            break;
        }
        at = prev[at];
    }
    nodes.reverse();
    let mut points = vec![p0];
    points.extend(nodes);
    points.push(p3);
    let points = simplify(&points);
    used.add(&points);
    Some(points)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct State {
    cost: f32,
    at: usize,
}

impl Eq for State {}

impl Ord for State {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        other
            .cost
            .total_cmp(&self.cost)
            .then_with(|| other.at.cmp(&self.at))
    }
}

impl PartialOrd for State {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

fn same(a: &Rect, b: &Rect) -> bool {
    (a.x - b.x).abs() < 0.01
        && (a.y - b.y).abs() < 0.01
        && (a.w - b.w).abs() < 0.01
        && (a.h - b.h).abs() < 0.01
}

/// The sides of `a` and `b` that face each other, or `None` when the boxes
/// overlap.
pub fn facing(a: Rect, b: Rect) -> Option<(Side, Side)> {
    let hgap = (b.x - a.right()).max(a.x - b.right());
    let vgap = (b.y - a.bottom()).max(a.y - b.bottom());
    if hgap <= 0.0 && vgap <= 0.0 {
        return None;
    }
    let (ca, cb) = (a.center(), b.center());
    Some(if hgap >= vgap {
        if cb[0] >= ca[0] {
            (Side::Right, Side::Left)
        } else {
            (Side::Left, Side::Right)
        }
    } else if cb[1] >= ca[1] {
        (Side::Bottom, Side::Top)
    } else {
        (Side::Top, Side::Bottom)
    })
}

/// An orthogonal route from a named side of `a` to a named side of `b`:
/// each end leaves along its side's normal, with at most two bends in
/// between.
pub fn port_route(a: Rect, sa: Side, b: Rect, sb: Side, stub: f32) -> Vec<[f32; 2]> {
    let p0 = sa.point(a, 0.5);
    let p3 = sb.point(b, 0.5);
    let na = sa.normal();
    let nb = sb.normal();
    let s0 = [p0[0] + na[0] * stub, p0[1] + na[1] * stub];
    let s3 = [p3[0] + nb[0] * stub, p3[1] + nb[1] * stub];
    // `k` is the axis the source leaves along, `j` the other one.
    let k = usize::from(!sa.horizontal());
    let j = 1 - k;
    let with = |mut p: [f32; 2], axis: usize, v: f32| {
        p[axis] = v;
        p
    };
    let mut pts = vec![p0];
    if sa.horizontal() == sb.horizontal() {
        // Straight when the ends already line up and face each other.
        let facing = (p3[k] - p0[k]) * na[k] > 0.0 && (p0[k] - p3[k]) * nb[k] > 0.0;
        if facing && (p0[j] - p3[j]).abs() < 0.5 {
            pts.push(p3);
            return pts;
        }
        let mid = if facing {
            (p0[k] + p3[k]) / 2.0
        } else if na[k] > 0.0 {
            s0[k].max(s3[k])
        } else {
            s0[k].min(s3[k])
        };
        pts.extend([with(p0, k, mid), with(p3, k, mid)]);
    } else if (p3[k] - p0[k]) * na[k] >= 0.0 && (p0[j] - p3[j]) * nb[j] >= 0.0 {
        // Out along `k`, then across into the target.
        pts.push(with(p0, k, p3[k]));
    } else {
        pts.extend([s0, with(s0, j, s3[j]), s3]);
    }
    pts.push(p3);
    simplify(&pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanning_cells_spread_their_shortfall() {
        let spec = GridSpec {
            items: vec![
                GridItem::new(0, 0, [40.0, 20.0]),
                GridItem::new(1, 0, [60.0, 20.0]),
                GridItem::new(0, 1, [200.0, 30.0]).span(2, 1),
            ],
            col_gap: 10.0,
            row_gap: 5.0,
            min_size: [0.0, 0.0],
        };
        let g = solve(&spec);
        assert_eq!(g.cols.len(), 2);
        let total = g.cols[1].0 + g.cols[1].1;
        assert!((total - 200.0).abs() < 0.01);
        assert!((g.cells[2].w - 200.0).abs() < 0.01);
        assert!((g.rows[1].0 - 25.0).abs() < 0.01);
    }

    #[test]
    fn min_size_grows_tracks_evenly() {
        let spec = GridSpec {
            items: vec![
                GridItem::new(0, 0, [10.0, 10.0]),
                GridItem::new(1, 0, [10.0, 10.0]),
            ],
            col_gap: 0.0,
            row_gap: 0.0,
            min_size: [100.0, 40.0],
        };
        let g = solve(&spec);
        assert_eq!(g.cells[1], Rect::new(50.0, 0.0, 50.0, 40.0));
    }

    #[test]
    fn flow_wraps_and_clamps() {
        assert_eq!(
            flow(&[1, 2, 1, 5], 3),
            vec![(0, 0, 1), (1, 0, 2), (0, 1, 1), (0, 2, 3)]
        );
    }

    #[test]
    fn constraints_place_neighbours_and_avoid_collisions() {
        // b right of a, c right of a (collides, pushed further), d below a.
        let cells = constrained_cells(5, &[(0, 1, 1, 0), (0, 2, 1, 0), (0, 3, 0, 1)]);
        assert_eq!(cells[0], (0, 0));
        assert_eq!(cells[1], (1, 0));
        assert_eq!(cells[2], (2, 0));
        assert_eq!(cells[3], (0, 1));
        // The lone item sits to the right of the first component.
        assert_eq!(cells[4], (3, 0));
    }

    #[test]
    fn connectors_face_each_other_and_run_straight_when_aligned() {
        let boxes = [
            Rect::new(0.0, 0.0, 50.0, 30.0),
            Rect::new(100.0, 0.0, 50.0, 30.0),
        ];
        let routes = connect_all(&boxes, &[Connect { from: 0, to: 1 }], &boxes, 20.0);
        assert_eq!(routes[0], vec![[50.0, 15.0], [100.0, 15.0]]);
    }

    #[test]
    fn port_routes_bend_once_for_mixed_sides() {
        let a = Rect::new(0.0, 0.0, 40.0, 40.0);
        let b = Rect::new(100.0, 100.0, 40.0, 40.0);
        let r = port_route(a, Side::Right, b, Side::Top, 10.0);
        assert_eq!(r, vec![[40.0, 20.0], [120.0, 20.0], [120.0, 100.0]]);
    }
}
