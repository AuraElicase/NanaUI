//! Cycle breaking and rank assignment.
//!
//! Ranks come from the network simplex method of Gansner, Koutsofios, North
//! and Vo ("A technique for drawing directed graphs", 1993): start from a
//! longest-path ranking, grow a tight spanning tree and pivot on negative
//! cut values until none is left, which minimises the weighted total edge
//! length. A pivot cap bounds the worst case; past it the current feasible
//! ranking is kept.

/// Which edges a depth-first search over `edges` (in input order) meets as
/// back edges. Reversing them leaves the graph acyclic.
pub(crate) fn back_edges(n: usize, edges: &[(usize, usize)]) -> Vec<bool> {
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, &(u, v)) in edges.iter().enumerate() {
        if u != v {
            out_adj[u].push(i);
        }
    }
    let mut state = vec![0u8; n]; // 0 new, 1 on stack, 2 done
    let mut reversed = vec![false; edges.len()];
    for root in 0..n {
        if state[root] != 0 {
            continue;
        }
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        state[root] = 1;
        while let Some(&mut (v, ref mut next)) = stack.last_mut() {
            if *next < out_adj[v].len() {
                let e = out_adj[v][*next];
                *next += 1;
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
    reversed
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct RankEdge {
    pub u: usize,
    pub v: usize,
    pub minlen: i32,
    pub weight: i32,
}

/// Ranks for an acyclic graph: every edge `u→v` gets
/// `rank[v] - rank[u] >= minlen`, each connected component starting at 0.
pub(crate) fn rank(n: usize, edges: &[RankEdge], pivot_budget: usize) -> Vec<i32> {
    if n == 0 {
        return Vec::new();
    }
    // Merge parallel edges: the longest minimum length, the summed weight.
    let mut merged: Vec<RankEdge> = Vec::new();
    {
        let mut index: std::collections::HashMap<(usize, usize), usize> =
            std::collections::HashMap::new();
        for e in edges {
            if e.u == e.v {
                continue;
            }
            match index.get(&(e.u, e.v)) {
                Some(&i) => {
                    merged[i].minlen = merged[i].minlen.max(e.minlen);
                    merged[i].weight += e.weight;
                }
                None => {
                    index.insert((e.u, e.v), merged.len());
                    merged.push(*e);
                }
            }
        }
    }
    let mut ranks = longest_path(n, &merged);
    let mut ns = Simplex::new(n, &merged);
    for component in components(n, &merged) {
        ns.solve(&component, &mut ranks, pivot_budget);
        let min = component.iter().map(|&v| ranks[v]).min().unwrap_or(0);
        for &v in &component {
            ranks[v] -= min;
        }
    }
    ranks
}

fn longest_path(n: usize, edges: &[RankEdge]) -> Vec<i32> {
    let mut out_adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for (i, e) in edges.iter().enumerate() {
        out_adj[e.u].push(i);
        indeg[e.v] += 1;
    }
    let mut order = Vec::with_capacity(n);
    let mut queue: std::collections::VecDeque<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    while let Some(v) = queue.pop_front() {
        order.push(v);
        for &e in &out_adj[v] {
            let w = edges[e].v;
            indeg[w] -= 1;
            if indeg[w] == 0 {
                queue.push_back(w);
            }
        }
    }
    // Any node a cycle left out (the caller should have broken cycles)
    // keeps rank 0.
    let mut rank = vec![0i32; n];
    for &v in order.iter().rev() {
        if let Some(r) = out_adj[v]
            .iter()
            .map(|&e| rank[edges[e].v] - edges[e].minlen)
            .min()
        {
            rank[v] = r;
        }
    }
    rank
}

fn components(n: usize, edges: &[RankEdge]) -> Vec<Vec<usize>> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    for e in edges {
        adj[e.u].push(e.v);
        adj[e.v].push(e.u);
    }
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for s in 0..n {
        if seen[s] {
            continue;
        }
        let mut comp = vec![s];
        seen[s] = true;
        let mut i = 0;
        while i < comp.len() {
            let v = comp[i];
            i += 1;
            for &w in &adj[v] {
                if !seen[w] {
                    seen[w] = true;
                    comp.push(w);
                }
            }
        }
        comp.sort_unstable();
        out.push(comp);
    }
    out
}

struct Simplex<'a> {
    edges: &'a [RankEdge],
    incident: Vec<Vec<usize>>,
    in_tree: Vec<bool>,
    tree_adj: Vec<Vec<usize>>,
    cut: Vec<i32>,
    low: Vec<u32>,
    lim: Vec<u32>,
    parent_edge: Vec<Option<usize>>,
    member: Vec<bool>,
}

impl<'a> Simplex<'a> {
    fn new(n: usize, edges: &'a [RankEdge]) -> Self {
        let mut incident: Vec<Vec<usize>> = vec![Vec::new(); n];
        for (i, e) in edges.iter().enumerate() {
            incident[e.u].push(i);
            incident[e.v].push(i);
        }
        Self {
            edges,
            incident,
            in_tree: vec![false; edges.len()],
            tree_adj: vec![Vec::new(); n],
            cut: vec![0; edges.len()],
            low: vec![0; n],
            lim: vec![0; n],
            parent_edge: vec![None; n],
            member: vec![false; n],
        }
    }

    fn slack(&self, e: usize, rank: &[i32]) -> i32 {
        let e = &self.edges[e];
        rank[e.v] - rank[e.u] - e.minlen
    }

    fn solve(&mut self, comp: &[usize], rank: &mut [i32], pivot_budget: usize) {
        if comp.len() < 2 {
            return;
        }
        let comp_edges: Vec<usize> = {
            let mut v: Vec<usize> = comp
                .iter()
                .flat_map(|&n| self.incident[n].iter().copied())
                .collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        self.feasible_tree(comp, &comp_edges, rank);
        let root = comp[0];
        self.low_lim(root);
        self.cut_values(root);
        let cap = pivot_budget.min(4 * comp_edges.len() + 16);
        for _ in 0..cap {
            let Some(leave) = comp_edges
                .iter()
                .copied()
                .find(|&e| self.in_tree[e] && self.cut[e] < 0)
            else {
                break;
            };
            let Some(enter) = self.enter_edge(leave, &comp_edges, rank) else {
                break;
            };
            self.exchange(leave, enter, root, rank);
        }
    }

    fn feasible_tree(&mut self, comp: &[usize], comp_edges: &[usize], rank: &mut [i32]) {
        for &v in comp {
            self.member[v] = false;
            self.tree_adj[v].clear();
        }
        for &e in comp_edges {
            self.in_tree[e] = false;
        }
        let mut members = vec![comp[0]];
        self.member[comp[0]] = true;
        loop {
            // Grow the tight tree from every member.
            let mut i = 0;
            while i < members.len() {
                let v = members[i];
                i += 1;
                for k in 0..self.incident[v].len() {
                    let e = self.incident[v][k];
                    let ed = self.edges[e];
                    let w = if ed.u == v { ed.v } else { ed.u };
                    if !self.member[w] && self.slack(e, rank) == 0 {
                        self.member[w] = true;
                        self.in_tree[e] = true;
                        self.tree_adj[v].push(e);
                        self.tree_adj[w].push(e);
                        members.push(w);
                    }
                }
            }
            if members.len() >= comp.len() {
                return;
            }
            let mut best: Option<(i32, usize)> = None;
            for &e in comp_edges {
                let ed = self.edges[e];
                if self.member[ed.u] != self.member[ed.v] {
                    let s = self.slack(e, rank);
                    if best.is_none_or(|(b, _)| s < b) {
                        best = Some((s, e));
                    }
                }
            }
            let Some((s, e)) = best else { return };
            let delta = if self.member[self.edges[e].u] { s } else { -s };
            for &v in &members {
                rank[v] += delta;
            }
        }
    }

    fn low_lim(&mut self, root: usize) {
        // Iterative post-order over the tree.
        let mut next_lim = 1u32;
        self.parent_edge[root] = None;
        let mut stack: Vec<(usize, usize, u32)> = vec![(root, 0, next_lim)];
        while let Some(&mut (v, ref mut k, low)) = stack.last_mut() {
            if *k < self.tree_adj[v].len() {
                let e = self.tree_adj[v][*k];
                *k += 1;
                if Some(e) == self.parent_edge[v] {
                    continue;
                }
                let ed = self.edges[e];
                let w = if ed.u == v { ed.v } else { ed.u };
                self.parent_edge[w] = Some(e);
                stack.push((w, 0, next_lim));
            } else {
                self.low[v] = low;
                self.lim[v] = next_lim;
                next_lim += 1;
                stack.pop();
            }
        }
    }

    fn postorder(&self, root: usize) -> Vec<usize> {
        let mut order = Vec::new();
        let mut stack: Vec<(usize, usize)> = vec![(root, 0)];
        while let Some(&mut (v, ref mut k)) = stack.last_mut() {
            if *k < self.tree_adj[v].len() {
                let e = self.tree_adj[v][*k];
                *k += 1;
                if Some(e) == self.parent_edge[v] {
                    continue;
                }
                let ed = self.edges[e];
                stack.push((if ed.u == v { ed.v } else { ed.u }, 0));
            } else {
                order.push(v);
                stack.pop();
            }
        }
        order
    }

    fn cut_values(&mut self, root: usize) {
        for v in self.postorder(root) {
            if v == root {
                continue;
            }
            let Some(pe) = self.parent_edge[v] else {
                continue;
            };
            let value = self.cut_value(v, pe);
            self.cut[pe] = value;
        }
    }

    fn cut_value(&self, child: usize, pe: usize) -> i32 {
        let ped = self.edges[pe];
        let parent = if ped.u == child { ped.v } else { ped.u };
        let child_is_tail = ped.u == child;
        let mut value = ped.weight;
        for &e in &self.incident[child] {
            if e == pe {
                continue;
            }
            let ed = self.edges[e];
            let is_out = ed.u == child;
            let other = if is_out { ed.v } else { ed.u };
            if other == parent {
                continue;
            }
            let points_to_head = is_out == child_is_tail;
            value += if points_to_head {
                ed.weight
            } else {
                -ed.weight
            };
            if self.in_tree[e] {
                value += if points_to_head {
                    -self.cut[e]
                } else {
                    self.cut[e]
                };
            }
        }
        value
    }

    fn descends(&self, v: usize, root: usize) -> bool {
        self.low[root] <= self.lim[v] && self.lim[v] <= self.lim[root]
    }

    fn enter_edge(&self, leave: usize, comp_edges: &[usize], rank: &[i32]) -> Option<usize> {
        let ed = self.edges[leave];
        let (v, w) = (ed.u, ed.v);
        let (tail, flip) = if self.lim[v] > self.lim[w] {
            (w, true)
        } else {
            (v, false)
        };
        let mut best: Option<(i32, usize)> = None;
        for &e in comp_edges {
            let f = self.edges[e];
            if flip == self.descends(f.u, tail) && flip != self.descends(f.v, tail) {
                let s = self.slack(e, rank);
                if best.is_none_or(|(b, _)| s < b) {
                    best = Some((s, e));
                }
            }
        }
        best.map(|(_, e)| e)
    }

    fn exchange(&mut self, leave: usize, enter: usize, root: usize, rank: &mut [i32]) {
        let l = self.edges[leave];
        self.in_tree[leave] = false;
        self.tree_adj[l.u].retain(|&e| e != leave);
        self.tree_adj[l.v].retain(|&e| e != leave);
        let f = self.edges[enter];
        self.in_tree[enter] = true;
        self.tree_adj[f.u].push(enter);
        self.tree_adj[f.v].push(enter);
        self.low_lim(root);
        self.cut_values(root);
        // Re-derive ranks down the tree from the root.
        let order = {
            let mut pre = Vec::new();
            let mut stack = vec![root];
            while let Some(v) = stack.pop() {
                pre.push(v);
                for &e in &self.tree_adj[v] {
                    if Some(e) == self.parent_edge[v] {
                        continue;
                    }
                    let ed = self.edges[e];
                    stack.push(if ed.u == v { ed.v } else { ed.u });
                }
            }
            pre
        };
        for v in order.into_iter().skip(1) {
            let Some(pe) = self.parent_edge[v] else {
                continue;
            };
            let ed = self.edges[pe];
            let parent = if ed.u == v { ed.v } else { ed.u };
            rank[v] = if ed.u == v {
                rank[parent] - ed.minlen
            } else {
                rank[parent] + ed.minlen
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn e(u: usize, v: usize) -> RankEdge {
        RankEdge {
            u,
            v,
            minlen: 1,
            weight: 1,
        }
    }

    fn feasible(ranks: &[i32], edges: &[RankEdge]) -> bool {
        edges.iter().all(|e| ranks[e.v] - ranks[e.u] >= e.minlen)
    }

    #[test]
    fn back_edges_break_every_cycle() {
        let edges = [(0, 1), (1, 2), (2, 0), (2, 3), (3, 1)];
        let rev = back_edges(4, &edges);
        let dag: Vec<(usize, usize)> = edges
            .iter()
            .zip(&rev)
            .map(|(&(u, v), &r)| if r { (v, u) } else { (u, v) })
            .collect();
        let ranks = rank(
            4,
            &dag.iter().map(|&(u, v)| e(u, v)).collect::<Vec<_>>(),
            1000,
        );
        assert!(dag.iter().all(|&(u, v)| ranks[v] > ranks[u]));
    }

    #[test]
    fn simplex_pulls_sources_toward_their_targets() {
        // a→b→c→d and x→d: longest path puts x at rank 0, simplex at rank 2.
        let edges = [e(0, 1), e(1, 2), e(2, 3), e(4, 3)];
        let ranks = rank(5, &edges, 1000);
        assert!(feasible(&ranks, &edges));
        assert_eq!(ranks, vec![0, 1, 2, 3, 2]);
    }

    #[test]
    fn components_each_start_at_zero_and_minlen_holds() {
        let mut edges = vec![e(0, 1), e(2, 3)];
        edges[1].minlen = 3;
        let ranks = rank(4, &edges, 1000);
        assert_eq!(ranks, vec![0, 1, 0, 3]);
    }

    #[test]
    fn diamond_graph_is_tight() {
        let edges = [e(0, 1), e(0, 2), e(1, 3), e(2, 3), e(0, 3)];
        let ranks = rank(4, &edges, 1000);
        assert!(feasible(&ranks, &edges));
        assert_eq!(ranks[3], 2);
    }
}
