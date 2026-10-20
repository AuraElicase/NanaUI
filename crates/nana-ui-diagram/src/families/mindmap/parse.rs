//! The mind map grammar: one node per line, nesting by indentation, an
//! optional id before the bracketed label and `::icon(...)` lines that
//! decorate the node above them. `:::class` lines are accepted and ignored.

use super::{Model, Node, NodeShape};
use crate::families::{ParseCx, kit};
use crate::layout::tree::{Outline, indent_width};
use crate::{DiagramError, LimitKind};

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let (header, rest) = kit::header(body)?;
    if !rest.is_empty() {
        cx.warn(header, "text after `mindmap` is ignored");
    }
    let lines: Vec<&str> = body.lines().collect();
    let mut nodes: Vec<Node> = Vec::new();
    let mut outline = Outline::new();
    let mut root_indent = 0;
    let mut next = header + 1;
    while next < lines.len() {
        let index = next;
        let line = lines[index];
        next += 1;
        let mut joined = String::new();
        let mut trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        // A markdown string may run over several lines.
        if let Some(open) = trimmed.find("\"`")
            && !trimmed[open + 2..].contains("`\"")
        {
            joined.push_str(trimmed);
            while next < lines.len() && !joined[open + 2..].contains("`\"") {
                joined.push('\n');
                joined.push_str(lines[next].trim());
                next += 1;
            }
            trimmed = joined.as_str();
        }
        if let Some(icon) = trimmed.strip_prefix("::icon(") {
            match nodes.last_mut() {
                Some(node) => node.icon = Some(icon.trim_end_matches(')').trim().to_string()),
                None => cx.warn(index, "an icon needs a node above it"),
            }
            continue;
        }
        // Mind maps have no `classDef`, so classes style nothing.
        if trimmed.starts_with(":::") {
            if nodes.is_empty() {
                cx.warn(index, "a class needs a node above it");
            }
            continue;
        }
        let (label, shape) = node_text(trimmed);
        let label = cx.label(label)?;
        let mut indent = indent_width(line);
        let id = nodes.len();
        if id == 0 {
            root_indent = indent;
        } else if indent <= root_indent {
            cx.warn(index, "a mind map has one root; this node joins it");
            indent = root_indent + 1;
        }
        let parent = outline.push(indent, id);
        cx.limits
            .check_count(id + 1, cx.limits.nodes, LimitKind::Nodes)?;
        nodes.push(Node {
            label,
            shape,
            icon: None,
            children: Vec::new(),
        });
        if let Some(p) = parent {
            nodes[p].children.push(id);
        }
    }
    Ok(Model { nodes })
}

/// Split `id[label]` and friends into the label text and its shape. Text
/// without brackets is its own label.
fn node_text(text: &str) -> (&str, NodeShape) {
    // A markdown or quoted label with no id.
    if text.starts_with('"') {
        return (text, NodeShape::Default);
    }
    let Some(open) = text.find(['[', '(', ')', '{']) else {
        return (text, NodeShape::Default);
    };
    let rest = &text[open..];
    let forms: [(&str, &str, NodeShape); 6] = [
        ("((", "))", NodeShape::Circle),
        ("))", "((", NodeShape::Bang),
        ("{{", "}}", NodeShape::Hexagon),
        ("(", ")", NodeShape::Rounded),
        (")", "(", NodeShape::Cloud),
        ("[", "]", NodeShape::Square),
    ];
    for (start, end, shape) in forms {
        if let Some(inner) = rest.strip_prefix(start).and_then(|r| r.strip_suffix(end)) {
            return (inner, shape);
        }
    }
    (text, NodeShape::Default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brackets_pick_the_shape() {
        assert_eq!(node_text("root((Hi))"), ("Hi", NodeShape::Circle));
        assert_eq!(node_text("b))Boom(("), ("Boom", NodeShape::Bang));
        assert_eq!(node_text("c)Cloud("), ("Cloud", NodeShape::Cloud));
        assert_eq!(node_text("h{{Hex}}"), ("Hex", NodeShape::Hexagon));
        assert_eq!(node_text("B(B)"), ("B", NodeShape::Rounded));
        assert_eq!(node_text("A[A]"), ("A", NodeShape::Square));
        assert_eq!(node_text("Plain text"), ("Plain text", NodeShape::Default));
        assert_eq!(node_text("x(open"), ("x(open", NodeShape::Default));
        assert_eq!(node_text("id1[\"`**md**`\"]").0, "\"`**md**`\"");
    }
}
