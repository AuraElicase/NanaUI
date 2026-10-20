//! Trees: building one from indentation, walking it without recursion and
//! the tidy layout the mind map and other outline families place with.
//!
//! The tidy layout follows Reingold and Tilford: every subtree is laid out
//! on its own, then siblings are pushed apart until their contours (the
//! extreme breadth of each level) keep a gap, and a parent sits centred
//! over its first and last child. Levels share a column on the depth axis,
//! so contours of different subtrees compare exactly level by level.

/// The width of a line's leading whitespace, a tab counting four columns.
pub fn indent_width(line: &str) -> usize {
    let mut width = 0;
    for c in line.chars() {
        match c {
            ' ' | '\u{a0}' | '\u{3000}' => width += 1,
            '\t' => width += 4,
            _ => break,
        }
    }
    width
}

/// Parent links from indentation: an item's parent is the closest earlier
/// item with a smaller indent.
#[derive(Debug, Default)]
pub struct Outline {
    stack: Vec<(usize, usize)>,
}

impl Outline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record item `index` at `indent` and return its parent.
    pub fn push(&mut self, indent: usize, index: usize) -> Option<usize> {
        while self.stack.last().is_some_and(|&(i, _)| i >= indent) {
            self.stack.pop();
        }
        let parent = self.stack.last().map(|&(_, p)| p);
        self.stack.push((indent, index));
        parent
    }

    /// The depth the next item at `indent` would get, without recording it.
    pub fn depth_for(&self, indent: usize) -> usize {
        self.stack.iter().filter(|&&(i, _)| i < indent).count()
    }
}

/// `(node, depth)` in pre-order from `roots`, without recursion. Nodes a
/// cycle would revisit are skipped.
pub fn preorder(children: &[Vec<usize>], roots: &[usize]) -> Vec<(usize, usize)> {
    let mut out = Vec::with_capacity(children.len());
    let mut seen = vec![false; children.len()];
    let mut stack: Vec<(usize, usize)> = roots.iter().rev().map(|&r| (r, 0)).collect();
    while let Some((node, depth)) = stack.pop() {
        if node >= children.len() || seen[node] {
            continue;
        }
        seen[node] = true;
        out.push((node, depth));
        for &child in children[node].iter().rev() {
            stack.push((child, depth + 1));
        }
    }
    out
}

/// One node of a tidy layout. Sizes are along the depth axis (the way
/// levels advance) and the breadth axis (the way siblings spread).
#[derive(Debug, Clone, Default)]
pub struct TidyNode {
    pub depth_size: f32,
    pub breadth_size: f32,
    pub children: Vec<usize>,
}

/// Gaps of a tidy layout.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TidyGaps {
    /// Between neighbouring levels.
    pub level: f32,
    /// Between siblings.
    pub sibling: f32,
    /// Between the descendants of neighbouring subtrees.
    pub subtree: f32,
}

/// Where a tidy layout put a node: the start of its column on the depth
/// axis and its centre on the breadth axis.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct TidyPlace {
    pub depth: f32,
    pub breadth: f32,
    pub level: usize,
}

/// Lay out the forest under `roots` (which become level 0, centred as a
/// group on breadth 0). Unreachable nodes stay at the origin.
pub fn tidy(nodes: &[TidyNode], roots: &[usize], gaps: TidyGaps) -> Vec<TidyPlace> {
    let n = nodes.len();
    let mut places = vec![TidyPlace::default(); n];
    // A virtual parent of the roots sits at index `n`.
    let child_list = |v: usize| -> &[usize] { if v == n { roots } else { &nodes[v].children } };
    let mut order = Vec::with_capacity(n + 1);
    let mut seen = vec![false; n + 1];
    let mut stack = vec![(n, 0usize)];
    while let Some((v, level)) = stack.pop() {
        if seen[v] {
            continue;
        }
        seen[v] = true;
        order.push((v, level));
        for &c in child_list(v).iter().rev() {
            if c < n && !seen[c] {
                stack.push((c, level + 1));
            }
        }
    }

    // Columns: every level starts past the widest node of the level before.
    let levels = order.iter().map(|&(_, l)| l).max().unwrap_or(0);
    let mut widest = vec![0.0f32; levels + 1];
    for &(v, level) in &order {
        if v < n {
            widest[level] = widest[level].max(nodes[v].depth_size);
        }
    }
    let mut column = vec![0.0f32; levels + 1];
    for level in 2..=levels {
        column[level] = column[level - 1] + widest[level - 1] + gaps.level;
    }

    // Children of a placed node, in the order they were reached.
    let reached = |v: usize, seen_order: &[Option<usize>]| -> Vec<usize> {
        child_list(v)
            .iter()
            .copied()
            .filter(|&c| c < n && seen_order[c].is_some_and(|parent| parent == v))
            .collect()
    };
    let mut parent_of: Vec<Option<usize>> = vec![None; n];
    {
        let mut owned = vec![false; n];
        for &(v, _) in &order {
            for &c in child_list(v) {
                if c < n && !owned[c] {
                    owned[c] = true;
                    parent_of[c] = Some(v);
                }
            }
        }
    }

    // Post-order: contours relative to each subtree's own centre.
    let mut contours: Vec<Option<Vec<(f32, f32)>>> = vec![None; n + 1];
    let mut offset = vec![0.0f32; n + 1];
    for &(v, _) in order.iter().rev() {
        let kids = reached(v, &parent_of);
        let half = if v < n {
            nodes[v].breadth_size / 2.0
        } else {
            0.0
        };
        let mut own = vec![(-half, half)];
        if !kids.is_empty() {
            let mut merged: Vec<(f32, f32)> = Vec::new();
            let mut positions = Vec::with_capacity(kids.len());
            for &c in &kids {
                let contour = contours[c].take().unwrap_or_default();
                let mut shift = f32::NEG_INFINITY;
                for (level, (&(_, bottom), &(top, _))) in merged.iter().zip(&contour).enumerate() {
                    let gap = if level == 0 {
                        gaps.sibling
                    } else {
                        gaps.subtree
                    };
                    shift = shift.max(bottom - top + gap);
                }
                if !shift.is_finite() {
                    shift = 0.0;
                }
                for (level, &(top, bottom)) in contour.iter().enumerate() {
                    let (top, bottom) = (top + shift, bottom + shift);
                    match merged.get_mut(level) {
                        Some(m) => *m = (m.0.min(top), m.1.max(bottom)),
                        None => merged.push((top, bottom)),
                    }
                }
                positions.push(shift);
            }
            let mid = (positions[0] + positions[positions.len() - 1]) / 2.0;
            for (&c, p) in kids.iter().zip(&positions) {
                offset[c] = p - mid;
            }
            own.extend(merged.into_iter().map(|(t, b)| (t - mid, b - mid)));
        }
        contours[v] = Some(own);
    }

    // Pre-order: absolute positions.
    let mut absolute = vec![0.0f32; n + 1];
    for &(v, level) in &order {
        if v == n {
            continue;
        }
        let parent = parent_of[v].unwrap_or(n);
        absolute[v] = absolute[parent] + offset[v];
        places[v] = TidyPlace {
            depth: column[level],
            breadth: absolute[v],
            level: level - 1,
        };
    }
    places
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(depth: f32, breadth: f32, children: Vec<usize>) -> TidyNode {
        TidyNode {
            depth_size: depth,
            breadth_size: breadth,
            children,
        }
    }

    #[test]
    fn outline_parents_follow_indentation() {
        let mut o = Outline::new();
        assert_eq!(o.push(0, 0), None);
        assert_eq!(o.push(2, 1), Some(0));
        assert_eq!(o.push(4, 2), Some(1));
        assert_eq!(o.push(2, 3), Some(0));
        assert_eq!(o.push(3, 4), Some(3));
        assert_eq!(o.push(0, 5), None);
        assert_eq!(indent_width("\t  x"), 6);
    }

    #[test]
    fn siblings_keep_their_gap_and_parents_centre_over_children() {
        // 0 -> (1 -> 3, 4), 2
        let nodes = vec![
            node(40.0, 20.0, vec![1, 2]),
            node(40.0, 20.0, vec![3, 4]),
            node(40.0, 20.0, vec![]),
            node(40.0, 20.0, vec![]),
            node(40.0, 20.0, vec![]),
        ];
        let gaps = TidyGaps {
            level: 10.0,
            sibling: 8.0,
            subtree: 8.0,
        };
        let p = tidy(&nodes, &[0], gaps);
        assert_eq!(p[0].breadth, 0.0);
        assert!((p[4].breadth - p[3].breadth - 28.0).abs() < 1e-3);
        assert!((p[1].breadth - (p[3].breadth + p[4].breadth) / 2.0).abs() < 1e-3);
        assert!(p[2].breadth - p[1].breadth >= 28.0 - 1e-3);
        assert_eq!(p[3].depth, 100.0);
        assert_eq!(p[3].level, 2);
    }

    #[test]
    fn deep_chains_do_not_recurse() {
        let n = 3_000;
        let nodes: Vec<TidyNode> = (0..n)
            .map(|i| node(1.0, 1.0, if i + 1 < n { vec![i + 1] } else { vec![] }))
            .collect();
        let places = tidy(
            &nodes,
            &[0],
            TidyGaps {
                level: 1.0,
                sibling: 1.0,
                subtree: 1.0,
            },
        );
        assert_eq!(places[n - 1].level, n - 1);
        let children: Vec<Vec<usize>> = nodes.iter().map(|t| t.children.clone()).collect();
        assert_eq!(preorder(&children, &[0]).len(), n);
    }
}
