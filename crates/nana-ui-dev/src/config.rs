//! Dev-loop configuration: what to watch, what to read, and how long to wait.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long a watched tree must stay quiet before a batch is released.
///
/// Covers an editor's write-truncate-rename and a tool (formatter, code
/// generator) writing several files as separate writes.
const DEFAULT_QUIET_PERIOD: Duration = Duration::from_millis(120);

/// Directory names never worth watching. A recursive watch on a Vite project
/// without this exhausts the inotify watch limit on Linux before it sees a
/// single useful event.
pub(crate) const IGNORED_DIRECTORIES: &[&str] = &["node_modules", "target", ".git"];

/// Which paths the dev loop watches, and which `.vue` views take the hot-text
/// path instead of a rebuild.
#[derive(Debug, Clone)]
pub struct DevConfig {
    watch: Vec<PathBuf>,
    templates: Vec<PathBuf>,
    quiet_period: Duration,
}

impl DevConfig {
    /// Watch the Rust L3 tier: `root` is the source tree whose changes should
    /// trigger a rebuild. The new code arrives as a new binary.
    pub fn new_rust(root: impl Into<PathBuf>) -> Self {
        Self {
            watch: vec![root.into()],
            templates: Vec::new(),
            quiet_period: DEFAULT_QUIET_PERIOD,
        }
    }

    /// Add a recursive watch root. Ignored directories still apply.
    #[must_use]
    pub fn watch(mut self, root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        if !self.watch.contains(&root) {
            self.watch.push(root);
        }
        self
    }

    pub fn watch_roots(&self) -> &[PathBuf] {
        &self.watch
    }

    /// Treat `.vue` files under `dir` as views compiled in hot mode: their
    /// changes arrive as [`crate::ReloadRequest::Template`], not a full
    /// reload. `dir` is watched too.
    #[must_use]
    pub fn templates(mut self, dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        if !self.watch.contains(&dir) {
            self.watch.push(dir.clone());
        }
        if !self.templates.contains(&dir) {
            self.templates.push(dir);
        }
        self
    }

    pub fn template_dirs(&self) -> &[PathBuf] {
        &self.templates
    }

    pub const fn quiet(&self) -> Duration {
        self.quiet_period
    }
}

/// Skip build output, VCS metadata, dependency trees, and editor dotfiles.
///
/// Only the part of `path` *below* `root` is inspected. Testing the whole path
/// would refuse to watch any project that happens to live under a directory
/// named `target` or under a dotted parent — including this crate's own
/// `CARGO_TARGET_TMPDIR` scratch space.
pub(crate) fn is_ignored(path: &Path, root: &Path) -> bool {
    let relative = path.strip_prefix(root).unwrap_or(path);
    relative.components().any(|component| {
        let Some(name) = component.as_os_str().to_str() else {
            return false;
        };
        IGNORED_DIRECTORIES.contains(&name) || (name.starts_with('.') && name.len() > 1)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn watch_roots_deduplicate() {
        let config = DevConfig::new_rust("src").watch("src").watch("assets");
        assert_eq!(
            config.watch_roots(),
            [PathBuf::from("src"), PathBuf::from("assets")]
        );
    }

    #[test]
    fn templates_are_watched_too() {
        let config = DevConfig::new_rust("src").templates("views");
        assert_eq!(
            config.watch_roots(),
            [PathBuf::from("src"), PathBuf::from("views")]
        );
        assert_eq!(config.template_dirs(), [PathBuf::from("views")]);
    }

    #[test]
    fn build_output_and_dotfiles_are_ignored_at_any_depth_below_the_root() {
        let root = Path::new("web");
        assert!(is_ignored(Path::new("web/node_modules/vue/index.js"), root));
        assert!(is_ignored(Path::new("web/dist/target/app"), root));
        assert!(is_ignored(Path::new("web/.git/HEAD"), root));
        assert!(is_ignored(Path::new("web/.vite/deps/vue.js"), root));
        assert!(!is_ignored(Path::new("web/dist/app.iife.js"), root));
    }

    #[test]
    fn an_ignored_name_above_the_watch_root_does_not_disqualify_the_project() {
        // A project checked out under `~/.local/work/target/app` is watchable;
        // only what the developer asked to watch is filtered.
        let root = Path::new("/home/dev/.local/target/app");
        assert!(!is_ignored(
            Path::new("/home/dev/.local/target/app/dist/a.js"),
            root
        ));
        assert!(is_ignored(
            Path::new("/home/dev/.local/target/app/node_modules/vue.js"),
            root
        ));
    }
}
