use super::*;
use crate::NanaTextShaper;
use nana_ui_runtime::LayoutViewport;
use nana_ui_scene::RuntimeDocument;

const MAIN: WindowId = WindowId::PRIMARY;
const TOOL: WindowId = WindowId(41);
const GUIDE: WindowId = WindowId(42);
const SECOND_TOOL: WindowId = WindowId(43);

/// The application's keys.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Operation,
    Task(String),
}

type Center = NoticeCenter<Key, &'static str>;
type Host = NoticeHost<Key, &'static str>;

const PLACEMENT: NoticePlacement = NoticePlacement {
    align: PopoverAlignment::End,
    max_width: 360.0,
    insets: PanelInsets {
        top: 12.0,
        right: 28.0,
        bottom: 24.0,
        left: 28.0,
    },
    max_height: 400.0,
};

struct Window {
    id: WindowId,
    document: RuntimeDocument,
    host: Host,
    shaper: NanaTextShaper,
}

impl Window {
    fn new(id: WindowId) -> Self {
        let document_id = DocumentId::new(id.0 + 1).unwrap();
        let mut document = RuntimeDocument::new(document_id);
        let root = document
            .context_mut()
            .create_component(document_id, Stack::fill_column(0.0))
            .unwrap();
        let host =
            NoticeHost::mount(document.context_mut(), document_id, root.stable_id(), 10).unwrap();
        Self {
            id,
            document,
            host,
            shaper: NanaTextShaper::default(),
        }
    }

    fn sync(&mut self, center: &mut Center) -> bool {
        center.sync(
            self.document.context_mut(),
            &mut self.host,
            self.id,
            640.0,
            480.0,
            PLACEMENT,
        )
    }

    fn frame(&mut self, center: &mut Center, ms: u64) {
        self.sync(center);
        self.document
            .context_mut()
            .advance_animations(Duration::from_millis(ms));
        self.document
            .flush(LayoutViewport::new(640.0, 480.0), &mut self.shaper)
            .unwrap();
    }

    fn toast(&self) -> Entity<Toast> {
        self.host.toasts().next().unwrap().1
    }
}

fn running() -> Notice<&'static str> {
    Notice::running("正在处理…", ToastProgress::Indeterminate, Icon::Add)
}

fn center() -> Center {
    let mut center = Center::default();
    center.set_visible(vec![MAIN, TOOL], None);
    center
}

fn key(id: &str) -> Key {
    Key::Task(id.into())
}

#[test]
fn sources_and_global_focus_choose_one_window_and_stay_there() {
    let mut center = center();
    let now = Instant::now();
    center.focus_changed(TOOL, true);
    center.show(NoticeSource::Global, key("a"), running(), now);
    center.show(NoticeSource::Window(MAIN), Key::Operation, running(), now);
    assert_eq!(center.entries()[0].owner, Some(TOOL));
    assert_eq!(center.entries()[1].owner, Some(MAIN));
    center.focus_changed(MAIN, true);
    center.set_visible(vec![MAIN, TOOL], None);
    center.show(NoticeSource::Global, key("a"), running(), now);
    assert_eq!(
        center.entries()[0].owner,
        Some(TOOL),
        "focus never moves it"
    );
    let mut main = Window::new(MAIN);
    let mut tool = Window::new(TOOL);
    main.sync(&mut center);
    tool.sync(&mut center);
    assert_eq!(main.host.len(), 1);
    assert_eq!(tool.host.len(), 1);
    assert!(!main.sync(&mut center), "an unchanged host earns no redraw");
}

#[test]
fn an_unavailable_window_uses_main_then_the_priority_window_then_recent_ones() {
    let mut center = center();
    center.focus_changed(TOOL, true);
    center.show(
        NoticeSource::Window(TOOL),
        Key::Operation,
        running(),
        Instant::now(),
    );
    center.set_visible(vec![MAIN, GUIDE], Some(GUIDE));
    assert_eq!(center.entries()[0].owner, Some(MAIN));
    center.set_visible(vec![GUIDE, SECOND_TOOL], Some(GUIDE));
    assert_eq!(center.entries()[0].owner, Some(GUIDE));
    center.focus_changed(SECOND_TOOL, true);
    center.focus_changed(SECOND_TOOL, false);
    center.set_visible(vec![TOOL, SECOND_TOOL], None);
    assert_eq!(center.entries()[0].owner, Some(SECOND_TOOL));
    center.set_visible(vec![], None);
    assert_eq!(center.entries()[0].owner, None);
}

#[test]
fn notices_wait_without_a_toast_or_timer_while_no_window_shows_then_hand_off_once() {
    let mut center = Center::default();
    let mut main = Window::new(MAIN);
    let mut guide = Window::new(GUIDE);
    center.message(
        NoticeSource::Global,
        Notice::message("保存失败", ToastTone::Danger),
        Instant::now(),
    );
    main.frame(&mut center, 60_000);
    assert!(main.host.is_empty());
    assert_eq!(center.len(), 1, "a hidden window never runs its timeout");
    center.set_visible(vec![GUIDE], Some(GUIDE));
    guide.sync(&mut center);
    assert_eq!(guide.host.len(), 1);
    center.set_visible(vec![MAIN], None);
    main.sync(&mut center);
    guide.sync(&mut center);
    assert_eq!(center.entries()[0].owner, Some(MAIN));
    assert!(guide.host.is_empty());
    assert_eq!(main.host.len(), 1);
}

#[test]
fn a_task_updates_and_settles_in_place_even_after_it_moved() {
    let mut center = center();
    let mut tool = Window::new(TOOL);
    let mut main = Window::new(MAIN);
    let now = Instant::now();
    center.show(NoticeSource::Window(TOOL), Key::Operation, running(), now);
    tool.sync(&mut center);
    let original = tool.toast();
    let mut progress = running();
    progress.progress = Some(ToastProgress::Determinate(0.5));
    center.show(NoticeSource::Window(MAIN), Key::Operation, progress, now);
    tool.sync(&mut center);
    assert_eq!(tool.toast(), original, "updated in place, in its window");
    center.set_visible(vec![MAIN], None);
    main.sync(&mut center);
    tool.sync(&mut center);
    assert!(tool.host.is_empty());
    let moved = main.toast();
    center.settle(
        NoticeSource::Window(TOOL),
        &Key::Operation,
        Some(Notice::message("截图已保存", ToastTone::Success)),
        now,
    );
    main.sync(&mut center);
    assert_eq!(main.toast(), moved);
    assert!(
        !center.contains(&Key::Operation),
        "a result is a message now"
    );
    assert_eq!(
        main.document
            .context()
            .read(moved, |toast| (toast.title.to_string(), toast.progress))
            .unwrap(),
        ("截图已保存".into(), None)
    );
    center.settle(NoticeSource::Window(MAIN), &Key::Operation, None, now);
    assert_eq!(center.len(), 1);
    center.settle(
        NoticeSource::Window(MAIN),
        &Key::Operation,
        Some(Notice::message("图片已保存", ToastTone::Success)),
        now,
    );
    assert_eq!(center.len(), 2, "a quick task still reports its result");
}

#[test]
fn a_moved_notice_does_not_repeat_what_was_said_but_new_words_are_live() {
    let mut center = center();
    let mut tool = Window::new(TOOL);
    let mut main = Window::new(MAIN);
    center.show(
        NoticeSource::Window(TOOL),
        Key::Operation,
        running(),
        Instant::now(),
    );
    for ms in [0, 16, 500, 516, 1000] {
        tool.frame(&mut center, ms);
    }
    assert!(
        center.entries()[0].announced.is_some(),
        "only a toast actually shown counts as said"
    );
    center.set_visible(vec![MAIN], None);
    for ms in [1000, 1016, 1500, 1516] {
        main.frame(&mut center, ms);
    }
    let live = |main: &Window| {
        main.document
            .context()
            .world()
            .live(main.toast().stable_id())
    };
    assert_eq!(live(&main), Some(AccessibilityLive::Off));
    let mut progress = running();
    progress.progress = Some(ToastProgress::Determinate(0.25));
    center.show(
        NoticeSource::Window(TOOL),
        Key::Operation,
        progress,
        Instant::now(),
    );
    main.frame(&mut center, 1540);
    assert_eq!(
        live(&main),
        Some(AccessibilityLive::Off),
        "progress alone says nothing new"
    );
    let mut updated = running().description("工作量已经确认");
    updated.progress = Some(ToastProgress::Determinate(0.5));
    center.show(
        NoticeSource::Window(TOOL),
        Key::Operation,
        updated,
        Instant::now(),
    );
    main.frame(&mut center, 1550);
    assert_eq!(live(&main), Some(AccessibilityLive::Polite));
    center.settle(
        NoticeSource::Window(TOOL),
        &Key::Operation,
        Some(Notice::message("保存失败", ToastTone::Danger)),
        Instant::now(),
    );
    main.frame(&mut center, 1600);
    assert_eq!(live(&main), Some(AccessibilityLive::Assertive));
}

#[test]
fn a_moved_button_runs_once_and_the_old_toast_cannot_consume_it() {
    let mut center = center();
    let mut tool = Window::new(TOOL);
    let mut main = Window::new(MAIN);
    let retry = Notice::message("没有完成优化", ToastTone::Warning).action("重试", "retry a");
    center.show(NoticeSource::Window(TOOL), key("a"), retry, Instant::now());
    tool.sync(&mut center);
    let stale = tool.toast();
    center.set_visible(vec![MAIN], None);
    main.sync(&mut center);
    tool.document
        .context_mut()
        .update_component(stale, |_, cx| cx.emit(ToastAction))
        .unwrap();
    assert!(center.drain().is_empty());
    assert_eq!(center.len(), 1);
    let toast = main.toast();
    main.document
        .context_mut()
        .update_component(toast, |_, cx| {
            cx.emit(ToastAction);
            cx.emit(ToastAction);
        })
        .unwrap();
    assert_eq!(
        center.drain(),
        [(NoticeSource::Window(MAIN), "retry a")],
        "once, from the window it was pressed in"
    );
    assert!(center.is_empty());
    center.message(
        NoticeSource::Window(MAIN),
        Notice::message("已复制", ToastTone::Info),
        Instant::now(),
    );
    main.sync(&mut center);
    let toast = main.toast();
    main.document
        .context_mut()
        .update_component(toast, |_, cx| cx.emit(ToastDismissed))
        .unwrap();
    assert!(center.drain().is_empty(), "closing runs nothing");
    assert!(center.is_empty());
}

#[test]
fn a_repeat_counts_once_per_window_and_the_budget_drops_old_messages_first() {
    let mut center = center();
    let now = Instant::now();
    let repeat = center.config().repeat_window;
    let max = center.config().max;
    let copied = || Notice::message("已复制", ToastTone::Info).action("查看", "view");
    center.message(NoticeSource::Window(MAIN), copied(), now);
    center.message(NoticeSource::Window(MAIN), copied(), now + repeat / 2);
    assert_eq!(center.len(), 1);
    center.message(NoticeSource::Window(TOOL), copied(), now);
    center.message(NoticeSource::Window(MAIN), copied(), now + repeat);
    assert_eq!(center.len(), 3);
    center.show(NoticeSource::Global, key("a"), running(), now);
    for index in 0..max {
        center.message(
            NoticeSource::Window(MAIN),
            Notice::message(format!("消息 {index}"), ToastTone::Info),
            now,
        );
    }
    assert_eq!(center.len(), max);
    assert!(center.contains(&key("a")), "a running task keeps its place");
    assert!(
        !center
            .entries()
            .iter()
            .any(|entry| entry.notice.title == "消息 0")
    );
}

#[test]
fn a_result_from_a_closed_window_prefers_main_over_a_focused_one() {
    let mut center = center();
    center.focus_changed(TOOL, true);
    center.message(
        NoticeSource::Window(GUIDE),
        Notice::message("保存失败", ToastTone::Danger),
        Instant::now(),
    );
    assert_eq!(center.entries()[0].owner, Some(MAIN));
}

#[test]
fn running_tasks_never_cause_a_result_to_be_dropped() {
    let mut center = center();
    let now = Instant::now();
    let max = center.config().max;
    for i in 0..max {
        center.show(NoticeSource::Global, key(&i.to_string()), running(), now);
    }
    center.settle(
        NoticeSource::Window(TOOL),
        &Key::Operation,
        Some(Notice::message("保存失败", ToastTone::Danger)),
        now,
    );
    assert!(
        center
            .entries()
            .iter()
            .any(|entry| entry.notice.title == "保存失败" && entry.owner == Some(TOOL))
    );
    assert_eq!(
        center
            .entries()
            .iter()
            .filter(|entry| matches!(entry.id, NoticeId::Key(Key::Task(_))))
            .count(),
        max
    );
}

/// A key shown elsewhere leaves its corner, without a fade that could show it
/// twice and without letting its old toast close it; it comes back with a
/// fresh toast.
#[test]
fn a_key_shown_elsewhere_leaves_the_corner_and_comes_back_fresh() {
    let mut center = center();
    center.set_visible(vec![MAIN, GUIDE], Some(GUIDE));
    let mut guide = Window::new(GUIDE);
    for id in ["a", "b"] {
        center.show(
            NoticeSource::Window(GUIDE),
            key(id),
            running(),
            Instant::now(),
        );
    }
    guide.frame(&mut center, 500);
    let stale = guide.host.toasts().next().unwrap().1;
    center.set_shown_elsewhere(vec![key("a")]);
    guide.sync(&mut center);
    assert_eq!(guide.host.len(), 1);
    assert!(
        guide
            .document
            .context()
            .read(stale, |toast| toast.style.layout.hidden)
            .unwrap_or(true),
        "its exiting corner cannot duplicate what is shown elsewhere"
    );
    guide
        .document
        .context_mut()
        .update_component(stale, |_, cx| cx.emit(ToastDismissed))
        .ok();
    assert!(center.drain().is_empty());
    assert!(center.contains(&key("a")), "the old toast cannot close it");
    center.set_shown_elsewhere(vec![key("b")]);
    guide.sync(&mut center);
    assert_eq!(guide.host.len(), 1);
    center.set_shown_elsewhere(Vec::new());
    guide.sync(&mut center);
    assert_eq!(guide.host.len(), 2);
}
