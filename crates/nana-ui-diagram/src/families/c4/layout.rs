//! C4 drawing: element cards and boundary boxes on row grids, measured
//! bottom-up and placed top-down, then labelled orthogonal connectors.

use nana_ui_core::{RadiusTier, SemanticColorMix, SemanticColorRole as R};

use super::{BoundaryKind, ElemKind, Element, End, Model, Variant};
use crate::families::{LayoutCx, graphkit};
use crate::geom::{Marker, Shape};
use crate::ir::{Align, Corner, Dash, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::layout::grid::{self, Connect, GridItem, GridSpec};
use crate::style;
use crate::text::{Label, LaidText};
use crate::{DiagramError, DiagramScene};

const INTERNAL_FILL: Ink = Ink::Mix(SemanticColorMix::new(R::Accent, R::Surface, 0.14));
const PERSON_FILL: Ink = Ink::Mix(SemanticColorMix::new(R::Accent, R::Surface, 0.22));
const INTERNAL_STROKE: Ink = Ink::Mix(SemanticColorMix::new(R::Accent, R::BorderStrong, 0.5));
const EXTERNAL_FILL: Ink = Ink::Mix(SemanticColorMix::new(R::Muted, R::Surface, 0.14));
const EXTERNAL_STROKE: Ink = Ink::Mix(SemanticColorMix::new(R::Muted, R::BorderStrong, 0.5));

/// The measured text of one card.
struct Card {
    stereotype: LaidText,
    name: LaidText,
    techn: Option<LaidText>,
    descr: Option<LaidText>,
    person: bool,
    content: [f32; 2],
    size: [f32; 2],
    shape: Shape,
}

struct Head {
    label: LaidText,
    kind: Option<LaidText>,
    descr: Option<LaidText>,
    size: [f32; 2],
}

struct Geo {
    cards: Vec<Card>,
    heads: Vec<Head>,
    /// Elements and boundaries directly inside each container (0 is the
    /// diagram, 1 + b boundary b).
    elements: Vec<Vec<usize>>,
    boundaries: Vec<Vec<usize>>,
    boundary_size: Vec<[f32; 2]>,
    element_rect: Vec<Rect>,
    boundary_rect: Vec<Rect>,
    col_gap: f32,
    row_gap: f32,
    pad: f32,
    shape_in_row: usize,
    boundary_in_row: usize,
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let strong = font.weight(m.strong_weight);
    let small = cx.small_font();
    let wrap = cx.wrap_width("c4.wrappingWidth") * 0.9;
    let rel_wrap = wrap * 0.75;
    let line_gap = m.small_size * 0.3;
    let glyph = m.font_size * 2.0;

    let cards: Vec<Card> = model
        .elements
        .iter()
        .map(|e| {
            let stereotype = cx.text.lay_plain(&e.stereotype(), small);
            let name = cx.text.lay(&e.label, strong, Some(wrap));
            let techn = e
                .techn
                .as_ref()
                .map(|t| cx.text.lay(&bracketed(t), small, Some(wrap)));
            let descr = e
                .descr
                .as_ref()
                .map(|d| cx.text.lay(d, small, Some(wrap)))
                .filter(|d| !d.is_empty());
            let person = e.kind == ElemKind::Person;
            let mut w = stereotype.width.max(name.width);
            let mut h = stereotype.height + line_gap + name.height;
            if person {
                h += glyph + line_gap;
            }
            for part in [&techn, &descr].into_iter().flatten() {
                w = w.max(part.width);
                h += line_gap + part.height;
            }
            let shape = match e.variant {
                Variant::Db => Shape::Cylinder,
                Variant::Queue => Shape::HorizontalCylinder,
                Variant::Plain => Shape::Rect,
            };
            let content = [w.max(m.font_size * 6.0), h];
            Card {
                size: shape.size(content, [m.node_pad_x, m.node_pad_y]),
                stereotype,
                name,
                techn,
                descr,
                person,
                content,
                shape,
            }
        })
        .collect();
    let heads: Vec<Head> = model
        .boundaries
        .iter()
        .map(|b| {
            let label = cx.text.lay(&b.label, strong, Some(wrap * 1.5));
            let kind = b
                .type_text
                .as_deref()
                .map(|t| cx.text.lay_plain(&format!("[{t}]"), small));
            let descr = b
                .descr
                .as_ref()
                .map(|d| cx.text.lay(d, small, Some(wrap * 1.5)));
            let mut size = label.size();
            for part in [&kind, &descr].into_iter().flatten() {
                size[0] = size[0].max(part.width);
                size[1] += line_gap + part.height;
            }
            Head {
                label,
                kind,
                descr,
                size,
            }
        })
        .collect();

    // Relationship labels decide how wide the channels must be.
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.2];
    let rel_texts: Vec<(Option<LaidText>, Option<LaidText>)> = model
        .rels
        .iter()
        .map(|r| {
            (
                r.label
                    .as_ref()
                    .map(|l| cx.text.lay(l, small, Some(rel_wrap)))
                    .filter(|l| !l.is_empty()),
                r.techn
                    .as_ref()
                    .map(|t| cx.text.lay(&bracketed(t), small, Some(rel_wrap))),
            )
        })
        .collect();
    let chip_size = |i: usize| -> [f32; 2] {
        let (l, t) = &rel_texts[i];
        let mut s = [0.0f32, 0.0f32];
        for part in [l, t].into_iter().flatten() {
            s[0] = s[0].max(part.width);
            s[1] += part.height;
        }
        [s[0] + 2.0 * chip_pad[0], s[1] + 2.0 * chip_pad[1]]
    };
    let widest = (0..model.rels.len())
        .map(chip_size)
        .fold([0.0f32, 0.0f32], |a, b| [a[0].max(b[0]), a[1].max(b[1])]);
    let containers = model.boundaries.len() + 1;
    let container = |p: Option<usize>| p.map_or(0, |b| b + 1);
    let mut elements = vec![Vec::new(); containers];
    let mut boundaries = vec![Vec::new(); containers];
    for (i, e) in model.elements.iter().enumerate() {
        elements[container(e.parent)].push(i);
    }
    for (i, b) in model.boundaries.iter().enumerate() {
        boundaries[container(b.parent)].push(i);
    }
    let mut geo = Geo {
        cards,
        heads,
        elements,
        boundaries,
        boundary_size: vec![[0.0, 0.0]; model.boundaries.len()],
        element_rect: vec![Rect::default(); model.elements.len()],
        boundary_rect: vec![Rect::default(); model.boundaries.len()],
        col_gap: (widest[0] + m.node_sep * 0.6).clamp(m.node_sep * 1.5, m.node_sep * 5.0),
        row_gap: (widest[1] + m.node_sep * 0.6).clamp(m.node_sep * 1.2, m.node_sep * 4.0),
        pad: m.cluster_pad,
        shape_in_row: model.shape_in_row,
        boundary_in_row: model.boundary_in_row,
    };

    // Boundaries bottom-up: a boundary always comes after its parent.
    for b in (0..model.boundaries.len()).rev() {
        let content = content_size(&geo, b + 1);
        let head = geo.heads[b].size;
        geo.boundary_size[b] = [
            content[0].max(head[0]) + 2.0 * geo.pad,
            head[1] + geo.pad * 0.75 + content[1] + 2.0 * geo.pad,
        ];
    }
    let root = content_size(&geo, 0);
    place(&mut geo, 0, Rect::new(0.0, 0.0, root[0], root[1]));

    let mut out = SceneBuilder::new();

    // Boundaries, outermost first.
    let parents: Vec<Option<usize>> = model.boundaries.iter().map(|b| b.parent).collect();
    for b in graphkit::outermost_first(&parents) {
        let boundary = &model.boundaries[b];
        let rect = geo.boundary_rect[b];
        let node = boundary.kind == BoundaryKind::Node;
        let mut stroke = Stroke::hairline(if node {
            style::NODE_STROKE
        } else {
            style::MUTED
        });
        if !node {
            stroke = stroke.dash(Dash::Dashed);
        }
        if let Some(c) = boundary.colors.border {
            stroke.ink = Ink::Rgba(c);
        }
        let fill = boundary
            .colors
            .bg
            .map(Ink::Rgba)
            .or(node.then_some(style::CLUSTER_FILL));
        out.shape(
            Geom::Rect {
                rect,
                corner: Corner::Tier(style::CLUSTER_CORNER),
            },
            fill,
            Some(stroke),
        );
        let head = &geo.heads[b];
        let ink = boundary.colors.font.map(Ink::Rgba);
        let mut y = rect.y + geo.pad * 0.75;
        let x = rect.x + geo.pad;
        out.text(
            &head.label,
            [x, y],
            head.label.width,
            Align::Start,
            ink.unwrap_or(style::TEXT),
        );
        y += head.label.height;
        for part in [&head.kind, &head.descr].into_iter().flatten() {
            y += line_gap;
            out.text(
                part,
                [x, y],
                part.width,
                Align::Start,
                ink.unwrap_or(style::MUTED),
            );
            y += part.height;
        }
    }

    // Relationships under the cards.
    let n = model.elements.len();
    let mut boxes = geo.element_rect.clone();
    boxes.extend(geo.boundary_rect.iter().copied());
    let index = |e: End| match e {
        End::Element(i) => i,
        End::Boundary(b) => n + b,
    };
    let links: Vec<Connect> = model
        .rels
        .iter()
        .map(|r| Connect {
            from: index(r.from),
            to: index(r.to),
        })
        .collect();
    // Boundary titles count as obstacles for lines and labels alike.
    let mut obstacles = geo.element_rect.clone();
    obstacles.extend(
        geo.boundary_rect
            .iter()
            .zip(&geo.heads)
            .map(|(r, h)| Rect::new(r.x + geo.pad, r.y + geo.pad * 0.75, h.size[0], h.size[1])),
    );
    let routes = grid::connect_all(&boxes, &links, &obstacles, geo.col_gap.min(geo.row_gap));
    let marker = cx.marker_size();
    let mut chips = Vec::new();
    let mut taken = Vec::new();
    for (i, rel) in model.rels.iter().enumerate() {
        let mut points = routes[i].clone();
        if points.len() < 2 {
            continue;
        }
        if let End::Element(e) = rel.from {
            crate::layout::attach_start(&mut points, geo.cards[e].shape, geo.element_rect[e]);
        }
        if let End::Element(e) = rel.to {
            crate::layout::attach_end(&mut points, geo.cards[e].shape, geo.element_rect[e]);
        }
        let mut stroke = style::edge_stroke();
        if let Some(c) = rel.line_color {
            stroke.ink = Ink::Rgba(c);
        }
        graphkit::edge(
            &mut out,
            &points,
            stroke,
            rel.both.then_some(Marker::Arrow),
            Some(Marker::Arrow),
            marker,
            m.bend_radius,
        );
        if rel_texts[i].0.is_some() || rel_texts[i].1.is_some() {
            chips.push((
                i,
                grid::place_label(&points, chip_size(i), &obstacles, &mut taken),
            ));
        }
    }

    // Cards.
    for (i, e) in model.elements.iter().enumerate() {
        draw_card(
            &mut out,
            e,
            &geo.cards[i],
            geo.element_rect[i],
            line_gap,
            glyph,
        );
        if let Some(link) = &e.link {
            out.link(geo.element_rect[i], link, None);
        }
    }

    // Labels on top.
    for (i, center) in chips {
        let size = chip_size(i);
        let rect = Rect::centered(center, size);
        out.shape(
            Geom::rounded(rect, style::CHIP_CORNER),
            Some(style::CHIP_FILL),
            Some(Stroke::hairline(style::CHIP_STROKE)),
        );
        let ink = model.rels[i].text_color.map_or(style::MUTED, Ink::Rgba);
        let mut y = rect.y + chip_pad[1];
        let (label, techn) = &rel_texts[i];
        if let Some(l) = label {
            out.text(
                l,
                [rect.x + chip_pad[0], y],
                size[0] - 2.0 * chip_pad[0],
                Align::Center,
                ink,
            );
            y += l.height;
        }
        if let Some(t) = techn {
            out.text(
                t,
                [rect.x + chip_pad[0], y],
                size[0] - 2.0 * chip_pad[0],
                Align::Center,
                style::MUTED,
            );
        }
    }

    let content = out.bounds();
    let content = if content.w <= 0.0 {
        Rect::new(0.0, 0.0, 1.0, 1.0)
    } else {
        content
    };
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

fn bracketed(label: &Label) -> Label {
    let mut out = label.clone();
    if let Some(first) = out.lines.first_mut().and_then(|l| l.first_mut()) {
        first.text.insert(0, '[');
    }
    if let Some(last) = out.lines.last_mut().and_then(|l| l.last_mut()) {
        last.text.push(']');
    }
    out
}

/// `sizes` in rows of `per_row`, statement order.
fn row_grid(geo: &Geo, sizes: Vec<[f32; 2]>, per_row: usize, min_w: f32) -> grid::GridLayout {
    let cells = grid::flow(&vec![1; sizes.len()], per_row);
    grid::solve(&GridSpec {
        items: sizes
            .iter()
            .zip(&cells)
            .map(|(&size, &(col, row, _))| GridItem::new(col, row, size))
            .collect(),
        col_gap: geo.col_gap,
        row_gap: geo.row_gap,
        min_size: [min_w, 0.0],
    })
}

fn element_grid(geo: &Geo, c: usize) -> grid::GridLayout {
    let sizes = geo.elements[c].iter().map(|&e| geo.cards[e].size).collect();
    row_grid(geo, sizes, geo.shape_in_row, 0.0)
}

fn boundary_grid(geo: &Geo, c: usize, min_w: f32) -> grid::GridLayout {
    let sizes = geo.boundaries[c]
        .iter()
        .map(|&b| geo.boundary_size[b])
        .collect();
    row_grid(geo, sizes, geo.boundary_in_row, min_w)
}

/// Elements on top, boundaries below.
fn content_size(geo: &Geo, c: usize) -> [f32; 2] {
    let e = element_grid(geo, c).size;
    let b = boundary_grid(geo, c, 0.0).size;
    let between = if e[1] > 0.0 && b[1] > 0.0 {
        geo.row_gap
    } else {
        0.0
    };
    [e[0].max(b[0]), e[1] + between + b[1]]
}

fn place(geo: &mut Geo, c: usize, area: Rect) {
    let eg = element_grid(geo, c);
    let ex = area.x + (area.w - eg.size[0]).max(0.0) / 2.0;
    for (k, &e) in geo.elements[c].clone().iter().enumerate() {
        geo.element_rect[e] = eg.cells[k].translated(ex, area.y);
    }
    let top = if eg.size[1] > 0.0 {
        area.y + eg.size[1] + geo.row_gap
    } else {
        area.y
    };
    let bg = boundary_grid(geo, c, area.w);
    for (k, &b) in geo.boundaries[c].clone().iter().enumerate() {
        let rect = bg.cells[k].translated(area.x, top);
        geo.boundary_rect[b] = rect;
        let head = geo.heads[b].size;
        let inner_top = rect.y + geo.pad * 0.75 + head[1] + geo.pad;
        let inner = Rect::new(
            rect.x + geo.pad,
            inner_top,
            rect.w - 2.0 * geo.pad,
            (rect.bottom() - geo.pad - inner_top).max(0.0),
        );
        place(geo, b + 1, inner);
    }
}

fn draw_card(
    out: &mut SceneBuilder,
    e: &Element,
    card: &Card,
    rect: Rect,
    line_gap: f32,
    glyph: f32,
) {
    let (mut fill, mut stroke_ink) = if e.external {
        (EXTERNAL_FILL, EXTERNAL_STROKE)
    } else if e.kind == ElemKind::Person {
        (PERSON_FILL, INTERNAL_STROKE)
    } else {
        (INTERNAL_FILL, INTERNAL_STROKE)
    };
    if let Some(c) = e.colors.bg {
        fill = Ink::Rgba(c);
    }
    if let Some(c) = e.colors.border {
        stroke_ink = Ink::Rgba(c);
    }
    let stroke = Stroke::hairline(stroke_ink);
    let text = e
        .colors
        .font
        .map(Ink::Rgba)
        .or_else(|| e.colors.bg.map(style::text_on));
    match card.shape {
        Shape::Rect => out.shape(
            Geom::rounded(rect, RadiusTier::Md),
            Some(fill),
            Some(stroke),
        ),
        shape => {
            out.shape(shape.outline(rect), Some(fill), Some(stroke));
            for path in shape.decorations(rect).1 {
                out.stroke(Geom::path(path), stroke);
            }
        }
    }
    let area = card.shape.label_rect(rect, card.content);
    let mut y = area.y;
    if card.person {
        let c = [area.x + area.w / 2.0, y];
        let head = glyph * 0.42;
        out.shape(
            Geom::Ellipse(Rect::new(c[0] - head / 2.0, c[1], head, head)),
            Some(stroke_ink),
            None,
        );
        let body = Rect::new(
            c[0] - glyph * 0.42,
            c[1] + head + glyph * 0.06,
            glyph * 0.84,
            glyph - head - glyph * 0.06,
        );
        out.shape(
            Geom::Rect {
                rect: body,
                corner: Corner::Px(body.h * 0.6),
            },
            Some(stroke_ink),
            None,
        );
        y += glyph + line_gap;
    }
    let lines: [(Option<&LaidText>, Ink); 4] = [
        (Some(&card.stereotype), text.unwrap_or(style::MUTED)),
        (Some(&card.name), text.unwrap_or(style::TEXT)),
        (card.techn.as_ref(), text.unwrap_or(style::MUTED)),
        (card.descr.as_ref(), text.unwrap_or(style::TEXT)),
    ];
    let mut first = true;
    for (part, ink) in lines {
        let Some(part) = part else { continue };
        if !first {
            y += line_gap;
        }
        first = false;
        out.text(part, [area.x, y], area.w, Align::Center, ink);
        y += part.height;
    }
}
