---
name: nanaui-window-materials
description: Maintain NanaUI's platform-owned native window boundary. Use when changing nana-window, title-bar chrome, raw handles, native materials, transparency, resize behavior, or platform fallbacks.
---

# NanaUI Window and Platform Boundary

Read [`docs/reference/window.md`](../../../docs/reference/window.md) and the platform contracts in [`docs/reference/architecture.md`](../../../docs/reference/architecture.md).

- Keep raw window handles and platform APIs inside `nana-window` and `nana-ui-platform`. Ordinary controls consume public outcomes and commands only.
- Reuse `nana-ui-platform::path` (`nana_ui::path`) for path normalization and read-only display; preserve extended paths when required for I/O. Display formatting does no I/O and its optional U+200B wrapping must never reach persistence, clipboard, editable fields or native commands. See the path contract in `docs/reference/window.md`.
- Clear an existing effect before reapplying it. Return the effect actually applied or an explicit fallback; never report a requested effect after failure.
- Use a readable opaque fallback when a native material is unavailable. Do not silently substitute a different material family.
- Keep title-bar drag, client-area chrome, native controls, scaling, input, IME, clipboard, display, and fullscreen ownership in the platform boundary. Framework UI supplies layout slots and public state.
- Window focus changes cancel native preedit as well as Runtime preedit. Defer native IME resets to the end of the event turn, retain loss/regain edges even for the same editor, and invoke the text client's unmarkText so its marked range clears before AppKit's input-context buffer; keep an unchanged AppKit session active instead of disabling it again. Cancel only through the key window: queued focus loss can arrive after another client activates; reset the old client on regain. Map empty winit Preedit to CompositionInput::End; an empty Update would restart Runtime composition and block ordinary editing. Suppress native text tagged to Command or non-AltGr Control chords even when the command is a no-op. Do not gate IME continuously on key-window status or replace real cross-window Pinyin evidence with pasted text.
- Route Scene/GPU changes to `$nanaui-gpu-integration` and evidence to `$nanaui-validation`.

Require a real target-platform window before claiming native material, resize, cleanup, or visual acceptance; compilation for another target is not runtime evidence.
