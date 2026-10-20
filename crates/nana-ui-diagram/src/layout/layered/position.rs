//! Horizontal coordinates: Brandes and Köpf, "Fast and Simple Horizontal
//! Coordinate Assignment" (2001). Four alignments (up/down × left/right)
//! each pull nodes into vertical blocks with their median neighbours and
//! compact the blocks; the result is the average of the two median
//! candidates per node, so long edges run straight and nodes centre over
//! their neighbours.

use std::collections::HashSet;

pub(crate) struct Separation<'a> {
    pub width: &'a [f32],
    pub dummy: &'a [bool],
    pub node_sep: f32,
    pub edge_sep: f32,
}

impl Separation<'_> {
    fn between(&self, a: usize, b: usize) -> f32 {
        let gap = |v: usize| {
            if self.dummy[v] {
                self.edge_sep
            } else {
                self.node_sep
            }
        };
        self.width[a] / 2.0 + gap(a) / 2.0 + gap(b) / 2.0 + self.width[b] / 2.0
    }
}

pub(crate) fn position_x(
    layers: &[Vec<usize>],
    up: &[Vec<usize>],
    down: &[Vec<usize>],
    sep: &Separation<'_>,
) -> Vec<f32> {
    let n = up.len();
    if n == 0 {
        return Vec::new();
    }
    let conflicts = type1_conflicts(layers, up, sep.dummy);
    let mut results: Vec<(Vec<f32>, bool)> = Vec::with_capacity(4);
    for vertical_up in [true, false] {
        let base: Vec<Vec<usize>> = if vertical_up {
            layers.to_vec()
        } else {
            layers.iter().rev().cloned().collect()
        };
        for right in [false, true] {
            let layering: Vec<Vec<usize>> = if right {
                base.iter()
                    .map(|l| l.iter().rev().copied().collect())
                    .collect()
            } else {
                base.clone()
            };
            let neighbours = if vertical_up { up } else { down };
            let (root, align) = vertical_alignment(n, &layering, &conflicts, neighbours);
            let mut xs = compaction(n, &layering, &root, &align, sep);
            if right {
                for x in &mut xs {
                    *x = -*x;
                }
            }
            results.push((xs, right));
        }
    }
    // Align every candidate to the narrowest one.
    let width_of = |xs: &[f32]| {
        let mut min = f32::INFINITY;
        let mut max = f32::NEG_INFINITY;
        for (x, w) in xs.iter().zip(sep.width) {
            min = min.min(x - w / 2.0);
            max = max.max(x + w / 2.0);
        }
        max - min
    };
    let narrowest = (0..results.len())
        .min_by(|&a, &b| width_of(&results[a].0).total_cmp(&width_of(&results[b].0)))
        .unwrap_or(0);
    let (to_min, to_max) = extent(&results[narrowest].0);
    for (i, (xs, right)) in results.iter_mut().enumerate() {
        if i == narrowest {
            continue;
        }
        let (min, max) = extent(xs);
        let delta = if *right { to_max - max } else { to_min - min };
        for x in xs.iter_mut() {
            *x += delta;
        }
    }
    (0..n)
        .map(|v| {
            let mut c = [
                results[0].0[v],
                results[1].0[v],
                results[2].0[v],
                results[3].0[v],
            ];
            c.sort_by(f32::total_cmp);
            (c[1] + c[2]) / 2.0
        })
        .collect()
}

fn extent(xs: &[f32]) -> (f32, f32) {
    xs.iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), &x| {
            (a.min(x), b.max(x))
        })
}

fn key(a: usize, b: usize) -> (usize, usize) {
    if a < b { (a, b) } else { (b, a) }
}

/// Non-inner segments that cross an inner segment (one between two dummy
/// nodes) are marked so alignment prefers keeping long edges straight.
fn type1_conflicts(
    layers: &[Vec<usize>],
    up: &[Vec<usize>],
    dummy: &[bool],
) -> HashSet<(usize, usize)> {
    let mut pos = vec![0usize; up.len()];
    for layer in layers {
        for (i, &v) in layer.iter().enumerate() {
            pos[v] = i;
        }
    }
    let mut conflicts = HashSet::new();
    for r in 1..layers.len() {
        let prev_len = layers[r - 1].len();
        let layer = &layers[r];
        let mut k0 = 0usize;
        let mut scan = 0usize;
        for (i, &v) in layer.iter().enumerate() {
            let inner = if dummy[v] {
                up[v].iter().copied().find(|&u| dummy[u])
            } else {
                None
            };
            let k1 = inner.map_or(prev_len, |w| pos[w]);
            if inner.is_some() || i + 1 == layer.len() {
                for &s in &layer[scan..=i] {
                    for &u in &up[s] {
                        let p = pos[u];
                        if (p < k0 || k1 < p) && !(dummy[u] && dummy[s]) {
                            conflicts.insert(key(u, s));
                        }
                    }
                }
                scan = i + 1;
                k0 = k1;
            }
        }
    }
    conflicts
}

fn vertical_alignment(
    n: usize,
    layering: &[Vec<usize>],
    conflicts: &HashSet<(usize, usize)>,
    neighbours: &[Vec<usize>],
) -> (Vec<usize>, Vec<usize>) {
    let mut root: Vec<usize> = (0..n).collect();
    let mut align: Vec<usize> = (0..n).collect();
    let mut pos = vec![0usize; n];
    for layer in layering {
        for (i, &v) in layer.iter().enumerate() {
            pos[v] = i;
        }
    }
    for layer in layering {
        let mut prev: isize = -1;
        for &v in layer {
            let mut ws: Vec<usize> = neighbours[v].clone();
            if ws.is_empty() {
                continue;
            }
            ws.sort_by_key(|&w| pos[w]);
            let mp = (ws.len() as f32 - 1.0) / 2.0;
            for &w in &ws[mp.floor() as usize..=mp.ceil() as usize] {
                if align[v] == v && prev < pos[w] as isize && !conflicts.contains(&key(v, w)) {
                    align[w] = v;
                    root[v] = root[w];
                    align[v] = root[v];
                    prev = pos[w] as isize;
                }
            }
        }
    }
    (root, align)
}

fn compaction(
    n: usize,
    layering: &[Vec<usize>],
    root: &[usize],
    _align: &[usize],
    sep: &Separation<'_>,
) -> Vec<f32> {
    // Block graph: an edge root(u) → root(v) for each left neighbour u of v,
    // weighted by the separation they need.
    let mut out_edges: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    let mut in_edges: Vec<Vec<(usize, f32)>> = vec![Vec::new(); n];
    let mut is_root = vec![false; n];
    for layer in layering {
        for w in layer.windows(2) {
            let (u, v) = (w[0], w[1]);
            let (ru, rv) = (root[u], root[v]);
            let s = sep.between(v, u);
            match out_edges[ru].iter_mut().find(|(t, _)| *t == rv) {
                Some(e) => e.1 = e.1.max(s),
                None => out_edges[ru].push((rv, s)),
            }
        }
        for &v in layer {
            is_root[root[v]] = true;
        }
    }
    for (u, edges) in out_edges.iter().enumerate() {
        for &(v, s) in edges {
            in_edges[v].push((u, s));
        }
    }
    // Topological order of the block graph.
    let mut indeg: Vec<usize> = (0..n).map(|v| in_edges[v].len()).collect();
    let mut queue: Vec<usize> = (0..n).filter(|&v| is_root[v] && indeg[v] == 0).collect();
    let mut topo = Vec::with_capacity(n);
    while let Some(v) = queue.pop() {
        topo.push(v);
        for &(w, _) in &out_edges[v] {
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push(w);
            }
        }
    }
    let mut xs = vec![0.0f32; n];
    for &v in &topo {
        xs[v] = in_edges[v]
            .iter()
            .map(|&(u, s)| xs[u] + s)
            .fold(0.0, f32::max);
    }
    for &v in topo.iter().rev() {
        let min = out_edges[v]
            .iter()
            .map(|&(w, s)| xs[w] - s)
            .fold(f32::INFINITY, f32::min);
        if min.is_finite() {
            xs[v] = xs[v].max(min);
        }
    }
    (0..n).map(|v| xs[root[v]]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_stays_straight_and_siblings_separate() {
        // 0 → 1, 0 → 2 ; 1 → 3
        let layers = vec![vec![0], vec![1, 2], vec![3]];
        let mut up = vec![vec![]; 4];
        let mut down = vec![vec![]; 4];
        for (u, v) in [(0, 1), (0, 2), (1, 3)] {
            down[u].push(v);
            up[v].push(u);
        }
        let width = [40.0; 4];
        let dummy = [false; 4];
        let xs = position_x(
            &layers,
            &up,
            &down,
            &Separation {
                width: &width,
                dummy: &dummy,
                node_sep: 20.0,
                edge_sep: 10.0,
            },
        );
        assert!(xs[2] - xs[1] >= 60.0 - 1e-3);
        assert!((xs[3] - xs[1]).abs() < 1e-3);
        assert!(xs[0] >= xs[1] - 1e-3 && xs[0] <= xs[2] + 1e-3);
    }
}
