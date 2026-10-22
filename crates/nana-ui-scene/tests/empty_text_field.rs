//! A focused text field through the product frame loop: with nothing in it —
//! no value, no placeholder — it still has a line for its caret, and it says
//! it is focused with its fill as well as its border.

use std::sync::{Arc, Mutex};

use nana_text::font::{FaceDescriptor, FallbackPolicy, FontSystem, GenericFamily, font_blob};
use nana_text::{NativeTextEngine, SharedTextEngine};
use nana_ui_core::SemanticColorRole;
use nana_ui_runtime::view::widget;
use nana_ui_runtime::{
    ComponentGeometry, DocumentId, LayoutBox, LayoutViewport, NanaTextEngineShaper, SemanticPaint,
    StableNodeId, TextInput,
};
use nana_ui_scene::RuntimeDocument;

fn engine() -> SharedTextEngine {
    let mut policy = FallbackPolicy::empty();
    policy.set_generic(GenericFamily::SansSerif, ["Noto Sans SC"]);
    let mut fonts = FontSystem::with_policy(policy);
    fonts
        .register_bytes(
            font_blob(nana_ui_core::fonts::UI_FONT_REGULAR),
            &FaceDescriptor::default(),
        )
        .expect("the bundled UI face registers");
    Arc::new(Mutex::new(NativeTextEngine::new(fonts)))
}

/// `field` mounted, laid out and focused, then flushed again.
fn focused(field: TextInput) -> (RuntimeDocument, StableNodeId) {
    let document = DocumentId::new(1).unwrap();
    let mut runtime = RuntimeDocument::new(document);
    let input = runtime
        .context_mut()
        .mount_view_root(document, || widget(field))
        .unwrap()
        .root::<TextInput>()
        .unwrap()
        .stable_id();
    let mut shaper = NanaTextEngineShaper::new(engine());
    let viewport = LayoutViewport::new(400.0, 200.0);
    runtime.flush(viewport, &mut shaper).unwrap();
    assert!(runtime.context_mut().focus_node(document, input).unwrap());
    runtime.flush(viewport, &mut shaper).unwrap();
    (runtime, input)
}

fn caret(runtime: &RuntimeDocument, id: StableNodeId) -> Option<LayoutBox> {
    match runtime.context().world().component_geometry(id)? {
        ComponentGeometry::TextInput { caret, .. } => caret,
        _ => None,
    }
}

/// The focused field paints [`SemanticPaint::FOCUS_FIELD`]: the focus
/// border over its fill deepened toward the text.
#[test]
fn a_focused_field_deepens_its_fill_and_takes_the_focus_border() {
    let (runtime, id) = focused(TextInput::new(""));
    let world = runtime.context().world();
    let rgba = |c: nana_ui_core::SemanticColor| [c.r, c.g, c.b, c.a];
    let Some(ComponentGeometry::TextInput {
        background, border, ..
    }) = world.component_geometry(id)
    else {
        panic!("the focused field has no geometry");
    };
    let fill = SemanticPaint::FOCUS_FIELD
        .background_mix
        .expect("a deepened fill")
        .resolve(world.style_model());
    let resting = world.style_model().color(SemanticColorRole::Background);
    assert_ne!(rgba(fill), rgba(resting));
    assert_eq!(background, Some(rgba(fill)));
    assert_eq!(
        border,
        Some(rgba(
            world.style_model().color(SemanticColorRole::FocusBorder)
        ))
    );
}

#[test]
fn a_focused_field_shows_its_caret_with_or_without_text() {
    for (name, field) in [
        ("empty", TextInput::new("")),
        ("placeholder", TextInput::new("").placeholder("Name")),
        ("value", TextInput::new("Nana")),
    ] {
        let (runtime, id) = focused(field);
        let caret = caret(&runtime, id).unwrap_or_else(|| panic!("{name}: no caret"));
        assert!(caret.height > 0.0, "{name}: {caret:?}");
    }
}
