//! Window-owned system dialogs. Completion invokes the host's callback once;
//! no process-global queue or per-frame polling is involved.

pub use nana_ui_core::{
    FileDialogError, FileDialogKind, FileDialogRequest, FileDialogResult, FileFilter,
};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDialogSupport {
    System,
    Unavailable,
}

pub const fn file_dialog_support() -> FileDialogSupport {
    if cfg!(any(
        target_os = "macos",
        target_os = "windows",
        target_os = "linux"
    )) {
        FileDialogSupport::System
    } else {
        FileDialogSupport::Unavailable
    }
}

/// Owns the native presentation. Dropping it dismisses an unfinished picker.
/// Keep it on the host thread (AppKit sheet teardown is main-thread only).
pub struct FileDialogHandle(Option<Box<dyn FnOnce()>>);
impl FileDialogHandle {
    pub(crate) fn new(cancel: impl FnOnce() + 'static) -> Self {
        Self(Some(Box::new(cancel)))
    }
}
impl Drop for FileDialogHandle {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel();
        }
    }
}

/// Open a dialog without blocking the host event loop. The caller retains
/// request/window identity and must reject callbacks for closed windows.
/// A retained parent keeps raw handles valid for the worker's lifetime.
pub fn open_file_dialog<W>(
    window: Arc<W>,
    request: FileDialogRequest,
    completion: impl FnOnce(FileDialogResult) + Send + 'static,
) -> Result<FileDialogHandle, FileDialogError>
where
    W: HasWindowHandle + HasDisplayHandle + Send + Sync + ?Sized + 'static,
{
    window
        .window_handle()
        .map_err(|_| FileDialogError::WindowClosed)?;
    // Every backend completes through the request's extension policy; the
    // Windows dialog also applies it before closing (see `file_dialog_windows`).
    let policy = request.clone();
    let completion = move |result| completion(policy.apply_extension_policy(result));
    #[cfg(target_os = "macos")]
    {
        crate::platform::open_file_dialog(window.as_ref(), request, Box::new(completion))
    }
    #[cfg(target_os = "windows")]
    {
        windows_dialog::open(window, request, completion)
    }
    #[cfg(target_os = "linux")]
    {
        linux::open(window, request, completion)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        let _ = (window, request, completion);
        Err(FileDialogError::Unavailable)
    }
}

/// Reads back how the platform configured a dialog for `request`, without
/// presenting it: title, starting directory, allowed extensions.
///
/// This complements hosted interaction checks by inspecting configuration
/// without displaying a native picker. Returns `None` where the platform
/// cannot be queried.
///
/// On Windows, directory comes from `GetFolder`. Title is echoed after
/// `SetTitle` (no COM getter). Folder kinds report no extensions.
pub fn describe_configured_dialog(
    request: &FileDialogRequest,
) -> Option<(Option<String>, Option<String>, Vec<String>)> {
    #[cfg(target_os = "macos")]
    {
        crate::platform::describe_configured_panel(request)
    }
    #[cfg(target_os = "windows")]
    {
        windows_dialog::describe(request)
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = request;
        None
    }
}

/// The extension a dialog appends to a name typed without one while `filter`
/// is selected: its first concrete extension. A wildcard type has none.
#[cfg(any(target_os = "windows", test))]
fn default_extension(filter: &FileFilter) -> Option<&str> {
    filter
        .extensions
        .first()
        .map(AsRef::as_ref)
        .filter(|extension| !extension.is_empty() && *extension != "*")
}

/// `name` without `.extension`, compared ignoring case, when what remains is
/// not empty.
#[cfg(any(target_os = "windows", test))]
fn stem_without<'a>(name: &'a str, extension: &str) -> Option<&'a str> {
    let (dot, _) = name.char_indices().rev().nth(extension.chars().count())?;
    let (stem, suffix) = name.split_at(dot);
    let suffix = suffix.strip_prefix('.')?;
    (!stem.is_empty() && suffix.to_lowercase() == extension.to_lowercase()).then_some(stem)
}

/// The save name after the user switches to a type whose default extension
/// is `extension`. Only a name ending with one of the request's own
/// extensions is renamed; `v1.2` or a hand-typed suffix is left to the
/// dialog's default extension and the request's extension policy.
#[cfg(any(target_os = "windows", test))]
fn retyped_name(name: &str, filters: &[FileFilter], extension: &str) -> Option<String> {
    let stem = filters
        .iter()
        .flat_map(|filter| &filter.extensions)
        .filter(|known| !known.is_empty() && known.as_ref() != "*")
        .filter_map(|known| stem_without(name, known))
        // The longest matching extension (`tar.gz` over `gz`) leaves the shortest stem.
        .min_by_key(|stem| stem.len())?;
    if stem_without(name, extension) == Some(stem) {
        return None;
    }
    Some(format!("{stem}.{extension}"))
}

#[cfg(target_os = "linux")]
#[path = "file_dialog_linux.rs"]
mod linux;

#[cfg(target_os = "windows")]
#[path = "file_dialog_windows.rs"]
mod windows_dialog;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn releasing_a_native_session_runs_its_cancellation_once() {
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = count.clone();
        let handle = FileDialogHandle::new(move || {
            observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        });
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        drop(handle);
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn switching_type_replaces_only_a_requested_extension() {
        let filters = [
            FileFilter::new("JSON", ["json"]),
            FileFilter::new("文本", ["txt", "md"]),
            FileFilter::new("归档", ["tar.gz", "gz"]),
            FileFilter::new("全部", ["*"]),
        ];
        let retype = |name: &str, extension: &str| retyped_name(name, &filters, extension);
        assert_eq!(retype("剧本.json", "txt").as_deref(), Some("剧本.txt"));
        assert_eq!(retype("Notes.MD", "json").as_deref(), Some("Notes.json"));
        assert_eq!(retype("v1.2.tar.gz", "json").as_deref(), Some("v1.2.json"));
        // Already the new type, ignoring case.
        assert_eq!(retype("REPORT.JSON", "json"), None);
        // Not one of the request's extensions: the dot is part of the name.
        assert_eq!(retype("v1.2", "json"), None);
        assert_eq!(retype("json", "txt"), None);
        assert_eq!(retype(".json", "txt"), None);
    }

    #[test]
    fn a_wildcard_type_has_no_default_extension() {
        assert_eq!(
            default_extension(&FileFilter::new("文本", ["txt", "md"])),
            Some("txt")
        );
        assert_eq!(default_extension(&FileFilter::new("全部", ["*"])), None);
        assert_eq!(
            default_extension(&FileFilter::new("空", Vec::<&str>::new())),
            None
        );
    }
}
