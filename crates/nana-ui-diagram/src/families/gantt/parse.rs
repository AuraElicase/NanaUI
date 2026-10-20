//! The gantt grammar: settings (`dateFormat`, `axisFormat`, `tickInterval`,
//! `excludes`, `includes`, `weekday`, `weekend`, `todayMarker`,
//! `inclusiveEndDates`, `topAxis`, `displayMode`), `title`, `section`,
//! tasks and `click ... href`.

use std::collections::HashMap;

use super::time::{self, DAY};
use super::{Excludes, Model, Task};
use crate::families::ParseCx;
use crate::families::kit::{self, first_word};
use crate::text::Label;
use crate::{DiagramError, LimitKind};

#[derive(Debug, Clone)]
enum Start {
    /// After the task written before this one.
    Previous,
    After(Vec<String>),
    Date(String),
}

#[derive(Debug, Clone)]
enum End {
    Until(Vec<String>),
    Text(String),
}

#[derive(Debug, Clone)]
struct Raw {
    line: usize,
    task: Task,
    id: Option<String>,
    start: Start,
    end: End,
}

#[derive(Default)]
struct Settings {
    date_format: Option<String>,
    excludes: Vec<String>,
    includes: Vec<String>,
    weekend_friday: bool,
    inclusive: bool,
    today: Option<(usize, String)>,
    clicks: Vec<(Vec<String>, String)>,
}

/// Day-by-day work spent pushing ends past excluded days, across a chart.
const EXCLUDE_BUDGET: u64 = 2_000_000;

pub(super) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    let mut model = Model {
        // Mermaid's default: weeks start on Sunday.
        week_start: 6,
        ..Model::default()
    };
    let mut settings = Settings::default();
    // Mermaid's `gantt.*` configuration; statements below override it.
    let config = cx.config;
    if let Some(f) = config.get("gantt.axisFormat").filter(|f| !f.trim().is_empty()) {
        model.axis_format = Some(f.trim().to_string());
    }
    model.tick_interval = config.get("gantt.tickInterval").and_then(time::parse_tick_interval);
    if let Some(d) = config.get("gantt.weekday").and_then(time::weekday_named) {
        model.week_start = d;
    }
    model.top_axis = config.get_bool("gantt.topAxis").unwrap_or(false);
    model.compact = config.get("gantt.displayMode").is_some_and(|m| m.trim() == "compact");
    let mut raws: Vec<Raw> = Vec::new();
    let (header, _) = kit::header(body)?;
    for (index, line) in kit::statements(body, header) {
        let line = line.trim_end_matches(';').trim_end();
        if line.is_empty() {
            continue;
        }
        let (word, rest) = first_word(line);
        match word.to_ascii_lowercase().as_str() {
            "title" => cx.meta.title = Some(rest.to_string()),
            "dateformat" => settings.date_format = Some(rest.to_string()),
            "axisformat" => model.axis_format = Some(rest.to_string()).filter(|f| !f.is_empty()),
            "tickinterval" => match time::parse_tick_interval(rest) {
                Some(t) => model.tick_interval = Some(t),
                None => cx.warn(index, format!("unknown tick interval `{rest}`")),
            },
            "excludes" => settings.excludes.extend(list(rest)),
            "includes" => settings.includes.extend(list(rest)),
            "weekday" => match time::weekday_named(rest) {
                Some(d) => model.week_start = d,
                None => cx.warn(index, format!("unknown weekday `{rest}`")),
            },
            "weekend" => match rest.to_ascii_lowercase().as_str() {
                "friday" => settings.weekend_friday = true,
                "saturday" => settings.weekend_friday = false,
                _ => cx.warn(index, format!("unknown weekend `{rest}`")),
            },
            "inclusiveenddates" => settings.inclusive = true,
            "topaxis" => model.top_axis = true,
            "displaymode" => model.compact = rest.eq_ignore_ascii_case("compact"),
            "todaymarker" => settings.today = Some((index, rest.to_string())),
            "section" => {
                if model.sections.len() >= cx.limits.clusters {
                    return Err(DiagramError::TooLarge(LimitKind::Clusters));
                }
                model.sections.push(cx.label(rest)?);
            }
            "click" => match click(rest) {
                Some(c) => settings.clicks.push(c),
                None => cx.warn(index, "only `click <id> href \"<url>\"` is drawn"),
            },
            _ => match line.split_once(':') {
                Some((name, data)) => {
                    if raws.len() >= cx.limits.elements {
                        return Err(DiagramError::TooLarge(LimitKind::Elements));
                    }
                    if model.sections.is_empty() {
                        model.sections.push(Label::default());
                    }
                    let section = model.sections.len() - 1;
                    raws.push(task(index, name, data, section, cx)?);
                }
                None => cx.warn(index, format!("unknown statement `{word}`")),
            },
        }
    }

    let fmt = settings.date_format.clone().unwrap_or_else(|| "YYYY-MM-DD".into());
    model.excludes = excludes(&settings, &fmt);
    if let Some((line, value)) = &settings.today {
        // Only a date draws the line; `off` and style declarations do not.
        model.today = time::parse_date(value, &fmt);
        if model.today.is_none() && !value.eq_ignore_ascii_case("off") && !value.contains(':') {
            cx.warn(*line, format!("todayMarker `{value}` is not a date"));
        }
    }
    model.tasks = resolve(raws, &fmt, &settings, &model.excludes, cx);
    Ok(model)
}

fn list(rest: &str) -> impl Iterator<Item = String> + '_ {
    rest.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// `click a,b href "url"` (a `call` part is ignored).
fn click(rest: &str) -> Option<(Vec<String>, String)> {
    let (ids, tail) = first_word(rest);
    let at = tail.find("href")?;
    let after = tail[at + 4..].trim_start();
    let url = if let Some(q) = after.strip_prefix('"') {
        &q[..q.find('"')?]
    } else {
        after.split_whitespace().next()?
    };
    let ids = ids.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
    (!url.is_empty()).then(|| (ids, url.to_string()))
}

fn task(
    line: usize,
    name: &str,
    data: &str,
    section: usize,
    cx: &mut ParseCx<'_>,
) -> Result<Raw, DiagramError> {
    let mut task = Task {
        name: cx.label(name.trim())?,
        section,
        ..Task::default()
    };
    cx.limits.check_label(data)?;
    let mut items: Vec<&str> = data.split(',').map(str::trim).collect();
    while let Some(first) = items.first() {
        match *first {
            "crit" => task.crit = true,
            "done" => task.done = true,
            "active" => task.active = true,
            "milestone" => task.milestone = true,
            "vert" => task.vert = true,
            _ => break,
        }
        items.remove(0);
    }
    items.retain(|s| !s.is_empty());
    let start_of = |s: &str| match s.strip_prefix("after ") {
        Some(ids) => Start::After(ids.split_whitespace().map(str::to_string).collect()),
        None => Start::Date(s.to_string()),
    };
    let end_of = |s: &str| match s.strip_prefix("until ") {
        Some(ids) => End::Until(ids.split_whitespace().map(str::to_string).collect()),
        None => End::Text(s.to_string()),
    };
    let (id, start, end) = match items.as_slice() {
        [] => {
            cx.warn(line, "task has no start or length");
            (None, Start::Previous, End::Text(String::new()))
        }
        [end] => (None, Start::Previous, end_of(end)),
        [start, end] => (None, start_of(start), end_of(end)),
        [id, start, end, extra @ ..] => {
            if !extra.is_empty() {
                cx.warn(line, "extra task fields ignored");
            }
            (Some(id.to_string()), start_of(start), end_of(end))
        }
    };
    Ok(Raw {
        line,
        task,
        id,
        start,
        end,
    })
}

fn excludes(settings: &Settings, fmt: &str) -> Excludes {
    let mut out = Excludes::default();
    let day = |s: &str| time::parse_date(s, fmt).map(|ms| ms.div_euclid(DAY));
    for item in &settings.excludes {
        let lower = item.to_ascii_lowercase();
        if lower == "weekends" {
            let (a, b) = if settings.weekend_friday { (4, 5) } else { (5, 6) };
            out.weekdays[a] = true;
            out.weekdays[b] = true;
        } else if let Some(d) = time::weekday_named(&lower).filter(|_| lower.len() > 3) {
            out.weekdays[d as usize] = true;
        } else if let Some(d) = day(item) {
            out.days.push(d);
        }
    }
    out.includes = settings.includes.iter().filter_map(|s| day(s)).collect();
    out.days.sort_unstable();
    out.includes.sort_unstable();
    out
}

#[derive(Debug, Clone, Copy)]
struct Resolved {
    start: i64,
    /// The end later tasks follow, past any excluded days.
    end: i64,
    /// The end the bar is drawn to.
    draw_end: i64,
}

struct Resolver<'a> {
    raws: &'a [Raw],
    ids: HashMap<&'a str, usize>,
    done: Vec<Option<Resolved>>,
    fmt: &'a str,
    inclusive: bool,
    excludes: &'a Excludes,
    anchor: i64,
    budget: u64,
}

/// Turn written starts and ends into instants. Tasks may follow tasks
/// written later, so this runs passes until nothing changes, then settles
/// whatever is left (unknown ids, cycles) in order with fallbacks.
fn resolve(
    raws: Vec<Raw>,
    fmt: &str,
    settings: &Settings,
    excludes: &Excludes,
    cx: &mut ParseCx<'_>,
) -> Vec<Task> {
    let mut ids: HashMap<&str, usize> = HashMap::new();
    for (i, raw) in raws.iter().enumerate() {
        if let Some(id) = &raw.id {
            ids.insert(id.as_str(), i);
        }
    }
    // Where a task with nothing to follow starts: the first date written.
    let anchor = raws
        .iter()
        .find_map(|r| match (&r.start, &r.end) {
            (Start::Date(s), _) | (_, End::Text(s)) => time::parse_date(s, fmt),
            _ => None,
        })
        .unwrap_or(0);
    let mut r = Resolver {
        raws: &raws,
        ids,
        done: vec![None; raws.len()],
        fmt,
        inclusive: settings.inclusive,
        excludes,
        anchor,
        budget: EXCLUDE_BUDGET,
    };
    for force in [false, true] {
        loop {
            let mut progress = false;
            for (i, raw) in raws.iter().enumerate() {
                if r.done[i].is_some() {
                    continue;
                }
                let mut notes = Vec::new();
                if let Some(value) = r.step(i, force, &mut notes) {
                    r.done[i] = Some(value);
                    progress = true;
                    for note in notes {
                        cx.warn(raw.line, note);
                    }
                }
            }
            if force || !progress {
                break;
            }
        }
    }
    let mut tasks: Vec<Task> = raws
        .iter()
        .zip(&r.done)
        .map(|(raw, done)| {
            let d = done.unwrap_or(Resolved {
                start: anchor,
                end: anchor,
                draw_end: anchor,
            });
            Task {
                start: d.start,
                end: d.draw_end.max(d.start),
                ..raw.task.clone()
            }
        })
        .collect();
    for (targets, url) in &settings.clicks {
        for id in targets {
            if let Some(&i) = r.ids.get(id.as_str()) {
                tasks[i].link = Some(url.clone());
            }
        }
    }
    tasks
}

impl Resolver<'_> {
    /// The end of the task written before `i`, if known.
    fn previous(&self, i: usize, force: bool) -> Option<i64> {
        if i == 0 {
            return Some(self.anchor);
        }
        match self.done[i - 1] {
            Some(r) => Some(r.end),
            None if force => Some(self.anchor),
            None => None,
        }
    }

    /// Resolved tasks named by `refs`: `None` while one is still pending.
    fn refs(&self, refs: &[String], force: bool, notes: &mut Vec<String>) -> Option<Vec<Resolved>> {
        let mut out = Vec::new();
        for id in refs {
            match self.ids.get(id.as_str()) {
                Some(&k) => match self.done[k] {
                    Some(r) => out.push(r),
                    None if force => notes.push(format!("task `{id}` depends on itself")),
                    None => return None,
                },
                None => notes.push(format!("unknown task id `{id}`")),
            }
        }
        Some(out)
    }

    fn step(&mut self, i: usize, force: bool, notes: &mut Vec<String>) -> Option<Resolved> {
        let raw = &self.raws[i];
        let start = match &raw.start {
            Start::Previous => self.previous(i, force)?,
            Start::After(refs) => {
                let found = self.refs(refs, force, notes)?;
                match found.iter().map(|r| r.end).max() {
                    Some(end) => end,
                    None => self.previous(i, force)?,
                }
            }
            Start::Date(s) => match time::parse_date(s, self.fmt) {
                Some(t) => t,
                None => {
                    notes.push(format!("`{s}` is not a date in `{}`", self.fmt));
                    self.previous(i, force)?
                }
            },
        };
        let (end, manual) = match &raw.end {
            End::Until(refs) => {
                let found = self.refs(refs, force, notes)?;
                (found.iter().map(|r| r.start).min().unwrap_or(start), true)
            }
            End::Text(s) if s.is_empty() => (start, true),
            End::Text(s) => {
                if let Some(t) = time::parse_date(s, self.fmt) {
                    (if self.inclusive { t.saturating_add(DAY) } else { t }, true)
                } else if let Some(span) = time::parse_duration(s) {
                    (time::add(start, span), false)
                } else {
                    notes.push(format!("`{s}` is neither a date nor a duration"));
                    (start, true)
                }
            }
        };
        let end = end.max(start);
        if manual || self.excludes.is_empty() || raw.task.milestone || raw.task.vert {
            return Some(Resolved {
                start,
                end,
                draw_end: end,
            });
        }
        // Mermaid's rule: every excluded day after the start pushes the end
        // one day later; the bar stops before trailing excluded days.
        let (mut t, mut end, mut draw_end, mut excluded) = (start + DAY, end, end, false);
        while t <= end && self.budget > 0 {
            if !excluded {
                draw_end = end;
            }
            excluded = self.excludes.excludes(t);
            if excluded {
                end += DAY;
            }
            t += DAY;
            self.budget -= 1;
        }
        Some(Resolved {
            start,
            end,
            draw_end,
        })
    }
}
