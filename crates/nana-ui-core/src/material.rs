//! Immutable, backend-neutral fragment materials shared by shapes and text.
//! A material never owns geometry, a device, a clock, or a render target.
use serde::{Deserialize, Serialize};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

static NEXT: AtomicU64 = AtomicU64::new(1);
fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

/// The portable WGSL fragment contract. Sampling helpers return linear RGBA.
/// The host, not the author, applies geometric coverage, clipping and opacity.
pub const MATERIAL_ABI: &str = r#"
struct MaterialInput {
    position: vec2<f32>, uv: vec2<f32>, size: vec2<f32>,
    glyph_uv: vec2<f32>, color: vec4<f32>,
};
@group(2) @binding(0) var<storage, read> nm_parameters: array<vec4<f32>>;
@group(2) @binding(1) var nm_texture0: texture_2d<f32>;
@group(2) @binding(2) var nm_texture1: texture_2d<f32>;
@group(2) @binding(3) var nm_sampler: sampler;
fn material_param(index: u32) -> vec4<f32> {
    return nm_parameters[min(index, arrayLength(&nm_parameters) - 1u)];
}
fn material_sample0(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(nm_texture0, nm_sampler, clamp(uv, vec2(0.), vec2(1.)), 0.);
}
fn material_sample1(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(nm_texture1, nm_sampler, clamp(uv, vec2(0.), vec2(1.)), 0.);
}
"#;

/// A pure `fn material(input: MaterialInput) -> vec4<f32>` and its helpers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialProgram {
    pub source: Arc<str>,
}
impl MaterialProgram {
    pub fn new(source: impl Into<Arc<str>>) -> Arc<Self> {
        Arc::new(Self {
            source: source.into(),
        })
    }
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::hash::DefaultHasher::new();
        self.source.hash(&mut hash);
        hash.finish()
    }
}

/// Straight-alpha sRGB image data. Replacement is immutable and revisioned.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialTexture {
    pub size: [u32; 2],
    pub rgba: Arc<[u8]>,
}

/// A retained reference owns its immutable resources. Copies share identity;
/// parameter/texture replacements keep that identity and advance its revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MaterialRef(Arc<MaterialInstance>);
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MaterialInstance {
    #[serde(skip, default = "next_id")]
    id: u64,
    revision: u64,
    program: Arc<MaterialProgram>,
    parameters: Arc<[[f32; 4]]>,
    textures: Arc<[MaterialTexture]>,
}
impl MaterialRef {
    pub fn new(program: Arc<MaterialProgram>) -> Self {
        Self(Arc::new(MaterialInstance {
            id: next_id(),
            revision: 1,
            program,
            parameters: Arc::from([[0.; 4]]),
            textures: Arc::from([]),
        }))
    }
    pub fn id(&self) -> u64 {
        self.0.id
    }
    pub fn key(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::hash::DefaultHasher::new();
        (self.0.id, self.0.revision).hash(&mut h);
        h.finish() | 1
    }
    pub fn revision(&self) -> u64 {
        self.0.revision
    }
    pub fn program(&self) -> &Arc<MaterialProgram> {
        &self.0.program
    }
    pub fn parameters(&self) -> &[[f32; 4]] {
        &self.0.parameters
    }
    pub fn textures(&self) -> &[MaterialTexture] {
        &self.0.textures
    }
    pub fn downgrade(&self) -> std::sync::Weak<MaterialInstance> {
        Arc::downgrade(&self.0)
    }
    pub fn upgrade(weak: &std::sync::Weak<MaterialInstance>) -> Option<Self> {
        weak.upgrade().map(Self)
    }
    pub fn with_parameters(mut self, parameters: impl Into<Arc<[[f32; 4]]>>) -> Self {
        let inner = Arc::make_mut(&mut self.0);
        inner.parameters = parameters.into();
        inner.revision = next_id();
        self
    }
    pub fn with_textures(mut self, textures: impl Into<Arc<[MaterialTexture]>>) -> Self {
        let inner = Arc::make_mut(&mut self.0);
        inner.textures = textures.into();
        inner.revision = next_id();
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacements_keep_identity_but_deserialization_creates_a_new_instance() {
        let a = MaterialRef::new(MaterialProgram::new(
            "fn material(i: MaterialInput)->vec4<f32>{return i.color;}",
        ));
        let b = a.clone().with_parameters(Arc::from([[1.; 4]]));
        assert_eq!(a.id(), b.id());
        assert_ne!(a.revision(), b.revision());
        let c: MaterialRef = serde_json::from_str(&serde_json::to_string(&b).unwrap()).unwrap();
        assert_ne!(b.id(), c.id());
        assert_eq!(b.program(), c.program());
    }
}
