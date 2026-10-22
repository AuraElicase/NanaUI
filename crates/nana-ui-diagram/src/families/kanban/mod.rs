//! Kanban boards: columns of cards, each card with optional ticket,
//! assignee and priority metadata.
//!
//! Columns are `Subtle` lanes of one width side by side; cards are surface
//! cards stacked inside them with a priority stripe along their leading
//! edge.

use nana_ui_core::{RadiusTier, SemanticColorRole as R};

use super::{LayoutCx, ParseCx, kit};
use crate::ir::{Align, Corner, Geom, Ink, Rect, SceneBuilder, Stroke};
use crate::layout::tree::indent_width;
use crate::text::{FontSpec, Label, LaidText};
use crate::{DiagramError, DiagramScene, LimitKind, Summary, style};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Priority {
    VeryHigh,
    High,
    Low,
    VeryLow,
}

impl Priority {
    fn parse(text: &str) -> Option<Self> {
        let key: String = text
            .chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect();
        Some(match key.as_str() {
            "veryhigh" | "critical" | "highest" => Self::VeryHigh,
            "high" => Self::High,
            "low" => Self::Low,
            "verylow" | "lowest" => Self::VeryLow,
            _ => return None,
        })
    }

    fn ink(self) -> Ink {
        match self {
            Self::VeryHigh => style::DANGER,
            Self::High => style::WARNING,
            Self::Low => style::MUTED,
            Self::VeryLow => style::FAINT,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Card {
    pub label: Label,
    pub ticket: Option<String>,
    pub assigned: Option<String>,
    pub priority: Option<Priority>,
}

#[derive(Debug, Clone)]
pub(crate) struct Column {
    pub title: Label,
    pub cards: Vec<Card>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub columns: Vec<Column>,
    pub ticket_url: Option<String>,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model {
        ticket_url: cx
            .config
            .get("kanban.ticketBaseUrl")
            .map(|u| u.trim().trim_matches(['\'', '"']).to_string())
            .filter(|u| !u.is_empty()),
        ..Model::default()
    };
    let (header, _) = kit::header(body)?;
    let mut column_indent = None;
    let mut count = 0;
    for (index, line) in body.lines().enumerate().skip(header + 1) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let indent = indent_width(line);
        let column_at = *column_indent.get_or_insert(indent);
        let (text, meta) = split_meta(trimmed);
        let label = cx.label(node_label(text))?;
        count += 1;
        cx.limits
            .check_count(count, cx.limits.elements, LimitKind::Elements)?;
        if indent <= column_at {
            if meta.is_some() {
                cx.warn(index, "column metadata is ignored");
            }
            model.columns.push(Column {
                title: label,
                cards: Vec::new(),
            });
            continue;
        }
        let Some(column) = model.columns.last_mut() else {
            cx.warn(index, "a card needs a column above it");
            continue;
        };
        let mut card = Card {
            label,
            ticket: None,
            assigned: None,
            priority: None,
        };
        for (key, value) in meta.map(parse_meta).unwrap_or_default() {
            match key.as_str() {
                "ticket" => card.ticket = Some(value).filter(|v| !v.is_empty()),
                "assigned" => card.assigned = Some(value).filter(|v| !v.is_empty()),
                "priority" => match Priority::parse(&value) {
                    Some(p) => card.priority = Some(p),
                    None => cx.warn(index, format!("unknown priority `{value}`")),
                },
                "label" => card.label = cx.label(&value)?,
                other => cx.warn(index, format!("unknown card field `{other}`")),
            }
        }
        if let Some(t) = &card.ticket {
            cx.limits.check_label(t)?;
        }
        if let Some(a) = &card.assigned {
            cx.limits.check_label(a)?;
        }
        column.cards.push(card);
    }
    Ok(model)
}

/// `id[Title]@{ ... }` → (`id[Title]`, `{ ... }` contents).
fn split_meta(text: &str) -> (&str, Option<&str>) {
    match text.find("@{") {
        Some(at) => {
            let meta = &text[at + 2..];
            let meta = meta.strip_suffix('}').unwrap_or(meta);
            (text[..at].trim(), Some(meta))
        }
        None => (text, None),
    }
}

/// `id[Title]`, `[Title]` or a bare title.
fn node_label(text: &str) -> &str {
    if let Some(open) = text.find('[')
        && let Some(inner) = text[open + 1..].strip_suffix(']')
    {
        return inner;
    }
    text
}

/// `key: value, key: 'quoted, value'`.
fn parse_meta(text: &str) -> Vec<(String, String)> {
    let mut quote: Option<char> = None;
    let mut start = 0;
    let mut pieces = Vec::new();
    for (i, c) in text.char_indices() {
        match (c, quote) {
            ('\'' | '"', None) => quote = Some(c),
            (c, Some(q)) if c == q => quote = None,
            (',', None) => {
                pieces.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    pieces.push(&text[start..]);
    pieces
        .into_iter()
        .filter_map(|piece| piece.split_once(':'))
        .map(|(key, value)| {
            (
                key.trim().to_ascii_lowercase(),
                kit::unquote(value).to_string(),
            )
        })
        .collect()
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    let m = *cx.metrics;
    let mut out = SceneBuilder::new();
    let width = cx
        .config
        .get_f32("kanban.sectionWidth")
        .filter(|w| *w >= 80.0)
        .unwrap_or(m.wrap_width * 1.1)
        .min(800.0);
    let gap = m.node_sep * 0.4;
    let lane_pad = m.node_pad_y;
    let card_gap = m.node_pad_y * 0.8;
    let card_pad = [m.node_pad_x * 0.75, m.node_pad_y];
    let stripe = 3.0;
    let stripe_gap = m.small_size * 0.5;
    let font = cx.font();
    let small = cx.small_font();
    let head_font = FontSpec::new(m.font_size).weight(m.strong_weight);
    let card_w = width - 2.0 * lane_pad;
    let text_x = card_pad[0] + stripe + stripe_gap;
    let text_w = (card_w - text_x - card_pad[0]).max(m.font_size);
    let meta_gap = m.small_size * 0.35;

    // Measure every lane first: lanes share the tallest height.
    struct LaidCard {
        label: LaidText,
        ticket: Option<LaidText>,
        assigned: Option<LaidText>,
        one_row: bool,
        height: f32,
    }
    let lay_meta = |text: &Option<String>| {
        text.as_ref()
            .map(|t| cx.text.lay(&Label::plain(t), small, Some(text_w)))
    };
    let mut lanes = Vec::with_capacity(model.columns.len());
    let mut tallest: f32 = 0.0;
    for column in &model.columns {
        let count = cx.text.lay_plain(&column.cards.len().to_string(), small);
        let title = cx.text.lay(
            &column.title,
            head_font,
            Some((card_w - count.width - meta_gap * 2.0).max(m.font_size)),
        );
        let head_h = title.height.max(count.height) + lane_pad;
        let mut cards = Vec::with_capacity(column.cards.len());
        let mut height = lane_pad + head_h;
        for card in &column.cards {
            let label = cx.text.lay(&card.label, font, Some(text_w));
            let ticket = lay_meta(&card.ticket);
            let assigned = lay_meta(&card.assigned);
            let tw = ticket.as_ref().map_or(0.0, |t| t.width);
            let aw = assigned.as_ref().map_or(0.0, |a| a.width);
            let one_row = tw + aw + 2.0 * meta_gap <= text_w
                && ticket.as_ref().is_none_or(|t| t.lines.len() == 1)
                && assigned.as_ref().is_none_or(|a| a.lines.len() == 1);
            let th = ticket.as_ref().map_or(0.0, |t| t.height);
            let ah = assigned.as_ref().map_or(0.0, |a| a.height);
            let meta_h = if one_row { th.max(ah) } else { th + ah };
            let mut h = card_pad[1] * 2.0 + label.height.max(m.font_size);
            if meta_h > 0.0 {
                h += meta_gap + meta_h;
            }
            height += h + card_gap;
            cards.push(LaidCard {
                label,
                ticket,
                assigned,
                one_row,
                height: h,
            });
        }
        if !cards.is_empty() {
            height -= card_gap;
        }
        height += lane_pad;
        height = height.max(lane_pad * 2.0 + head_h + m.font_size * 2.0);
        tallest = tallest.max(height);
        lanes.push((title, count, head_h, cards));
    }

    let lane_fill = Ink::Role(R::Subtle);
    let card_stroke = Stroke::hairline(Ink::Role(R::Border));
    for (c, (title, count, head_h, cards)) in lanes.iter().enumerate() {
        let x = c as f32 * (width + gap);
        out.fill(
            Geom::Rect {
                rect: Rect::new(x, 0.0, width, tallest),
                corner: Corner::Tier(RadiusTier::Md),
            },
            lane_fill,
        );
        let tx = x + lane_pad + card_pad[0] * 0.5;
        out.text(
            title,
            [tx, lane_pad],
            title.width,
            Align::Start,
            style::TEXT,
        );
        out.text(
            count,
            [
                x + width - lane_pad - card_pad[0] * 0.5 - count.width,
                lane_pad + (title.line_height - count.line_height) / 2.0,
            ],
            count.width,
            Align::End,
            style::MUTED,
        );
        let mut y = lane_pad + head_h;
        for (card, laid) in model.columns[c].cards.iter().zip(cards) {
            let rect = Rect::new(x + lane_pad, y, card_w, laid.height);
            out.shape(
                Geom::Rect {
                    rect,
                    corner: Corner::Tier(RadiusTier::Sm),
                },
                Some(style::NODE_FILL),
                Some(card_stroke),
            );
            if let Some(p) = card.priority {
                out.fill(
                    Geom::Rect {
                        rect: Rect::new(
                            rect.x + card_pad[0],
                            rect.y + card_pad[1],
                            stripe,
                            rect.h - 2.0 * card_pad[1],
                        ),
                        corner: Corner::Tier(RadiusTier::Full),
                    },
                    p.ink(),
                );
            }
            let lx = rect.x + text_x;
            let mut ly = rect.y + card_pad[1];
            out.text(&laid.label, [lx, ly], text_w, Align::Start, style::TEXT);
            ly += laid.label.height.max(m.font_size) + meta_gap;
            if let Some(t) = &laid.ticket {
                out.text(t, [lx, ly], text_w, Align::Start, style::MUTED);
                if !laid.one_row {
                    ly += t.height;
                }
            }
            if let Some(a) = &laid.assigned {
                let ax = if laid.one_row {
                    lx + text_w - a.width
                } else {
                    lx
                };
                out.text(a, [ax, ly], a.width, Align::Start, style::MUTED);
            }
            if let (Some(base), Some(ticket)) = (&model.ticket_url, &card.ticket) {
                out.link(rect, &base.replace("#TICKET#", ticket), None);
            }
            y += laid.height + card_gap;
        }
    }

    let content = if lanes.is_empty() {
        Rect::new(0.0, 0.0, width, m.font_size)
    } else {
        out.bounds()
    };
    let content = cx.title_above(&mut out, content);
    Ok(out.finish(content, m.margin))
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.columns.iter().map(|c| c.cards.len() as u32).sum(),
        edges: 0,
        groups: model.columns.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metadata_reads_quoted_values_with_commas() {
        let (text, meta) = split_meta(
            "id2[Write]@{ assigned: 'Alice, B', priority: \"Very High\", ticket: MC-1 }",
        );
        assert_eq!(node_label(text), "Write");
        let meta = parse_meta(meta.unwrap());
        assert_eq!(meta[0], ("assigned".into(), "Alice, B".into()));
        assert_eq!(Priority::parse(&meta[1].1), Some(Priority::VeryHigh));
        assert_eq!(meta[2].1, "MC-1");
        assert_eq!(node_label("[In progress]"), "In progress");
        assert_eq!(node_label("Todo"), "Todo");
    }
}
