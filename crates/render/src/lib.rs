//! Scene IR backends: Skia on CPU raster or GPU (PNG, GIF) and ramose's own SVG writer.

mod draw;
mod effects;
mod features;
mod field;
mod gif;
mod gpu;
mod mark;
mod paint;
mod ribbon;
mod sdf;
mod svg;
mod text;
mod warp;

use ramose_ir::{Framing, Profile, Scene};

pub use features::raster_only_features;
pub use gpu::{Backend, gpu_available};

#[derive(Debug, Clone, PartialEq)]
pub struct Unsupported {
    /// Node path that uses the feature.
    pub node: String,
    /// Short feature name, e.g. `field fill`, `fx grain`.
    pub feature: String,
}

#[derive(Debug)]
pub enum RenderError {
    /// Under `Profile::Vector`, every node that uses a raster-only feature.
    NotVector(Vec<Unsupported>),
    /// Under `Profile::Svg`, every node whose brush reads `m.below`: a baked tile has no
    /// backdrop, so such a node cannot bake.
    NoBackdrop(Vec<Unsupported>),
    Skia(String),
    /// A failure inside one node, reported at that node's `file:line` instead of the target.
    AtNode { origin: String, message: String },
    TransparentGif,
    /// `width × height × frames × 4` exceeds [`RASTER_BUDGET`].
    GifTooLarge,
}

/// RGBA bytes of every GIF frame. Encoding also holds a second copy for the palette.
/// `sample` uses this same cap, and parallel encodes reserve against it and wait.
pub const RASTER_BUDGET: u64 = 256 * 1024 * 1024;

/// Renders the scene's view through `framing` and encodes it as PNG of `framing.pixels()`.
/// Deterministic per backend on one machine. `Backend::Gpu` may differ from `Cpu` and across machines.
pub fn render_png(scene: &Scene, framing: &Framing, backend: Backend) -> Result<Vec<u8>, RenderError> {
    draw::render_png(scene, framing, backend)
}

/// Writes SVG of `framing.out` units. Under `Profile::Svg`, subtrees using raster-only features
/// are baked to embedded PNG tiles rendered at `bake_scale`. Under `Profile::Vector` they are an
/// error.
pub fn render_svg(
    scene: &Scene,
    framing: &Framing,
    profile: Profile,
    bake_scale: f32,
    backend: Backend,
) -> Result<String, RenderError> {
    svg::render_svg(scene, framing, profile, bake_scale, backend)
}

/// Unpremultiplied RGBA8888 bytes, row-major, length `width * height * 4` of `framing.pixels()`.
pub fn render_rgba(
    scene: &Scene,
    framing: &Framing,
    backend: Backend,
) -> Result<(u32, u32, Vec<u8>), RenderError> {
    draw::render_rgba(scene, framing, backend)
}

/// One global palette, deterministic NeuQuant, delays already in centiseconds. Every frame is
/// `framing.pixels()`.
pub fn render_gif(
    scenes: &[&Scene],
    framing: &Framing,
    delays: &[u16],
    loop_forever: bool,
    backend: Backend,
) -> Result<Vec<u8>, RenderError> {
    gif::encode(scenes, framing, delays, loop_forever, backend)
}
