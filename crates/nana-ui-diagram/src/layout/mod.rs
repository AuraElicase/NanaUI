//! Layout machinery shared by the families.

pub mod axis;
pub mod graph;
pub mod layered;
pub mod squarify;
pub mod tree;

pub use graph::{
    ClusterSpec, Direction, EdgeSpec, End, Graph, GraphLayout, NodeSpec, Route, RouteKind, Spacing,
};

use crate::geom::Shape;
use crate::geom::path::dist;
use crate::ir::Rect;

/// The point halfway along a polyline.
pub fn midpoint(points: &[[f32; 2]]) -> [f32; 2] {
    let total: f32 = points.windows(2).map(|w| dist(w[0], w[1])).sum();
    let mut left = total / 2.0;
    for w in points.windows(2) {
        let d = dist(w[0], w[1]);
        if d >= left && d > 0.0 {
            let t = left / d;
            return [
                w[0][0] + (w[1][0] - w[0][0]) * t,
                w[0][1] + (w[1][1] - w[0][1]) * t,
            ];
        }
        left -= d;
    }
    points.first().copied().unwrap_or_default()
}

/// Move a route's end onto a shape's outline along its last segment, so
/// edges meet diamonds, circles and slanted sides exactly.
pub fn attach_end(points: &mut [[f32; 2]], shape: Shape, rect: Rect) {
    let n = points.len();
    if n < 2 || shape == Shape::Text {
        return;
    }
    let (outside, end) = (points[n - 2], points[n - 1]);
    let d = dist(outside, end);
    if d < 1e-3 {
        return;
    }
    let dir = [(end[0] - outside[0]) / d, (end[1] - outside[1]) / d];
    let reach = rect.w + rect.h;
    let beyond = [end[0] + dir[0] * reach, end[1] + dir[1] * reach];
    if let Some(p) = shape.outline_path(rect).exit_point(beyond, outside) {
        points[n - 1] = p;
    }
}

/// [`attach_end`] for the first point.
pub fn attach_start(points: &mut [[f32; 2]], shape: Shape, rect: Rect) {
    points.reverse();
    attach_end(points, shape, rect);
    points.reverse();
}
