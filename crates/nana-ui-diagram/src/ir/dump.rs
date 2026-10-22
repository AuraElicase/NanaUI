//! A stable text form of a [`DiagramScene`] for golden tests: one line per
//! item, coordinates rounded to a tenth of a pixel.

use std::fmt::Write;

use super::{Corner, DiagramScene, Geom, Ink, Item, Stroke, StrokeWidth, Tone};
use crate::geom::Verb;

pub fn dump(scene: &DiagramScene) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "size {} {}", n(scene.size[0]), n(scene.size[1]));
    let s = scene.summary;
    let _ = writeln!(
        out,
        "summary nodes={} edges={} groups={}",
        s.nodes, s.edges, s.groups
    );
    for item in &scene.items {
        match item {
            Item::Shape { geom, fill, stroke } => {
                let _ = write!(out, "shape {}", geom_text(geom));
                if let Some(fill) = fill {
                    let _ = write!(out, " fill={}", ink(fill));
                }
                if let Some(stroke) = stroke {
                    let _ = write!(out, " stroke={}", stroke_text(stroke));
                }
                out.push('\n');
            }
            Item::Text {
                rect,
                text,
                font,
                ink: i,
                ..
            } => {
                let _ = writeln!(
                    out,
                    "text {} {} {} {} size={} w={}{} ink={} {:?}",
                    n(rect.x),
                    n(rect.y),
                    n(rect.w),
                    n(rect.h),
                    n(font.size),
                    font.weight,
                    if font.italic { " italic" } else { "" },
                    ink(i),
                    text
                );
            }
            Item::ClipBegin(geom) => {
                let _ = writeln!(out, "clip {}", geom_text(geom));
            }
            Item::ClipEnd => out.push_str("unclip\n"),
        }
    }
    for link in &scene.links {
        let _ = writeln!(
            out,
            "link {} {} {} {} {:?}",
            n(link.rect.x),
            n(link.rect.y),
            n(link.rect.w),
            n(link.rect.h),
            link.href
        );
    }
    out
}

fn n(v: f32) -> String {
    let r = (v * 10.0).round() / 10.0;
    if r == 0.0 { "0".into() } else { format!("{r}") }
}

fn p(pt: [f32; 2]) -> String {
    format!("{},{}", n(pt[0]), n(pt[1]))
}

fn geom_text(geom: &Geom) -> String {
    match geom {
        Geom::Rect { rect, corner } => {
            let c = match corner {
                Corner::None => String::new(),
                Corner::Tier(t) => format!(" r={t:?}"),
                Corner::Px(px) => format!(" r={}", n(*px)),
            };
            format!(
                "rect {} {} {} {}{c}",
                n(rect.x),
                n(rect.y),
                n(rect.w),
                n(rect.h)
            )
        }
        Geom::Ellipse(rect) => format!(
            "ellipse {} {} {} {}",
            n(rect.x),
            n(rect.y),
            n(rect.w),
            n(rect.h)
        ),
        Geom::Path(path) => {
            let mut s = String::from("path");
            for v in &path.verbs {
                match *v {
                    Verb::Move(a) => s += &format!(" M{}", p(a)),
                    Verb::Line(a) => s += &format!(" L{}", p(a)),
                    Verb::Quad(c, a) => s += &format!(" Q{} {}", p(c), p(a)),
                    Verb::Cubic(c1, c2, a) => s += &format!(" C{} {} {}", p(c1), p(c2), p(a)),
                    Verb::Close => s += " Z",
                }
            }
            s
        }
    }
}

fn ink(ink: &Ink) -> String {
    match ink {
        Ink::Role(role) => format!("{role:?}"),
        Ink::Mix(mix) => format!("{mix:?}").replace(' ', ""),
        Ink::Series { index, tone } => format!(
            "series{index}{}",
            match tone {
                Tone::Solid => "",
                Tone::Soft => ".soft",
                Tone::On => ".on",
            }
        ),
        Ink::Rgba(c) => format!(
            "rgba({},{},{},{})",
            n(c[0] * 255.0),
            n(c[1] * 255.0),
            n(c[2] * 255.0),
            n(c[3])
        ),
    }
}

fn stroke_text(stroke: &Stroke) -> String {
    let w = match stroke.width {
        StrokeWidth::Hairline => "hairline".to_string(),
        StrokeWidth::Thick => "thick".to_string(),
        StrokeWidth::Px(px) => n(px),
    };
    format!("{}/{w}/{:?}", ink(&stroke.ink), stroke.dash)
}
