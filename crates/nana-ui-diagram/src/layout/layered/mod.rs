//! Layered (Sugiyama-style) layout with nested groups and orthogonal edges.
//!
//! Each group is laid out on its own, in its own direction, and placed in
//! its parent as one box. An edge that crosses a group's border passes
//! through a port on the box's top or bottom side (in the parent's flow), so
//! the inside and outside routes meet exactly on the border. Within a level:
//! cycles are broken, ranks come from network simplex, long edges become
//! chains of dummy nodes (one carries the label), crossings are reduced,
//! Brandes–Köpf places nodes, edge ends spread along node sides, and every
//! horizontal run gets its own track in the gap between two ranks so
//! parallel edges never merge.

mod order;
mod position;
mod rank;

use std::collections::HashMap;

use super::graph::{Direction, End, Graph, GraphLayout, Route, RouteKind, Spacing};
use crate::ir::Rect;
use crate::{DiagramError, LimitKind, Limits};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Side {
    Top,
    Bottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemKind {
    Node(usize),
    Box(usize),
    Port { edge: usize, side: Side },
}

/// Which end of the global edge a level-edge end stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GlobalEnd {
    Source,
    Target,
}

#[derive(Debug, Clone)]
struct LevelEdge {
    global: usize,
    /// Upper item (layout direction runs u → v).
    u: usize,
    v: usize,
    u_end: Option<GlobalEnd>,
    v_end: Option<GlobalEnd>,
    /// Layout direction runs from the global target to the global source.
    flipped: bool,
    label: Option<[f32; 2]>,
    minlen: i32,
    weight: i32,
    port: bool,
}

#[derive(Debug, Clone, Default)]
struct Level {
    cluster: Option<usize>,
    dir: Direction,
    items: Vec<ItemKind>,
    edges: Vec<LevelEdge>,
    self_loops: Vec<(usize, usize)>,
    ranks: Vec<i32>,
    doubled: bool,
}

/// A route on screen and its label's centre.
type Segment = (Vec<[f32; 2]>, Option<[f32; 2]>);

/// One level's geometry, on screen, relative to its box's top-left.
#[derive(Debug, Clone, Default)]
struct LevelGeom {
    size: [f32; 2],
    items: Vec<Rect>,
    /// Port points on the box border, by global edge.
    ports: HashMap<usize, [f32; 2]>,
    /// Polylines oriented from the global source to the global target.
    segments: HashMap<usize, Segment>,
    /// Self-loops, by global edge.
    loops: Vec<(usize, Segment)>,
    title: Rect,
}

struct EdgePlan {
    lca: usize,
    source_path: Vec<usize>,
    target_path: Vec<usize>,
    kind: RouteKind,
}

pub fn layout(
    graph: &Graph,
    spacing: &Spacing,
    limits: &Limits,
) -> Result<GraphLayout, DiagramError> {
    limits.check_count(graph.nodes.len(), limits.nodes, LimitKind::Nodes)?;
    limits.check_count(graph.edges.len(), limits.edges, LimitKind::Edges)?;
    limits.check_count(graph.clusters.len(), limits.clusters, LimitKind::Clusters)?;

    let nc = graph.clusters.len();
    // Validate the cluster tree and find each cluster's ancestor chain.
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(nc);
    for c in 0..nc {
        let mut chain = vec![c];
        let mut p = graph.clusters[c].parent;
        while let Some(q) = p {
            if q >= nc || chain.contains(&q) || chain.len() > limits.cluster_depth {
                return Err(DiagramError::TooLarge(LimitKind::ClusterDepth));
            }
            chain.push(q);
            p = graph.clusters[q].parent;
        }
        chain.reverse();
        chains.push(chain);
    }
    let path_to = |container: Option<usize>| -> Vec<usize> {
        container.map(|c| chains[c].clone()).unwrap_or_default()
    };
    let container = |end: End| -> Option<usize> {
        match end {
            End::Node(n) => graph.nodes[n].cluster.filter(|&c| c < nc),
            End::Cluster(c) => graph.clusters[c].parent,
        }
    };

    // Levels: the root, then every cluster in breadth-first order.
    let mut level_of_cluster = vec![usize::MAX; nc];
    let mut order: Vec<Option<usize>> = vec![None];
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); nc + 1];
    for c in 0..nc {
        let parent = graph.clusters[c].parent.unwrap_or(nc);
        children[parent].push(c);
    }
    let mut i = 0;
    while i < order.len() {
        let key = order[i].unwrap_or(nc);
        for &c in &children[key] {
            level_of_cluster[c] = order.len();
            order.push(Some(c));
        }
        i += 1;
    }
    let level_index = |c: Option<usize>| c.map_or(0, |c| level_of_cluster[c]);

    let mut levels: Vec<Level> = order
        .iter()
        .map(|&cluster| Level {
            cluster,
            dir: graph.direction,
            ..Level::default()
        })
        .collect();
    // Items: member nodes, then member clusters.
    let mut item_of_node = vec![(0usize, 0usize); graph.nodes.len()];
    let mut item_of_cluster = vec![(0usize, 0usize); nc];
    for (n, node) in graph.nodes.iter().enumerate() {
        let l = level_index(node.cluster.filter(|&c| c < nc));
        item_of_node[n] = (l, levels[l].items.len());
        levels[l].items.push(ItemKind::Node(n));
    }
    for (c, cluster) in graph.clusters.iter().enumerate() {
        let l = level_index(cluster.parent);
        item_of_cluster[c] = (l, levels[l].items.len());
        levels[l].items.push(ItemKind::Box(c));
    }

    // Plan every edge: the level it is laid out on and the groups it
    // crosses on each side.
    let mut plans = Vec::with_capacity(graph.edges.len());
    for edge in &graph.edges {
        let (cf, ct) = (container(edge.from), container(edge.to));
        let (pf, pt) = (path_to(cf), path_to(ct));
        let mut kind = RouteKind::Normal;
        if edge.from == edge.to {
            kind = match edge.from {
                End::Node(_) => RouteKind::SelfLoop,
                End::Cluster(_) => RouteKind::Dropped,
            };
        }
        if let End::Cluster(c) = edge.from
            && (pt.contains(&c))
        {
            kind = RouteKind::Dropped;
        }
        if let End::Cluster(c) = edge.to
            && (pf.contains(&c))
        {
            kind = RouteKind::Dropped;
        }
        let common = pf.iter().zip(&pt).take_while(|(a, b)| a == b).count();
        let lca = if common == 0 {
            None
        } else {
            Some(pf[common - 1])
        };
        plans.push(EdgePlan {
            lca: level_index(lca),
            source_path: pf[common..].to_vec(),
            target_path: pt[common..].to_vec(),
            kind,
        });
    }

    let item_at = |end: End, level: usize, path: &[usize]| -> usize {
        // The end itself when it lives on this level, else the first group
        // on its path.
        match path.first() {
            Some(&c) => item_of_cluster[c].1,
            None => match end {
                End::Node(n) => {
                    debug_assert_eq!(item_of_node[n].0, level);
                    item_of_node[n].1
                }
                End::Cluster(c) => item_of_cluster[c].1,
            },
        }
    };

    // Phase A, top-down: edges, ports, cycle breaking and ranks.
    // pending[level] = (global edge, side, which end continues inside).
    let mut pending: Vec<Vec<(usize, Side, GlobalEnd)>> = vec![Vec::new(); levels.len()];
    let mut depth_of_path: HashMap<(usize, GlobalEnd), usize> = HashMap::new();
    for li in 0..levels.len() {
        let mut level = std::mem::take(&mut levels[li]);
        // Direction: a group that edges leave follows its parent.
        if let Some(c) = level.cluster {
            let parent_dir = levels[level_index(graph.clusters[c].parent)].dir;
            level.dir = if pending[li].is_empty() {
                graph.clusters[c].direction.unwrap_or(parent_dir)
            } else {
                parent_dir
            };
        }
        // Edges laid out here.
        for (e, plan) in plans.iter().enumerate() {
            if plan.lca != li {
                continue;
            }
            let edge = &graph.edges[e];
            match plan.kind {
                RouteKind::Dropped => continue,
                RouteKind::SelfLoop => {
                    if let End::Node(n) = edge.from {
                        level.self_loops.push((e, item_of_node[n].1));
                    }
                    continue;
                }
                RouteKind::Normal => {}
            }
            let u = item_at(edge.from, li, &plan.source_path);
            let v = item_at(edge.to, li, &plan.target_path);
            if u == v {
                continue;
            }
            level.edges.push(LevelEdge {
                global: e,
                u,
                v,
                u_end: Some(GlobalEnd::Source),
                v_end: Some(GlobalEnd::Target),
                flipped: false,
                label: edge.label,
                minlen: edge.min_len.max(1) as i32,
                weight: edge.weight.max(1) as i32,
                port: false,
            });
        }
        // Edges arriving through this group's border.
        for &(e, side, end) in &pending[li] {
            let plan = &plans[e];
            let edge = &graph.edges[e];
            let d = *depth_of_path.get(&(e, end)).unwrap_or(&0);
            let (path, global_end) = match end {
                GlobalEnd::Source => (&plan.source_path, edge.from),
                GlobalEnd::Target => (&plan.target_path, edge.to),
            };
            let inner = item_at(global_end, li, &path[d + 1..]);
            let port = level.items.len();
            level.items.push(ItemKind::Port { edge: e, side });
            let (u, v, u_end, v_end) = match side {
                Side::Top => (port, inner, None, Some(end)),
                Side::Bottom => (inner, port, Some(end), None),
            };
            let flipped = matches!(
                (end, side),
                (GlobalEnd::Source, Side::Top) | (GlobalEnd::Target, Side::Bottom)
            );
            level.edges.push(LevelEdge {
                global: e,
                u,
                v,
                u_end,
                v_end,
                flipped,
                label: None,
                minlen: 1,
                weight: 1,
                port: true,
            });
        }
        // Break cycles among the level's own edges.
        let pairs: Vec<(usize, usize)> = level
            .edges
            .iter()
            .map(|e| if e.port { (e.u, e.u) } else { (e.u, e.v) })
            .collect();
        let reversed = rank::back_edges(level.items.len(), &pairs);
        for (edge, rev) in level.edges.iter_mut().zip(reversed) {
            if rev && !edge.port {
                std::mem::swap(&mut edge.u, &mut edge.v);
                std::mem::swap(&mut edge.u_end, &mut edge.v_end);
                edge.flipped = !edge.flipped;
            }
        }
        // Hand the side each edge enters a member group on down to it.
        for edge in &level.edges {
            for (item, end, side) in [
                (edge.u, edge.u_end, Side::Bottom),
                (edge.v, edge.v_end, Side::Top),
            ] {
                let (ItemKind::Box(c), Some(end)) = (level.items[item], end) else {
                    continue;
                };
                let plan = &plans[edge.global];
                let ge = &graph.edges[edge.global];
                let (path, gend) = match end {
                    GlobalEnd::Source => (&plan.source_path, ge.from),
                    GlobalEnd::Target => (&plan.target_path, ge.to),
                };
                // Does the end continue inside `c`?
                let depth = if edge.port {
                    depth_of_path.get(&(edge.global, end)).map_or(0, |d| d + 1)
                } else {
                    0
                };
                let continues = path.get(depth) == Some(&c) && gend != End::Cluster(c);
                if continues {
                    depth_of_path.insert((edge.global, end), depth);
                    pending[level_of_cluster[c]].push((edge.global, side, end));
                }
            }
        }
        // Ranks.
        level.doubled = level.edges.iter().any(|e| !e.port && e.label.is_some());
        let k = if level.doubled { 2 } else { 1 };
        let rank_edges: Vec<rank::RankEdge> = level
            .edges
            .iter()
            .filter(|e| !e.port)
            .map(|e| rank::RankEdge {
                u: e.u,
                v: e.v,
                minlen: e.minlen * k,
                weight: e.weight,
            })
            .collect();
        let pivots = (limits.order_work / 4).max(64) as usize;
        let mut ranks = rank::rank(level.items.len(), &rank_edges, pivots);
        let real: Vec<usize> = (0..level.items.len())
            .filter(|&i| !matches!(level.items[i], ItemKind::Port { .. }))
            .collect();
        let max = real.iter().map(|&i| ranks[i]).max().unwrap_or(0);
        let has_top = level.items.iter().any(|k| {
            matches!(
                k,
                ItemKind::Port {
                    side: Side::Top,
                    ..
                }
            )
        });
        let shift = i32::from(has_top);
        for &i in &real {
            ranks[i] += shift;
        }
        for (i, kind) in level.items.iter().enumerate() {
            if let ItemKind::Port { side, .. } = kind {
                ranks[i] = match side {
                    Side::Top => 0,
                    Side::Bottom => max + shift + 1,
                };
            }
        }
        level.ranks = ranks;
        levels[li] = level;
    }

    // Phase B, bottom-up: geometry.
    let mut geoms: Vec<LevelGeom> = vec![LevelGeom::default(); levels.len()];
    for li in (0..levels.len()).rev() {
        let geom = level_geometry(
            &levels[li],
            graph,
            spacing,
            limits,
            &geoms,
            &level_of_cluster,
        )?;
        geoms[li] = geom;
    }

    // Assemble absolute coordinates top-down.
    let mut offsets = vec![[0.0f32; 2]; levels.len()];
    let mut nodes = vec![Rect::default(); graph.nodes.len()];
    let mut clusters = vec![Rect::default(); nc];
    let mut titles = vec![Rect::default(); nc];
    let mut cluster_directions = vec![graph.direction; nc];
    for li in 0..levels.len() {
        let [ox, oy] = offsets[li];
        for (i, kind) in levels[li].items.iter().enumerate() {
            let r = geoms[li].items[i].translated(ox, oy);
            match *kind {
                ItemKind::Node(n) => nodes[n] = r,
                ItemKind::Box(c) => {
                    clusters[c] = r;
                    let cl = level_of_cluster[c];
                    offsets[cl] = [r.x, r.y];
                    titles[c] = geoms[cl].title.translated(r.x, r.y);
                    cluster_directions[c] = levels[cl].dir;
                }
                ItemKind::Port { .. } => {}
            }
        }
    }
    let shift = |pts: &[[f32; 2]], o: [f32; 2]| -> Vec<[f32; 2]> {
        pts.iter().map(|p| [p[0] + o[0], p[1] + o[1]]).collect()
    };
    let mut edges = Vec::with_capacity(graph.edges.len());
    for (e, plan) in plans.iter().enumerate() {
        match plan.kind {
            RouteKind::Dropped => {
                edges.push(Route {
                    points: Vec::new(),
                    label: None,
                    kind: RouteKind::Dropped,
                });
                continue;
            }
            RouteKind::SelfLoop => {
                let found = geoms[plan.lca].loops.iter().find(|(g, _)| *g == e);
                let o = offsets[plan.lca];
                edges.push(match found {
                    Some((_, (pts, label))) => Route {
                        points: shift(pts, o),
                        label: label.map(|l| [l[0] + o[0], l[1] + o[1]]),
                        kind: RouteKind::SelfLoop,
                    },
                    None => Route {
                        points: Vec::new(),
                        label: None,
                        kind: RouteKind::Dropped,
                    },
                });
                continue;
            }
            RouteKind::Normal => {}
        }
        let mut points: Vec<[f32; 2]> = Vec::new();
        let mut label = None;
        let mut append = |pts: Vec<[f32; 2]>| {
            for p in pts {
                if points
                    .last()
                    .is_none_or(|q| (q[0] - p[0]).abs() > 0.01 || (q[1] - p[1]).abs() > 0.01)
                {
                    points.push(p);
                }
            }
        };
        // Source side, innermost group first.
        for &c in plan.source_path.iter().rev() {
            let l = level_of_cluster[c];
            if let Some((pts, _)) = geoms[l].segments.get(&e) {
                append(shift(pts, offsets[l]));
            }
        }
        if let Some((pts, lab)) = geoms[plan.lca].segments.get(&e) {
            append(shift(pts, offsets[plan.lca]));
            label = lab.map(|l| [l[0] + offsets[plan.lca][0], l[1] + offsets[plan.lca][1]]);
        }
        for &c in &plan.target_path {
            let l = level_of_cluster[c];
            if let Some((pts, _)) = geoms[l].segments.get(&e) {
                append(shift(pts, offsets[l]));
            }
        }
        edges.push(Route {
            points,
            label,
            kind: RouteKind::Normal,
        });
    }
    let bounds = Rect::new(0.0, 0.0, geoms[0].size[0], geoms[0].size[1]);
    Ok(GraphLayout {
        nodes,
        clusters,
        titles,
        cluster_directions,
        edges,
        bounds,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum VKind {
    Item(usize),
    Dummy,
    Label,
}

fn level_geometry(
    level: &Level,
    graph: &Graph,
    sp: &Spacing,
    limits: &Limits,
    geoms: &[LevelGeom],
    level_of_cluster: &[usize],
) -> Result<LevelGeom, DiagramError> {
    let dir = level.dir;
    let ni = level.items.len();
    // Item sizes, canonical.
    let mut w = Vec::with_capacity(ni);
    let mut h = Vec::with_capacity(ni);
    let mut port_span = Vec::with_capacity(ni);
    for kind in &level.items {
        let (size, span) = match *kind {
            ItemKind::Node(n) => (graph.nodes[n].size, graph.nodes[n].port_span),
            ItemKind::Box(c) => (geoms[level_of_cluster[c]].size, 0.8),
            ItemKind::Port { .. } => ([0.0, 0.0], 0.0),
        };
        let s = dir.canonical_size(size);
        w.push(s[0].max(0.0));
        h.push(s[1].max(0.0));
        port_span.push(span.clamp(0.0, 1.0));
    }
    // Self-loop room on the canonical right.
    let mut loop_room = vec![0.0f32; ni];
    for &(e, item) in &level.self_loops {
        let label_w = graph.edges[e]
            .label
            .map_or(0.0, |l| dir.canonical_size(l)[0] + 6.0);
        loop_room[item] = loop_room[item].max(sp.self_loop + label_w)
            + if loop_room[item] > 0.0 {
                sp.edge_sep
            } else {
                0.0
            };
    }

    // Virtual nodes: items first, then chain dummies.
    let mut vkind: Vec<VKind> = (0..ni).map(VKind::Item).collect();
    let mut vrank: Vec<i32> = level.ranks.clone();
    let mut vw: Vec<f32> = (0..ni).map(|i| w[i] + 2.0 * loop_room[i]).collect();
    let mut vh: Vec<f32> = h.clone();
    let mut chains: Vec<Vec<usize>> = Vec::with_capacity(level.edges.len());
    let mut label_v: Vec<Option<usize>> = Vec::with_capacity(level.edges.len());
    for edge in &level.edges {
        let (ru, rv) = (level.ranks[edge.u], level.ranks[edge.v]);
        let mut chain = vec![edge.u];
        let label_rank = edge
            .label
            .filter(|_| rv - ru >= 2)
            .map(|_| ru + (rv - ru) / 2);
        let mut lv = None;
        for r in ru + 1..rv {
            let id = vkind.len();
            if Some(r) == label_rank {
                let s = dir.canonical_size(edge.label.unwrap_or_default());
                vkind.push(VKind::Label);
                vw.push(s[0]);
                vh.push(s[1]);
                lv = Some(id);
            } else {
                vkind.push(VKind::Dummy);
                vw.push(0.0);
                vh.push(0.0);
            }
            vrank.push(r);
            chain.push(id);
        }
        chain.push(edge.v);
        chains.push(chain);
        label_v.push(lv);
    }
    let nv = vkind.len();
    limits.check_count(nv, limits.nodes * 8, LimitKind::Nodes)?;
    let mut up: Vec<Vec<usize>> = vec![Vec::new(); nv];
    let mut down: Vec<Vec<usize>> = vec![Vec::new(); nv];
    for chain in &chains {
        for pair in chain.windows(2) {
            down[pair[0]].push(pair[1]);
            up[pair[1]].push(pair[0]);
        }
    }
    let max_rank = vrank.iter().copied().max().unwrap_or(0).max(0) as usize;
    let mut layers: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    // Initial order: depth-first from items by rank.
    {
        let mut starts: Vec<usize> = (0..ni).collect();
        starts.sort_by_key(|&i| (vrank[i], i));
        let mut seen = vec![false; nv];
        for s in starts {
            if seen[s] {
                continue;
            }
            let mut stack = vec![s];
            while let Some(v) = stack.pop() {
                if seen[v] {
                    continue;
                }
                seen[v] = true;
                layers[vrank[v].max(0) as usize].push(v);
                for &d in down[v].iter().rev() {
                    if !seen[d] {
                        stack.push(d);
                    }
                }
            }
        }
    }
    let edge_count = chains
        .iter()
        .map(|c| c.len().saturating_sub(1))
        .sum::<usize>()
        .max(1) as u64;
    let sweeps =
        (limits.order_work / (edge_count * layers.len().max(1) as u64)).clamp(2, 24) as usize;
    order::order(&mut layers, &up, &down, sweeps, nv <= 600);

    let dummy: Vec<bool> = (0..nv)
        .map(|v| match vkind[v] {
            VKind::Item(i) => matches!(level.items[i], ItemKind::Port { .. }),
            _ => true,
        })
        .collect();
    let mut x = position::position_x(
        &layers,
        &up,
        &down,
        &position::Separation {
            width: &vw,
            dummy: &dummy,
            node_sep: sp.node_sep,
            edge_sep: sp.edge_sep,
        },
    );

    // Fixed ports on member boxes, from their own geometry.
    let fixed = |item: usize, global: usize| -> Option<f32> {
        let ItemKind::Box(c) = level.items[item] else {
            return None;
        };
        let g = &geoms[level_of_cluster[c]];
        let p = g.ports.get(&global)?;
        let off = [p[0] - g.size[0] / 2.0, p[1] - g.size[1] / 2.0];
        Some(if dir.horizontal() { off[1] } else { off[0] })
    };
    // Edge-end offsets along each item side.
    let mut start_x = vec![0.0f32; chains.len()];
    let mut end_x = vec![0.0f32; chains.len()];
    let mut bottom: Vec<Vec<(usize, f32)>> = vec![Vec::new(); ni];
    let mut top: Vec<Vec<(usize, f32)>> = vec![Vec::new(); ni];
    for (k, chain) in chains.iter().enumerate() {
        let (u, v) = (chain[0], chain[chain.len() - 1]);
        let g = level.edges[k].global;
        match fixed(u, g) {
            Some(off) => start_x[k] = x[u] + off,
            None => bottom[u].push((k, x[chain[1]])),
        }
        match fixed(v, g) {
            Some(off) => end_x[k] = x[v] + off,
            None => top[v].push((k, x[chain[chain.len() - 2]])),
        }
    }
    for i in 0..ni {
        let limit = (w[i] * port_span[i] / 2.0).max(0.0);
        for (bottom_side, side, out) in [
            (true, &mut bottom[i], &mut start_x),
            (false, &mut top[i], &mut end_x),
        ] {
            if side.is_empty() {
                continue;
            }
            side.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
            let n = side.len();
            let gap = if n > 1 {
                sp.port_gap.min(2.0 * limit / (n - 1) as f32)
            } else {
                0.0
            };
            // Too many ends for the side: edges that leave here (no arrow
            // head on this end) share one port and fan out in the gap, which
            // reads cleaner than a comb of lines a few px apart. Arrow heads
            // always keep their own port.
            let all_leave = side
                .iter()
                .all(|&(k, _)| level.edges[k].flipped != bottom_side);
            if n > 1 && gap < sp.edge_sep * 0.45 && all_leave {
                for &(k, _) in side.iter() {
                    out[k] = x[i];
                }
                continue;
            }
            let mut offs: Vec<f32> = side
                .iter()
                .map(|&(_, nx)| (nx - x[i]).clamp(-limit, limit))
                .collect();
            for j in 1..n {
                offs[j] = offs[j].max(offs[j - 1] + gap);
            }
            if n > 0 && offs[n - 1] > limit {
                offs[n - 1] = limit;
                for j in (0..n - 1).rev() {
                    offs[j] = offs[j].min(offs[j + 1] - gap);
                }
            }
            for (j, &(k, _)) in side.iter().enumerate() {
                out[k] = x[i] + offs[j].clamp(-limit.max(0.0), limit.max(0.0));
            }
        }
    }
    // Straighten small drifts along dummy chains.
    for (k, chain) in chains.iter().enumerate() {
        let mut prev = start_x[k];
        for &d in &chain[1..chain.len() - 1] {
            if (x[d] - prev).abs() < 2.0 {
                x[d] = prev;
            }
            prev = x[d];
        }
    }

    // Horizontal runs, one per bend, grouped by the gap they sit in.
    let gaps = layers.len().saturating_sub(1);
    struct Jog {
        chain: usize,
        step: usize,
        from: f32,
        to: f32,
        track: usize,
    }
    let mut jogs: Vec<Vec<Jog>> = (0..gaps).map(|_| Vec::new()).collect();
    let xs_of = |k: usize, chain: &[usize], x: &[f32]| -> Vec<f32> {
        let mut xs: Vec<f32> = chain.iter().map(|&v| x[v]).collect();
        xs[0] = start_x[k];
        let last = xs.len() - 1;
        xs[last] = end_x[k];
        xs
    };
    for (k, chain) in chains.iter().enumerate() {
        let xs = xs_of(k, chain, &x);
        let r0 = vrank[chain[0]] as usize;
        for step in 0..chain.len() - 1 {
            if (xs[step] - xs[step + 1]).abs() > 0.5 {
                jogs[r0 + step].push(Jog {
                    chain: k,
                    step,
                    from: xs[step],
                    to: xs[step + 1],
                    track: 0,
                });
            }
        }
    }
    let mut tracks = vec![0usize; gaps];
    for (g, list) in jogs.iter_mut().enumerate() {
        // Rightward runs: the one starting further right takes the higher
        // track; leftward runs mirror that. This avoids the crossings two
        // overlapping runs would otherwise make with each other's verticals.
        list.sort_by(|a, b| {
            let ka = if a.to > a.from {
                (0, -a.from)
            } else {
                (1, a.from)
            };
            let kb = if b.to > b.from {
                (0, -b.from)
            } else {
                (1, b.from)
            };
            ka.0.cmp(&kb.0)
                .then(ka.1.total_cmp(&kb.1))
                .then(a.chain.cmp(&b.chain))
        });
        let margin = sp.edge_sep * 0.5;
        for i in 0..list.len() {
            let (lo, hi) = (list[i].from.min(list[i].to), list[i].from.max(list[i].to));
            let mut t = 0;
            for earlier in &list[..i] {
                let (a, b) = (earlier.from.min(earlier.to), earlier.from.max(earlier.to));
                if a < hi + margin && lo < b + margin {
                    t = t.max(earlier.track + 1);
                }
            }
            list[i].track = t;
        }
        tracks[g] = list.iter().map(|j| j.track + 1).max().unwrap_or(0);
    }

    // Rank heights and y positions.
    let has_top = level.items.iter().any(|k| {
        matches!(
            k,
            ItemKind::Port {
                side: Side::Top,
                ..
            }
        )
    });
    let has_bottom = level.items.iter().any(|k| {
        matches!(
            k,
            ItemKind::Port {
                side: Side::Bottom,
                ..
            }
        )
    });
    let first_real = usize::from(has_top);
    let last_real = if has_bottom {
        layers.len().saturating_sub(2)
    } else {
        layers.len().saturating_sub(1)
    };
    let mut layer_h = vec![0.0f32; layers.len()];
    for (r, layer) in layers.iter().enumerate() {
        layer_h[r] = layer.iter().map(|&v| vh[v]).fold(0.0, f32::max);
    }
    let base_gap = if level.doubled {
        sp.rank_sep / 2.0
    } else {
        sp.rank_sep
    };
    let mut y = vec![0.0f32; layers.len()];
    let mut cursor = 0.0f32;
    for r in first_real..=last_real.max(first_real) {
        if r >= layers.len() {
            break;
        }
        if r > first_real {
            let need = (tracks[r - 1] + 1) as f32 * sp.track_sep;
            cursor += layer_h[r - 1] / 2.0 + base_gap.max(need) + layer_h[r] / 2.0;
        } else {
            cursor = layer_h[r] / 2.0;
        }
        y[r] = cursor;
    }
    let content_top = if layers.len() > first_real {
        y[first_real] - layer_h[first_real] / 2.0
    } else {
        0.0
    };
    let content_bottom =
        if !layers.is_empty() && last_real < layers.len() && last_real >= first_real {
            y[last_real] + layer_h[last_real] / 2.0
        } else {
            content_top
        };
    let top_need = if has_top && gaps > 0 {
        (tracks[0] + 1) as f32 * sp.track_sep
    } else {
        0.0
    };
    let bottom_need = if has_bottom && gaps > 0 {
        (tracks[gaps - 1] + 1) as f32 * sp.track_sep
    } else {
        0.0
    };

    // Content bounds (canonical).
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    for v in 0..nv {
        let half = vw[v] / 2.0;
        min_x = min_x.min(x[v] - half);
        max_x = max_x.max(x[v] + half);
    }
    for k in 0..chains.len() {
        min_x = min_x.min(start_x[k]).min(end_x[k]);
        max_x = max_x.max(start_x[k]).max(end_x[k]);
    }
    if !min_x.is_finite() {
        min_x = 0.0;
        max_x = 0.0;
    }

    // Box margins: padding, track room for ports, and the title at the top
    // of the screen.
    let is_cluster = level.cluster.is_some();
    let title = level
        .cluster
        .map_or([0.0, 0.0], |c| graph.clusters[c].title);
    let pad = if is_cluster { sp.cluster_pad } else { 0.0 };
    let title_room = if title[1] > 0.0 {
        title[1] + pad * 0.5
    } else {
        0.0
    };
    let (mut lm, rm, mut tm, mut bm) = (pad, pad, pad.max(top_need), pad.max(bottom_need));
    if is_cluster {
        match dir {
            Direction::TB => tm += title_room,
            Direction::BT => bm += title_room,
            Direction::LR | Direction::RL => lm += title_room,
        }
    }
    let mut left = min_x - lm;
    let mut right = max_x + rm;
    let top_edge = content_top - tm;
    let bottom_edge = content_bottom + bm;
    // A title wider than the content widens the box evenly.
    if is_cluster {
        let need = title[0] + 2.0 * pad;
        if dir.horizontal() {
            // On screen the width is the canonical height; handled below.
        } else if right - left < need {
            let extra = (need - (right - left)) / 2.0;
            left -= extra;
            right += extra;
        }
    }
    let (mut top_b, mut bottom_b) = (top_edge, bottom_edge);
    if is_cluster && dir.horizontal() {
        let need = title[0] + 2.0 * pad;
        if bottom_b - top_b < need {
            let extra = (need - (bottom_b - top_b)) / 2.0;
            top_b -= extra;
            bottom_b += extra;
        }
    }
    let cw = (right - left).max(0.0);
    let ch = (bottom_b - top_b).max(0.0);
    let (ox, oy) = (left, top_b);
    let canon = |p: [f32; 2]| -> [f32; 2] { dir.to_real([p[0] - ox, p[1] - oy], ch) };

    // Port layers sit on the box border.
    if has_top {
        y[0] = top_b;
    }
    if has_bottom && !layers.is_empty() {
        let last = layers.len() - 1;
        y[last] = bottom_b;
    }
    let track_y = |g: usize, t: usize| -> f32 {
        let n = tracks[g].max(1) as f32;
        let (band_top, band_bottom) = if has_top && g == 0 {
            (content_top - pad.max(top_need), content_top)
        } else if has_bottom && g + 1 == layers.len() - 1 {
            (content_bottom, content_bottom + pad.max(bottom_need))
        } else {
            (y[g] + layer_h[g] / 2.0, y[g + 1] - layer_h[g + 1] / 2.0)
        };
        band_top + (band_bottom - band_top) * (t as f32 + 1.0) / (n + 1.0)
    };
    let mut track_of: HashMap<(usize, usize), usize> = HashMap::new();
    for list in &jogs {
        for j in list {
            track_of.insert((j.chain, j.step), j.track);
        }
    }

    let mut geom = LevelGeom::default();
    let real_w = dir.real_size([cw, ch]);
    geom.size = real_w;
    geom.items = (0..ni)
        .map(|i| {
            let c = canon([x[i], y[vrank[i].max(0) as usize]]);
            let s = dir.real_size([w[i], h[i]]);
            Rect::centered(c, s)
        })
        .collect();
    for (i, kind) in level.items.iter().enumerate() {
        if let ItemKind::Port { edge, .. } = *kind {
            geom.ports
                .insert(edge, canon([x[i], y[vrank[i].max(0) as usize]]));
        }
    }
    for (k, chain) in chains.iter().enumerate() {
        let edge = &level.edges[k];
        let xs = xs_of(k, chain, &x);
        let (u, v) = (chain[0], chain[chain.len() - 1]);
        let ru = vrank[u] as usize;
        let rv = vrank[v] as usize;
        let y_start = if matches!(level.items[u], ItemKind::Port { .. }) {
            y[ru]
        } else {
            y[ru] + h[u] / 2.0
        };
        let y_end = if matches!(level.items[v], ItemKind::Port { .. }) {
            y[rv]
        } else {
            y[rv] - h[v] / 2.0
        };
        let mut pts = vec![[xs[0], y_start]];
        for step in 0..chain.len() - 1 {
            if let Some(&t) = track_of.get(&(k, step)) {
                let ty = track_y(ru + step, t);
                pts.push([xs[step], ty]);
                pts.push([xs[step + 1], ty]);
            }
        }
        pts.push([xs[xs.len() - 1], y_end]);
        let label = label_v[k]
            .map(|lv| canon([x[lv], y[vrank[lv] as usize]]))
            .or_else(|| {
                // A one-rank edge with a label: centre it on the longest run.
                edge.label.map(|_| {
                    let mid = crate::layout::midpoint(&pts);
                    canon(mid)
                })
            });
        let mut real: Vec<[f32; 2]> = pts.into_iter().map(canon).collect();
        if edge.flipped {
            real.reverse();
        }
        geom.segments.insert(edge.global, (real, label));
    }
    for &(e, item) in &level.self_loops {
        let cx = x[item];
        let cy = y[vrank[item].max(0) as usize];
        let (iw, ih) = (w[item], h[item]);
        let reach = sp.self_loop;
        let x0 = cx + iw / 2.0;
        let pts = vec![
            [x0, cy - ih / 4.0],
            [x0 + reach, cy - ih / 4.0],
            [x0 + reach, cy + ih / 4.0],
            [x0, cy + ih / 4.0],
        ];
        let label = graph.edges[e].label.map(|l| {
            let s = dir.canonical_size(l);
            canon([x0 + reach + 4.0 + s[0] / 2.0, cy])
        });
        geom.loops
            .push((e, (pts.into_iter().map(canon).collect(), label)));
    }
    if is_cluster {
        geom.title = Rect::new((real_w[0] - title[0]) / 2.0, pad * 0.6, title[0], title[1]);
    }
    Ok(geom)
}
