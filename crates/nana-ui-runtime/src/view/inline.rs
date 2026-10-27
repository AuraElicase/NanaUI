//! Authoring projection: text pieces remain text, children keep ordinary ids.
use super::{
    AnyView, Class, El, FieldWrite, InlineStyle, IntoProp, IntoView, NodeRef, PropSource,
    ViewBuilder, ViewSource, widget,
};
use crate::{InlineContent, InlineFlow, NodeStyle, StableNodeId};
use std::{
    borrow::Cow,
    sync::{Arc, Mutex},
};

enum Piece {
    Text(PropSource<String>),
    Styled(InlineContent),
    Span(PropSource<String>, Box<nana_ui_core::RichSpanStyle>),
    Child(AnyView),
}
enum Source {
    Text(PropSource<String>),
    Styled(InlineContent),
    Span(PropSource<String>, Box<nana_ui_core::RichSpanStyle>),
    Child(usize),
}

/// A native paragraph. Text pieces are not retained Text components, and each
/// child is the same component a Row or Grid would own.
pub struct Inline {
    element: El<InlineFlow>,
    pieces: Vec<Piece>,
}
pub fn inline() -> Inline {
    Inline {
        element: widget(InlineFlow::new("")),
        pieces: Vec::new(),
    }
}
impl Inline {
    pub fn text(mut self, text: impl IntoProp<String>) -> Self {
        self.pieces.push(Piece::Text(text.into_source()));
        self
    }
    pub fn styled(mut self, content: impl Into<InlineContent>) -> Self {
        self.pieces.push(Piece::Styled(content.into().normalized()));
        self
    }
    pub fn span(mut self, text: impl IntoProp<String>, style: nana_ui_core::RichSpanStyle) -> Self {
        self.pieces
            .push(Piece::Span(text.into_source(), Box::new(style)));
        self
    }
    pub fn child(mut self, child: impl IntoView) -> Self {
        self.pieces.push(Piece::Child(child.into_any()));
        self
    }
    pub fn line_break(self) -> Self {
        self.text("\n")
    }
    pub fn key(mut self, key: impl Into<Cow<'static, str>>) -> Self {
        self.element = self.element.key(key);
        self
    }
    pub fn node_ref(mut self, node_ref: NodeRef) -> Self {
        self.element = self.element.node_ref(node_ref);
        self
    }
    pub fn source_site(mut self, source: &'static ViewSource) -> Self {
        self.element = self.element.source_site(source);
        self
    }
    pub fn class(mut self, class: Class) -> Self {
        self.element = self.element.class(class);
        self
    }
    pub fn class_when(mut self, class: Class, condition: impl IntoProp<bool>) -> Self {
        self.element = self.element.class_when(class, condition);
        self
    }
    pub fn css(mut self, style: InlineStyle) -> Self {
        self.element = self.element.css(style);
        self
    }
    pub fn visible(mut self, visible: impl IntoProp<bool>) -> Self {
        self.element = self.element.visible(visible);
        self
    }
    pub fn style(mut self, style: NodeStyle) -> Self {
        self.element = self
            .element
            .map_component(|component| component.style(style));
        self
    }
}
struct Content;
impl FieldWrite<InlineFlow, InlineContent> for Content {
    const FIELD: &'static str = "content";
    fn write(target: &mut InlineFlow, value: InlineContent) {
        target.content = value;
    }
    fn differs(target: &InlineFlow, value: &InlineContent) -> bool {
        target.content != *value
    }
}
impl IntoView for Inline {
    fn build(self, vb: &mut ViewBuilder<'_, '_, '_>) {
        let dynamic=self.pieces.iter().any(|piece|matches!(piece,Piece::Text(source)|Piece::Span(source,_) if !matches!(source,PropSource::Const(_))));
        let total_children = self
            .pieces
            .iter()
            .filter(|piece| matches!(piece, Piece::Child(_)))
            .count();
        let offsets = Arc::new(Mutex::new(Vec::<usize>::with_capacity(total_children)));
        let mut element = self.element;
        let mut sources = Vec::new();
        let mut child_index = 0;
        let ids = Arc::new(Mutex::new(Vec::<StableNodeId>::new()));
        let mut static_content = InlineContent::builder();
        let mut offset = 0;
        for piece in self.pieces {
            match piece {
                Piece::Text(source) => {
                    if let PropSource::Const(text) = &source {
                        offset += text.len();
                        static_content = static_content.text(text.as_str());
                    }
                    sources.push(Source::Text(source));
                }
                Piece::Styled(content) => {
                    offset += content.text().len();
                    static_content = static_content.span(content.clone());
                    sources.push(Source::Styled(content));
                }
                Piece::Span(source, style) => {
                    if let PropSource::Const(text) = &source {
                        offset += text.len();
                        static_content =
                            static_content.styled(text.as_str(), style.as_ref().clone());
                    }
                    sources.push(Source::Span(source, style));
                }
                Piece::Child(child) => {
                    sources.push(Source::Child(child_index));
                    child_index += 1;
                    let ids = Arc::clone(&ids);
                    let last = child_index == total_children;
                    offsets.lock().unwrap().push(offset);
                    let offsets = Arc::clone(&offsets);
                    element = element.child_slot(child, move |mut component, id| {
                        let mut ids = ids.lock().unwrap();
                        ids.push(id);
                        if !dynamic && last {
                            let atoms = ids
                                .iter()
                                .zip(offsets.lock().unwrap().iter())
                                .map(|(&node, &offset)| crate::InlineAtomic {
                                    offset,
                                    node,
                                    text: Default::default(),
                                })
                                .collect::<Vec<_>>();
                            component.content = InlineContent::from_parts(
                                component.content.to_rich_text(),
                                atoms.into(),
                            );
                        }
                        component
                    });
                    static_content = static_content.text("\u{fffc}");
                    offset += '\u{fffc}'.len_utf8();
                }
            }
        }
        element = element.map_component(|mut component| {
            component.content = static_content.build();
            component
        });
        if dynamic {
            element = element.prop::<InlineContent, Content>(move || {
                let ids = ids.lock().unwrap();
                let mut content = InlineContent::builder();
                for source in &sources {
                    content = match source {
                        Source::Text(text) => content.text(text.get()),
                        Source::Styled(text) => content.span(text.clone()),
                        Source::Span(text, style) => {
                            content.styled(text.get(), style.as_ref().clone())
                        }
                        Source::Child(index) => content.child(ids[*index]),
                    };
                }
                content.build()
            });
        }
        element.build(vb);
    }
}
impl super::controls::StyledComponent for InlineFlow {
    fn node_style(&self) -> &NodeStyle {
        &self.style
    }
    fn node_style_mut(&mut self) -> &mut NodeStyle {
        &mut self.style
    }
}
