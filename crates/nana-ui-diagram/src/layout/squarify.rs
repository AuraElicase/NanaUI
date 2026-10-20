//! Squarified treemaps (Bruls, Huizing and van Wijk): items, largest
//! first, fill rows along the shorter side of the space left, and a row
//! closes as soon as one more item would make its worst aspect ratio worse.

use crate::ir::Rect;

/// One rectangle per value inside `rect`, in the input order, with areas
/// proportional to the values. Values that are not positive and finite get
/// an empty rectangle at `rect`'s origin.
pub fn squarify(values: &[f64], rect: Rect) -> Vec<Rect> {
    let mut out = vec![Rect::new(rect.x, rect.y, 0.0, 0.0); values.len()];
    let mut order: Vec<usize> = (0..values.len())
        .filter(|&i| values[i].is_finite() && values[i] > 0.0)
        .collect();
    let total: f64 = order.iter().map(|&i| values[i]).sum();
    if order.is_empty() || !(total.is_finite() && total > 0.0) || rect.w <= 0.0 || rect.h <= 0.0 {
        return out;
    }
    order.sort_by(|&a, &b| values[b].total_cmp(&values[a]).then(a.cmp(&b)));
    let scale = f64::from(rect.w) * f64::from(rect.h) / total;
    let areas: Vec<f64> = order.iter().map(|&i| values[i] * scale).collect();

    let (mut x, mut y, mut w, mut h) = (
        f64::from(rect.x),
        f64::from(rect.y),
        f64::from(rect.w),
        f64::from(rect.h),
    );
    let mut start = 0;
    while start < areas.len() {
        let side = w.min(h).max(1e-9);
        let mut end = start + 1;
        let mut sum = areas[start];
        let mut current = worst(&areas[start..end], sum, side);
        while end < areas.len() {
            let next_sum = sum + areas[end];
            let next = worst(&areas[start..=end], next_sum, side);
            if next > current {
                break;
            }
            current = next;
            sum = next_sum;
            end += 1;
        }
        // The last row takes whatever is left, so rounding never leaves a gap.
        let last = end == areas.len();
        let thickness = if last {
            if w >= h { w } else { h }
        } else {
            (sum / side).min(if w >= h { w } else { h })
        };
        let mut along = 0.0;
        for k in start..end {
            let length = if sum > 0.0 {
                areas[k] / sum * side
            } else {
                0.0
            };
            let r = if w >= h {
                // A column at the left edge.
                Rect::new(
                    x as f32,
                    (y + along) as f32,
                    thickness as f32,
                    length as f32,
                )
            } else {
                // A row along the top edge.
                Rect::new(
                    (x + along) as f32,
                    y as f32,
                    length as f32,
                    thickness as f32,
                )
            };
            out[order[k]] = r;
            along += length;
        }
        if w >= h {
            x += thickness;
            w = (w - thickness).max(0.0);
        } else {
            y += thickness;
            h = (h - thickness).max(0.0);
        }
        start = end;
    }
    out
}

/// The worst aspect ratio of a row of `areas` summing to `sum` laid along
/// `side`.
fn worst(areas: &[f64], sum: f64, side: f64) -> f64 {
    let (mut max, mut min) = (f64::MIN, f64::MAX);
    for &a in areas {
        max = max.max(a);
        min = min.min(a);
    }
    if sum <= 0.0 || min <= 0.0 {
        return f64::MAX;
    }
    let s2 = sum * sum;
    let w2 = side * side;
    (w2 * max / s2).max(s2 / (w2 * min))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn areas_follow_values_and_tile_the_rect() {
        let values = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let rect = Rect::new(0.0, 0.0, 600.0, 400.0);
        let rects = squarify(&values, rect);
        let total: f32 = rects.iter().map(|r| r.w * r.h).sum();
        assert!((total - 240_000.0).abs() < 1.0, "{total}");
        for (v, r) in values.iter().zip(&rects) {
            let expected = *v as f32 / 24.0 * 240_000.0;
            assert!((r.w * r.h - expected).abs() < 1.0, "{r:?}");
            assert!(r.x >= -1e-3 && r.y >= -1e-3 && r.right() <= 600.01 && r.bottom() <= 400.01);
        }
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                assert!(!a.inset(0.01, 0.01).intersects(&b.inset(0.01, 0.01)));
            }
        }
        // The paper's example keeps every ratio under 3.
        for r in &rects {
            assert!(r.w.max(r.h) / r.w.min(r.h) < 3.0, "{r:?}");
        }
    }

    #[test]
    fn non_positive_values_get_no_area() {
        let rects = squarify(
            &[0.0, -1.0, f64::NAN, 5.0],
            Rect::new(10.0, 10.0, 50.0, 20.0),
        );
        assert_eq!(rects[0].w * rects[0].h, 0.0);
        assert_eq!(rects[3], Rect::new(10.0, 10.0, 50.0, 20.0));
    }
}
