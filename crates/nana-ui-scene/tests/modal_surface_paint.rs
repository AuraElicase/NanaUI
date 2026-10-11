//! A dialog that paints its own card, as the scene hands it to a painter.
//!
//! The card surface painter's output takes the theme card's place: over the
//! scrim, under the card's title and text, on the frame's own layer, so it
//! fades and moves with the card.

use std::time::Duration;

use nana_ui_core::SemanticColorRole;
use nana_ui_runtime::{
    ComputedStyle, ConfirmDialog, DocumentId, LayoutViewport, OverlayHost, PaintContext, PaintPath,
    Painter, StableNodeId, TextContent, TextMetrics, TextShapeConstraints, TextShaper,
};
use nana_ui_scene::{PrimitiveId, RuntimeDocument};

struct NoText;

impl TextShaper for NoText {
    fn shape(
        &mut self,
        _id: StableNodeId,
        _text: &TextContent,
        _style: &ComputedStyle,
        _constraints: TextShapeConstraints,
    ) -> TextMetrics {
        TextMetrics::default()
    }
}

/// A card with a band across its header: two fills, card coordinates.
struct BandedCard {
    themed: bool,
}

impl Painter for BandedCard {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        if self.themed {
            cx.draw_default();
        }
        let [width, height] = cx.size();
        let header = cx
            .modal_sections()
            .map_or(0.0, |sections| sections.header.height);
        let mut card = PaintPath::new();
        card.rounded_rect(
            nana_ui_runtime::LayoutBox {
                x: 0.0,
                y: 0.0,
                width,
                height,
            },
            [12.0; 4],
        );
        cx.fill_path(&card, SemanticColorRole::Surface);
        let mut band = PaintPath::new();
        band.rounded_rect(
            nana_ui_runtime::LayoutBox {
                x: 0.0,
                y: 0.0,
                width,
                height: header,
            },
            [12.0, 12.0, 0.0, 0.0],
        );
        cx.fill_path(&band, SemanticColorRole::Accent);
    }

    fn paint_key(&self) -> u64 {
        u64::from(self.themed)
    }
}

/// The slot namespace the scene gives a card surface painter's primitives.
const MODAL_SURFACE_PAINT: u64 = 0xFFFF_FFD0;

fn open(themed: bool) -> (RuntimeDocument, StableNodeId) {
    let document = DocumentId::new(1).unwrap();
    let mut runtime = RuntimeDocument::new(document);
    let cx = runtime.context_mut();
    let host = cx.create_component(document, OverlayHost::new()).unwrap();
    let dialog = cx
        .create_detached_component(
            document,
            ConfirmDialog::new("删除场景", "无法恢复。").surface_painter(BandedCard { themed }),
        )
        .unwrap();
    cx.assemble_confirm_dialog(dialog).unwrap();
    cx.append_child(host, dialog).unwrap();
    cx.set_reduced_motion(true);
    runtime
        .flush(LayoutViewport::new(800.0, 600.0), &mut NoText)
        .unwrap();
    assert!(
        runtime
            .context_mut()
            .activate_overlay(host, dialog)
            .unwrap()
    );
    runtime
        .context_mut()
        .advance_animations(Duration::from_millis(1000));
    runtime
        .flush(LayoutViewport::new(800.0, 600.0), &mut NoText)
        .unwrap();
    (runtime, dialog.stable_id())
}

/// The frame's own primitives, in paint order.
fn frame_slots(runtime: &RuntimeDocument, id: StableNodeId) -> Vec<u64> {
    runtime
        .scene()
        .primitives()
        .filter(|primitive| primitive.node == id)
        .map(|primitive| primitive.id.slot)
        .collect()
}

#[test]
fn a_painted_card_takes_the_themed_cards_place_between_scrim_and_title() {
    let (runtime, id) = open(false);
    let slots = frame_slots(&runtime, id);
    let painted = |index: u64| (MODAL_SURFACE_PAINT << 32) | index;
    assert!(!slots.contains(&11), "no theme card under it: {slots:x?}");
    let position = |slot: u64| {
        slots
            .iter()
            .position(|held| *held == slot)
            .unwrap_or_else(|| panic!("slot {slot:x} in {slots:x?}"))
    };
    let (scrim, card, band, title) = (
        position(10),
        position(painted(0)),
        position(painted(1)),
        position(12),
    );
    assert!(
        scrim < card && card < band && band < title,
        "scrim, card, band, then the title: {slots:x?}"
    );

    let scene = runtime.scene();
    let card = scene
        .draw_primitive(PrimitiveId {
            node: id,
            slot: painted(0),
        })
        .expect("the painted card draws");
    let scrim = scene
        .draw_primitive(PrimitiveId { node: id, slot: 10 })
        .expect("the scrim draws");
    assert_eq!(
        card.primitive.transform, scrim.primitive.transform,
        "the card rides the frame's own layer"
    );
}

#[test]
fn draw_default_keeps_the_themed_card_under_the_painted_one() {
    let (runtime, id) = open(true);
    let slots = frame_slots(&runtime, id);
    let card = slots
        .iter()
        .position(|slot| *slot == 11)
        .expect("theme card");
    // `draw_default()` is the recording's first op, so the painted ones
    // start after it.
    let painted = slots
        .iter()
        .position(|slot| *slot >> 32 == MODAL_SURFACE_PAINT)
        .expect("painted card");
    assert!(card < painted, "the theme's card is under: {slots:x?}");
    assert!(
        slots[painted..].iter().all(|slot| *slot != 11),
        "and only once: {slots:x?}"
    );
}
