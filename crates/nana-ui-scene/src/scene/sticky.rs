use super::*;

impl UiScene {
    pub(super) fn sticky_translation(&self, node: &ExtractedNode) -> [f32; 2] {
        if node.source_style.layout.position != nana_ui_core::PositionSpec::Sticky {
            return [0.0; 2];
        }
        let ancestors = node
            .parent
            .into_iter()
            .flat_map(|parent| ancestor_nodes(&self.nodes, parent))
            .map(|(_, ancestor)| nana_ui_runtime::StickyAncestor {
                style: &ancestor.source_style.layout,
                bounds: ancestor.layout,
                result: ancestor.layout_result.as_deref(),
                scroll: ancestor.scroll_offset,
                font_size: ancestor.style.font_size,
                writing: ancestor.style.writing_context(),
            });
        nana_ui_runtime::sticky_translation(
            &node.source_style.layout,
            node.layout,
            node.style.font_size,
            node.style.writing_context(),
            ancestors,
        )
    }

    pub(super) fn with_sticky_translation(
        &self,
        node: &ExtractedNode,
        transform: AffineTransform,
    ) -> AffineTransform {
        let [x, y] = self.sticky_translation(node);
        AffineTransform::from_matrix([1.0, 0.0, 0.0, 1.0, x, y]).then(transform)
    }

    pub(super) fn logical_local_transform(
        &self,
        node: &ExtractedNode,
        blocks_3d: bool,
    ) -> AffineTransform {
        self.with_sticky_translation(
            node,
            node_scene_transform(&node.source_style.layout, node.layout, blocks_3d),
        )
    }
}
