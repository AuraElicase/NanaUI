use super::*;
use nana_ui_runtime::{
    DocumentId, HighlightRequest, LayoutViewport, MeasureTextShaper, NativeMarkdown,
};

fn flush(runtime: &mut crate::RuntimeDocument) {
    runtime
        .flush(LayoutViewport::new(430.0, 900.0), &mut MeasureTextShaper)
        .unwrap();
}

#[test]
fn markdown_preserves_large_plain_text_projection_and_removes_it() {
    // Long documents retain unique identities for every drawing command.
    let source = (0..300)
        .map(|i| format!("Paragraph {i}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let view = NativeMarkdown::from_source(&source);
    assert_eq!(view.blocks().len(), 300);
    let expected = view.plain_text();
    let document = DocumentId::new(1).unwrap();
    let mut runtime = crate::RuntimeDocument::new(document);
    let markdown = runtime
        .context_mut()
        .create_component(document, view)
        .unwrap();
    flush(&mut runtime);
    let content = runtime
        .scene()
        .primitives()
        .filter_map(|p| match &p.kind {
            ScenePrimitiveKind::Text { content, .. } if p.node == markdown.stable_id() => {
                Some(content.as_str())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(content.len(), 300);
    assert_eq!(content.join("\n\n"), expected);
    let ids = runtime
        .scene()
        .primitives()
        .filter(|p| p.node == markdown.stable_id())
        .map(|p| p.id)
        .collect::<Vec<_>>();
    assert_eq!(
        ids.iter().collect::<std::collections::HashSet<_>>().len(),
        ids.len()
    );
    runtime.context_mut().remove_view(markdown).unwrap();
    flush(&mut runtime);
    assert!(
        runtime
            .scene()
            .primitives()
            .all(|p| p.node != markdown.stable_id())
    );
}

#[test]
fn markdown_fences_keep_host_presenter_identity_without_duplicate_scene_text() {
    let source = "$$\\frac{1}{\\sqrt{x^2+1}}$$\n\n```mermaid\nflowchart TD\nA-->B\n```";
    let view = NativeMarkdown::from_source(source);
    let expected = view.plain_text();
    let document = DocumentId::new(1).unwrap();
    let mut runtime = crate::RuntimeDocument::new(document);
    let markdown = runtime
        .context_mut()
        .create_component(document, view)
        .unwrap();
    runtime.context_mut().assemble_markdown(markdown).unwrap();
    let children = runtime
        .context()
        .world()
        .node(markdown.stable_id())
        .unwrap()
        .children;
    assert_eq!(children.len(), 2);
    assert_eq!(
        runtime.context().world().highlight_request(children[0]),
        Some(&HighlightRequest::new(
            NativeMarkdown::MATH_PRESENTER,
            "\\frac{1}{\\sqrt{x^2+1}}"
        ))
    );
    assert_eq!(
        runtime.context().world().highlight_request(children[1]),
        Some(&HighlightRequest::new(
            NativeMarkdown::MERMAID_PRESENTER,
            "flowchart TD\nA-->B"
        ))
    );
    runtime.context_mut().assemble_markdown(markdown).unwrap();
    assert_eq!(
        runtime
            .context()
            .world()
            .node(markdown.stable_id())
            .unwrap()
            .children,
        children
    );
    flush(&mut runtime);
    for child in children {
        assert!(runtime.scene().primitives().all(|p| p.node != child));
    }
    let diagram_slot = |p: &ScenePrimitive| {
        p.id.slot >> 32 == u64::from(crate::scene::custom_paint::MARKDOWN_DIAGRAM_PAINT)
    };
    let text = runtime
        .scene()
        .primitives()
        .filter(|p| !diagram_slot(p))
        .filter_map(|p| match &p.kind {
            ScenePrimitiveKind::Text { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        text.is_empty(),
        "typeset blocks do not duplicate their fallback text"
    );
    // The diagram draws its own labels, nothing else.
    let mut labels = runtime
        .scene()
        .primitives()
        .filter(|p| diagram_slot(p))
        .filter_map(|p| match &p.kind {
            ScenePrimitiveKind::Text { content, .. } => Some(content.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    labels.sort_unstable();
    assert_eq!(labels, ["A", "B"]);
    assert_eq!(
        runtime.context().world().text(markdown.stable_id()),
        Some(expected.as_str())
    );
    assert_eq!(runtime.scene().primitives().filter(|p| matches!(&p.kind, ScenePrimitiveKind::Quad { surface, .. } if surface.content_image.is_some())).count(), 1);
}

#[test]
fn markdown_diagrams_draw_as_vector_geometry_reused_until_dropped() {
    let document = DocumentId::new(14).unwrap();
    let mut runtime = crate::RuntimeDocument::new(document);
    let markdown = runtime
        .context_mut()
        .create_component(
            document,
            NativeMarkdown::from_source(
                "Intro\n\n```mermaid\nflowchart LR\nA[Start] -->|go| B{Check} --> C((End))\n```\n\n```mermaid\ngraph TD\nX-->Y\n```",
            ),
        )
        .unwrap();
    flush(&mut runtime);
    let id = markdown.stable_id();
    let in_namespace = |p: &&ScenePrimitive| {
        p.node == id
            && p.id.slot >> 32 == u64::from(crate::scene::custom_paint::MARKDOWN_DIAGRAM_PAINT)
    };
    let meshes = |runtime: &crate::RuntimeDocument| -> Vec<Arc<PathMesh>> {
        runtime
            .scene()
            .primitives()
            .filter(in_namespace)
            .filter_map(|p| match &p.kind {
                ScenePrimitiveKind::Path { mesh, .. } => Some(Arc::clone(mesh)),
                _ => None,
            })
            .collect()
    };
    let first = meshes(&runtime);
    assert!(!first.is_empty(), "edges and non-rect shapes are meshes");
    assert!(
        runtime
            .scene()
            .primitives()
            .filter(in_namespace)
            .any(|p| matches!(p.kind, ScenePrimitiveKind::Quad { .. }))
    );
    // Two diagrams, two slot blocks.
    let blocks: std::collections::BTreeSet<u64> = runtime
        .scene()
        .primitives()
        .filter(in_namespace)
        .map(|p| {
            (p.id.slot & 0xFFFF_FFFF) / crate::scene::custom_paint::MARKDOWN_DIAGRAM_BLOCK as u64
        })
        .collect();
    assert_eq!(blocks.len(), 2);
    assert_eq!(
        runtime
            .scene()
            .markdown_diagram_paint
            .get(&id)
            .map(|b| b.len()),
        Some(2)
    );
    // Selecting text rebuilds the node without re-triangulating anything.
    runtime
        .context_mut()
        .update_component(markdown, |view, _| {
            *view = NativeMarkdown::from_source(
                "Intro\n\n```mermaid\nflowchart LR\nA[Start] -->|go| B{Check} --> C((End))\n```\n\n```mermaid\ngraph TD\nX-->Y\n```",
            )
        })
        .unwrap();
    flush(&mut runtime);
    let again = meshes(&runtime);
    assert_eq!(first.len(), again.len());
    assert!(first.iter().zip(&again).all(|(a, b)| Arc::ptr_eq(a, b)));
    runtime
        .context_mut()
        .update_component(markdown, |view, _| {
            *view = NativeMarkdown::from_source("plain")
        })
        .unwrap();
    flush(&mut runtime);
    assert!(meshes(&runtime).is_empty());
    assert!(runtime.scene().markdown_diagram_paint.is_empty());
}

#[test]
fn markdown_inline_decorations_share_scene_strokes_and_clear_on_update() {
    let document = DocumentId::new(13).unwrap();
    let mut runtime = crate::RuntimeDocument::new(document);
    let markdown = runtime
        .context_mut()
        .create_component(
            document,
            NativeMarkdown::from_source("[link](https://example.test) ~~strike~~ `code`"),
        )
        .unwrap();
    flush(&mut runtime);
    let primitives = runtime
        .scene()
        .primitives()
        .filter(|p| p.node == markdown.stable_id())
        .collect::<Vec<_>>();
    let strokes = primitives
        .iter()
        .filter(|p| matches!(p.kind, ScenePrimitiveKind::Stroke { .. }))
        .collect::<Vec<_>>();
    assert_eq!(strokes.len(), 2);
    assert!(
        strokes
            .iter()
            .all(|p| p.bounds.width > 0.0 && p.bounds.height > 0.0)
    );
    let ids = primitives
        .iter()
        .map(|p| p.id)
        .collect::<std::collections::HashSet<_>>();
    assert_eq!(
        ids.len(),
        primitives.len(),
        "text, code background and decoration slots must remain distinct"
    );
    runtime
        .context_mut()
        .update_component(markdown, |view, _| {
            *view = NativeMarkdown::from_source("plain")
        })
        .unwrap();
    flush(&mut runtime);
    assert!(
        !runtime
            .scene()
            .primitives()
            .any(|p| p.node == markdown.stable_id()
                && matches!(p.kind, ScenePrimitiveKind::Stroke { .. }))
    );
}
