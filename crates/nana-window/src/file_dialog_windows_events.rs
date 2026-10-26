//! Save-dialog events. A type change moves the default extension and the
//! visible name to the new type; with `enforce_extension`, `OnFileOk` keeps
//! the dialog open when the completed name would replace an existing file
//! the native overwrite prompt never saw.
use super::{FileDialogRequest, FileDialogResult, dialog_file_name, shell_item_path, wide};
use crate::file_dialog::{default_extension, retyped_name};
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::Win32::Foundation::S_FALSE;
use windows::Win32::UI::Shell::{
    FDE_OVERWRITE_RESPONSE, FDE_SHAREVIOLATION_RESPONSE, FDEOR_DEFAULT, FDESVR_DEFAULT,
    IFileDialog, IFileDialogEvents, IFileDialogEvents_Impl, IShellItem,
};
use windows::core::{Error, PCWSTR, Ref, Result, implement};

/// The selected type as an index into the request's filters.
/// `GetFileTypeIndex` is one-based; anything outside the request is `None`.
pub(super) fn selected_filter(dialog: &IFileDialog, count: usize) -> Option<usize> {
    let index = unsafe { dialog.GetFileTypeIndex() }.ok()?;
    let index = usize::try_from(index).ok()?.checked_sub(1)?;
    (index < count).then_some(index)
}

/// Unadvises on drop, before the dialog is released.
pub(super) struct Subscription {
    dialog: IFileDialog,
    cookie: u32,
}

impl Drop for Subscription {
    fn drop(&mut self) {
        let _ = unsafe { self.dialog.Unadvise(self.cookie) };
    }
}

/// Only a save dialog with types to switch between needs the events.
pub(super) fn subscribe(
    dialog: &IFileDialog,
    request: &FileDialogRequest,
) -> Result<Option<Subscription>> {
    if !request.kind.is_save() || request.filters.is_empty() {
        return Ok(None);
    }
    let events: IFileDialogEvents = TypeEvents {
        request: request.clone(),
    }
    .into();
    let cookie = unsafe { dialog.Advise(&events) }?;
    Ok(Some(Subscription {
        dialog: dialog.clone(),
        cookie,
    }))
}

#[implement(IFileDialogEvents)]
struct TypeEvents {
    request: FileDialogRequest,
}

impl TypeEvents {
    fn type_changed(&self, dialog: &IFileDialog) -> Result<()> {
        let filters = &self.request.filters;
        let Some(extension) = selected_filter(dialog, filters.len())
            .and_then(|index| default_extension(&filters[index]))
        else {
            // A wildcard type keeps the previous default and name.
            return Ok(());
        };
        let value = wide(extension);
        unsafe { dialog.SetDefaultExtension(PCWSTR(value.as_ptr())) }?;
        if let Some(name) = dialog_file_name(dialog)
            && let Some(name) = retyped_name(&name, filters, extension)
        {
            set_file_name(dialog, OsStr::new(&name))?;
        }
        Ok(())
    }

    fn file_ok(&self, dialog: &IFileDialog) -> Result<()> {
        if !self.request.enforce_extension {
            return Ok(());
        }
        let item = unsafe { dialog.GetResult() }?;
        // An unreadable result is reported by the completion, not here.
        let Ok(path) = shell_item_path(&item) else {
            return Ok(());
        };
        let selected = selected_filter(dialog, self.request.filters.len());
        let completed = self.request.apply_extension_policy(
            FileDialogResult::selected(self.request.id, vec![path.clone()])
                .with_selected_filter(selected),
        );
        let Some(target) = completed.paths.first().filter(|target| **target != path) else {
            return Ok(());
        };
        // A new name is completed after the dialog closes. An existing (or
        // unknowable) one is shown instead, so saving again runs the native
        // overwrite prompt against the file that will actually be written.
        if !target.try_exists().unwrap_or(true) {
            return Ok(());
        }
        if let Some(name) = target.file_name() {
            set_file_name(dialog, name)?;
        }
        Err(Error::from_hresult(S_FALSE))
    }
}

#[allow(non_snake_case)]
impl IFileDialogEvents_Impl for TypeEvents_Impl {
    fn OnTypeChange(&self, dialog: Ref<IFileDialog>) -> Result<()> {
        match dialog.as_ref() {
            Some(dialog) => self.type_changed(dialog),
            None => Ok(()),
        }
    }

    fn OnFileOk(&self, dialog: Ref<IFileDialog>) -> Result<()> {
        match dialog.as_ref() {
            Some(dialog) => self.file_ok(dialog),
            None => Ok(()),
        }
    }

    fn OnFolderChanging(&self, _: Ref<IFileDialog>, _: Ref<IShellItem>) -> Result<()> {
        Ok(())
    }

    fn OnFolderChange(&self, _: Ref<IFileDialog>) -> Result<()> {
        Ok(())
    }

    fn OnSelectionChange(&self, _: Ref<IFileDialog>) -> Result<()> {
        Ok(())
    }

    fn OnShareViolation(
        &self,
        _: Ref<IFileDialog>,
        _: Ref<IShellItem>,
    ) -> Result<FDE_SHAREVIOLATION_RESPONSE> {
        Ok(FDESVR_DEFAULT)
    }

    fn OnOverwrite(
        &self,
        _: Ref<IFileDialog>,
        _: Ref<IShellItem>,
    ) -> Result<FDE_OVERWRITE_RESPONSE> {
        Ok(FDEOR_DEFAULT)
    }
}

fn set_file_name(dialog: &IFileDialog, name: &OsStr) -> Result<()> {
    let name: Vec<u16> = name.encode_wide().chain(Some(0)).collect();
    unsafe { dialog.SetFileName(PCWSTR(name.as_ptr())) }
}

#[cfg(test)]
mod tests {
    use super::super::{ComScope, NativeDialog, configure};
    use super::*;
    use crate::file_dialog::{FileDialogKind, FileFilter};

    fn request() -> FileDialogRequest {
        FileDialogRequest::new(71, FileDialogKind::SaveFile)
            .file_name("剧本.json")
            .filters([
                FileFilter::new("JSON", ["json"]),
                FileFilter::new("文本", ["txt"]),
                FileFilter::new("全部", ["*"]),
            ])
    }

    #[test]
    fn a_type_change_renames_the_visible_extension_and_reports_the_index() {
        let _com = ComScope::enter().unwrap();
        let request = request();
        let native = NativeDialog::create(request.kind).unwrap();
        configure(&native, &request).unwrap();
        let dialog = native.file_dialog();
        let events = TypeEvents {
            request: request.clone(),
        };
        assert_eq!(selected_filter(dialog, 3), Some(0));
        for (index, name) in [(2, "剧本.txt"), (3, "剧本.txt"), (1, "剧本.json")] {
            unsafe { dialog.SetFileTypeIndex(index) }.unwrap();
            events.type_changed(dialog).unwrap();
            assert_eq!(dialog_file_name(dialog).as_deref(), Some(name));
            assert_eq!(selected_filter(dialog, 3), Some(index as usize - 1));
        }
        assert_eq!(selected_filter(dialog, 0), None);
    }

    #[test]
    fn a_save_dialog_with_types_subscribes_and_unadvises() {
        let _com = ComScope::enter().unwrap();
        let request = request();
        let native = NativeDialog::create(request.kind).unwrap();
        configure(&native, &request).unwrap();
        let subscription = subscribe(native.file_dialog(), &request).unwrap();
        assert!(subscription.is_some());
        drop(subscription);
        let open = FileDialogRequest::new(72, FileDialogKind::OpenFile).filters(request.filters);
        let native = NativeDialog::create(open.kind).unwrap();
        assert!(subscribe(native.file_dialog(), &open).unwrap().is_none());
    }

    #[test]
    fn set_file_name_round_trips() {
        let _com = ComScope::enter().unwrap();
        let native = NativeDialog::create(FileDialogKind::SaveFile).unwrap();
        set_file_name(native.file_dialog(), OsStr::new("v1.2.json")).unwrap();
        assert_eq!(
            dialog_file_name(native.file_dialog()).as_deref(),
            Some("v1.2.json")
        );
    }
}
