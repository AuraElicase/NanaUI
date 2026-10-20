//! The diagram types and the header keywords that select them.

/// Every diagram type the engine names. The reference table of header
/// keywords lives in `docs/reference/diagram-engine.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DiagramKind {
    Agentflow,
    Architecture,
    Block,
    C4,
    Class,
    Cynefin,
    Er,
    EventModeling,
    Flowchart,
    Gantt,
    GitGraph,
    Info,
    Ishikawa,
    Journey,
    Kanban,
    Mindmap,
    Packet,
    Pie,
    Quadrant,
    Radar,
    Railroad,
    Requirement,
    Sankey,
    Sequence,
    State,
    Swimlane,
    Timeline,
    Treemap,
    TreeView,
    Usecase,
    Venn,
    Wardley,
    XyChart,
    ZenUml,
}

impl DiagramKind {
    pub const ALL: [Self; 34] = [
        Self::Agentflow,
        Self::Architecture,
        Self::Block,
        Self::C4,
        Self::Class,
        Self::Cynefin,
        Self::Er,
        Self::EventModeling,
        Self::Flowchart,
        Self::Gantt,
        Self::GitGraph,
        Self::Info,
        Self::Ishikawa,
        Self::Journey,
        Self::Kanban,
        Self::Mindmap,
        Self::Packet,
        Self::Pie,
        Self::Quadrant,
        Self::Radar,
        Self::Railroad,
        Self::Requirement,
        Self::Sankey,
        Self::Sequence,
        Self::State,
        Self::Swimlane,
        Self::Timeline,
        Self::Treemap,
        Self::TreeView,
        Self::Usecase,
        Self::Venn,
        Self::Wardley,
        Self::XyChart,
        Self::ZenUml,
    ];

    /// The stable identifier: fixture directories, goldens and diagnostics.
    pub const fn name(self) -> &'static str {
        match self {
            Self::Agentflow => "agentflow",
            Self::Architecture => "architecture",
            Self::Block => "block",
            Self::C4 => "c4",
            Self::Class => "class",
            Self::Cynefin => "cynefin",
            Self::Er => "er",
            Self::EventModeling => "eventmodeling",
            Self::Flowchart => "flowchart",
            Self::Gantt => "gantt",
            Self::GitGraph => "gitgraph",
            Self::Info => "info",
            Self::Ishikawa => "ishikawa",
            Self::Journey => "journey",
            Self::Kanban => "kanban",
            Self::Mindmap => "mindmap",
            Self::Packet => "packet",
            Self::Pie => "pie",
            Self::Quadrant => "quadrant",
            Self::Radar => "radar",
            Self::Railroad => "railroad",
            Self::Requirement => "requirement",
            Self::Sankey => "sankey",
            Self::Sequence => "sequence",
            Self::State => "state",
            Self::Swimlane => "swimlane",
            Self::Timeline => "timeline",
            Self::Treemap => "treemap",
            Self::TreeView => "treeview",
            Self::Usecase => "usecase",
            Self::Venn => "venn",
            Self::Wardley => "wardley",
            Self::XyChart => "xychart",
            Self::ZenUml => "zenuml",
        }
    }
}

/// Header keywords, longest first where one is a prefix of another. A
/// keyword matches when the line starts with it and the next character ends
/// the word.
const HEADERS: &[(&str, DiagramKind)] = &[
    ("flowchart-elk", DiagramKind::Flowchart),
    ("flowchart", DiagramKind::Flowchart),
    ("graph", DiagramKind::Flowchart),
    ("sequenceDiagram", DiagramKind::Sequence),
    ("classDiagram-v2", DiagramKind::Class),
    ("classDiagram", DiagramKind::Class),
    ("stateDiagram-v2", DiagramKind::State),
    ("stateDiagram", DiagramKind::State),
    ("erDiagram", DiagramKind::Er),
    ("journey", DiagramKind::Journey),
    ("gantt", DiagramKind::Gantt),
    ("pie", DiagramKind::Pie),
    ("quadrantChart", DiagramKind::Quadrant),
    ("requirementDiagram", DiagramKind::Requirement),
    ("requirement", DiagramKind::Requirement),
    ("gitGraph", DiagramKind::GitGraph),
    ("C4Context", DiagramKind::C4),
    ("C4Container", DiagramKind::C4),
    ("C4Component", DiagramKind::C4),
    ("C4Dynamic", DiagramKind::C4),
    ("C4Deployment", DiagramKind::C4),
    ("mindmap", DiagramKind::Mindmap),
    ("timeline", DiagramKind::Timeline),
    ("zenuml", DiagramKind::ZenUml),
    ("sankey-beta", DiagramKind::Sankey),
    ("sankey", DiagramKind::Sankey),
    ("xychart-beta", DiagramKind::XyChart),
    ("xychart", DiagramKind::XyChart),
    ("block-beta", DiagramKind::Block),
    ("block", DiagramKind::Block),
    ("packet-beta", DiagramKind::Packet),
    ("packet", DiagramKind::Packet),
    ("kanban", DiagramKind::Kanban),
    ("architecture-beta", DiagramKind::Architecture),
    ("architecture", DiagramKind::Architecture),
    ("radar-beta", DiagramKind::Radar),
    ("radar", DiagramKind::Radar),
    ("treemap-beta", DiagramKind::Treemap),
    ("treemap", DiagramKind::Treemap),
    ("info", DiagramKind::Info),
    ("agentflow-beta", DiagramKind::Agentflow),
    ("swimlane-beta", DiagramKind::Swimlane),
    ("eventmodeling", DiagramKind::EventModeling),
    ("treeView-beta", DiagramKind::TreeView),
    ("ishikawa-beta", DiagramKind::Ishikawa),
    ("ishikawa", DiagramKind::Ishikawa),
    ("railroad-ebnf-beta", DiagramKind::Railroad),
    ("railroad-abnf-beta", DiagramKind::Railroad),
    ("railroad-peg-beta", DiagramKind::Railroad),
    ("railroad-beta", DiagramKind::Railroad),
    ("venn-beta", DiagramKind::Venn),
    ("wardley-beta", DiagramKind::Wardley),
    ("cynefin-beta", DiagramKind::Cynefin),
    ("usecase-beta", DiagramKind::Usecase),
];

/// The type named by the first statement of a preprocessed body.
pub fn detect(body: &str) -> Option<DiagramKind> {
    let line = body.lines().map(str::trim).find(|line| !line.is_empty())?;
    header(line).map(|(kind, _)| kind)
}

/// The type a header line names and the rest of that line after the
/// keyword (`"TD"` in `flowchart TD`).
pub fn header(line: &str) -> Option<(DiagramKind, &str)> {
    let line = line.trim_start();
    HEADERS.iter().find_map(|&(keyword, kind)| {
        let rest = line.strip_prefix(keyword)?;
        let ends = rest
            .chars()
            .next()
            .is_none_or(|c| c.is_whitespace() || matches!(c, ';' | ':' | '{'));
        ends.then_some((kind, rest.trim()))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_select_their_type_and_reject_longer_words() {
        let cases = [
            ("flowchart TD", Some(DiagramKind::Flowchart)),
            ("graph LR;", Some(DiagramKind::Flowchart)),
            ("graphic", None),
            ("flowchart-elk TB", Some(DiagramKind::Flowchart)),
            ("stateDiagram-v2", Some(DiagramKind::State)),
            ("classDiagram-v2", Some(DiagramKind::Class)),
            ("requirementDiagram", Some(DiagramKind::Requirement)),
            ("pie title Pets", Some(DiagramKind::Pie)),
            ("gitGraph:", Some(DiagramKind::GitGraph)),
            ("C4Deployment", Some(DiagramKind::C4)),
            ("xychart-beta", Some(DiagramKind::XyChart)),
            ("railroad-ebnf-beta", Some(DiagramKind::Railroad)),
            ("treeView-beta", Some(DiagramKind::TreeView)),
            ("zenuml", Some(DiagramKind::ZenUml)),
            ("informal", None),
        ];
        for (line, expected) in cases {
            assert_eq!(detect(line), expected, "{line:?}");
        }
    }

    #[test]
    fn every_kind_has_a_header() {
        for kind in DiagramKind::ALL {
            assert!(HEADERS.iter().any(|&(_, k)| k == kind), "{kind:?}");
        }
    }
}
