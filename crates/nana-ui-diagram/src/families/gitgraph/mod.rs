//! Git graphs: `gitGraph`, `gitGraph LR:` / `TB:` / `BT:`.
//!
//! The parser replays the statements against branch heads exactly as git
//! would, so the model is the finished history: commits with parents and a
//! branch each. Mermaid's random commit ids become stable `seq-hash` ids.

mod layout;

use std::collections::HashMap;

use super::{LayoutCx, ParseCx};
use crate::{DiagramError, DiagramScene, LimitKind, Summary, kind};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CommitKind {
    Normal,
    Reverse,
    Highlight,
    Merge,
    CherryPick,
}

#[derive(Debug, Clone)]
pub(crate) struct Commit {
    pub id: String,
    pub branch: usize,
    /// The first parent is the branch's previous head; a merge's second is
    /// the merged branch's head, a cherry-pick's second its source.
    pub parents: Vec<usize>,
    pub kind: CommitKind,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct Branch {
    pub name: String,
    /// Lane order: Mermaid's `order`, or a fraction below 1 by creation.
    pub order: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Flow {
    #[default]
    LeftRight,
    TopBottom,
    BottomTop,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub flow: Flow,
    pub commits: Vec<Commit>,
    pub branches: Vec<Branch>,
    pub show_branches: bool,
    pub show_labels: bool,
    /// `parallelCommits`: a commit sits one step after its latest parent
    /// instead of after every earlier commit.
    pub parallel: bool,
}

struct State<'m> {
    model: &'m mut Model,
    heads: Vec<Option<usize>>,
    names: HashMap<String, usize>,
    ids: HashMap<String, usize>,
    current: usize,
}

/// `key: value` options after a command: values are quoted strings or bare
/// words.
fn options(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut rest = text.trim();
    while !rest.is_empty() {
        let colon = rest.find(':').ok_or_else(|| format!("expected `key: value` at `{rest}`"))?;
        let key = rest[..colon].trim();
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err(format!("expected an option name at `{rest}`"));
        }
        let after = rest[colon + 1..].trim_start();
        let (value, tail) = if let Some(q) = after.strip_prefix('"') {
            let end = q.find('"').ok_or("unterminated string")?;
            (&q[..end], &q[end + 1..])
        } else {
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            (&after[..end], &after[end..])
        };
        out.push((key.to_string(), value.to_string()));
        rest = tail.trim_start();
    }
    Ok(out)
}

fn unquote(s: &str) -> &str {
    let s = s.trim();
    s.strip_prefix('"').and_then(|r| r.strip_suffix('"')).unwrap_or(s)
}

/// A stable stand-in for Mermaid's random id suffix.
fn auto_id(seq: usize) -> String {
    let mut h = 0xcbf2_9ce4_8422_2325_u64;
    for b in (seq as u64).to_le_bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{seq}-{:07x}", h & 0x0fff_ffff)
}

impl State<'_> {
    fn branch_index(&self, name: &str) -> Option<usize> {
        self.names.get(name).copied()
    }

    fn add_commit(&mut self, id: Option<String>, parents: Vec<usize>, kind: CommitKind, tags: Vec<String>) -> Result<(), String> {
        let seq = self.model.commits.len();
        let id = match id {
            Some(id) => {
                if self.ids.contains_key(&id) {
                    return Err(format!("commit id `{id}` already exists"));
                }
                id
            }
            None => auto_id(seq),
        };
        self.ids.insert(id.clone(), seq);
        self.model.commits.push(Commit {
            id,
            branch: self.current,
            parents,
            kind,
            tags,
        });
        self.heads[self.current] = Some(seq);
        Ok(())
    }

    fn statement(&mut self, word: &str, rest: &str, limits: &crate::Limits) -> Result<Option<String>, String> {
        match word {
            "commit" => {
                let mut id = None;
                let mut kind = CommitKind::Normal;
                let mut tags = Vec::new();
                for (key, value) in options(rest)? {
                    match key.as_str() {
                        "id" => id = Some(value),
                        "tag" => tags.push(value),
                        "type" => kind = commit_type(&value)?,
                        "msg" => {}
                        other => return Ok(Some(format!("unknown commit option `{other}`"))),
                    }
                }
                if self.model.commits.len() >= limits.elements {
                    return Err("too many commits".into());
                }
                let parents = self.heads[self.current].into_iter().collect();
                self.add_commit(id, parents, kind, tags)?;
            }
            "branch" => {
                let (name, tail) = split_name(rest);
                if name.is_empty() {
                    return Err("`branch` needs a name".into());
                }
                if self.names.contains_key(name) {
                    return Err(format!("branch `{name}` already exists"));
                }
                if self.model.branches.len() >= limits.clusters {
                    return Err("too many branches".into());
                }
                let mut order = None;
                for (key, value) in options(tail)? {
                    match key.as_str() {
                        "order" => order = value.parse::<f64>().ok().filter(|v| v.is_finite()),
                        other => return Ok(Some(format!("unknown branch option `{other}`"))),
                    }
                }
                let index = self.model.branches.len();
                self.model.branches.push(Branch {
                    name: name.to_string(),
                    order: order.unwrap_or(f64::NAN),
                });
                self.names.insert(name.to_string(), index);
                self.heads.push(self.heads[self.current]);
                self.current = index;
            }
            "checkout" | "switch" => {
                let (name, _) = split_name(rest);
                self.current = self
                    .branch_index(name)
                    .ok_or_else(|| format!("cannot check out unknown branch `{name}`"))?;
            }
            "merge" => {
                let (name, tail) = split_name(rest);
                let other = self
                    .branch_index(name)
                    .ok_or_else(|| format!("cannot merge unknown branch `{name}`"))?;
                if other == self.current {
                    return Err("cannot merge a branch into itself".into());
                }
                let (Some(ours), Some(theirs)) = (self.heads[self.current], self.heads[other]) else {
                    return Err(format!("cannot merge `{name}`: a branch has no commits"));
                };
                if ours == theirs {
                    return Err(format!("cannot merge `{name}`: both branches point at the same commit"));
                }
                let mut id = None;
                let mut kind = CommitKind::Merge;
                let mut tags = Vec::new();
                for (key, value) in options(tail)? {
                    match key.as_str() {
                        "id" => id = Some(value),
                        "tag" => tags.push(value),
                        "type" => kind = commit_type(&value)?,
                        other => return Ok(Some(format!("unknown merge option `{other}`"))),
                    }
                }
                if self.model.commits.len() >= limits.elements {
                    return Err("too many commits".into());
                }
                self.add_commit(id, vec![ours, theirs], kind, tags)?;
            }
            "cherry-pick" => {
                let mut source = None;
                let mut parent = None;
                let mut tags = Vec::new();
                for (key, value) in options(rest)? {
                    match key.as_str() {
                        "id" => source = Some(value),
                        "parent" => parent = Some(value),
                        "tag" => tags.push(value),
                        other => return Ok(Some(format!("unknown cherry-pick option `{other}`"))),
                    }
                }
                let source = source.ok_or("`cherry-pick` needs `id:`")?;
                let &from = self.ids.get(&source).ok_or_else(|| format!("unknown commit `{source}`"))?;
                let picked = &self.model.commits[from];
                if picked.branch == self.current {
                    return Err("cannot cherry-pick a commit from the same branch".into());
                }
                if picked.kind == CommitKind::Merge || picked.parents.len() > 1 {
                    let parent = parent.as_deref().ok_or("cherry-picking a merge needs `parent:`")?;
                    let ok = picked.parents.iter().any(|&p| self.model.commits[p].id == parent);
                    if !ok {
                        return Err(format!("`{parent}` is not a parent of `{source}`"));
                    }
                }
                let Some(head) = self.heads[self.current] else {
                    return Err("cannot cherry-pick onto a branch with no commits".into());
                };
                if tags.is_empty() {
                    let mut tag = format!("cherry-pick:{source}");
                    if let Some(p) = &parent {
                        tag.push_str(&format!("|parent:{p}"));
                    }
                    tags.push(tag);
                }
                if self.model.commits.len() >= limits.elements {
                    return Err("too many commits".into());
                }
                self.add_commit(None, vec![head, from], CommitKind::CherryPick, tags)?;
            }
            other => return Ok(Some(format!("unknown statement `{other}`"))),
        }
        Ok(None)
    }
}

/// A branch name, quoted or bare, and what follows it.
fn split_name(rest: &str) -> (&str, &str) {
    let rest = rest.trim();
    if let Some(q) = rest.strip_prefix('"')
        && let Some(end) = q.find('"')
    {
        return (&q[..end], &q[end + 1..]);
    }
    match rest.find(char::is_whitespace) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    }
}

fn commit_type(value: &str) -> Result<CommitKind, String> {
    match value.trim() {
        "NORMAL" => Ok(CommitKind::Normal),
        "REVERSE" => Ok(CommitKind::Reverse),
        "HIGHLIGHT" => Ok(CommitKind::Highlight),
        other => Err(format!("unknown commit type `{other}`")),
    }
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let config = cx.config;
    let main = config.get("gitGraph.mainBranchName").map(unquote).filter(|n| !n.is_empty()).unwrap_or("main").to_string();
    let main_order = config.get_f32("gitGraph.mainBranchOrder").map_or(0.0, f64::from);
    let mut model = Model {
        show_branches: config.get_bool("gitGraph.showBranches").unwrap_or(true),
        show_labels: config.get_bool("gitGraph.showCommitLabel").unwrap_or(true),
        parallel: config.get_bool("gitGraph.parallelCommits").unwrap_or(false),
        branches: vec![Branch {
            name: main.clone(),
            order: main_order,
        }],
        ..Model::default()
    };
    let mut header = false;
    let mut state = State {
        model: &mut model,
        heads: vec![None],
        names: HashMap::from([(main, 0)]),
        ids: HashMap::new(),
        current: 0,
    };
    for (index, line) in body.lines().enumerate() {
        let line = line.trim().trim_end_matches(';').trim();
        if line.is_empty() {
            continue;
        }
        if !header {
            let Some((_, rest)) = kind::header(line) else {
                return Err(DiagramError::syntax(index as u32 + 1, 1, "expected a gitGraph header"));
            };
            state.model.flow = match rest.trim_end_matches(':').trim() {
                "" | "LR" => Flow::LeftRight,
                "TB" | "TD" => Flow::TopBottom,
                "BT" => Flow::BottomTop,
                other => {
                    cx.warn(index, format!("unknown direction `{other}`"));
                    Flow::LeftRight
                }
            };
            header = true;
            continue;
        }
        let (word, rest) = match line.find(char::is_whitespace) {
            Some(i) => (&line[..i], line[i..].trim()),
            None => (line, ""),
        };
        if word == "title" {
            cx.meta.title = Some(rest.to_string());
            continue;
        }
        cx.limits.check_label(rest).map_err(|_| DiagramError::TooLarge(LimitKind::LabelChars))?;
        match state.statement(word, rest, cx.limits) {
            Ok(None) => {}
            Ok(Some(warning)) => cx.warn(index, warning),
            Err(message) if message.starts_with("too many commits") => {
                return Err(DiagramError::TooLarge(LimitKind::Elements));
            }
            Err(message) if message.starts_with("too many branches") => {
                return Err(DiagramError::TooLarge(LimitKind::Clusters));
            }
            Err(message) => return Err(DiagramError::syntax(index as u32 + 1, 1, message)),
        }
    }
    if !header {
        return Err(DiagramError::Empty);
    }
    // Branches without an order sit between the main branch and the
    // explicitly ordered ones, by creation.
    let count = model.branches.len() as f64;
    for (i, branch) in model.branches.iter_mut().enumerate() {
        if branch.order.is_nan() {
            branch.order = i as f64 / (count + 1.0);
        }
    }
    Ok(model)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.commits.len() as u32,
        edges: model.commits.iter().map(|c| c.parents.len()).sum::<usize>() as u32,
        groups: model.branches.len() as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_read_quoted_and_bare_values() {
        let o = options(r#"id: "a b" type: HIGHLIGHT tag:"v1""#).unwrap();
        assert_eq!(o, vec![
            ("id".into(), "a b".into()),
            ("type".into(), "HIGHLIGHT".into()),
            ("tag".into(), "v1".into())
        ]);
        assert!(options("id").is_err());
        assert_ne!(auto_id(0), auto_id(1));
    }
}

#[cfg(test)]
mod history_tests {
    use crate::{DiagramError, Limits, parse};

    #[test]
    fn misuse_is_a_syntax_error() {
        for source in [
            "gitGraph\n checkout nope",
            "gitGraph\n commit\n merge main",
            "gitGraph\n commit\n branch b\n branch b",
            "gitGraph\n commit id: \"x\"\n commit id: \"x\"",
            "gitGraph\n commit\n branch b\n checkout main\n merge b",
            "gitGraph\n commit id: \"a\"\n cherry-pick id: \"a\"",
        ] {
            assert!(matches!(parse(source, &Limits::DEFAULT), Err(DiagramError::Syntax { .. })), "{source}");
        }
    }
}
