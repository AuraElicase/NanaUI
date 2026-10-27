//! Issue #202: replaced content is one kind of layout item in every context.
//!
//! - Gate A: an image with no size of its own takes the natural size its
//!   resource reports, and one with only a width takes its height from the
//!   aspect ratio, as a flex item, a grid item, an atomic inline box and an
//!   absolutely positioned box alike. Each matches a cold layout.
//! - Gate B: replaced content aligns by its border-box bottom: as an atom of
//!   a native paragraph and under `align-items: baseline`, an image's and a
//!   custom renderer's bottom edge sits on the text's baseline.

#![cfg(test)]

use nana_ui_core::{
    AlignSpec, DisplaySpec, FlexDirection, GridTrack, LayoutStyle, LengthSpec, PositionSpec,
};

use super::issue263::{image_layout, url};
use super::reflow_oracle::{Builder, assert_matches_cold, bundled_face_shaper, product_frame};
use super::{DocumentId, StableNodeId, UiWorld};
use crate::{
    AppContext, CustomRenderNode, InlineContent, InlineFlow, LayoutViewport, MutationQueue,
    NanaTextEngineShaper, ParticipationKind as Part, ReplacedMetadata,
};

fn viewport() -> LayoutViewport {
    LayoutViewport::new(800.0, 600.0)
}

/// How a context's container and its replaced children are styled.
#[derive(Debug, Clone, Copy)]
enum Context {
    Flex,
    /// A flex row that stretches its items down the line.
    FlexStretch,
    Grid,
    /// A grid that stretches its items down their areas.
    GridStretch,
    Inline,
    Absolute,
}

impl Context {
    fn container(self) -> LayoutStyle {
        let mut layout = LayoutStyle {
            width: Some(LengthSpec::Px(600.0)),
            ..LayoutStyle::default()
        };
        match self {
            Self::Flex => {
                layout.display = Some(DisplaySpec::Flex);
                layout.direction = Some(FlexDirection::Row);
            }
            Self::FlexStretch => {
                layout.display = Some(DisplaySpec::Flex);
                layout.direction = Some(FlexDirection::Row);
                layout.align_items = AlignSpec::Stretch;
            }
            Self::Grid | Self::GridStretch => {
                layout.display = Some(DisplaySpec::Grid);
                layout.grid_columns = Some(vec![GridTrack::Auto, GridTrack::Auto]);
                if matches!(self, Self::GridStretch) {
                    layout.align_items = AlignSpec::Stretch;
                }
            }
            Self::Inline => layout.display = Some(DisplaySpec::Block),
            Self::Absolute => {
                layout.display = Some(DisplaySpec::Block);
                layout.position = PositionSpec::Relative;
                layout.height = Some(LengthSpec::Px(300.0));
            }
        }
        layout
    }

    fn item(self, mut layout: LayoutStyle, offset: f32) -> LayoutStyle {
        match self {
            Self::Flex | Self::FlexStretch | Self::Grid | Self::GridStretch => {}
            Self::Inline => layout.display = Some(DisplaySpec::InlineBlock),
            Self::Absolute => {
                layout.position = PositionSpec::Absolute;
                layout.offset_top = Some(LengthSpec::Px(offset));
                layout.offset_left = Some(LengthSpec::Px(offset));
            }
        }
        layout
    }

    fn participation(self) -> Part {
        match self {
            Self::Flex | Self::FlexStretch => Part::FlexItem,
            Self::Grid | Self::GridStretch => Part::GridItem,
            Self::Inline => Part::AtomicInline,
            Self::Absolute => Part::Absolute,
        }
    }
}

struct Page {
    context: AppContext,
    document: DocumentId,
    shaper: NanaTextEngineShaper,
    /// Shows `pic` with no size of its own.
    auto: StableNodeId,
    /// Shows `pic` 60 wide; its height follows the aspect ratio.
    wide: StableNodeId,
}

impl Page {
    fn new(context: Context, reported: bool) -> Self {
        let document = DocumentId::new(1).unwrap();
        let (mut b, page) = Builder::page(document, 1, 800.0);
        let container = b.element(page, context.container());
        let auto = b.element(
            container,
            context.item(image_layout("pic", None, None), 0.0),
        );
        let wide = b.element(
            container,
            context.item(image_layout("pic", Some(60.0), None), 100.0),
        );
        if reported {
            b.queue
                .set_replaced_metadata(url("pic"), Some(ReplacedMetadata::new(120.0, 80.0)));
        }
        let mut app = AppContext::new();
        app.commit_mutations(b.queue).unwrap();
        let mut page = Self {
            context: app,
            document,
            shaper: bundled_face_shaper(),
            auto,
            wide,
        };
        page.frame();
        page
    }

    fn world(&self) -> &UiWorld {
        self.context.world()
    }

    fn frame(&mut self) -> nana_ui_core::WorkCounters {
        product_frame(
            &mut self.context,
            self.document,
            viewport(),
            &mut self.shaper,
        )
    }

    fn size(&self, id: StableNodeId) -> (f32, f32) {
        let bounds = self.world().layout_box(id).unwrap();
        (bounds.width, bounds.height)
    }
}

/// Gate A.
#[test]
fn issue202_a_natural_size_reaches_every_context() {
    for context in [
        Context::Flex,
        Context::FlexStretch,
        Context::Grid,
        Context::GridStretch,
        Context::Inline,
        Context::Absolute,
    ] {
        let mut page = Page::new(context, false);
        for id in [page.auto, page.wide] {
            assert_eq!(
                page.world().layout_node(id).unwrap().participation,
                Some(context.participation()),
                "{context:?}"
            );
        }
        let mut queue = MutationQueue::new();
        queue.set_replaced_metadata(url("pic"), Some(ReplacedMetadata::new(120.0, 80.0)));
        page.context.commit_mutations(queue).unwrap();
        let work = page.frame();
        assert_eq!(work.replaced_layout_seeds, 2, "{context:?}");
        assert_eq!(work.layout_full_document_fallbacks, 0, "{context:?}");
        assert_eq!(page.size(page.auto), (120.0, 80.0), "{context:?}");
        assert_eq!(page.size(page.wide), (60.0, 40.0), "{context:?}");
        let mut cold = Page::new(context, true);
        assert_matches_cold(&mut page.context, &mut cold.context, page.document);
    }
}

/// Gate B.
#[test]
fn issue202_replaced_content_sits_on_the_text_baseline_by_its_bottom_edge() {
    let document = DocumentId::new(1).unwrap();
    let (mut b, page) = Builder::page(document, 1, 800.0);
    let fixed = |width: f32, height: f32| LayoutStyle {
        width: Some(LengthSpec::Px(width)),
        height: Some(LengthSpec::Px(height)),
        ..LayoutStyle::default()
    };
    let render_anchor = |b: &mut Builder, id| {
        b.queue.set_custom_render(
            id,
            Some(CustomRenderNode::new("fixture-renderer", "anchor", 0)),
        );
    };
    // A flex row aligning by baseline.
    let row = b.element(
        page,
        LayoutStyle {
            display: Some(DisplaySpec::Flex),
            direction: Some(FlexDirection::Row),
            align_items: AlignSpec::Baseline,
            width: Some(LengthSpec::Px(600.0)),
            ..LayoutStyle::default()
        },
    );
    let row_text = b.label(row, "Baseline");
    let row_image = b.element(row, image_layout("tall", Some(20.0), Some(48.0)));
    let row_render = b.element(row, fixed(30.0, 36.0));
    render_anchor(&mut b, row_render);
    // The same two as atoms of a native paragraph.
    let line_image = b.detached(image_layout("tall", Some(20.0), Some(48.0)));
    let line_render = b.detached(fixed(30.0, 36.0));
    render_anchor(&mut b, line_render);
    let mut context = AppContext::new();
    context.commit_mutations(b.queue).unwrap();
    let paragraph = context
        .create_component(
            document,
            InlineFlow::new(
                InlineContent::builder()
                    .text("Baseline ")
                    .child(line_image)
                    .text(" and ")
                    .child(line_render)
                    .build(),
            )
            .width(LengthSpec::Px(600.0)),
        )
        .unwrap()
        .stable_id();
    let mut shaper = bundled_face_shaper();
    product_frame(&mut context, document, viewport(), &mut shaper);
    let world = context.world();
    // Text baselines are in document space.
    let baseline = |id| world.layout_result(id).unwrap().first_baseline.unwrap();
    for (text, replaced) in [
        (row_text, [row_image, row_render]),
        (paragraph, [line_image, line_render]),
    ] {
        let text_baseline = baseline(text);
        for id in replaced {
            assert_eq!(
                world.layout_node(id).unwrap().content,
                crate::LayoutContentKind::Replaced
            );
            let bounds = world.layout_box(id).unwrap();
            let bottom = bounds.y + bounds.height;
            assert!(
                (bottom - text_baseline).abs() < 0.01,
                "{id:?} bottom {bottom} vs text baseline {text_baseline}"
            );
        }
    }
    assert_eq!(
        world.layout_node(line_image).unwrap().participation,
        Some(Part::AtomicInline)
    );
}
