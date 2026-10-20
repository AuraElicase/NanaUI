//! Architecture drawing. Every group lays its direct members (services,
//! junctions and child groups) on its own constraint grid: an edge
//! `a:R -- L:b` asks for `b` one column right of `a`, `a:R -- T:b` one
//! column right and one row down. Groups are sized bottom-up and placed
//! top-down; edges then run orthogonally from side to side, around any box
//! in the way.

use super::{Model, Service};
use crate::families::LayoutCx;
use crate::geom::path::rounded_polyline;
use crate::geom::{Marker, PathData, Shape};
use crate::ir::{Align, Corner, Geom, Rect, SceneBuilder, Stroke};
use crate::layout::grid::{self, GridItem, GridSpec, Side, Used};
use crate::style;
use crate::text::{FontSpec, LaidText};
use crate::{DiagramError, DiagramScene};
use nana_ui_core::RadiusTier;

/// A member of a container's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Member {
    Service(usize),
    Group(usize),
}

struct Card {
    title: Option<LaidText>,
    icon: Option<LaidText>,
    size: [f32; 2],
}

pub(super) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let font = cx.font();
    let small = cx.small_font();
    let wrap = cx.wrap_width("architecture.wrappingWidth") * 0.75;
    let gap = m.node_sep * 1.2;
    let pad = m.cluster_pad;
    let glyph = m.small_size * 1.3;
    let icon_gap = m.small_size * 0.4;
    let junction = m.font_size * 0.7;

    // Service cards.
    let cards: Vec<Card> = model
        .services
        .iter()
        .map(|s| {
            if s.junction {
                return Card {
                    title: None,
                    icon: None,
                    size: [junction, junction],
                };
            }
            let title = Some(cx.text.lay(&s.title, font, Some(wrap))).filter(|t| !t.is_empty());
            let icon = s.icon.as_deref().map(|i| cx.text.lay_plain(i, small));
            let tsize = title.as_ref().map_or([0.0, 0.0], LaidText::size);
            let row = icon.as_ref().map_or([0.0, 0.0], |i| [glyph + icon_gap + i.width, glyph.max(i.height)]);
            let between = if icon.is_some() && title.is_some() { icon_gap } else { 0.0 };
            Card {
                size: [tsize[0].max(row[0]).max(m.font_size * 3.0) + 2.0 * m.node_pad_x, tsize[1] + row[1] + between + 2.0 * m.node_pad_y],
                title,
                icon,
            }
        })
        .collect();

    // Group titles.
    let title_font = FontSpec::new(m.small_size).weight(m.strong_weight);
    let group_titles: Vec<(LaidText, Option<LaidText>)> = model
        .groups
        .iter()
        .map(|g| (cx.text.lay(&g.title, title_font, Some(wrap * 2.0)), g.icon.as_deref().map(|i| cx.text.lay_plain(i, small))))
        .collect();
    let header = |g: usize| -> [f32; 2] {
        let (t, icon) = &group_titles[g];
        let w = glyph + icon_gap + t.width + icon.as_ref().map_or(0.0, |i| icon_gap * 2.0 + i.width);
        [w, t.height.max(glyph)]
    };

    // Members of each container: 0 is the root, 1 + g is group g.
    let containers = model.groups.len() + 1;
    let container_of = |parent: Option<usize>| parent.map_or(0, |g| g + 1);
    let mut members: Vec<Vec<Member>> = vec![Vec::new(); containers];
    for (i, s) in model.services.iter().enumerate() {
        members[container_of(s.parent)].push(Member::Service(i));
    }
    for (g, group) in model.groups.iter().enumerate() {
        members[container_of(group.parent)].push(Member::Group(g));
    }
    // The member of container `c` that holds service `s`.
    let lift = |s: usize, c: usize| -> Option<Member> {
        let svc: &Service = &model.services[s];
        if container_of(svc.parent) == c {
            return Some(Member::Service(s));
        }
        let mut g = svc.parent?;
        loop {
            let parent = model.groups[g].parent;
            if container_of(parent) == c {
                return Some(Member::Group(g));
            }
            g = parent?;
        }
    };

    // Grid cells per container.
    let mut cells: Vec<Vec<(usize, usize)>> = Vec::with_capacity(containers);
    for (c, list) in members.iter().enumerate() {
        let index = |mem: Member| list.iter().position(|&x| x == mem);
        let mut constraints = Vec::new();
        for e in &model.edges {
            let (Some(a), Some(b)) = (lift(e.from, c), lift(e.to, c)) else { continue };
            if a == b {
                continue;
            }
            let (Some(ia), Some(ib)) = (index(a), index(b)) else { continue };
            let dx = match (e.from_side, e.to_side) {
                (Side::Right, _) => 1,
                (Side::Left, _) => -1,
                (_, Side::Left) => 1,
                (_, Side::Right) => -1,
                _ => 0,
            };
            let dy = match (e.from_side, e.to_side) {
                (Side::Bottom, _) => 1,
                (Side::Top, _) => -1,
                (_, Side::Top) => 1,
                (_, Side::Bottom) => -1,
                _ => 0,
            };
            constraints.push((ia, ib, dx, dy));
        }
        for align in &model.aligns {
            let at: Vec<usize> = align.members.iter().filter_map(|&s| lift(s, c)).filter_map(index).collect();
            if at.len() == align.members.len() {
                for w in at.windows(2) {
                    let (dx, dy) = if align.row { (1, 0) } else { (0, 1) };
                    constraints.push((w[0], w[1], dx, dy));
                }
            }
        }
        cells.push(grid::constrained_cells(list.len(), &constraints));
    }

    // Sizes bottom-up: deeper groups first.
    let depth = |mut g: usize| {
        let mut d = 0;
        while let Some(p) = model.groups[g].parent {
            d += 1;
            g = p;
        }
        d
    };
    let mut order: Vec<usize> = (0..model.groups.len()).collect();
    order.sort_by_key(|&g| (std::cmp::Reverse(depth(g)), g));
    let mut group_size = vec![[0.0f32; 2]; model.groups.len()];
    let size_of = |mem: Member, group_size: &[[f32; 2]]| match mem {
        Member::Service(s) => cards[s].size,
        Member::Group(g) => group_size[g],
    };
    let spec = |c: usize, group_size: &[[f32; 2]], min: [f32; 2]| GridSpec {
        items: members[c]
            .iter()
            .zip(&cells[c])
            .map(|(&mem, &(col, row))| GridItem::new(col, row, size_of(mem, group_size)))
            .collect(),
        col_gap: gap,
        row_gap: gap,
        min_size: min,
    };
    for &g in &order {
        let inner = grid::solve(&spec(g + 1, &group_size, [0.0, 0.0]));
        let head = header(g);
        group_size[g] = [inner.size[0].max(head[0]) + 2.0 * pad, inner.size[1] + head[1] + pad * 0.5 + 2.0 * pad];
    }

    // Placement top-down.
    let mut service_rect = vec![Rect::default(); model.services.len()];
    let mut group_rect = vec![Rect::default(); model.groups.len()];
    let mut queue: Vec<(usize, Rect)> = vec![(0, Rect::default())];
    while let Some((c, area)) = queue.pop() {
        let g = grid::solve(&spec(c, &group_size, [area.w, area.h]));
        for (k, &mem) in members[c].iter().enumerate() {
            let cell = g.cells[k].translated(area.x, area.y);
            // Groups fill their cell so siblings in a column share a width.
            let rect = match mem {
                Member::Group(_) => cell,
                Member::Service(_) => Rect::centered(cell.center(), size_of(mem, &group_size)),
            };
            match mem {
                Member::Service(s) => service_rect[s] = rect,
                Member::Group(gi) => {
                    group_rect[gi] = rect;
                    let head = header(gi);
                    let inner = Rect::new(rect.x + pad, rect.y + pad + head[1] + pad * 0.5, rect.w - 2.0 * pad, rect.h - 2.0 * pad - head[1] - pad * 0.5);
                    queue.push((gi + 1, inner));
                }
            }
        }
    }

    let mut out = SceneBuilder::new();

    // Groups, outermost first.
    let mut by_depth: Vec<usize> = (0..model.groups.len()).collect();
    by_depth.sort_by_key(|&g| (depth(g), g));
    for &g in &by_depth {
        let rect = group_rect[g];
        out.shape(Geom::Rect { rect, corner: Corner::Tier(style::CLUSTER_CORNER) }, Some(style::CLUSTER_FILL), Some(style::cluster_stroke()));
        let (title, icon) = &group_titles[g];
        let head = header(g);
        let top = rect.y + pad * 0.75;
        let gr = Rect::new(rect.x + pad, top + (head[1] - glyph) / 2.0, glyph, glyph);
        draw_glyph(&mut out, model.groups[g].icon.as_deref(), gr);
        let tx = gr.right() + icon_gap;
        out.text(title, [tx, top + (head[1] - title.height) / 2.0], title.width, Align::Start, style::MUTED);
        if let Some(icon) = icon {
            out.text(icon, [tx + title.width + icon_gap * 2.0, top + (head[1] - icon.height) / 2.0], icon.width, Align::Start, style::MUTED);
        }
    }

    // Edges.
    let end_rect = |s: usize, group: bool| -> Rect {
        match (group, model.services[s].parent) {
            (true, Some(g)) => group_rect[g],
            _ => service_rect[s],
        }
    };
    let contains = |outer: &Rect, inner: &Rect| outer.x <= inner.x && outer.y <= inner.y && outer.right() >= inner.right() && outer.bottom() >= inner.bottom();
    let mut used = Used::new();
    let marker = cx.marker_size();
    let chip_pad = [m.small_size * 0.45, m.small_size * 0.15];
    let mut chips = Vec::new();
    let mut taken = Vec::new();
    for e in &model.edges {
        let a = end_rect(e.from, e.from_group);
        let b = end_rect(e.to, e.to_group);
        let mut walls: Vec<Rect> = service_rect
            .iter()
            .enumerate()
            .filter(|&(s, r)| s != e.from && s != e.to && !contains(&a, r) && !contains(&b, r) && !model.services[s].junction)
            .map(|(_, r)| *r)
            .collect();
        walls.extend(group_rect.iter().filter(|r| !contains(r, &a) && !contains(r, &b) && !contains(&a, r) && !contains(&b, r)).copied());
        let mut points = grid::port_route(a, e.from_side, b, e.to_side, gap * 0.4);
        if grid::blocked(&points, &walls) > 0.0 {
            let mut all = walls.clone();
            all.push(a);
            all.push(b);
            if let Some(route) = grid::route_around(e.from_side.point(a, 0.5), e.from_side, e.to_side.point(b, 0.5), e.to_side, &all, gap, &mut used) {
                points = route;
            }
        } else {
            used.add(&points);
        }
        if points.len() < 2 {
            continue;
        }
        let radius = m.bend_radius;
        out.edge(
            &points,
            move |p| rounded_polyline(p, radius),
            style::edge_stroke(),
            e.start_arrow.then_some(Marker::Arrow),
            e.end_arrow.then_some(Marker::Arrow),
            marker,
        );
        if let Some(label) = &e.label {
            let laid = cx.text.lay(label, small, Some(wrap));
            if !laid.is_empty() {
                let size = [laid.width + 2.0 * chip_pad[0], laid.height + 2.0 * chip_pad[1]];
                let at = grid::place_label(&points, size, &service_rect, &mut taken);
                chips.push((laid, at));
            }
        }
    }

    // Services and junctions.
    for (s, svc) in model.services.iter().enumerate() {
        let rect = service_rect[s];
        if svc.junction {
            out.shape(Geom::Ellipse(rect.inset(rect.w * 0.2, rect.h * 0.2)), Some(style::EDGE), None);
            continue;
        }
        let card = &cards[s];
        out.shape(Geom::rounded(rect, RadiusTier::Md), Some(style::NODE_FILL), Some(style::node_stroke()));
        let mut y = rect.y + m.node_pad_y;
        if let Some(icon) = &card.icon {
            let row_h = glyph.max(icon.height);
            let row_w = glyph + icon_gap + icon.width;
            let x = rect.x + (rect.w - row_w) / 2.0;
            draw_glyph(&mut out, svc.icon.as_deref(), Rect::new(x, y + (row_h - glyph) / 2.0, glyph, glyph));
            out.text(icon, [x + glyph + icon_gap, y + (row_h - icon.height) / 2.0], icon.width, Align::Start, style::MUTED);
            y += row_h + icon_gap;
        }
        if let Some(title) = &card.title {
            out.text(title, [rect.x, y], rect.w, Align::Center, style::TEXT);
        }
    }
    for (laid, center) in &chips {
        out.chip(laid, *center, chip_pad);
    }

    let content = out.bounds();
    let content = if content.w <= 0.0 { Rect::new(0.0, 0.0, 1.0, 1.0) } else { content };
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

/// A small line glyph standing in for an icon: the five built-in names get
/// a hint of their shape, anything else a rounded square.
fn draw_glyph(out: &mut SceneBuilder, icon: Option<&str>, r: Rect) {
    let stroke = Stroke::hairline(style::MUTED);
    let name = icon.unwrap_or("").rsplit(':').next().unwrap_or("");
    let inner = r.inset(r.w * 0.08, r.h * 0.08);
    match name {
        "database" => {
            let shape = Shape::Cylinder;
            out.shape(shape.outline(inner), Some(style::NODE_FILL), Some(stroke));
            for path in shape.decorations(inner).1 {
                out.stroke(Geom::path(path), stroke);
            }
        }
        "disk" => {
            out.shape(Geom::Ellipse(inner), Some(style::NODE_FILL), Some(stroke));
            out.shape(Geom::Ellipse(Rect::centered(inner.center(), [inner.w * 0.3, inner.h * 0.3])), Some(style::MUTED), None);
        }
        "internet" => {
            out.shape(Geom::Ellipse(inner), Some(style::NODE_FILL), Some(stroke));
            let c = inner.center();
            out.stroke(Geom::path(PathData::polyline(&[[inner.x, c[1]], [inner.right(), c[1]]])), stroke);
            out.stroke(Geom::Ellipse(Rect::centered(c, [inner.w * 0.4, inner.h])), stroke);
        }
        "server" => {
            out.shape(Geom::rounded(inner, RadiusTier::Xs), Some(style::NODE_FILL), Some(stroke));
            for t in [0.36, 0.64] {
                let y = inner.y + inner.h * t;
                out.stroke(Geom::path(PathData::polyline(&[[inner.x, y], [inner.right(), y]])), stroke);
            }
        }
        "cloud" => {
            let body = Rect::new(inner.x, inner.y + inner.h * 0.3, inner.w, inner.h * 0.5);
            out.shape(Geom::rounded(body, RadiusTier::Full), Some(style::NODE_FILL), Some(stroke));
        }
        _ => {
            out.shape(Geom::rounded(inner, RadiusTier::Xs), Some(style::NODE_FILL), Some(stroke));
            out.shape(Geom::Ellipse(Rect::centered(inner.center(), [inner.w * 0.36, inner.h * 0.36])), None, Some(stroke));
        }
    }
}
