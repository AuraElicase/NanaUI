//! The rich editor's clipboard: the system clipboard holds plain text only,
//! and the process remembers the last rich piece it copied.
//!
//! A paste takes that piece back, styling and objects with it, only while the
//! clipboard still holds the write that copied it. Where the host keeps a
//! clipboard change count, that is the count read right after the copy being
//! the count at the paste, so a later copy elsewhere of the same text pastes
//! as plain text. Where it keeps none, the clipboard text being the piece's
//! text stands in.

use std::sync::Mutex;

use nana_ui_core::RichText;
use nana_ui_input::HostServices;

use super::{RichTextEditor, RichTextEditorEvent};
use crate::{AppContext, Entity, FrameworkError};

/// A clipboard operation on a [`RichTextEditor`], as an edit menu, context
/// menu or toolbar issues it through [`AppContext::rich_clipboard`]. The
/// keyboard's Ctrl/Cmd + C, X and V run the same operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RichClipboardCommand {
    Copy,
    Cut,
    /// The editor's own copy with its styling and objects while the
    /// clipboard still holds it, else the clipboard's text, typed as typing
    /// would style it.
    Paste,
    /// The clipboard's text, typed as typing would style it, even over the
    /// editor's own copy.
    PastePlainText,
}

/// The last piece copied out of a rich editor, and the clipboard's change
/// count right after it was written (`None` where the host keeps none).
#[derive(Debug, Clone)]
struct RichCopy {
    change_count: Option<u64>,
    piece: RichText,
}

static RICH_CLIPBOARD: Mutex<Option<RichCopy>> = Mutex::new(None);

/// The remembered piece, if the clipboard still holds the write that copied
/// it. `change_count` is read after `text`: an equal count means nothing
/// was written since the copy, so `text` is that copy's.
fn own_copy(
    text: &str,
    change_count: Result<u64, nana_ui_input::HostServiceError>,
) -> Option<RichText> {
    let clipboard = RICH_CLIPBOARD.lock().ok()?;
    let copy = clipboard.as_ref()?;
    let same_write = match copy.change_count {
        Some(copied) => change_count == Ok(copied),
        None => true,
    };
    (same_write && copy.piece.text() == text).then(|| copy.piece.clone())
}

impl AppContext {
    /// Run `command` on `entity`'s clipboard through the host's `services`.
    /// Returns whether it did anything.
    ///
    /// Nothing happens on a disabled editor or while the IME composes. Copy
    /// works on a read-only editor; cut and paste do not. An empty selection
    /// copies nothing and leaves the clipboard as it was. A refused write
    /// copies nothing and a cut then deletes nothing. A paste with no text
    /// to insert emits [`RichTextEditorEvent::PasteRequested`] for the
    /// application's own formats (an image). Every edit is one undo step and
    /// reports [`RichTextEditorEvent::Changed`].
    pub fn rich_clipboard(
        &mut self,
        entity: Entity<RichTextEditor>,
        command: RichClipboardCommand,
        services: &mut dyn HostServices,
    ) -> Result<bool, FrameworkError> {
        let allowed = self.read(entity, |view| {
            !view.disabled
                && view.state.preedit.is_none()
                && (command == RichClipboardCommand::Copy || view.editable())
        })?;
        if !allowed {
            return Ok(false);
        }
        match command {
            RichClipboardCommand::Copy | RichClipboardCommand::Cut => {
                let piece = self.read(entity, |view| {
                    let range = view.range();
                    (!range.is_empty()).then(|| view.value.slice(range))
                })?;
                let Some(piece) = piece else {
                    return Ok(false);
                };
                if services.write_clipboard(piece.text()).is_err() {
                    return Ok(false);
                }
                let change_count = services.clipboard_change_count().ok();
                if let Ok(mut clipboard) = RICH_CLIPBOARD.lock() {
                    *clipboard = Some(RichCopy {
                        change_count,
                        piece,
                    });
                }
                if command == RichClipboardCommand::Cut {
                    self.edit_with(entity, |view, _| {
                        let range = view.range();
                        view.delete(range)
                    })?;
                }
                Ok(true)
            }
            RichClipboardCommand::Paste | RichClipboardCommand::PastePlainText => {
                let text = services
                    .read_clipboard()
                    .ok()
                    .flatten()
                    .filter(|text| !text.is_empty());
                let Some(text) = text else {
                    self.update_component(entity, |_, cx| {
                        cx.emit(RichTextEditorEvent::PasteRequested)
                    })?;
                    return Ok(true);
                };
                let piece = (command == RichClipboardCommand::Paste)
                    .then(|| own_copy(&text, services.clipboard_change_count()))
                    .flatten();
                self.edit_with(entity, |view, _| match &piece {
                    Some(piece) => view.replace_selection(piece, false),
                    None => {
                        // A paste is its own undo step, not part of the
                        // typing around it.
                        view.state.coalesce = false;
                        let changed = view.insert_text(&text.replace("\r\n", "\n"));
                        view.state.coalesce = false;
                        changed
                    }
                })
            }
        }
    }
}
