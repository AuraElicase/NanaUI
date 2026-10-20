//! Crossing reduction: alternating barycenter sweeps with adjacent-swap
//! transposition, keeping the best order seen (Sugiyama, Tagawa and Toda
//! 1981; the transpose step as in Gansner et al. 1993). Crossings are
//! counted with the accumulator tree of Barth, Jünger and Mutzel (2002).

/// `layers[r]` lists the nodes on rank `r` in order; `up[v]` / `down[v]`
/// are `v`'s neighbours on the ranks directly above and below.
pub(crate) fn order(
    layers: &mut [Vec<usize>],
    up: &[Vec<usize>],
    down: &[Vec<usize>],
    sweeps: usize,
    transpose: bool,
) {
    let n = up.len();
    let mut pos = vec![0usize; n];
    let sync = |layers: &[Vec<usize>], pos: &mut [usize]| {
        for layer in layers {
            for (i, &v) in layer.iter().enumerate() {
                pos[v] = i;
            }
        }
    };
    sync(layers, &mut pos);
    let mut best: Vec<Vec<usize>> = layers.to_vec();
    let mut best_count = total_crossings(layers, down, &pos);
    if best_count == 0 {
        return;
    }
    let mut stale = 0;
    for sweep in 0..sweeps {
        let downward = sweep % 2 == 0;
        if downward {
            for layer in layers.iter_mut().skip(1) {
                reorder(layer, up, &pos);
                for (i, &v) in layer.iter().enumerate() {
                    pos[v] = i;
                }
            }
        } else {
            for r in (0..layers.len().saturating_sub(1)).rev() {
                reorder(&mut layers[r], down, &pos);
                for (i, &v) in layers[r].iter().enumerate() {
                    pos[v] = i;
                }
            }
        }
        if transpose {
            transpose_layers(layers, up, down, &mut pos);
        }
        let count = total_crossings(layers, down, &pos);
        if count < best_count {
            best_count = count;
            best = layers.to_vec();
            stale = 0;
        } else {
            stale += 1;
        }
        if best_count == 0 || stale >= 4 {
            break;
        }
    }
    for (layer, b) in layers.iter_mut().zip(best) {
        *layer = b;
    }
}

/// Sort one layer by the mean position of each node's neighbours on the
/// fixed layer. Nodes with no neighbours there keep their slot.
fn reorder(layer: &mut [usize], neighbours: &[Vec<usize>], pos: &[usize]) {
    let mut sortable: Vec<(f32, usize, usize)> = Vec::new();
    let mut fixed: Vec<bool> = vec![false; layer.len()];
    for (i, &v) in layer.iter().enumerate() {
        let ns = &neighbours[v];
        if ns.is_empty() {
            fixed[i] = true;
        } else {
            let sum: usize = ns.iter().map(|&w| pos[w]).sum();
            sortable.push((sum as f32 / ns.len() as f32, i, v));
        }
    }
    sortable.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let mut it = sortable.into_iter();
    let original: Vec<usize> = layer.to_vec();
    for i in 0..layer.len() {
        if fixed[i] {
            layer[i] = original[i];
        } else if let Some((_, _, v)) = it.next() {
            layer[i] = v;
        }
    }
}

fn transpose_layers(
    layers: &mut [Vec<usize>],
    up: &[Vec<usize>],
    down: &[Vec<usize>],
    pos: &mut [usize],
) {
    for _round in 0..4 {
        let mut improved = false;
        for layer in layers.iter_mut() {
            if layer.len() < 2 || layer.len() > 400 {
                continue;
            }
            for i in 0..layer.len() - 1 {
                let (a, b) = (layer[i], layer[i + 1]);
                let keep = pair_crossings(a, b, up, pos) + pair_crossings(a, b, down, pos);
                let swap = pair_crossings(b, a, up, pos) + pair_crossings(b, a, down, pos);
                if swap < keep {
                    layer.swap(i, i + 1);
                    pos[a] = i + 1;
                    pos[b] = i;
                    improved = true;
                }
            }
        }
        if !improved {
            break;
        }
    }
}

/// Crossings between `a`'s and `b`'s edges toward one neighbouring layer
/// when `a` sits left of `b`.
fn pair_crossings(a: usize, b: usize, neighbours: &[Vec<usize>], pos: &[usize]) -> usize {
    let mut count = 0;
    for &x in &neighbours[a] {
        for &y in &neighbours[b] {
            if pos[x] > pos[y] {
                count += 1;
            }
        }
    }
    count
}

pub(crate) fn total_crossings(layers: &[Vec<usize>], down: &[Vec<usize>], pos: &[usize]) -> usize {
    let mut total = 0;
    for r in 0..layers.len().saturating_sub(1) {
        total += bilayer(&layers[r], layers[r + 1].len(), down, pos);
    }
    total
}

fn bilayer(north: &[usize], south_len: usize, down: &[Vec<usize>], pos: &[usize]) -> usize {
    if south_len == 0 {
        return 0;
    }
    let mut south: Vec<usize> = Vec::new();
    for &v in north {
        let mut targets: Vec<usize> = down[v].iter().map(|&w| pos[w]).collect();
        targets.sort_unstable();
        south.extend(targets);
    }
    let mut first = 1;
    while first < south_len {
        first <<= 1;
    }
    let mut tree = vec![0usize; 2 * first - 1];
    let first_index = first - 1;
    let mut count = 0;
    for p in south {
        let mut index = p + first_index;
        tree[index] += 1;
        while index > 0 {
            if index % 2 == 1 {
                count += tree[index + 1];
            }
            index = (index - 1) / 2;
            tree[index] += 1;
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sweeps_untangle_a_crossed_pair() {
        // 0,1 on top; 2,3 below; 0→3, 1→2 cross in the given order.
        let mut layers = vec![vec![0, 1], vec![2, 3]];
        let mut up = vec![vec![]; 4];
        let mut down = vec![vec![]; 4];
        for (u, v) in [(0, 3), (1, 2)] {
            down[u].push(v);
            up[v].push(u);
        }
        let mut pos = vec![0; 4];
        for l in &layers {
            for (i, &v) in l.iter().enumerate() {
                pos[v] = i;
            }
        }
        assert_eq!(total_crossings(&layers, &down, &pos), 1);
        order(&mut layers, &up, &down, 8, true);
        for l in &layers {
            for (i, &v) in l.iter().enumerate() {
                pos[v] = i;
            }
        }
        assert_eq!(total_crossings(&layers, &down, &pos), 0);
    }
}
