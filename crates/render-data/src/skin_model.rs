use std::sync::Arc;

/// Named geometry and texture slots used by the persona render controllers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkinAnimationKind {
    Face,
    Body32,
    Body128,
}

impl SkinAnimationKind {
    /// Returns the stable atlas slot for this animation kind.
    pub const fn slot(self) -> usize {
        match self {
            Self::Face => 0,
            Self::Body32 => 1,
            Self::Body128 => 2,
        }
    }
    /// Returns the resource-patch geometry slot for this animation image.
    pub const fn geometry_key(self) -> &'static str {
        match self {
            Self::Face => "animated_face",
            Self::Body32 => "animated_32x32",
            Self::Body128 => "animated_128x128",
        }
    }
}

/// A transmitted persona animation atlas, without resampling or alpha modification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinAnimation {
    pub kind: SkinAnimationKind,
    pub width: u32,
    pub height: u32,
    pub rgba8: Arc<[u8]>,
    pub frames: u32,
    pub blinking: bool,
}

/// The resource patch and geometry JSON a skin carries; parsed by the actor runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinGeometrySource {
    pub resource_patch: Arc<str>,
    pub geometry_data: Arc<str>,
    pub animations: Arc<[SkinAnimation]>,
}

impl SkinGeometrySource {
    #[must_use]
    pub fn byte_len(&self) -> usize {
        self.resource_patch.len()
            + self.geometry_data.len()
            + self
                .animations
                .iter()
                .map(|image| image.rgba8.len())
                .sum::<usize>()
    }
}
