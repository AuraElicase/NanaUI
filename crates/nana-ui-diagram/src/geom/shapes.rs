//! Node shapes: the classic flowchart brackets and the Mermaid v11
//! `@{ shape: ... }` table. Every shape knows how big it must be to hold a
//! label, its outline, the extra strokes that decorate it and where an edge
//! meets it.

use nana_ui_core::RadiusTier;

use super::path::{PathData, dist};
use crate::ir::{Corner, Geom, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Shape {
    Rect,
    Rounded,
    Stadium,
    Subroutine,
    Cylinder,
    Circle,
    DoubleCircle,
    Odd,
    Diamond,
    Hexagon,
    LeanRight,
    LeanLeft,
    TrapezoidBottom,
    TrapezoidTop,
    Text,
    NotchRect,
    LinedRect,
    SmallCircle,
    FramedCircle,
    FilledCircle,
    Fork,
    Hourglass,
    BraceLeft,
    BraceRight,
    Braces,
    Bolt,
    Document,
    Delay,
    HorizontalCylinder,
    LinedCylinder,
    CurvedTrapezoid,
    DividedRect,
    Triangle,
    WindowPane,
    Documents,
    StackedRect,
    Flag,
    LinedDocument,
    TaggedDocument,
    TaggedRect,
    BowTieRect,
    FlippedTriangle,
    SlopedRect,
    CrossedCircle,
    NotchedPentagon,
}

impl Shape {
    /// The shape a v11 `shape:` name or alias selects.
    pub fn from_name(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        Some(match name.as_str() {
            "rect" | "rectangle" | "proc" | "process" | "square" => Self::Rect,
            "rounded" | "event" | "round" => Self::Rounded,
            "stadium" | "pill" | "terminal" => Self::Stadium,
            "fr-rect" | "subproc" | "subprocess" | "subroutine" | "framed-rectangle" => {
                Self::Subroutine
            }
            "cyl" | "db" | "database" | "cylinder" => Self::Cylinder,
            "circle" | "circ" => Self::Circle,
            "dbl-circ" | "double-circle" | "doublecircle" => Self::DoubleCircle,
            "odd" | "asymmetric" => Self::Odd,
            "diam" | "diamond" | "decision" | "question" | "rhombus" => Self::Diamond,
            "hex" | "hexagon" | "prepare" => Self::Hexagon,
            "lean-r" | "lean-right" | "in-out" => Self::LeanRight,
            "lean-l" | "lean-left" | "out-in" => Self::LeanLeft,
            "trap-b" | "trapezoid-bottom" | "priority" | "trapezoid" => Self::TrapezoidBottom,
            "trap-t" | "trapezoid-top" | "manual" | "inv-trapezoid" | "inv_trapezoid" => {
                Self::TrapezoidTop
            }
            "text" => Self::Text,
            "notch-rect" | "card" | "notched-rectangle" => Self::NotchRect,
            "lin-rect" | "lined-rectangle" | "lin-proc" | "lined-process" | "shaded-process" => {
                Self::LinedRect
            }
            "sm-circ" | "small-circle" | "start" => Self::SmallCircle,
            "fr-circ" | "framed-circle" | "stop" => Self::FramedCircle,
            "f-circ" | "filled-circle" | "junction" => Self::FilledCircle,
            "fork" | "join" => Self::Fork,
            "hourglass" | "collate" => Self::Hourglass,
            "comment" | "brace" | "brace-l" => Self::BraceLeft,
            "brace-r" => Self::BraceRight,
            "braces" => Self::Braces,
            "bolt" | "com-link" | "lightning-bolt" => Self::Bolt,
            "doc" | "document" => Self::Document,
            "delay" | "half-rounded-rectangle" => Self::Delay,
            "h-cyl" | "das" | "horizontal-cylinder" => Self::HorizontalCylinder,
            "lin-cyl" | "disk" | "lined-cylinder" => Self::LinedCylinder,
            "curv-trap" | "curved-trapezoid" | "display" => Self::CurvedTrapezoid,
            "div-rect" | "div-proc" | "divided-rectangle" | "divided-process" => Self::DividedRect,
            "tri" | "extract" | "triangle" => Self::Triangle,
            "win-pane" | "internal-storage" | "window-pane" => Self::WindowPane,
            "docs" | "documents" | "st-doc" | "stacked-document" => Self::Documents,
            "st-rect" | "procs" | "processes" | "stacked-rectangle" => Self::StackedRect,
            "flag" | "paper-tape" => Self::Flag,
            "lin-doc" | "lined-document" => Self::LinedDocument,
            "tag-doc" | "tagged-document" => Self::TaggedDocument,
            "tag-rect" | "tag-proc" | "tagged-process" | "tagged-rectangle" => Self::TaggedRect,
            "bow-rect" | "stored-data" | "bow-tie-rectangle" => Self::BowTieRect,
            "flip-tri" | "manual-file" | "flipped-triangle" => Self::FlippedTriangle,
            "sl-rect" | "manual-input" | "sloped-rectangle" => Self::SlopedRect,
            "cross-circ" | "summary" | "crossed-circle" => Self::CrossedCircle,
            "notch-pent" | "loop-limit" | "notched-pentagon" => Self::NotchedPentagon,
            _ => return None,
        })
    }

    /// Shapes too small to carry their label inside; it sits below.
    pub fn label_outside(self) -> bool {
        matches!(
            self,
            Self::SmallCircle | Self::FramedCircle | Self::FilledCircle | Self::Fork
        )
    }

    /// The node size that holds a `label` box with the given padding.
    pub fn size(self, label: [f32; 2], pad: [f32; 2]) -> [f32; 2] {
        let [lw, lh] = label;
        let [px, py] = pad;
        let base = [lw + 2.0 * px, lh + 2.0 * py];
        match self {
            Self::Text => [lw.max(1.0), lh.max(1.0)],
            Self::Stadium => [base[0] + base[1] * 0.5, base[1]],
            Self::Subroutine => [base[0] + 16.0, base[1]],
            Self::Cylinder | Self::LinedCylinder => {
                let ry = cyl_ry(base[0]);
                [base[0], base[1] + 2.0 * ry]
            }
            Self::HorizontalCylinder => [base[0] + 2.0 * cyl_ry(base[1]) + 8.0, base[1]],
            Self::Circle | Self::CrossedCircle => {
                let d = (lw.hypot(lh) + py).max(base[1]);
                [d, d]
            }
            Self::DoubleCircle => {
                let d = (lw.hypot(lh) + py).max(base[1]) + 10.0;
                [d, d]
            }
            Self::SmallCircle | Self::FilledCircle => [14.0, 14.0],
            Self::FramedCircle => [18.0, 18.0],
            Self::Fork => [72.0, 10.0],
            Self::Odd => [base[0] + base[1] * 0.25, base[1]],
            Self::Diamond => {
                let w = lw + 2.0 * lh + px;
                let fit = 1.0 - lw / w.max(1.0);
                let b = if fit > 0.01 { lh / (2.0 * fit) } else { lh };
                let h = (2.0 * b + py).max(w * 0.5);
                [w, h]
            }
            Self::Hexagon => [base[0] + base[1] * 0.5, base[1]],
            Self::LeanRight
            | Self::LeanLeft
            | Self::TrapezoidBottom
            | Self::TrapezoidTop
            | Self::CurvedTrapezoid
            | Self::SlopedRect => [base[0] + base[1] * 0.6, base[1]],
            Self::Triangle | Self::FlippedTriangle => {
                let w = (lw + 2.0 * px) * 2.0;
                [w.max(base[1] * 2.0), base[1] * 2.0]
            }
            Self::Hourglass => [base[0].max(base[1]), base[1] * 1.6],
            Self::Document | Self::LinedDocument | Self::TaggedDocument => {
                [base[0], base[1] + base[1] * 0.25]
            }
            Self::Documents => [base[0] + 8.0, base[1] + base[1] * 0.25 + 8.0],
            Self::StackedRect => [base[0] + 8.0, base[1] + 8.0],
            Self::Flag => [base[0], base[1] + base[1] * 0.3],
            Self::Delay => [base[0] + base[1] * 0.5, base[1]],
            Self::WindowPane => [base[0] + 10.0, base[1] + 10.0],
            Self::DividedRect => [base[0], base[1] + 10.0],
            Self::LinedRect => [base[0] + 10.0, base[1]],
            Self::BowTieRect => [base[0] + base[1] * 0.5, base[1]],
            Self::NotchedPentagon => [base[0], base[1] + 10.0],
            Self::BraceLeft | Self::BraceRight => [base[0] + 10.0, base[1]],
            Self::Braces => [base[0] + 20.0, base[1]],
            Self::Bolt => [base[0].max(30.0), base[1].max(40.0)],
            _ => base,
        }
    }

    /// Where the label box sits inside `rect`.
    pub fn label_rect(self, rect: Rect, label: [f32; 2]) -> Rect {
        let c = rect.center();
        let dy = match self {
            Self::Cylinder | Self::LinedCylinder => cyl_ry(rect.w) * 0.5,
            Self::Document | Self::LinedDocument | Self::TaggedDocument | Self::Flag => {
                -rect.h * 0.06
            }
            Self::Documents => -rect.h * 0.06 + 4.0,
            Self::StackedRect => 4.0,
            Self::WindowPane | Self::DividedRect | Self::NotchedPentagon => 5.0,
            Self::Triangle => rect.h * 0.2,
            Self::FlippedTriangle => -rect.h * 0.2,
            _ => 0.0,
        };
        let dx = match self {
            Self::WindowPane => 5.0,
            Self::LinedRect => 5.0,
            Self::Documents | Self::StackedRect => -4.0,
            Self::HorizontalCylinder => -cyl_ry(rect.h) * 0.5,
            _ => 0.0,
        };
        if self.label_outside() {
            return Rect::new(
                c[0] - label[0] / 2.0,
                rect.bottom() + 4.0,
                label[0],
                label[1],
            );
        }
        Rect::centered([c[0] + dx, c[1] + dy], label)
    }

    /// The filled outline. Rect-like shapes stay [`Geom::Rect`] so the scene
    /// draws them as rounded quads.
    pub fn outline(self, r: Rect) -> Geom {
        let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
        let (w, h) = (r.w, r.h);
        let poly = |pts: &[[f32; 2]]| Geom::path(PathData::polygon(pts));
        match self {
            Self::Rect
            | Self::LinedRect
            | Self::DividedRect
            | Self::WindowPane
            | Self::Subroutine
            | Self::TaggedRect => Geom::Rect {
                rect: r,
                corner: Corner::Tier(RadiusTier::Xs),
            },
            Self::Rounded => Geom::Rect {
                rect: r,
                corner: Corner::Tier(RadiusTier::Md),
            },
            Self::Stadium => Geom::Rect {
                rect: r,
                corner: Corner::Tier(RadiusTier::Full),
            },
            Self::Text => Geom::rect(r),
            Self::Circle
            | Self::DoubleCircle
            | Self::SmallCircle
            | Self::FramedCircle
            | Self::FilledCircle
            | Self::CrossedCircle => Geom::Ellipse(r),
            Self::Fork => Geom::Rect {
                rect: r,
                corner: Corner::Tier(RadiusTier::Xs),
            },
            Self::Cylinder | Self::LinedCylinder => {
                let ry = cyl_ry(w);
                let rx = w / 2.0;
                let mut p = PathData::new();
                p.move_to([x0, y0 + ry]);
                ellipse_arc(
                    &mut p,
                    [x0 + rx, y0 + ry],
                    rx,
                    ry,
                    std::f32::consts::PI,
                    std::f32::consts::PI,
                );
                p.line_to([x1, y1 - ry]);
                ellipse_arc(
                    &mut p,
                    [x0 + rx, y1 - ry],
                    rx,
                    ry,
                    0.0,
                    std::f32::consts::PI,
                );
                p.close();
                Geom::path(p)
            }
            Self::HorizontalCylinder => {
                let rx = cyl_ry(h);
                let ry = h / 2.0;
                let mut p = PathData::new();
                p.move_to([x0 + rx, y0]);
                p.line_to([x1 - rx, y0]);
                ellipse_arc(
                    &mut p,
                    [x1 - rx, y0 + ry],
                    rx,
                    ry,
                    -std::f32::consts::FRAC_PI_2,
                    std::f32::consts::PI,
                );
                p.line_to([x0 + rx, y1]);
                ellipse_arc(
                    &mut p,
                    [x0 + rx, y0 + ry],
                    rx,
                    ry,
                    std::f32::consts::FRAC_PI_2,
                    std::f32::consts::PI,
                );
                p.close();
                Geom::path(p)
            }
            Self::Odd => {
                let n = h * 0.25;
                poly(&[
                    [x0, y0],
                    [x1, y0],
                    [x1, y1],
                    [x0, y1],
                    [x0 + n, y0 + h / 2.0],
                ])
            }
            Self::Diamond => poly(&[
                [x0 + w / 2.0, y0],
                [x1, y0 + h / 2.0],
                [x0 + w / 2.0, y1],
                [x0, y0 + h / 2.0],
            ]),
            Self::Hexagon => {
                let s = h * 0.25;
                poly(&[
                    [x0 + s, y0],
                    [x1 - s, y0],
                    [x1, y0 + h / 2.0],
                    [x1 - s, y1],
                    [x0 + s, y1],
                    [x0, y0 + h / 2.0],
                ])
            }
            Self::LeanRight => {
                let s = h * 0.3;
                poly(&[[x0 + s, y0], [x1, y0], [x1 - s, y1], [x0, y1]])
            }
            Self::LeanLeft => {
                let s = h * 0.3;
                poly(&[[x0, y0], [x1 - s, y0], [x1, y1], [x0 + s, y1]])
            }
            Self::TrapezoidBottom => {
                let s = h * 0.3;
                poly(&[[x0 + s, y0], [x1 - s, y0], [x1, y1], [x0, y1]])
            }
            Self::TrapezoidTop => {
                let s = h * 0.3;
                poly(&[[x0, y0], [x1, y0], [x1 - s, y1], [x0 + s, y1]])
            }
            Self::SlopedRect => {
                let s = h * 0.3;
                poly(&[[x0, y0 + s], [x1, y0], [x1, y1], [x0, y1]])
            }
            Self::NotchRect => {
                let n = (h * 0.3).min(12.0);
                poly(&[[x0 + n, y0], [x1, y0], [x1, y1], [x0, y1], [x0, y0 + n]])
            }
            Self::Triangle => poly(&[[x0 + w / 2.0, y0], [x1, y1], [x0, y1]]),
            Self::FlippedTriangle => poly(&[[x0, y0], [x1, y0], [x0 + w / 2.0, y1]]),
            Self::Hourglass => poly(&[[x0, y0], [x1, y0], [x0, y1], [x1, y1]]),
            Self::BowTieRect => {
                let s = h * 0.25;
                let mut p = PathData::new();
                p.move_to([x0 + s, y0]).line_to([x1 - s, y0]);
                p.quad_to([x1 - 2.0 * s, y0 + h / 2.0], [x1 - s, y1]);
                p.line_to([x0 + s, y1]);
                p.quad_to([x0, y0 + h / 2.0], [x0 + s, y0]);
                p.close();
                Geom::path(p)
            }
            Self::Delay => {
                let rr = h / 2.0;
                let mut p = PathData::new();
                p.move_to([x0, y0]).line_to([x1 - rr, y0]);
                p.arc(
                    [x1 - rr, y0 + rr],
                    rr,
                    -std::f32::consts::FRAC_PI_2,
                    std::f32::consts::PI,
                    true,
                );
                p.line_to([x0, y1]).close();
                Geom::path(p)
            }
            Self::CurvedTrapezoid => {
                let s = h * 0.3;
                let rr = h / 2.0;
                let mut p = PathData::new();
                p.move_to([x0 + s, y0]).line_to([x1 - rr, y0]);
                p.arc(
                    [x1 - rr, y0 + rr],
                    rr,
                    -std::f32::consts::FRAC_PI_2,
                    std::f32::consts::PI,
                    true,
                );
                p.line_to([x0 + s, y1]).line_to([x0, y0 + h / 2.0]).close();
                Geom::path(p)
            }
            Self::Document | Self::LinedDocument | Self::TaggedDocument => {
                Geom::path(wave_rect(Rect::new(x0, y0, w, h)))
            }
            Self::Documents => {
                let front = Rect::new(x0, y0 + 8.0, w - 8.0, h - 8.0);
                Geom::path(wave_rect(front))
            }
            Self::StackedRect => Geom::Rect {
                rect: Rect::new(x0, y0 + 8.0, w - 8.0, h - 8.0),
                corner: Corner::Tier(RadiusTier::Xs),
            },
            Self::Flag => {
                let a = h * 0.12;
                let mut p = PathData::new();
                p.move_to([x0, y0 + a]);
                p.cubic_to(
                    [x0 + w * 0.25, y0 - a],
                    [x0 + w * 0.75, y0 + 3.0 * a],
                    [x1, y0 + a],
                );
                p.line_to([x1, y1 - a]);
                p.cubic_to(
                    [x0 + w * 0.75, y1 + a],
                    [x0 + w * 0.25, y1 - 3.0 * a],
                    [x0, y1 - a],
                );
                p.close();
                Geom::path(p)
            }
            Self::NotchedPentagon => {
                let n = 10.0f32.min(w / 4.0);
                poly(&[
                    [x0 + n, y0],
                    [x1 - n, y0],
                    [x1, y0 + n],
                    [x1, y1],
                    [x0, y1],
                    [x0, y0 + n],
                ])
            }
            Self::BraceLeft | Self::BraceRight | Self::Braces => Geom::rect(r),
            Self::Bolt => poly(&[
                [x0 + w * 0.55, y0],
                [x0 + w * 0.15, y0 + h * 0.55],
                [x0 + w * 0.45, y0 + h * 0.55],
                [x0 + w * 0.35, y1],
                [x0 + w * 0.85, y0 + h * 0.4],
                [x0 + w * 0.55, y0 + h * 0.4],
            ]),
        }
    }

    /// Whether the outline is filled and stroked (braces and text only
    /// draw decorations).
    pub fn has_body(self) -> bool {
        !matches!(
            self,
            Self::Text | Self::BraceLeft | Self::BraceRight | Self::Braces
        )
    }

    /// The filled dot shapes take the stroke colour as their fill.
    pub fn solid(self) -> bool {
        matches!(self, Self::FilledCircle | Self::Fork)
    }

    /// Stroke-only decorations drawn over the body: back sheets first.
    pub fn decorations(self, r: Rect) -> (Vec<Geom>, Vec<PathData>) {
        let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
        let (w, h) = (r.w, r.h);
        let mut behind = Vec::new();
        let mut over = Vec::new();
        match self {
            Self::Subroutine => {
                over.push(PathData::polyline(&[[x0 + 8.0, y0], [x0 + 8.0, y1]]));
                over.push(PathData::polyline(&[[x1 - 8.0, y0], [x1 - 8.0, y1]]));
            }
            Self::LinedRect => over.push(PathData::polyline(&[[x0 + 10.0, y0], [x0 + 10.0, y1]])),
            Self::DividedRect => over.push(PathData::polyline(&[[x0, y0 + 10.0], [x1, y0 + 10.0]])),
            Self::WindowPane => {
                over.push(PathData::polyline(&[[x0 + 10.0, y0], [x0 + 10.0, y1]]));
                over.push(PathData::polyline(&[[x0, y0 + 10.0], [x1, y0 + 10.0]]));
            }
            Self::Cylinder => {
                let ry = cyl_ry(w);
                let mut p = PathData::new();
                p.move_to([x0, y0 + ry]);
                ellipse_arc(
                    &mut p,
                    [x0 + w / 2.0, y0 + ry],
                    w / 2.0,
                    ry,
                    std::f32::consts::PI,
                    -std::f32::consts::PI,
                );
                over.push(p);
            }
            Self::LinedCylinder => {
                let ry = cyl_ry(w);
                for dy in [0.0, 6.0] {
                    let mut p = PathData::new();
                    p.move_to([x0, y0 + ry + dy]);
                    ellipse_arc(
                        &mut p,
                        [x0 + w / 2.0, y0 + ry + dy],
                        w / 2.0,
                        ry,
                        std::f32::consts::PI,
                        -std::f32::consts::PI,
                    );
                    over.push(p);
                }
            }
            Self::HorizontalCylinder => {
                let rx = cyl_ry(h);
                let mut p = PathData::new();
                p.move_to([x1 - rx, y0]);
                ellipse_arc(
                    &mut p,
                    [x1 - rx, y0 + h / 2.0],
                    rx,
                    h / 2.0,
                    -std::f32::consts::FRAC_PI_2,
                    -std::f32::consts::PI,
                );
                over.push(p);
            }
            Self::DoubleCircle => over.push(PathData::ellipse(r.inset(5.0, 5.0))),
            Self::FramedCircle => over.push(PathData::ellipse(r.inset(4.0, 4.0))),
            Self::CrossedCircle => {
                let c = r.center();
                let k = w / 2.0 * std::f32::consts::FRAC_1_SQRT_2;
                over.push(PathData::polyline(&[
                    [c[0] - k, c[1] - k],
                    [c[0] + k, c[1] + k],
                ]));
                over.push(PathData::polyline(&[
                    [c[0] + k, c[1] - k],
                    [c[0] - k, c[1] + k],
                ]));
            }
            Self::LinedDocument => over.push(PathData::polyline(&[
                [x0 + 10.0, y0],
                [x0 + 10.0, y1 - h * 0.12],
            ])),
            Self::TaggedDocument | Self::TaggedRect => {
                let t = (h * 0.25).min(12.0);
                let base_y = if self == Self::TaggedDocument {
                    y1 - h * 0.16
                } else {
                    y1
                };
                over.push(PathData::polyline(&[
                    [x1 - 2.0 * t, base_y],
                    [x1, base_y - t],
                ]));
            }
            Self::Documents => {
                behind.push(Geom::path(wave_rect(Rect::new(
                    x0 + 8.0,
                    y0,
                    w - 8.0,
                    h - 8.0,
                ))));
                behind.push(Geom::path(wave_rect(Rect::new(
                    x0 + 4.0,
                    y0 + 4.0,
                    w - 8.0,
                    h - 8.0,
                ))));
            }
            Self::StackedRect => {
                behind.push(Geom::Rect {
                    rect: Rect::new(x0 + 8.0, y0, w - 8.0, h - 8.0),
                    corner: Corner::Tier(RadiusTier::Xs),
                });
                behind.push(Geom::Rect {
                    rect: Rect::new(x0 + 4.0, y0 + 4.0, w - 8.0, h - 8.0),
                    corner: Corner::Tier(RadiusTier::Xs),
                });
            }
            Self::BraceLeft | Self::Braces => {
                over.push(brace([x0 + 8.0, y0], [x0 + 8.0, y1], -6.0))
            }
            _ => {}
        }
        if matches!(self, Self::BraceRight | Self::Braces) {
            over.push(brace([x1 - 8.0, y0], [x1 - 8.0, y1], 6.0));
        }
        (behind, over)
    }

    /// The outline as a path, for intersection tests.
    pub fn outline_path(self, r: Rect) -> PathData {
        match self.outline(r) {
            Geom::Rect { rect, corner } => match corner {
                Corner::Tier(RadiusTier::Full) => {
                    PathData::rounded_rect(rect, rect.w.min(rect.h) / 2.0)
                }
                _ => PathData::rect(rect),
            },
            Geom::Ellipse(rect) => PathData::ellipse(rect),
            Geom::Path(p) => (*p).clone(),
        }
    }

    /// The point on the outline where a line from the centre toward
    /// `toward` leaves the shape.
    pub fn boundary(self, r: Rect, toward: [f32; 2]) -> [f32; 2] {
        let c = r.center();
        if dist(c, toward) < 0.01 {
            return c;
        }
        // A ray long enough to leave any shape.
        let reach = (r.w + r.h) * 2.0 + dist(c, toward);
        let d = dist(c, toward);
        let far = [
            c[0] + (toward[0] - c[0]) / d * reach,
            c[1] + (toward[1] - c[1]) / d * reach,
        ];
        let path = self.outline_path(r);
        path.exit_point(c, far).unwrap_or(c)
    }
}

/// Vertical radius of a cylinder's end caps.
fn cyl_ry(width: f32) -> f32 {
    (width * 0.08).clamp(4.0, 10.0)
}

/// An elliptical arc as cubics, continuing the current subpath.
fn ellipse_arc(p: &mut PathData, c: [f32; 2], rx: f32, ry: f32, start: f32, sweep: f32) {
    let segments = ((sweep.abs() / std::f32::consts::FRAC_PI_2).ceil() as usize).max(1);
    let step = sweep / segments as f32;
    let k = 4.0 / 3.0 * (step / 4.0).tan();
    let mut a = start;
    for _ in 0..segments {
        let b = a + step;
        let p0 = [c[0] + rx * a.cos(), c[1] + ry * a.sin()];
        let p3 = [c[0] + rx * b.cos(), c[1] + ry * b.sin()];
        let c1 = [p0[0] - k * rx * a.sin(), p0[1] + k * ry * a.cos()];
        let c2 = [p3[0] + k * rx * b.sin(), p3[1] - k * ry * b.cos()];
        if p.is_empty() {
            p.move_to(p0);
        }
        p.cubic_to(c1, c2, p3);
        a = b;
    }
}

/// A rectangle whose bottom edge is a wave: the document symbol.
fn wave_rect(r: Rect) -> PathData {
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let a = r.h * 0.12;
    let mut p = PathData::new();
    p.move_to([x0, y0]).line_to([x1, y0]).line_to([x1, y1 - a]);
    p.cubic_to(
        [x0 + r.w * 0.75, y1 - 3.0 * a],
        [x0 + r.w * 0.25, y1 + a],
        [x0, y1 - a],
    );
    p.close();
    p
}

/// A curly brace from `top` to `bottom`, bulging by `depth` (negative is left).
fn brace(top: [f32; 2], bottom: [f32; 2], depth: f32) -> PathData {
    let mid = (top[1] + bottom[1]) / 2.0;
    let x = top[0];
    let mut p = PathData::new();
    p.move_to([x - depth, top[1]]);
    p.quad_to([x, top[1]], [x, top[1] + 6.0]);
    p.line_to([x, mid - 6.0]);
    p.quad_to([x, mid], [x + depth, mid]);
    p.quad_to([x, mid], [x, mid + 6.0]);
    p.line_to([x, bottom[1] - 6.0]);
    p.quad_to([x, bottom[1]], [x - depth, bottom[1]]);
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diamond_holds_its_label() {
        let label = [100.0, 20.0];
        let [w, h] = Shape::Diamond.size(label, [16.0, 10.0]);
        // The label's corner must sit inside the rhombus.
        assert!(label[0] / w + label[1] / h <= 1.0, "{w}x{h}");
    }

    #[test]
    fn boundary_points_lie_on_each_outline() {
        let r = Rect::new(0.0, 0.0, 80.0, 40.0);
        let p = Shape::Rect.boundary(r, [40.0, 400.0]);
        assert!((p[1] - 40.0).abs() < 0.1);
        let q = Shape::Circle.boundary(Rect::new(0.0, 0.0, 40.0, 40.0), [200.0, 20.0]);
        assert!((q[0] - 40.0).abs() < 0.2, "{q:?}");
        let d = Shape::Diamond.boundary(r, [40.0, -100.0]);
        assert!(d[1].abs() < 0.1);
    }

    #[test]
    fn every_v11_name_resolves() {
        for name in [
            "rect",
            "rounded",
            "stadium",
            "fr-rect",
            "cyl",
            "circle",
            "dbl-circ",
            "odd",
            "diam",
            "hex",
            "lean-r",
            "lean-l",
            "trap-b",
            "trap-t",
            "text",
            "notch-rect",
            "lin-rect",
            "sm-circ",
            "fr-circ",
            "f-circ",
            "fork",
            "hourglass",
            "comment",
            "brace-r",
            "braces",
            "bolt",
            "doc",
            "delay",
            "h-cyl",
            "lin-cyl",
            "curv-trap",
            "div-rect",
            "tri",
            "win-pane",
            "docs",
            "st-rect",
            "flag",
            "lin-doc",
            "tag-doc",
            "tag-rect",
            "bow-rect",
            "flip-tri",
            "sl-rect",
            "cross-circ",
            "notch-pent",
        ] {
            assert!(Shape::from_name(name).is_some(), "{name}");
        }
    }
}
