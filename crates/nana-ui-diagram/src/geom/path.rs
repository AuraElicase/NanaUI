//! Vector paths and the curve helpers edges and shapes are built from.

use crate::ir::Rect;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

/// A path in diagram coordinates.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PathData {
    pub verbs: Vec<Verb>,
}

const KAPPA: f32 = 0.552_284_8;

impl PathData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_to(&mut self, p: [f32; 2]) -> &mut Self {
        self.verbs.push(Verb::Move(p));
        self
    }

    pub fn line_to(&mut self, p: [f32; 2]) -> &mut Self {
        self.verbs.push(Verb::Line(p));
        self
    }

    pub fn quad_to(&mut self, c: [f32; 2], p: [f32; 2]) -> &mut Self {
        self.verbs.push(Verb::Quad(c, p));
        self
    }

    pub fn cubic_to(&mut self, c1: [f32; 2], c2: [f32; 2], p: [f32; 2]) -> &mut Self {
        self.verbs.push(Verb::Cubic(c1, c2, p));
        self
    }

    pub fn close(&mut self) -> &mut Self {
        self.verbs.push(Verb::Close);
        self
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// A closed polygon.
    pub fn polygon(points: &[[f32; 2]]) -> Self {
        let mut path = Self::new();
        if let Some((first, rest)) = points.split_first() {
            path.move_to(*first);
            for p in rest {
                path.line_to(*p);
            }
            path.close();
        }
        path
    }

    /// An open polyline.
    pub fn polyline(points: &[[f32; 2]]) -> Self {
        let mut path = Self::new();
        if let Some((first, rest)) = points.split_first() {
            path.move_to(*first);
            for p in rest {
                path.line_to(*p);
            }
        }
        path
    }

    pub fn rect(r: Rect) -> Self {
        Self::polygon(&[
            [r.x, r.y],
            [r.right(), r.y],
            [r.right(), r.bottom()],
            [r.x, r.bottom()],
        ])
    }

    /// A rectangle with circular corners, clamped to half the shorter side.
    pub fn rounded_rect(r: Rect, radius: f32) -> Self {
        let rad = radius.max(0.0).min(r.w / 2.0).min(r.h / 2.0);
        if rad <= 0.01 {
            return Self::rect(r);
        }
        let k = rad * (1.0 - KAPPA);
        let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
        let mut p = Self::new();
        p.move_to([x0 + rad, y0])
            .line_to([x1 - rad, y0])
            .cubic_to([x1 - k, y0], [x1, y0 + k], [x1, y0 + rad])
            .line_to([x1, y1 - rad])
            .cubic_to([x1, y1 - k], [x1 - k, y1], [x1 - rad, y1])
            .line_to([x0 + rad, y1])
            .cubic_to([x0 + k, y1], [x0, y1 - k], [x0, y1 - rad])
            .line_to([x0, y0 + rad])
            .cubic_to([x0, y0 + k], [x0 + k, y0], [x0 + rad, y0])
            .close();
        p
    }

    pub fn ellipse(r: Rect) -> Self {
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (rx, ry) = (r.w / 2.0, r.h / 2.0);
        let (kx, ky) = (rx * KAPPA, ry * KAPPA);
        let mut p = Self::new();
        p.move_to([cx + rx, cy])
            .cubic_to([cx + rx, cy + ky], [cx + kx, cy + ry], [cx, cy + ry])
            .cubic_to([cx - kx, cy + ry], [cx - rx, cy + ky], [cx - rx, cy])
            .cubic_to([cx - rx, cy - ky], [cx - kx, cy - ry], [cx, cy - ry])
            .cubic_to([cx + kx, cy - ry], [cx + rx, cy - ky], [cx + rx, cy])
            .close();
        p
    }

    /// A circular arc from `start` sweeping `sweep` radians, as cubics.
    /// Starts a new subpath unless `connect`.
    pub fn arc(
        &mut self,
        center: [f32; 2],
        radius: f32,
        start: f32,
        sweep: f32,
        connect: bool,
    ) -> &mut Self {
        let segments = ((sweep.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize).max(1);
        let step = sweep / segments as f32;
        let point = |a: f32| [center[0] + radius * a.cos(), center[1] + radius * a.sin()];
        let first = point(start);
        if connect && !self.verbs.is_empty() {
            self.line_to(first);
        } else {
            self.move_to(first);
        }
        let k = 4.0 / 3.0 * (step / 4.0).tan() * radius;
        let mut a = start;
        for _ in 0..segments {
            let b = a + step;
            let (p0, p3) = (point(a), point(b));
            let c1 = [p0[0] - k * a.sin(), p0[1] + k * a.cos()];
            let c2 = [p3[0] + k * b.sin(), p3[1] - k * b.cos()];
            self.cubic_to(c1, c2, p3);
            a = b;
        }
        self
    }

    pub fn translated(&self, dx: f32, dy: f32) -> Self {
        self.mapped(|[x, y]| [x + dx, y + dy])
    }

    pub fn mapped(&self, f: impl Fn([f32; 2]) -> [f32; 2]) -> Self {
        Self {
            verbs: self
                .verbs
                .iter()
                .map(|v| match *v {
                    Verb::Move(p) => Verb::Move(f(p)),
                    Verb::Line(p) => Verb::Line(f(p)),
                    Verb::Quad(c, p) => Verb::Quad(f(c), f(p)),
                    Verb::Cubic(a, b, p) => Verb::Cubic(f(a), f(b), f(p)),
                    Verb::Close => Verb::Close,
                })
                .collect(),
        }
    }

    /// Control-point bounds: a conservative box around the curve.
    pub fn bounds(&self) -> Rect {
        let mut min = [f32::INFINITY; 2];
        let mut max = [f32::NEG_INFINITY; 2];
        let mut add = |p: [f32; 2]| {
            min[0] = min[0].min(p[0]);
            min[1] = min[1].min(p[1]);
            max[0] = max[0].max(p[0]);
            max[1] = max[1].max(p[1]);
        };
        for v in &self.verbs {
            match *v {
                Verb::Move(p) | Verb::Line(p) => add(p),
                Verb::Quad(c, p) => {
                    add(c);
                    add(p);
                }
                Verb::Cubic(a, b, p) => {
                    add(a);
                    add(b);
                    add(p);
                }
                Verb::Close => {}
            }
        }
        if min[0].is_finite() {
            Rect::new(min[0], min[1], max[0] - min[0], max[1] - min[1])
        } else {
            Rect::default()
        }
    }

    /// Flatten into subpaths of points.
    pub fn flatten(&self, tolerance: f32) -> Vec<Vec<[f32; 2]>> {
        let mut out: Vec<Vec<[f32; 2]>> = Vec::new();
        let mut start = [0.0; 2];
        let mut last = [0.0; 2];
        for v in &self.verbs {
            match *v {
                Verb::Move(p) => {
                    out.push(vec![p]);
                    start = p;
                    last = p;
                }
                Verb::Line(p) => {
                    current(&mut out, last).push(p);
                    last = p;
                }
                Verb::Quad(c, p) => {
                    let n = steps(dist(last, c) + dist(c, p), tolerance);
                    let sub = current(&mut out, last);
                    for i in 1..=n {
                        let t = i as f32 / n as f32;
                        let u = 1.0 - t;
                        sub.push([
                            u * u * last[0] + 2.0 * u * t * c[0] + t * t * p[0],
                            u * u * last[1] + 2.0 * u * t * c[1] + t * t * p[1],
                        ]);
                    }
                    last = p;
                }
                Verb::Cubic(a, b, p) => {
                    let n = steps(dist(last, a) + dist(a, b) + dist(b, p), tolerance);
                    let sub = current(&mut out, last);
                    for i in 1..=n {
                        sub.push(cubic_point(last, a, b, p, i as f32 / n as f32));
                    }
                    last = p;
                }
                Verb::Close => {
                    if let Some(sub) = out.last_mut()
                        && sub.last() != Some(&start)
                    {
                        sub.push(start);
                    }
                    last = start;
                }
            }
        }
        out
    }

    /// Where a straight ray from `inside` toward `outside` leaves the path's
    /// outline, if it does.
    pub fn exit_point(&self, inside: [f32; 2], outside: [f32; 2]) -> Option<[f32; 2]> {
        let mut best: Option<(f32, [f32; 2])> = None;
        for sub in self.flatten(0.5) {
            for w in sub.windows(2) {
                if let Some(t) = segment_hit(inside, outside, w[0], w[1])
                    && best.is_none_or(|(bt, _)| t > bt)
                {
                    best = Some((t, lerp(inside, outside, t)));
                }
            }
        }
        best.map(|(_, p)| p)
    }
}

fn current(out: &mut Vec<Vec<[f32; 2]>>, last: [f32; 2]) -> &mut Vec<[f32; 2]> {
    if out.is_empty() {
        out.push(vec![last]);
    }
    out.last_mut().expect("pushed above")
}

fn steps(length: f32, tolerance: f32) -> usize {
    ((length / (tolerance * 8.0).max(0.5)).sqrt().ceil() as usize).clamp(2, 64)
}

pub fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

pub fn lerp(a: [f32; 2], b: [f32; 2], t: f32) -> [f32; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

pub fn cubic_point(p0: [f32; 2], p1: [f32; 2], p2: [f32; 2], p3: [f32; 2], t: f32) -> [f32; 2] {
    let u = 1.0 - t;
    let a = u * u * u;
    let b = 3.0 * u * u * t;
    let c = 3.0 * u * t * t;
    let d = t * t * t;
    [
        a * p0[0] + b * p1[0] + c * p2[0] + d * p3[0],
        a * p0[1] + b * p1[1] + c * p2[1] + d * p3[1],
    ]
}

/// Parameter along `p→q` where it crosses segment `a→b`.
fn segment_hit(p: [f32; 2], q: [f32; 2], a: [f32; 2], b: [f32; 2]) -> Option<f32> {
    let r = [q[0] - p[0], q[1] - p[1]];
    let s = [b[0] - a[0], b[1] - a[1]];
    let denom = r[0] * s[1] - r[1] * s[0];
    if denom.abs() < 1e-9 {
        return None;
    }
    let ap = [a[0] - p[0], a[1] - p[1]];
    let t = (ap[0] * s[1] - ap[1] * s[0]) / denom;
    let u = (ap[0] * r[1] - ap[1] * r[0]) / denom;
    ((0.0..=1.0).contains(&t) && (-1e-4..=1.0 + 1e-4).contains(&u)).then_some(t)
}

/// An orthogonal polyline with each bend rounded by up to `radius`, never
/// more than half of either neighbouring segment.
pub fn rounded_polyline(points: &[[f32; 2]], radius: f32) -> PathData {
    let points = dedup(points);
    let mut path = PathData::new();
    let Some(&first) = points.first() else {
        return path;
    };
    path.move_to(first);
    for i in 1..points.len() {
        let p = points[i];
        if i + 1 == points.len() {
            path.line_to(p);
            break;
        }
        let prev = points[i - 1];
        let next = points[i + 1];
        let d1 = dist(prev, p);
        let d2 = dist(p, next);
        let r = radius.min(d1 / 2.0).min(d2 / 2.0);
        if r < 0.5 || collinear(prev, p, next) {
            path.line_to(p);
            continue;
        }
        let a = lerp(p, prev, r / d1);
        let b = lerp(p, next, r / d2);
        path.line_to(a);
        path.quad_to(p, b);
    }
    path
}

/// A smooth curve through `points` (the uniform B-spline `basis` curve):
/// it starts and ends exactly on the end points.
pub fn basis_curve(points: &[[f32; 2]]) -> PathData {
    let pts = dedup(points);
    if pts.len() < 3 {
        return PathData::polyline(&pts);
    }
    let mix = |a: [f32; 2], wa: f32, b: [f32; 2], wb: f32, c: [f32; 2], wc: f32| {
        let w = wa + wb + wc;
        [
            (a[0] * wa + b[0] * wb + c[0] * wc) / w,
            (a[1] * wa + b[1] * wb + c[1] * wc) / w,
        ]
    };
    let mut path = PathData::new();
    let n = pts.len();
    path.move_to(pts[0]);
    path.line_to(mix(pts[0], 5.0, pts[1], 1.0, pts[1], 0.0));
    for i in 2..n {
        let (a, b, c) = (pts[i - 2], pts[i - 1], pts[i]);
        path.cubic_to(
            mix(a, 2.0, b, 1.0, c, 0.0),
            mix(a, 1.0, b, 2.0, c, 0.0),
            mix(a, 1.0, b, 4.0, c, 1.0),
        );
    }
    let (a, b) = (pts[n - 2], pts[n - 1]);
    path.cubic_to(
        mix(a, 2.0, b, 1.0, b, 0.0),
        mix(a, 1.0, b, 2.0, b, 0.0),
        mix(a, 1.0, b, 5.0, b, 0.0),
    );
    path.line_to(b);
    path
}

pub(crate) fn dedup(points: &[[f32; 2]]) -> Vec<[f32; 2]> {
    let mut out: Vec<[f32; 2]> = Vec::with_capacity(points.len());
    for &p in points {
        if out.last().is_none_or(|&q| dist(p, q) > 0.01) {
            out.push(p);
        }
    }
    // Drop interior points that sit on a straight run.
    let mut i = 1;
    while i + 1 < out.len() {
        if collinear(out[i - 1], out[i], out[i + 1]) {
            out.remove(i);
        } else {
            i += 1;
        }
    }
    out
}

fn collinear(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> bool {
    let cross = (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0]);
    let dot = (b[0] - a[0]) * (c[0] - b[0]) + (b[1] - a[1]) * (c[1] - b[1]);
    cross.abs() < 0.01 * (dist(a, b) + dist(b, c)).max(1.0) && dot >= 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_point_lands_on_the_outline() {
        let path = PathData::rounded_rect(Rect::new(0.0, 0.0, 100.0, 40.0), 8.0);
        let p = path.exit_point([50.0, 20.0], [50.0, 200.0]).unwrap();
        assert!((p[1] - 40.0).abs() < 0.1, "{p:?}");
        let diamond = PathData::polygon(&[[50.0, 0.0], [100.0, 50.0], [50.0, 100.0], [0.0, 50.0]]);
        let q = diamond.exit_point([50.0, 50.0], [200.0, 50.0]).unwrap();
        assert!((q[0] - 100.0).abs() < 0.1);
    }

    #[test]
    fn rounded_polylines_bend_with_quads_and_keep_ends() {
        let path = rounded_polyline(&[[0.0, 0.0], [0.0, 50.0], [40.0, 50.0], [40.0, 100.0]], 8.0);
        assert_eq!(
            path.verbs
                .iter()
                .filter(|v| matches!(v, Verb::Quad(..)))
                .count(),
            2
        );
        assert_eq!(path.verbs.last(), Some(&Verb::Line([40.0, 100.0])));
    }

    #[test]
    fn basis_curves_interpolate_their_ends() {
        let path = basis_curve(&[[0.0, 0.0], [10.0, 30.0], [40.0, 40.0], [50.0, 90.0]]);
        let flat = path.flatten(0.25);
        assert_eq!(flat[0].first(), Some(&[0.0, 0.0]));
        assert_eq!(flat[0].last(), Some(&[50.0, 90.0]));
    }
}
