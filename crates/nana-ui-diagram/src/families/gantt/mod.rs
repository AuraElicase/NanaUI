//! Gantt charts: `gantt`.
//!
//! Dates resolve while parsing, against the chart's final `dateFormat`,
//! `excludes` and `includes`, so the layout only maps instants to pixels.
//! Nothing reads the clock: a "today" line appears only when the source
//! names its date.

mod layout;
mod parse;
mod time;

use super::{LayoutCx, ParseCx};
use crate::text::Label;
use crate::{DiagramError, DiagramScene, Summary};

pub(crate) const NATIVE: bool = true;

#[derive(Debug, Clone, Default)]
pub(crate) struct Task {
    pub name: Label,
    pub section: usize,
    pub crit: bool,
    pub done: bool,
    pub active: bool,
    pub milestone: bool,
    /// A `vert` marker: a vertical line at `start`, not a bar.
    pub vert: bool,
    pub start: i64,
    /// Where the bar is drawn to; excluded days at the end are left off.
    pub end: i64,
    pub link: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Excludes {
    /// Monday = 0 … Sunday = 6.
    pub weekdays: [bool; 7],
    /// Excluded days, as sorted day numbers since 1970-01-01.
    pub days: Vec<i64>,
    /// `includes`, sorted: days that stay working days whatever `excludes`
    /// says.
    pub includes: Vec<i64>,
}

impl Excludes {
    pub fn is_empty(&self) -> bool {
        !self.weekdays.iter().any(|w| *w) && self.days.is_empty()
    }

    /// Whether the day `ms` falls on is excluded.
    pub fn excludes(&self, ms: i64) -> bool {
        let day = ms.div_euclid(time::DAY);
        if self.includes.binary_search(&day).is_ok() {
            return false;
        }
        self.weekdays[time::weekday(ms) as usize] || self.days.binary_search(&day).is_ok()
    }
}

#[derive(Debug, Clone, Default)]
pub(crate) struct Model {
    pub sections: Vec<Label>,
    pub tasks: Vec<Task>,
    pub axis_format: Option<String>,
    pub tick_interval: Option<(i64, time::TickUnit)>,
    /// First day of a week tick, Monday = 0.
    pub week_start: u32,
    pub excludes: Excludes,
    pub today: Option<i64>,
    pub top_axis: bool,
    pub compact: bool,
}

pub(crate) fn parse(body: &str, cx: &mut ParseCx<'_>) -> Result<Model, DiagramError> {
    parse::parse(body, cx)
}

pub(crate) fn layout(model: &Model, cx: &mut LayoutCx<'_>) -> Result<DiagramScene, DiagramError> {
    layout::layout(model, cx)
}

pub(crate) fn summary(model: &Model) -> Summary {
    Summary {
        nodes: model.tasks.iter().filter(|t| !t.vert).count() as u32,
        edges: 0,
        groups: model.sections.iter().filter(|s| !s.is_empty()).count() as u32,
    }
}

#[cfg(test)]
mod tests {
    use crate::{DiagramError, DiagramMetrics, FixedMeasure, LimitKind, Limits, render};

    fn run(source: &str) -> Result<crate::DiagramScene, DiagramError> {
        render(source, &FixedMeasure, &DiagramMetrics::DEFAULT, &Limits::DEFAULT)
    }

    #[test]
    fn hostile_dates_stay_bounded() {
        for source in [
            "gantt\n a : 0001-01-01, 1ms\n b : 9999-12-31, 1d",
            "gantt\n excludes monday tuesday wednesday thursday friday saturday sunday\n a : 2024-01-01, 400w\n b : after a, 1d",
            "gantt\n tickInterval 1millisecond\n a : 2000-01-01, 10y",
            "gantt\n tickInterval 1month\n a : 2000-01-01, 1h",
            "gantt\n dateFormat X\n a : 99999999999999, 1d",
            "gantt\n a : a, after a, 1d\n b : b, after b c, until a\n c : c, after b, 2d",
            "gantt\n todayMarker 2024-01-01\n vert : vert, 2024-01-05, 0d",
        ] {
            let scene = run(source).unwrap_or_else(|e| panic!("{source}: {e}"));
            assert!(scene.size.iter().all(|v| v.is_finite()), "{source}");
        }
    }

    #[test]
    fn excluded_days_push_the_end() {
        let parsed = crate::parse(
            "gantt\n excludes weekends\n a : a, 2024-01-05, 2d\n b : after a, 1d",
            &Limits::DEFAULT,
        )
        .unwrap();
        let crate::families::Model::Gantt(model) = &parsed.model else {
            unreachable!()
        };
        // Friday + 2 working days runs to Tuesday; the bar stops on Monday
        // night, the next task starts Tuesday.
        let day = super::time::DAY;
        assert_eq!((model.tasks[0].end - model.tasks[0].start) / day, 4);
        assert_eq!((model.tasks[1].start - model.tasks[0].start) / day, 4);
    }

    #[test]
    fn too_many_tasks_end_cleanly() {
        let limits = Limits {
            elements: 10,
            ..Limits::DEFAULT
        };
        let source = "gantt\n".to_string() + &" t : 1d\n".repeat(11);
        let err = render(&source, &FixedMeasure, &DiagramMetrics::DEFAULT, &limits).unwrap_err();
        assert_eq!(err, DiagramError::TooLarge(LimitKind::Elements));
    }
}
