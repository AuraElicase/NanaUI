//! Line-end markers drawn as inline shapes, so a theme switch recolours them
//! with the line and the line can stop short of a filled head.

use std::sync::Arc;

use super::path::PathData;
use crate::ir::{Geom, Ink, Item, Rect, Stroke};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Marker {
    /// A filled triangle.
    Arrow,
    /// Two strokes meeting at the tip.
    OpenArrow,
    /// A hollow triangle: UML inheritance and realisation.
    Triangle,
    /// A hollow circle at the end.
    Circle,
    /// A cross at the end.
    Cross,
    /// A filled diamond: composition.
    Diamond,
    /// A hollow diamond: aggregation.
    DiamondHollow,
    /// A hollow circle on a short stem: a provided interface.
    Lollipop,
    /// A hollow circle with a plus: UML containment (requirement `contains`).
    Containment,
    /// ER cardinalities, read from the entity outward.
    ErExactlyOne,
    ErZeroOrOne,
    ErOneOrMore,
    ErZeroOrMore,
}

/// A marker placed at the end of a line.
pub struct Placed {
    pub items: Vec<Item>,
    /// How far before the tip the line itself should stop.
    pub inset: f32,
}

impl Marker {
    /// The marker at `tip`, with `dir` the unit direction the line travels
    /// into the tip. `size` is the head length; `stroke` colours it and
    /// `background` fills hollow heads.
    pub fn place(
        self,
        tip: [f32; 2],
        dir: [f32; 2],
        size: f32,
        stroke: Stroke,
        background: Ink,
    ) -> Placed {
        let n = [-dir[1], dir[0]];
        let at = |along: f32, across: f32| {
            [
                tip[0] - dir[0] * along + n[0] * across,
                tip[1] - dir[1] * along + n[1] * across,
            ]
        };
        let solid = Stroke {
            dash: crate::ir::Dash::Solid,
            ..stroke
        };
        let shape = |pts: &[[f32; 2]], fill: Option<Ink>| Item::Shape {
            geom: Geom::Path(Arc::new(PathData::polygon(pts))),
            fill,
            stroke: Some(solid),
        };
        let lines = |segments: &[[[f32; 2]; 2]]| {
            let mut path = PathData::new();
            for [a, b] in segments {
                path.move_to(*a).line_to(*b);
            }
            Item::Shape {
                geom: Geom::path(path),
                fill: None,
                stroke: Some(solid),
            }
        };
        let circle = |center: [f32; 2], r: f32| Item::Shape {
            geom: Geom::Ellipse(Rect::new(center[0] - r, center[1] - r, 2.0 * r, 2.0 * r)),
            fill: Some(background),
            stroke: Some(solid),
        };
        let l = size;
        let w = size * 0.42;
        match self {
            Self::Arrow => Placed {
                items: vec![shape(
                    &[tip, at(l, w), at(l * 0.82, 0.0), at(l, -w)],
                    Some(stroke.ink),
                )],
                inset: l * 0.8,
            },
            Self::OpenArrow => Placed {
                items: vec![lines(&[[at(l, w), tip], [tip, at(l, -w)]])],
                inset: 0.5,
            },
            Self::Triangle => Placed {
                items: vec![shape(
                    &[tip, at(l * 1.15, w * 1.2), at(l * 1.15, -w * 1.2)],
                    Some(background),
                )],
                inset: l * 1.15,
            },
            Self::Circle => {
                let r = l * 0.4;
                Placed {
                    items: vec![circle(at(r, 0.0), r)],
                    inset: 2.0 * r,
                }
            }
            Self::Cross => {
                let c = at(l * 0.5, 0.0);
                let k = l * 0.38;
                let p =
                    |a: f32, b: f32| [c[0] + dir[0] * a + n[0] * b, c[1] + dir[1] * a + n[1] * b];
                Placed {
                    items: vec![lines(&[[p(-k, -k), p(k, k)], [p(-k, k), p(k, -k)]])],
                    inset: l * 0.5,
                }
            }
            Self::Diamond | Self::DiamondHollow => {
                let fill = if self == Self::Diamond {
                    stroke.ink
                } else {
                    background
                };
                Placed {
                    items: vec![shape(
                        &[tip, at(l, w), at(2.0 * l, 0.0), at(l, -w)],
                        Some(fill),
                    )],
                    inset: 2.0 * l,
                }
            }
            Self::Lollipop => {
                let r = l * 0.45;
                Placed {
                    items: vec![circle(at(r, 0.0), r)],
                    inset: 2.0 * r,
                }
            }
            Self::Containment => {
                let r = l * 0.5;
                let c = at(r, 0.0);
                let p =
                    |a: f32, b: f32| [c[0] + dir[0] * a + n[0] * b, c[1] + dir[1] * a + n[1] * b];
                Placed {
                    items: vec![
                        circle(c, r),
                        lines(&[[p(-r, 0.0), p(r, 0.0)], [p(0.0, -r), p(0.0, r)]]),
                    ],
                    inset: 2.0 * r,
                }
            }
            Self::ErExactlyOne => Placed {
                items: vec![lines(&[
                    [at(l * 0.7, w), at(l * 0.7, -w)],
                    [at(l * 1.2, w), at(l * 1.2, -w)],
                ])],
                inset: 0.0,
            },
            Self::ErZeroOrOne => {
                let r = l * 0.32;
                Placed {
                    items: vec![
                        lines(&[[at(l * 0.7, w), at(l * 0.7, -w)]]),
                        circle(at(l * 1.2 + r, 0.0), r),
                    ],
                    inset: 0.0,
                }
            }
            Self::ErOneOrMore => Placed {
                items: vec![lines(&[
                    [at(l, 0.0), at(0.0, w * 1.3)],
                    [at(l, 0.0), at(0.0, -w * 1.3)],
                    [at(l * 1.3, w), at(l * 1.3, -w)],
                ])],
                inset: 0.0,
            },
            Self::ErZeroOrMore => {
                let r = l * 0.32;
                Placed {
                    items: vec![
                        lines(&[
                            [at(l, 0.0), at(0.0, w * 1.3)],
                            [at(l, 0.0), at(0.0, -w * 1.3)],
                        ]),
                        circle(at(l * 1.3 + r, 0.0), r),
                    ],
                    inset: 0.0,
                }
            }
        }
    }
}
