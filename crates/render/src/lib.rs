//! Scene IR backends: Skia on CPU raster or GPU (PNG, GIF) and artscript's own SVG writer.

mod draw;
mod effects;
mod features;
mod field;
mod gif;
mod gpu;
mod paint;
mod svg;
mod text;

use artscript_ir::{Profile, Scene};

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
    Skia(String),
    TransparentGif,
    /// `width × height × frames × 4` exceeds [`RASTER_BUDGET`].
    GifTooLarge,
}

/// RGBA bytes of every GIF frame. Encoding also holds a second copy for the palette.
/// `sample` uses this same cap, and parallel encodes reserve against it and wait.
pub const RASTER_BUDGET: u64 = 256 * 1024 * 1024;

/// Renders the scene at `scale` (1 = one pixel per canvas unit) and encodes it as PNG.
/// Deterministic per backend on one machine. `Backend::Gpu` may differ from `Cpu` and across machines.
pub fn render_png(scene: &Scene, scale: f32, backend: Backend) -> Result<Vec<u8>, RenderError> {
    draw::render_png(scene, scale, backend)
}

/// Writes SVG. Under `Profile::Svg`, subtrees using raster-only features are baked to
/// embedded PNG tiles rendered at `bake_scale`. Under `Profile::Vector` they are an error.
pub fn render_svg(
    scene: &Scene,
    profile: Profile,
    bake_scale: f32,
    backend: Backend,
) -> Result<String, RenderError> {
    svg::render_svg(scene, profile, bake_scale, backend)
}

/// Unpremultiplied RGBA8888 bytes, row-major, length `width * height * 4` at `scale`.
pub fn render_rgba(
    scene: &Scene,
    scale: f32,
    backend: Backend,
) -> Result<(u32, u32, Vec<u8>), RenderError> {
    draw::render_rgba(scene, scale, backend)
}

/// One global palette, deterministic NeuQuant, delays already in centiseconds.
pub fn render_gif(
    scenes: &[&Scene],
    scale: f32,
    delays: &[u16],
    loop_forever: bool,
    backend: Backend,
) -> Result<Vec<u8>, RenderError> {
    gif::encode(scenes, scale, delays, loop_forever, backend)
}
