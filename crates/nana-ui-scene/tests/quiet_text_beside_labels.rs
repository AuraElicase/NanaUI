//! A list row's note and a select option's hint reach the scene as their own
//! text, beside the label they go with.

use nana_ui_core::ControlSize;
use nana_ui_runtime::{
    DocumentId, Entity, LayoutViewport, ListItem, Select, SelectOption, StableNodeId, Stack,
};
use nana_ui_scene::{RuntimeDocument, ScenePrimitiveKind};

fn flush(document: &mut RuntimeDocument) {
    let id = document.document();
    document
        .flush_with(|context, work| {
            if !work.layout_frontier_seeds.is_empty() {
                context.layout_document(id, LayoutViewport::new(480.0, 2_400.0))?;
            }
            Ok(())
        })
        .unwrap();
}

/// Every text the scene paints for `node`.
fn texts(document: &RuntimeDocument, node: StableNodeId) -> Vec<String> {
    document
        .scene()
        .primitives()
        .filter(|primitive| primitive.node == node)
        .filter_map(|primitive| match &primitive.kind {
            ScenePrimitiveKind::Text { content, .. } => Some(content.to_string()),
            _ => None,
        })
        .collect()
}

fn mount<C: nana_ui_runtime::ComponentView>(
    document: &mut RuntimeDocument,
    component: C,
) -> Entity<C> {
    let id = document.document();
    let root = document
        .context_mut()
        .create_component(id, Stack::column(4.0))
        .unwrap();
    let entity = document
        .context_mut()
        .create_detached_component(id, component)
        .unwrap();
    document.context_mut().append_child(root, entity).unwrap();
    flush(document);
    entity
}

#[test]
fn a_rows_note_paints_after_its_label() {
    let mut document = RuntimeDocument::new(DocumentId::new(1).unwrap());
    let row = mount(
        &mut document,
        ListItem::new("角度X")
            .note("ParamAngleX")
            .detail("头部左右转"),
    );
    let texts = texts(&document, row.stable_id());
    for text in ["角度X", "ParamAngleX", "头部左右转"] {
        assert!(
            texts.iter().any(|painted| painted == text),
            "{text} in {texts:?}"
        );
    }
}

#[test]
fn a_long_select_menu_keeps_every_row_and_its_hints() {
    let mut document = RuntimeDocument::new(DocumentId::new(1).unwrap());
    let mut select = Select::new(Some("0")).size(ControlSize::Small);
    select.options = (0..41)
        .map(|index| {
            SelectOption::new(index.to_string(), format!("Option {index}"))
                .hint(format!("H{index}"))
        })
        .collect();
    let select = mount(&mut document, select);
    document.context_mut().toggle_select(select).unwrap();
    flush(&mut document);
    // The 31st row's highlight once took the first row's label slot.
    document
        .context_mut()
        .update_component(select, |select, _| select.highlighted = Some(30))
        .unwrap();
    flush(&mut document);
    let texts = texts(&document, select.stable_id());
    for text in ["Option 0", "Option 30", "Option 40", "H0", "H30", "H40"] {
        assert!(
            texts.iter().any(|painted| painted == text),
            "{text} in {texts:?}"
        );
    }
    // The field shows the chosen option's hint after its label.
    assert_eq!(
        texts.iter().filter(|painted| *painted == "H0").count(),
        2,
        "the field's hint and its menu row's: {texts:?}"
    );
}
