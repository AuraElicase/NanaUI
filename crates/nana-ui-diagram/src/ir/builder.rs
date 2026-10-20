//! Helpers every family uses to emit display-list items.

use std::sync::Arc;

use super::{Align, DiagramScene, Geom, Ink, Item, Link, Rect, Stroke};
use crate::geom::{Marker, PathData};
use crate::style;
use crate::text::LaidText;

#[derive(Debug, Default)]
pub struct SceneBuilder {
    items: Vec<Item>,
    links: Vec<Link>,
}

impl SceneBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn push(&mut self, item: Item) {
        self.items.push(item);
    }

    pub fn shape(&mut self, geom: Geom, fill: Option<Ink>, stroke: Option<Stroke>) {
        if fill.is_none() && stroke.is_none() {
            return;
        }
        self.items.push(Item::Shape { geom, fill, stroke });
    }

    pub fn fill(&mut self, geom: Geom, ink: Ink) {
        self.shape(geom, Some(ink), None);
    }

    pub fn stroke(&mut self, geom: Geom, stroke: Stroke) {
        self.shape(geom, None, Some(stroke));
    }

    pub fn clip(&mut self, geom: Geom) {
        self.items.push(Item::ClipBegin(geom));
    }

    pub fn unclip(&mut self) {
        self.items.push(Item::ClipEnd);
    }

    /// Draw `text` with its top-left at `origin`, each line aligned inside
    /// `width`.
    pub fn text(&mut self, text: &LaidText, origin: [f32; 2], width: f32, align: Align, ink: Ink) {
        let mut y = origin[1];
        for line in &text.lines {
            let offset = match align {
                Align::Start => 0.0,
                Align::Center => (width - line.width) / 2.0,
                Align::End => width - line.width,
            };
            for run in &line.runs {
                self.items.push(Item::Text {
                    rect: Rect::new(origin[0] + offset + run.x, y, run.width, text.line_height),
                    text: run.text.clone(),
                    font: run.font,
                    ink,
                    align: Align::Start,
                });
            }
            y += text.line_height;
        }
    }

    /// Draw `text` centred on `center`.
    pub fn text_centered(&mut self, text: &LaidText, center: [f32; 2], ink: Ink) {
        let origin = [center[0] - text.width / 2.0, center[1] - text.height / 2.0];
        self.text(text, origin, text.width, Align::Center, ink);
    }

    /// An edge label on a surface chip, centred on `center`. Returns the
    /// chip's rectangle.
    pub fn chip(&mut self, text: &LaidText, center: [f32; 2], pad: [f32; 2]) -> Rect {
        let rect = Rect::centered(
            center,
            [text.width + 2.0 * pad[0], text.height + 2.0 * pad[1]],
        );
        self.shape(
            Geom::rounded(rect, style::CHIP_CORNER),
            Some(style::CHIP_FILL),
            Some(Stroke::hairline(style::CHIP_STROKE)),
        );
        self.text_centered(text, center, style::MUTED);
        rect
    }

    /// A line through `points` with optional end markers. The path follows
    /// `shape` (already built from the same points); markers sit at the
    /// first and last points and the line stops short of filled heads.
    pub fn edge(
        &mut self,
        points: &[[f32; 2]],
        build: impl Fn(&[[f32; 2]]) -> PathData,
        stroke: Stroke,
        start: Option<Marker>,
        end: Option<Marker>,
        marker_size: f32,
    ) {
        if points.len() < 2 {
            return;
        }
        let mut pts = points.to_vec();
        let mut heads = Vec::new();
        if let Some(marker) = end {
            let n = pts.len();
            let dir = unit(pts[n - 2], pts[n - 1]);
            let placed = marker.place(pts[n - 1], dir, marker_size, stroke, style::NODE_FILL);
            shorten_end(&mut pts, placed.inset);
            heads.extend(placed.items);
        }
        if let Some(marker) = start {
            let dir = unit(pts[1], pts[0]);
            let placed = marker.place(pts[0], dir, marker_size, stroke, style::NODE_FILL);
            pts.reverse();
            shorten_end(&mut pts, placed.inset);
            pts.reverse();
            heads.extend(placed.items);
        }
        self.items.push(Item::Shape {
            geom: Geom::Path(Arc::new(build(&pts))),
            fill: None,
            stroke: Some(stroke),
        });
        self.items.extend(heads);
    }

    pub fn link(&mut self, rect: Rect, href: &str, tooltip: Option<&str>) {
        self.links.push(Link {
            rect,
            href: Arc::from(href),
            tooltip: tooltip.map(Arc::from),
        });
    }

    /// Finish with every item inside `[margin, size - margin]`: `content`
    /// is the drawing's bounds before the shift.
    pub fn finish(self, content: Rect, margin: f32) -> DiagramScene {
        let dx = margin - content.x;
        let dy = margin - content.y;
        let items = self
            .items
            .into_iter()
            .map(|item| translate(item, dx, dy))
            .collect();
        let links = self
            .links
            .into_iter()
            .map(|l| Link {
                rect: l.rect.translated(dx, dy),
                ..l
            })
            .collect();
        DiagramScene {
            size: [content.w + 2.0 * margin, content.h + 2.0 * margin],
            items,
            links,
            summary: Default::default(),
        }
    }

    /// The bounds of everything emitted so far.
    pub fn bounds(&self) -> Rect {
        let mut out: Option<Rect> = None;
        for item in &self.items {
            let r = match item {
                Item::Shape { geom, stroke, .. } => {
                    let b = geom.bounds();
                    let pad = if stroke.is_some() { 1.0 } else { 0.0 };
                    Rect::new(b.x - pad, b.y - pad, b.w + 2.0 * pad, b.h + 2.0 * pad)
                }
                Item::Text { rect, .. } => *rect,
                Item::ClipBegin(_) | Item::ClipEnd => continue,
            };
            out = Some(out.map_or(r, |o| o.union(&r)));
        }
        out.unwrap_or_default()
    }
}

fn translate(item: Item, dx: f32, dy: f32) -> Item {
    let geom = |g: Geom| match g {
        Geom::Rect { rect, corner } => Geom::Rect {
            rect: rect.translated(dx, dy),
            corner,
        },
        Geom::Ellipse(rect) => Geom::Ellipse(rect.translated(dx, dy)),
        Geom::Path(p) => Geom::Path(Arc::new(p.translated(dx, dy))),
    };
    match item {
        Item::Shape {
            geom: g,
            fill,
            stroke,
        } => Item::Shape {
            geom: geom(g),
            fill,
            stroke,
        },
        Item::Text {
            rect,
            text,
            font,
            ink,
            align,
        } => Item::Text {
            rect: rect.translated(dx, dy),
            text,
            font,
            ink,
            align,
        },
        Item::ClipBegin(g) => Item::ClipBegin(geom(g)),
        Item::ClipEnd => Item::ClipEnd,
    }
}

pub fn unit(from: [f32; 2], to: [f32; 2]) -> [f32; 2] {
    let d = [to[0] - from[0], to[1] - from[1]];
    let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
    if len < 1e-6 {
        [0.0, 1.0]
    } else {
        [d[0] / len, d[1] / len]
    }
}

/// Pull the last point back along the last segment by `by`.
fn shorten_end(points: &mut [[f32; 2]], by: f32) {
    let n = points.len();
    if n < 2 || by <= 0.0 {
        return;
    }
    let a = points[n - 2];
    let b = points[n - 1];
    let len = crate::geom::path::dist(a, b);
    let t = if len > 1e-6 {
        (by.min(len * 0.9)) / len
    } else {
        0.0
    };
    points[n - 1] = [b[0] - (b[0] - a[0]) * t, b[1] - (b[1] - a[1]) * t];
}
