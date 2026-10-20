//! A whole [`RuntimeProgram`] offscreen: [`nana_ui::headless::HeadlessRuntime`]
//! drives its hooks, this paints and reads back its windows.
//!
//! The program keeps its documents; nothing is moved into a session and back.
//! Application-specific commands reach the program through
//! [`HeadlessRuntime::program_mut`] (via `Deref`).

use std::borrow::Cow;
use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};

use nana_gpu::{GpuContext, GpuTextureDescriptor, GpuTextureUsages};
use nana_ui::runtime::{AccessibilityAction, AccessibilityActionRequest, StableNodeId};
use nana_ui::{
    RuntimeProgram, ScenePaintViewport, SceneWgpuPainter, WindowDescriptor,
    commit_image_natural_sizes,
};
use nana_ui_platform::{
    InputModifiers, InputPayload, KeyInput, KeyState, LogicalKey, PhysicalKey, PointerInput,
    PointerPhase, WindowGeometry, WindowId,
};

pub use nana_ui::headless::{
    HeadlessFileDialog, HeadlessOptions, HeadlessReport, HeadlessRuntime, HeadlessTurn,
    HeadlessWindow, headless_geometry,
};

use crate::agent::{AccessibilityDumpNode, AgentError, accessibility_dump};
use crate::offscreen::{FORMAT, Size, readback};

/// How long a screenshot waits for the images its scene shows to decode.
const IMAGE_DEADLINE: Duration = Duration::from_secs(35);

/// [`HeadlessRuntime`] with pixels.
pub struct HeadlessHost<P: RuntimeProgram> {
    runtime: HeadlessRuntime<P>,
    painter: SceneWgpuPainter,
}

impl<P: RuntimeProgram> Deref for HeadlessHost<P> {
    type Target = HeadlessRuntime<P>;
    fn deref(&self) -> &Self::Target {
        &self.runtime
    }
}

impl<P: RuntimeProgram> DerefMut for HeadlessHost<P> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.runtime
    }
}

impl<P: RuntimeProgram> HeadlessHost<P> {
    /// Initialize the program on `gpu`, the device its content producers
    /// share, with a primary window of `geometry`.
    pub fn new(
        gpu: GpuContext,
        primary: WindowDescriptor,
        geometry: WindowGeometry,
        options: HeadlessOptions,
    ) -> Result<Self, AgentError> {
        let painter = SceneWgpuPainter::new(&gpu, FORMAT);
        let runtime = HeadlessRuntime::new(gpu, primary, geometry, options).map_err(AgentError)?;
        Ok(Self { runtime, painter })
    }

    pub fn runtime(&mut self) -> &mut HeadlessRuntime<P> {
        &mut self.runtime
    }

    /// `id`'s accessibility tree as an agent sees it.
    pub fn accessibility(&self, id: WindowId) -> Vec<AccessibilityDumpNode> {
        self.runtime
            .with_document(id, accessibility_dump)
            .ok()
            .flatten()
            .unwrap_or_default()
    }

    /// Present `id` once more, then paint it over `clear` and read it back,
    /// waiting for the images it shows to decode. Image sizes the paint
    /// learned go to the document for its next frame, as after a native one.
    pub fn screenshot(
        &mut self,
        id: WindowId,
        clear: [f32; 4],
    ) -> Result<(Size<u32>, Vec<u8>), AgentError> {
        let error = |error: &dyn std::fmt::Display| AgentError(error.to_string());
        self.runtime.present(id);
        let geometry = self
            .runtime
            .window(id)
            .ok_or_else(|| AgentError(format!("no window {}", id.0)))?
            .geometry;
        let size = Size::new(geometry.physical_size.0, geometry.physical_size.1);
        let gpu = self.runtime.gpu().clone();
        let texture = gpu
            .create_texture(&GpuTextureDescriptor {
                label: Some("nana-ui headless screenshot"),
                width: size.width,
                height: size.height,
                format: FORMAT,
                usage: GpuTextureUsages::RENDER_TARGET | GpuTextureUsages::COPY_SRC,
            })
            .map_err(|e| error(&e))?;
        let target = texture.render_target().map_err(|e| error(&e))?;
        let scene = self
            .runtime
            .with_document(id, |document| document.shared_scene())
            .ok()
            .flatten()
            .ok_or_else(|| AgentError(format!("window {} has no document", id.0)))?;
        let textures = self.runtime.program().host_textures(id);
        let renderers = self.runtime.program().scene_gpu_renderers(id);
        let viewport = ScenePaintViewport {
            logical_size: [geometry.logical_size.0, geometry.logical_size.1],
            physical_size: [size.width, size.height],
            scale_factor: geometry.scale_factor,
            scene_origin: [0.0; 2],
            target_origin: [0.0; 2],
            clear_color: clear,
            clear: true,
        };
        let deadline = Instant::now() + IMAGE_DEADLINE;
        let pixels = loop {
            let revision = self.painter.image_revision();
            let mut frame = gpu.begin_frame("nana-ui headless screenshot");
            self.painter
                .paint(
                    &scene,
                    &mut frame,
                    &target,
                    viewport,
                    textures.as_ref(),
                    renderers.as_ref(),
                )
                .map_err(|e| AgentError(format!("scene paint failed: {e:?}")))?;
            frame.submit();
            let pixels = readback(&gpu, &texture, size).map_err(|e| error(&e))?;
            if revision == self.painter.image_revision() && !self.painter.has_pending_images() {
                break pixels;
            }
            if Instant::now() >= deadline {
                return Err(AgentError("screenshot timed out waiting for images".into()));
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        let sizes = self.painter.take_image_natural_sizes(None);
        if !sizes.is_empty() {
            let _ = self.runtime.with_document_mut(id, |document| {
                commit_image_natural_sizes(document.context_mut(), &sizes)
            });
        }
        Ok((size, pixels))
    }

    /// Click `node` through its accessibility action.
    pub fn click_node(&mut self, id: WindowId, node: u64) -> Result<(), AgentError> {
        let target =
            StableNodeId::new(node).ok_or_else(|| AgentError("node id 0 is reserved".into()))?;
        self.runtime
            .accessibility_action(
                id,
                AccessibilityActionRequest {
                    target,
                    action: AccessibilityAction::Click,
                },
            )
            .map_err(|error| AgentError(error.to_string()))
    }

    /// A mouse sample at `(x, y)`: `button` is the one that changed (-1 for
    /// none), `buttons` the ones held after it.
    pub fn mouse(
        &mut self,
        id: WindowId,
        phase: PointerPhase,
        (x, y): (f32, f32),
        button: i16,
        buttons: u16,
        modifiers: InputModifiers,
    ) -> Result<(), AgentError> {
        let pointer = PointerInput {
            button,
            buttons,
            pressure: if buttons == 0 { 0.0 } else { 0.5 },
            modifiers,
            ..PointerInput::mouse(phase, x, y)
        };
        self.input(id, InputPayload::Pointer(pointer), None)
    }

    /// Move to `(x, y)`, then press and release the primary button there.
    pub fn click(&mut self, id: WindowId, point: (f32, f32)) -> Result<(), AgentError> {
        let none = InputModifiers::default();
        self.mouse(id, PointerPhase::Move, point, -1, 0, none)?;
        self.mouse(id, PointerPhase::Down, point, 0, 1, none)?;
        self.mouse(id, PointerPhase::Up, point, 0, 0, none)
    }

    /// One key transition; a press may carry the `text` it types.
    pub fn key(
        &mut self,
        id: WindowId,
        (logical, physical): (&str, &str),
        state: KeyState,
        repeat: bool,
        modifiers: InputModifiers,
        text: Option<&str>,
    ) -> Result<(), AgentError> {
        self.input(
            id,
            InputPayload::Key(KeyInput {
                physical: PhysicalKey(Cow::Owned(physical.to_owned())),
                logical: LogicalKey(Cow::Owned(logical.to_owned())),
                state,
                repeat,
                modifiers,
            }),
            text,
        )
    }

    /// Type `text` one character per key press.
    pub fn type_text(&mut self, id: WindowId, text: &str) -> Result<(), AgentError> {
        for character in text.chars() {
            let character = character.to_string();
            self.key(
                id,
                (&character, "Unidentified"),
                KeyState::Pressed,
                false,
                InputModifiers::default(),
                Some(&character),
            )?;
        }
        Ok(())
    }

    /// Route `payload` into `id`; see [`HeadlessRuntime::input`].
    pub fn input(
        &mut self,
        id: WindowId,
        payload: InputPayload,
        text: Option<&str>,
    ) -> Result<(), AgentError> {
        self.runtime
            .input(id, payload, text)
            .map_err(|error| AgentError(error.to_string()))
    }
}
